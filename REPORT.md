# Bee over Nym: results

Private 5-full-node + 1-light-node Bee 2.8.2 cluster, node traffic carried over Nym mainnet by a per-node sidecar
(`sidecar/`). Measured with `bench/bench.mjs`; raw data in `results/`. All runs: 5 repetitions per size, upload on one
node, download on another, content verified.

## Verdict

- **Full nodes on the mixnet: no.** Stock Bee collapses under load; even with relaxed timeouts the throughput a Nym
  client provides (~50 KB/s per node, shared by all peers and pullsync) is far below what full-node background traffic
  needs. Option 3 (native libp2p Nym transport) would not change this, the limit is the mixnet, not the bridge.
- **A Nym-hidden light client against a normal network: yes, for small content, with a Bee patch.** Uploads from the
  hidden client work up to 1 MB (~7 KB/s); downloads work reliably up to 4 KB and partly at 64 KB. Stock Bee only
  manages single chunks, because its payment protocol punishes slow peers.

## Results (p50, ok/5)

| Setup | 4 KB up / down | 64 KB up / down | 1 MB up / down |
|---|---|---|---|
| Baseline (plain libp2p) | 0.01 s / 0.01 s | 0.03 s / 2.4 s | 0.76 s / 16 s |
| Baseline + fork | 0.01 s / 0.01 s | 0.03 s / 2.4 s | 0.74 s / 18 s |
| All nodes on Nym, stock | 2.4 s / 2.8 s | 3.7 s / 28 s (2/5) | mesh collapsed (0/5) |
| Edge, stock: hidden light node downloads | – / 4.8 s | – / 0/5 | – / 0/5 |
| Edge + fork: hidden light node downloads | – / 4.2 s | – / 17–22 s (5/5, then 1/5) | – / 0/5 |
| Edge + fork: hidden light node uploads | 3.0 s | 20 s | 144 s |

"Edge" = full nodes talk directly; only the light node (the user) is behind Nym. In the upload rows the content was
read back from a full node to confirm it reached the network (5/5 every size).

Mesh formation: 35 s baseline, 108 s all-Nym, about 50 s edge.

## Why it breaks

1. **Pseudosettle punishes latency.** A payment refresh must complete within 5 s and its timestamps must agree within
   ±2 s (`pkg/settlement/pseudosettle`). Over the mixnet that fails often, accounting then blocklists the peer
   ("failed to refresh"). More traffic means more refreshes, so load drains the peer table. This is the main failure
   for both full nodes and the hidden light client.
2. **Retrieval preemptive retries every 1 s**, below the 1.3–2.7 s mixnet RTT: the origin exhausts all peers before any
   answer ("no peer found").
3. **Throughput.** A Nym client sends at a fixed Poisson rate (spike: 56 KiB/s). Once demand exceeds it, queueing
   latency grows without bound and libp2p's yamux keepalive drops the connection. Seen in a fork run later found to be
   invalid (see below), so treat as likely, not proven.

## Bee patch (test only, `fork/bee-nym-timeouts.patch`)

Pseudosettle timeout 5→30 s and timestamp tolerance ±2/3→±30 s, retrieval preemptive retry 1→5 s and chunk timeout
30→60 s, handshake / headers / connect timeouts →60 s. Behaves like stock Bee on a plain network (baseline + fork row).
The pseudosettle change is what unlocks the hidden client; the rest is unverified individually.

## Pitfalls hit (worth knowing for any follow-up)

- Build Bee with `VERSION=2.8.2` and `REACHABILITY_OVERRIDE_PUBLIC=true`. Bee selects protocol handlers by peer semver;
  a suffix like `2.8.2-foo` counts as a pre-release and silently stalls pushsync. This invalidated the first fork runs.
- Bee rejects a private `--nat-addr`; use `--allow-private-cidrs` and the listen address.
- Light nodes without SWAP are throttled to a few chunks/s, hiding any network effect.
- Nym mainnet is not a stable test bed: a shared gateway dropped all its sessions mid-run, and Docker Desktop's VM
  clock drifted 15 min (gateways reject skewed timestamps). The sidecar now reconnects and `up.sh` resyncs the clock,
  but a clean all-nodes-on-Nym run with the fork was never obtained.
- `nym-sdk` 1.21.6 from crates.io does not build; use the git dependency pinned in `sidecar/Cargo.toml` with Nym's
  lockfile. Its stream API has no close signal; the sidecar adds DATA/FIN framing.

## Paid bandwidth (branch `feat/nym-credentials`)

The runs above used Nym's free mode. With credentials mode the hidden client spends zk-nym ticketbooks instead.

