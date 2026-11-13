# poc-bee-nym

Proof of concept: run a private Bee (Swarm) cluster with node-to-node traffic carried over the
[Nym](https://nym.com) mixnet, and measure how well Bee keeps working. Results and conclusions are in
[REPORT.md](REPORT.md).

Bee itself is unmodified. A per-node sidecar intercepts Bee's p2p traffic with iptables and tunnels it through Nym
mainnet.

## Status: investigation concluded

> **Disclaimer:** running the whole Swarm network over Nym does not work. Hiding only the user's node behind Nym works
> for small content, slowly, and only with a patched Bee. Don't pursue a native integration (option 3) for full nodes.

**What was found**

- **Full nodes on the mixnet: no.** Full nodes constantly sync with each other, and that needs far more bandwidth than
  a Nym client provides (about 50 KB/s per node). Stock Bee also blocklists slow peers, so the mesh collapses under
  load. A native libp2p Nym transport wouldn't help, because the limit is the mixnet itself, not the sidecar bridge.
- **Only the user's light node on Nym: yes, for small content, with a Bee patch.** Uploads from a hidden light node
  worked up to 1 MB at about 7 KB/s. Downloads were reliable at 4 KB and unreliable at 64 KB. Stock Bee manages only
  single chunks.

| | Plain Swarm | Hidden user (patched Bee) |
|---|---|---|
| 4 KB upload / download | 0.01 s / 0.01 s | 3 s / 4 s |
| 64 KB upload / download | 0.03 s / 2.4 s | 20 s / 17–22 s (unreliable) |
| 1 MB upload / download | 0.8 s / 16 s | 144 s / failed |

**Why it breaks**

1. **Payments:** pseudosettle expects each payment exchange to finish within 5 s with timestamps agreeing to ±2 s, and
   blocklists the peer otherwise. Mixnet delay trips this constantly. Relaxing it is what makes the hidden client
   usable.
2. **Retries:** retrieval retries a chunk request every 1 s, faster than a mixnet round trip (1–3 s), so all peers are
   tried before any of them answers.
3. **Bandwidth:** once traffic exceeds the Nym client's send rate, queueing delay grows until connections time out.

**Limitations of this investigation**

- **Test bed:** results come from a 6-node local cluster with only 5 repetitions per size, on Nym mainnet. Mainnet was
  unreliable: a shared gateway dropped all its sessions mid-run, and the Docker Desktop VM clock drifted 15 minutes.
  64 KB results vary a lot between runs.
- **Missing run:** a clean "all nodes on Nym + patched Bee" run was never obtained, so how far the patch helps full
  nodes is unknown.
- **Unconfirmed cause:** the yamux keepalive failure in point 3 was seen in an invalid run, so it's likely but not
  proven.
- **Patch is untested piece by piece:** the fork patch was never split, so the minimal change set isn't known. It is
  test-only and must never be upstreamed as is.
- **Bridge, not native:** only the sidecar bridge was tested, not a native transport. Bee's handling of Nym addresses
  (in place of IP underlays) was not exercised.

**Recommendation:** if Swarm user privacy is the goal, the realistic path is putting only users' light clients behind
Nym. That needs Bee's settlement protocol to tolerate seconds of latency, which is worth raising with the Bee team.
Next steps are listed at the end of [REPORT.md](REPORT.md).

## Layout

```
cluster/   Docker Compose cluster: anvil chain, 5 full nodes, 1 light node (bee-6)
sidecar/   Rust TCP <-> Nym mixnet bridge (nym-sdk stream API)
fork/      test-only Bee patch with mixnet-tolerant timeouts
bench/     benchmark harness (Node, no dependencies)
spike/     Phase 0 Nym latency/throughput probe
results/   benchmark output (JSON per run)
```

## Requirements

- Docker (Desktop on macOS is fine; everything Rust and Go builds in containers)
- Node.js ≥ 18.3
- Python 3 (helper one-liners in the scripts)
- Internet access to Nym mainnet; no Nym credentials are needed

Bee and chain images come from bee-factory (`ethersphere/bee-factory-*:v2.8.2`) and are pulled automatically.

## Running a cluster

```sh
cluster/up.sh                # baseline: plain libp2p
cluster/up.sh nym            # every node behind a Nym sidecar
cluster/up.sh edge           # only the light node (bee-6) behind Nym, full nodes direct
cluster/up.sh nym fork       # any mode can add "fork" to use the patched Bee binary
cluster/up.sh edge sandbox   # nym/edge modes can add "sandbox" (Nym testnet) and/or "paid" (credentials mode)
PUBLIC_IP=<ip> cluster/up.sh twohop   # bee-6 behind NymVPN WireGuard 2-hop; see "WireGuard 2-hop" below
```

`up.sh` resyncs the Docker VM clock if needed, starts the stack, funds bee-6 and waits until every node has at least
`MIN_PEERS` peers (default 4, up to `MESH_TIMEOUT` seconds, default 600).

Bee APIs on the host:

| Node | Role | API |
|---|---|---|
| bee-1 | full node, bootnode | http://localhost:21633 |
| bee-2 | full node | http://localhost:21635 |
| bee-3 | full node | http://localhost:21637 |
| bee-4 | full node | http://localhost:21639 |
| bee-5 | full node | http://localhost:21641 |
| bee-6 | light node | http://localhost:21643 |

Quick check: `cluster/smoke.sh` uploads on bee-2 and downloads from bee-5 (`SIZE`, `UPLOADER`, `DOWNLOADER` override
the defaults, e.g. `DOWNLOADER=http://localhost:21643` for the light node).

Stop everything (pass all overlays so no orphans remain):

```sh
cd cluster
COMPOSE_FILE=docker-compose.yml:docker-compose.nym.yml:docker-compose.edge.yml:docker-compose.fork.yml \
  docker compose down --remove-orphans
```

## Benchmarks

```sh
node bench/bench.mjs --mode nym --fork --restart
node bench/bench.mjs --mode edge --fork --uploader 6 --downloader 3
```

| Option | Default | Meaning |
|---|---|---|
| `--mode` | `baseline` | `baseline`, `nym` or `edge` |
| `--fork` | off | use the patched Bee binary |
| `--restart` | off | recreate the cluster first and record mesh formation time |
| `--reps` | 5 | repetitions per size |
| `--sizes` | `4096,65536,1048576` | payload sizes in bytes |
| `--timeout` | 300 | per-request timeout in seconds |
| `--uploader` / `--downloader` | 2 / 6 | node numbers |

Each run writes `results/<mode>[-fork][-upN-downM]-<timestamp>.json` with per-run timings, a summary, topology before
and after, Bee log error counts, sidecar connection stats and per-container network deltas.

## Sidecar

- Bee shares the sidecar's network namespace. iptables redirects outbound `10.0.0.0/24:1634` to the sidecar, and a
  leak guard drops direct inbound Bee traffic.
- The sidecar looks up the destination's Nym address in a shared `/peers` volume and opens a `MixnetStream`. Inbound
  streams are piped to the local Bee; replies travel on SURBs, so the dialer stays anonymous.
- DATA/FIN framing replaces the close signal the SDK lacks.
- In edge mode the full nodes get ingress-only sidecars (`INGRESS_ONLY=1`, no iptables).
- Logs are JSON lines: `connected`, one `conn` per closed connection, `stats` every 10 s. The Nym client reconnects
  in-process if its gateway drops it.

Tests: `docker run --rm -v "$PWD/sidecar":/src -w /src rust:1 cargo test --release`

## Bee fork

`fork/build.sh` clones Bee v2.8.2, applies `fork/bee-nym-timeouts.patch` and builds `fork/bin/bee`, which
`docker-compose.fork.yml` mounts into the Bee containers. It is for testing only and must never go upstream. Build with
`VERSION=2.8.2`: Bee picks protocol handlers by peer semver, and a suffix like `2.8.2-foo` silently breaks pushsync.

## Paid bandwidth (credentials mode)

All results above used the free mode, in which mainnet gateways accept clients without credentials. In credentials mode
a sidecar spends zk-nym ticketbooks instead. Paying did not improve performance on either network (see below and
[REPORT.md](REPORT.md)).

> **Cost warning:** a mainnet ticketbook costs about **75 NYM** (sandbox: about 1 NYM). One holds 50 tickets of about
> 190 MiB (9.31 GiB) and expires about a week after purchase. Buy one and share it rather than one per sidecar.

Two overlays, deliberately separate:

- **`paid`** spends tickets that are already stored; it never buys and loads no wallet. Only the hidden client's sidecar
  (`nym-6`) runs in credentials mode. Its `/state` must already hold a ticketbook.
- **`buy`** is the only path that spends money. It loads the wallet named by `NYM_CREDENTIALS_FILE` (required, no
  default) and sets `NYM_ALLOW_PURCHASE=1`; each sidecar without `<STATE_DIR>/.ticketbook-acquired` buys one ticketbook.
  Purchases are staggered and retried (parallel transactions from one wallet fail with `account sequence mismatch`), and
  a sidecar allowed to buy exits instead of retrying a failed connect.

```sh
# spend-only, using a ticketbook already in nym-6's state
cluster/up.sh edge fork paid

# buying: try the sandbox first (free faucet tokens, settings in cluster/sandbox.env)
NYM_CREDENTIALS_FILE=nym-credentials.sandbox.env cluster/up.sh edge sandbox buy
```

Wallet files (`cluster/nym-credentials*.env`, from `cluster/nym-credentials.env.example`) are gitignored; never commit
them. Sandbox state lives in `/state/sandbox`, separate from mainnet state in the same volume.

Ticketbooks are portable: copying a sidecar's `credentials_database.db` (with its `-wal`/`-shm` files, containers
stopped) into another sidecar's state let that sidecar spend it; the gateway accepted the copied tickets.

