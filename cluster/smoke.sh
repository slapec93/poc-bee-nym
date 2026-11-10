#!/usr/bin/env bash
set -euo pipefail

UPLOADER=${UPLOADER:-http://localhost:21635}
DOWNLOADER=${DOWNLOADER:-http://localhost:21641}
SIZE=${SIZE:-1048576}

json() { python3 -I -c "import json,sys; print(json.load(sys.stdin)$1)"; }
now() { python3 -I -c 'import time; print(time.time())'; }
since() { python3 -I -c "import time; print(f'{time.time()-$1:.2f}s')"; }

batch=$(curl -sf "$UPLOADER/stamps" | python3 -I -c 'import json,sys; s=[b for b in json.load(sys.stdin)["stamps"] if b["usable"]]; print(s[0]["batchID"] if s else "")')
if [ -z "$batch" ]; then
  echo "buying postage batch on $UPLOADER"
  batch=$(curl -sf -X POST "$UPLOADER/stamps/100000000/20" | json '["batchID"]')
  until [ "$(curl -sf "$UPLOADER/stamps/$batch" | json '["usable"]')" = "True" ]; do sleep 1; done
fi
echo "batch $batch"

file=$(mktemp)
trap 'rm -f "$file" "$file.out"' EXIT
head -c "$SIZE" /dev/urandom > "$file"

t=$(now)
ref=$(curl -sf -X POST "$UPLOADER/bytes" \
  -H "swarm-postage-batch-id: $batch" -H "swarm-deferred-upload: false" \
  -H "content-type: application/octet-stream" --data-binary @"$file" | json '["reference"]')
echo "upload ${SIZE}B (pushsync) $(since "$t") ref=$ref"

t=$(now)
curl -sf "$DOWNLOADER/bytes/$ref" -o "$file.out"
echo "download from $DOWNLOADER $(since "$t")"

cmp -s "$file" "$file.out" && echo "OK content matches" || { echo "FAIL content mismatch"; exit 1; }
