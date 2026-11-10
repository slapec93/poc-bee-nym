#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p state
docker run --rm --entrypoint cat ethersphere/bee-factory-blockchain:v2.8.2 /anvil-state.json | python3 -I -c '
import gzip, json, sys
d = json.load(sys.stdin)
sys.stdout.buffer.write(gzip.decompress(bytes.fromhex(d["state"][2:])))
' > state/anvil-state.json
echo "wrote $(pwd)/state/anvil-state.json"
