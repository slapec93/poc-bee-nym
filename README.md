# poc-bee-nym

Proof of concept: run a private Bee (Swarm) cluster with node-to-node traffic carried over the
[Nym](https://nym.com) mixnet, and measure how well Bee keeps working. Results and conclusions are in
[REPORT.md](REPORT.md).

Bee itself is unmodified. A per-node sidecar intercepts Bee's p2p traffic with iptables and tunnels it through Nym
mainnet.

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

## Known limitations

- Nym mainnet is a moving target: gateways occasionally drop sessions, and Docker Desktop's VM clock can drift.
  Expect variance between runs.
- `nym-sdk` from crates.io (1.21.6) doesn't build; the sidecar uses a pinned git revision with Nym's own `Cargo.lock`.
