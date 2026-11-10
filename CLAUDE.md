# poc-bee-nym

Proof of concept: run a private Bee cluster where node-to-node traffic goes through the Nym mixnet instead of plain
libp2p transport, and measure how well Bee's protocols keep working.

## Why

Bee nodes connect directly over libp2p, so peers and network observers see IPs plus the timing and volume of
pushsync/retrieval traffic, which can link an IP to content. Nym (Loopix-style mixnet: Sphinx packets, per-hop random
delays, cover traffic) hides who talks to whom, even against a global passive adversary. Expected gains: IP privacy,
unlinkable uploads/downloads, censorship resistance, stronger PSS sender/receiver anonymity.

Trade-off: mixnet latency (often seconds) and cover-traffic overhead. The core question is whether Bee survives it.

## Decision: sidecar first (option 2)

Options considered:

1. SOCKS5 via `nym-socks5-client` — rejected: outbound only, exits to IPs, not representative.
2. **Sidecar bridge — chosen.** Unmodified Bee; a per-node sidecar carries TCP traffic over the mixnet. Latency and
   throughput are dominated by the mixnet, so results should be close to a native integration at a fraction of the
   effort.
3. Native go-libp2p transport backed by the Nym SDK — deferred. Most realistic but expensive (Bee is Go/go-libp2p, Nym
   SDK is Rust; only a Rust libp2p Nym transport is known, by ChainSafe). Only worth doing if option 2 looks promising.
   It is the only way to test BzzAddress/underlay handling with Nym addresses and libp2p stream multiplexing.

## Setup plan

- Each Bee node in its own container with a private IP `10.0.0.N` and `--allow-private-cidrs` (Bee rejects a private
  `nat-addr`; it advertises the listen address instead).
- Bee config only: private network ID, bootnodes, swap enabled (light nodes are throttled to a few chunks/s without it).
  No changes to the Bee repo.
- Per-node sidecar (small Rust program on the Nym SDK's TCP proxy):
  - iptables DNAT redirects outbound `10.0.0.0/24` traffic to the sidecar.
  - Sidecar maps destination IP → peer's Nym address and sends over the mixnet.
  - Receiving sidecar delivers to local Bee on `:1634`.
- Baseline: the same cluster with plain libp2p, no sidecar.

## Risks

- Hardcoded Bee timeouts (retrieval, pushsync, pricing, handshake) may be shorter than mixnet latency. Failure is itself
  a result; to test "would it work with longer timeouts", patch a Bee fork, never upstream.
- Bee may reject or rewrite private/observed underlay addresses (libp2p identify, NAT detection); may need config or a
  small fork patch.
- Nym SDK details (TCP proxy API, Go support) are from memory and must be verified against current Nym docs.

## Metrics (Nym vs baseline)

- Handshake and peering success rate
- Hive/Kademlia topology convergence
- Pushsync and retrieval latency and failure rate
- Throughput, timeouts, CPU and bandwidth overhead (including cover traffic)

## Next steps

1. Verify current Nym SDK capabilities (TCP proxy, stream support, gateway costs).
2. Build the sidecar.
3. Containerised private Bee cluster plus baseline run.
4. Run the measurements and decide whether option 3 is worth pursuing.
