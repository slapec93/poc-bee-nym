# Bee over Nym: results

Private Bee 2.8.2 cluster (5 full nodes, 1 light node "bee-6") on Nym mainnet. 5 runs per size; content verified on
another node.

## Verdict

- **Full nodes on the mixnet: no.** A Nym client sends about 50 KB/s; full-node sync traffic needs far more, and stock
  Bee drops slow peers, so the mesh collapses. A native Nym transport wouldn't help: the limit is the mixnet.
- **Hiding only the user (light node): partly.** Small content works with a Bee patch; 1 MB downloads fail.

## Results

Median per cell; successes out of 5 in brackets where not all.

| Setup | Down 4 KB | Down 64 KB | Down 1 MB | Up 4 KB | Up 64 KB | Up 1 MB |
|---|---|---|---|---|---|---|
| Plain Swarm | 0.01 s | 2.4 s | 16 s | 0.01 s | 0.03 s | 0.76 s |
| All nodes on mixnet | 2.8 s | 28 s (2/5) | fail | 2.4 s | 3.7 s | collapsed |
| Hidden user, stock Bee | 4.8 s | fail | fail | – | – | – |
| Hidden user, patched Bee | 4.2 s | 17–22 s (unreliable) | fail | 3.0 s | 20 s | 144 s |

"Hidden user" uploads and downloads go through bee-6. The patched Bee behaves like stock Bee on a plain network.

## Why the mixnet breaks Bee

1. **Payments:** a payment exchange must finish within 5 s with clocks agreeing to ±2 s, or Bee blocklists the peer.
   Mixnet delay trips this constantly. Relaxing it is what makes the hidden user work.
2. **Retries:** Bee retries a chunk request every 1 s, faster than a mixnet round trip (1–3 s).
3. **Bandwidth:** above the client's send rate, delay grows until connections time out (likely; not fully proven).

The test-only patch (`fork/`) relaxes these: payment timeout 5→30 s, clock tolerance ±30 s, retry 1→5 s, longer
handshake and chunk timeouts.

## Limitations

- Small local cluster, 5 runs per size; mainnet results vary a lot between runs.
- No clean "all nodes on mixnet + patched Bee" run, because of mainnet gateway failures.
- The patch was never split, so the minimal set of changes is unknown.
- Only the sidecar bridge was tested, not a native transport.

## Next steps

- More repetitions for the hidden-user runs.
- Find the minimal patch (likely payments only) and raise latency-tolerant settlement with the Bee team.
- A local Nym network for repeatable runs.
- Measure the privacy vs speed trade-off of Nym's send-rate and cover-traffic settings.