Results (edge mode + fork, hidden light node, p50):

| | Sandbox paid | Sandbox free | Mainnet paid | Mainnet free |
|---|---|---|---|---|
| Download 4 KB / 64 KB / 1 MB | 1.8 s / 10.9 s / 87 s | 1.6 s / 10.5 s / 83 s | 5.1 s / 35.8 s / failed | 4.2 s / 16–22 s (unreliable) / failed |
| Upload 4 KB / 64 KB / 1 MB | 1.5 s / 14.0 s / 101 s | 1.5 s / 9.7 s / 82 s | 12.5 s / 41 s / 231 s | 3.0 s / 20 s / 144 s |

Paying made no measurable difference on the sandbox; on mainnet paid was slower, which reflects a different gateway and
time of day (one run each), not an effect of paying. The sandbox is faster than mainnet regardless.

## WireGuard 2-hop

`twohop` mode puts bee-6 inside a containerised NymVPN client (`nymvpn/`, official v2026.15.0 arm64 packages) in
WireGuard 2-hop mode. All of bee-6's traffic exits through Nym's exit gateway to the internet, so the full nodes must be
publicly reachable:

1. Forward TCP 31634–31638 on every router between the internet and this machine to its LAN address (with double NAT,
   on both the ISP modem and the inner router). Reserve the machine's LAN address so the rules don't go stale.