| Hidden light node, p50 | Sandbox paid | Sandbox free | Mainnet paid | Mainnet free |
|---|---|---|---|---|
| Download 4 KB / 64 KB / 1 MB | 1.8 s / 10.9 s / 87 s | 1.6 s / 10.5 s / 83 s | 5.1 s / 35.8 s / failed | 4.2 s / 16–22 s (unreliable) / failed |
| Upload 4 KB / 64 KB / 1 MB | 1.5 s / 14.0 s / 101 s | 1.5 s / 9.7 s / 82 s | 12.5 s / 41 s / 231 s | 3.0 s / 20 s / 144 s |

- **Paying doesn't buy performance.** Paid and free are within noise on the sandbox. On mainnet paid was slower in
  every cell, but it used a different gateway at a different time with one run each, so that is mainnet variance, not
  an effect of paying. Paid 64 KB downloads were 5/5, free had one 1/5 run: too thin to claim better stability.
- **The sandbox is faster than mainnet regardless,** probably because it is less loaded, and it is the only setup where
  1 MB downloads worked.
- **Cost:** a ticketbook is about 1 NYM on the sandbox but **about 75 NYM on mainnet**. Buying one per sidecar on mainnet
  cost about 225 NYM for three before the run was stopped. Purchasing now requires an explicit `NYM_ALLOW_PURCHASE=1`;
  see the README.
- **Ticketbooks are portable.** The mainnet paid run reused a ticketbook bought by another sidecar by copying its
  credential database; the gateway accepted it ("managed to claim ecash bandwidth"). One mainnet ticketbook holds 50
  tickets of about 190 MiB (9.31 GiB) and expires about a week after purchase, so sharing one across sidecars beats
  buying one each.

## WireGuard 2-hop (branch `feat/nym-2hop`)

NymVPN's "fast mode": a WireGuard tunnel through an entry and an exit gateway to the public internet, with no mixing,
delays or cover traffic. It only carries outbound traffic, so it can only hide a client: bee-6 runs inside a containerised
NymVPN client (`nymvpn/`), and the full nodes must be publicly reachable (here via router port forwarding to
`PUBLIC_IP:31634–31638`). Stock Bee, mainnet gateways, the user's NymVPN subscription.

- **Performance is close to plain Swarm**: 4 KB in 0.2 s, 1 MB downloads in about 19 s, 1 MB uploads in about 6 s. Every
  transfer succeeded, including the 1 MB downloads that failed over the mainnet mixnet.
- **Privacy is much weaker than the mixnet**: peers see the exit gateway's IP instead of the user's, but there is no
  mixing, so an observer of both gateways can correlate traffic by timing, and the exit gateway sees which Swarm nodes the
  client talks to.
- **Practical caveats**: needs a NymVPN subscription per client; the full nodes need public addresses; NymVPN's routing
  breaks Docker's published ports on its own container (bee-6's API is relayed through a separate container).

## Benchmark summary

Median per cell, successes out of 5 in brackets where not 5/5. "Hidden node" is the light node bee-6; uploads in the
baseline and all-nodes rows come from full node bee-2.

| Setup | Down 4 KB | Down 64 KB | Down 1 MB | Up 4 KB | Up 64 KB | Up 1 MB |
|---|---|---|---|---|---|---|
| Baseline, plain libp2p | 0.01 s | 2.4 s | 16 s | 0.01 s | 0.03 s | 0.76 s |
| Baseline + fork | 0.01 s | 2.4 s | 18 s | 0.01 s | 0.03 s | 0.74 s |
| All nodes on mixnet, stock | 2.8 s | 28 s (2/5) | fail | 2.4 s | 3.7 s | mesh collapsed |
| Hidden node, mixnet, stock (mainnet) | 4.8 s | fail | fail | – | – | – |
| Hidden node, mixnet + fork, mainnet free | 4.2 s | 17–22 s (5/5, 1/5) | fail | 3.0 s | 20 s | 144 s |
| Hidden node, mixnet + fork, mainnet paid | 5.1 s | 36 s | fail | 12.5 s | 41 s | 231 s |
| Hidden node, mixnet + fork, sandbox free | 1.6 s | 10.5 s | 83 s | 1.5 s | 9.7 s | 82 s |
| Hidden node, mixnet + fork, sandbox paid | 1.8 s | 10.9 s | 87 s | 1.5 s | 14.0 s | 101 s |
| **Hidden node, 2-hop WireGuard, stock (mainnet)** | **0.2 s** | **2.6 s** | **19 s** | **0.2 s** | **0.9 s** | **6.3 s** |

The mixnet costs one to two orders of magnitude in latency and breaks large transfers on mainnet; 2-hop costs almost
nothing but gives up the mixing that hides who talks to whom.

## If this continues

- Repeat the edge runs with more repetitions; 64 KB download success varies a lot between runs.
- Split the fork patch to find the minimal change set (likely pseudosettle only) as the basis for an upstream
  discussion about latency-tolerant settlement.
- A local Nym network for repeatable runs, or gateway pinning on mainnet.
- Try Nym's send rate / cover traffic settings to quantify the privacy vs throughput trade-off.
