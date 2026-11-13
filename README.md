# poc-bee-nym

Proof of concept: a private Bee (Swarm) cluster whose node traffic goes through [Nym](https://nym.com), to see whether
Bee still works. Results: [REPORT.md](REPORT.md).

## Status: concluded

- **Bee does talk over Nym**, unmodified, via a per-node sidecar.
- **A whole Swarm network on the mixnet doesn't work**: too slow, and the mesh collapses under load.
- **Hiding only the user's light node works for small content**, slowly, and only with a patched Bee.
- **WireGuard 2-hop is fast** (near plain Swarm) but gives up the mixing that hides who talks to whom.
- **Paying for Nym doesn't make it faster.**

## Requirements

Docker (everything builds in containers), Node.js ≥ 18.3, Python 3, internet access. Bee images come from bee-factory
(`ethersphere/bee-factory-*:v2.8.2`).

## Usage

```sh
cluster/up.sh [mode] [add-ons]          # start; waits until nodes have peers
node bench/bench.mjs --mode <mode> ...  # benchmark; writes results/*.json
cluster/smoke.sh                        # quick upload/download check
```

| Mode | What |
|---|---|
| *(none)* | baseline, plain libp2p |
| `nym` | every node behind a Nym mixnet sidecar |
| `edge` | only the light node bee-6 behind Nym |
| `twohop` | bee-6 behind NymVPN WireGuard 2-hop (needs `PUBLIC_IP`, see below) |

Add-ons: `fork` (patched Bee), `sandbox` (Nym testnet), `paid` (spend stored ticketbooks), `buy` (buy ticketbooks).

Bee APIs: bee-1 to bee-6 on `localhost:21633`, `21635`, … `21643`. Benchmark options: `--fork`, `--sandbox`, `--paid`,
`--restart`, `--reps`, `--sizes`, `--timeout`, `--uploader N`, `--downloader N`.

## Layout

```
cluster/  Docker Compose: test chain, 5 full nodes, light node bee-6, one overlay per mode
sidecar/  Rust bridge: Bee TCP <-> Nym mixnet stream
nymvpn/   NymVPN client image for 2-hop
fork/     test-only Bee patch with relaxed timeouts (never upstream)
bench/    benchmark harness
results/  benchmark output (gitignored)
```

## How it works

- **Mixnet modes:** Bee shares its sidecar's network. iptables redirects Bee's peer traffic to the sidecar, which sends it
  over Nym to the peer's sidecar. A leak guard blocks direct Bee-to-Bee traffic.
- **2-hop mode:** bee-6 runs inside a NymVPN container; its traffic leaves through Nym's exit gateway to the internet and
  reaches the full nodes on your public IP.

## Paid mode

> **Cost:** a mainnet ticketbook costs about **75 NYM** (sandbox: about 1) and expires after about a week. Ticketbooks can
> be shared between sidecars by copying `credentials_database.db`.

- `paid` only spends tickets already stored in nym-6's state. It never buys.
- `buy` is the only path that spends money. It needs `NYM_CREDENTIALS_FILE` set to a wallet file explicitly. Try the
  sandbox first: `NYM_CREDENTIALS_FILE=nym-credentials.sandbox.env cluster/up.sh edge sandbox buy`.
- Wallet files (`cluster/nym-credentials*.env`) are gitignored. Never commit them.

## 2-hop setup

1. Forward TCP 31634–31638 to this machine on every router in front of it (with double NAT, on both), and reserve the
   machine's LAN address.
2. Turn off any NymVPN running on the host.
3. `PUBLIC_IP=<your public IP> cluster/up.sh twohop`
4. Log the NymVPN container in once:
   ```sh
   read -rs "m?NymVPN account phrase: " && echo && docker exec bee-nym-nymvpn-1 nym-vpnc account set "$m"; unset m
   docker exec bee-nym-nymvpn-1 nym-vpnc connect --wait
   ```
5. Remove the port forwards when done.

## Known issues

- Nym mainnet varies a lot: gateways drop sessions, and Docker Desktop's clock can drift (`up.sh` resyncs it).
- `nym-sdk` from crates.io doesn't build; the sidecar pins a git revision with Nym's lockfile.
- Build the Bee fork with `VERSION=2.8.2`; a version suffix silently breaks pushsync.