2. Start with `PUBLIC_IP=<your public IP> cluster/up.sh twohop`. The full nodes advertise `PUBLIC_IP:3163N`.
3. Log the NymVPN container in once (stored in the `nymvpn-data` volume; counts as a device on the account):

   ```sh
   read -rs "m?NymVPN account phrase: " && echo && docker exec bee-nym-nymvpn-1 nym-vpnc account set "$m"; unset m
   docker exec bee-nym-nymvpn-1 nym-vpnc connect --wait
   ```

- Turn off any NymVPN running on the host during the test: it tunnels all Docker traffic and breaks inbound connections
  to the full nodes.
- A leak guard drops bee-6's direct traffic to the full nodes' private addresses; local-network access stays allowed for
  the test chain only.
- bee-6's API is relayed through `bee6-api` on `localhost:21643`, because NymVPN's routing breaks Docker's published
  ports on its own container.
- Benchmarks: `PUBLIC_IP=<ip> node bench/bench.mjs --mode twohop` (and `--uploader 6 --downloader 3`).
- Remove the port forwards when done.

## Known limitations

- Nym mainnet is a moving target: gateways occasionally drop sessions, and Docker Desktop's VM clock can drift.
  Expect variance between runs.
- `nym-sdk` from crates.io (1.21.6) doesn't build; the sidecar uses a pinned git revision with Nym's own `Cargo.lock`.
