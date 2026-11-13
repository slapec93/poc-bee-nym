# Bee over Nym: results

Private Bee 2.8.2 cluster (5 full nodes, 1 light node "bee-6"). 5 runs per size; content verified on another node.

## Verdict

- **Full nodes on the mixnet: no.** A Nym client sends about 50 KB/s; full-node sync traffic needs far more, and stock
  Bee drops slow peers, so the mesh collapses. A native Nym transport wouldn't help: the limit is the mixnet.
- **Hiding only the user (light node) on the mixnet: partly.** Small content works with a Bee patch; 1 MB downloads fail
  on mainnet.
- **Hiding the user with WireGuard 2-hop: works well**, near plain-Swarm speed, but without mixing. Observers of both
  gateways can link the traffic.
- **Paying for Nym:** no performance gain.

## Results

Median per cell; successes out of 5 in brackets where not all.

| Setup | Down 4 KB | Down 64 KB | Down 1 MB | Up 4 KB | Up 64 KB | Up 1 MB |
|---|---|---|---|---|---|---|
| Plain Swarm | 0.01 s | 2.4 s | 16 s | 0.01 s | 0.03 s | 0.76 s |
| All nodes on mixnet | 2.8 s | 28 s (2/5) | fail | 2.4 s | 3.7 s | collapsed |
| Hidden user, mixnet, stock Bee | 4.8 s | fail | fail | – | – | – |
| Hidden user, mixnet, patched Bee | 4.2 s | 17–22 s (unreliable) | fail | 3.0 s | 20 s | 144 s |
| Same, paid | 5.1 s | 36 s | fail | 12.5 s | 41 s | 231 s |
| Same, sandbox (free / paid) | 1.6 / 1.8 s | 10.5 / 10.9 s | 83 / 87 s | 1.5 / 1.5 s | 9.7 / 14 s | 82 / 101 s |
| **Hidden user, 2-hop, stock Bee** | **0.2 s** | **2.6 s** | **19 s** | **0.2 s** | **0.9 s** | **6.3 s** |

"Hidden user" uploads and downloads go through bee-6. The patched Bee behaves like stock Bee on a plain network.

## Why the mixnet breaks Bee

1. **Payments:** a payment exchange must finish within 5 s with clocks agreeing to ±2 s, or Bee blocklists the peer.
   Mixnet delay trips this constantly. Relaxing it is what makes the hidden user work.
2. **Retries:** Bee retries a chunk request every 1 s, faster than a mixnet round trip (1–3 s).
3. **Bandwidth:** above the client's send rate, delay grows until connections time out (likely; not fully proven).

The test-only patch (`fork/`) relaxes these: payment timeout 5→30 s, clock tolerance ±30 s, retry 1→5 s, longer
handshake and chunk timeouts.

## Notes

- **2-hop limits:** outbound only, so the full nodes need public addresses (port forwarding); needs a NymVPN subscription
  per client.
- **Paid mode:** a mainnet ticketbook is about 75 NYM (sandbox about 1), lasts about a week and can be shared between
  clients. Paid and free performed the same; the slower mainnet-paid numbers reflect a different gateway and time.
- **Sandbox vs mainnet:** the sandbox is about twice as fast, likely because it is less loaded.

## Limitations

- Small local cluster, 5 runs per size; mainnet results vary a lot between runs.
- No clean "all nodes on mixnet + patched Bee" run, because of mainnet gateway failures.
- The patch was never split, so the minimal set of changes is unknown.
- Only the sidecar bridge was tested, not a native transport.

## Next steps

- More repetitions for the hidden-user mixnet runs.
- Find the minimal patch (likely payments only) and raise latency-tolerant settlement with the Bee team.
- A local Nym network for repeatable runs.
- Measure the privacy vs speed trade-off of Nym's send-rate and cover-traffic settings.
