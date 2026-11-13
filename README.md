# poc-bee-nym

Proof of concept: a private Bee (Swarm) cluster whose node traffic goes through the [Nym](https://nym.com) mixnet, to
see whether Bee still works. Results: [REPORT.md](REPORT.md).

## Status: concluded

- **Bee does talk over Nym**, unmodified, via a per-node sidecar.
- **A whole Swarm network on the mixnet doesn't work**: too slow, and the mesh collapses under load.
- **Hiding only the user's light node works for small content**, slowly, and only with a patched Bee.

Follow-up work lives on branches: `feat/nym-credentials` (paid Nym bandwidth) and `feat/nym-2hop` (NymVPN WireGuard
2-hop instead of the mixnet).

## Requirements

Docker (everything builds in containers), Node.js ≥ 18.3, Python 3, internet access. Bee images come from bee-factory
(`ethersphere/bee-factory-*:v2.8.2`).

## Usage

```sh
cluster/up.sh [mode] [fork]             # start; waits until nodes have peers
node bench/bench.mjs --mode <mode> ...  # benchmark; writes results/*.json
cluster/smoke.sh                        # quick upload/download check
```

| Mode | What |
|---|---|
| *(none)* | baseline, plain libp2p |
| `nym` | every node behind a Nym mixnet sidecar |
| `edge` | only the light node bee-6 behind Nym |

Add `fork` to use the patched Bee. Bee APIs: bee-1 to bee-6 on `localhost:21633`, `21635`, … `21643`. Benchmark
options: `--fork`, `--restart`, `--reps`, `--sizes`, `--timeout`, `--uploader N`, `--downloader N`.

## Layout

```
cluster/  Docker Compose: test chain, 5 full nodes, light node bee-6, one overlay per mode
sidecar/  Rust bridge: Bee TCP <-> Nym mixnet stream
fork/     test-only Bee patch with relaxed timeouts (never upstream)
bench/    benchmark harness
spike/    early Nym latency/throughput probe
results/  benchmark output
```

## How it works

Bee shares its sidecar's network. iptables redirects Bee's peer traffic to the sidecar, which sends it over Nym to the
peer's sidecar. A leak guard blocks direct Bee-to-Bee traffic.

## Known issues

- Nym mainnet varies a lot: gateways drop sessions, and Docker Desktop's clock can drift (`up.sh` resyncs it).
- `nym-sdk` from crates.io doesn't build; the sidecar pins a git revision with Nym's lockfile.
- Build the Bee fork with `VERSION=2.8.2`; a version suffix silently breaks pushsync.
