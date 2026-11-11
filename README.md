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

All results above used the free mode, in which mainnet gateways accept clients without credentials. Paid mode makes
each sidecar spend a zk-nym ticketbook bought from a Nyx wallet. Paying buys access guarantees, not lower latency or a
higher send rate.

1. Create `cluster/nym-credentials.env` from `cluster/nym-credentials.env.example` and set `NYM_MNEMONIC` to a funded
   Nyx wallet. The file is gitignored; never commit it.
2. Start with `paid`, preferably on the sandbox testnet first (free faucet tokens; network settings in
   `cluster/sandbox.env`):

   ```sh
   cluster/up.sh edge sandbox paid
   ```

- On first connect each sidecar buys one `V1MixnetEntry` ticketbook. It logs `ticketbook_acquired` and writes
  `<STATE_DIR>/.ticketbook-acquired`; delete that file to buy again.
- Sandbox state lives in `/state/sandbox`, separate from mainnet state in the same volume.
- Untested so far: sandbox connectivity without credentials was verified, but no funded wallet has been tried. Purchase
  amounts, ticketbook size and whether one ticketbook lasts a benchmark run (cover traffic consumes it continuously) are
  unknown.

## Known limitations

- Nym mainnet is a moving target: gateways occasionally drop sessions, and Docker Desktop's VM clock can drift.
  Expect variance between runs.
- `nym-sdk` from crates.io (1.21.6) doesn't build; the sidecar uses a pinned git revision with Nym's own `Cargo.lock`.
