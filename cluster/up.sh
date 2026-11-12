#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

COMPOSE_FILE=docker-compose.yml
while [ $# -gt 0 ]; do
  case "$1" in
    nym) COMPOSE_FILE+=:docker-compose.nym.yml ;;
    edge) COMPOSE_FILE+=:docker-compose.edge.yml ;;
    fork) COMPOSE_FILE+=:docker-compose.fork.yml ;;
    sandbox) COMPOSE_FILE+=:docker-compose.sandbox.yml ;;
    paid) COMPOSE_FILE+=:docker-compose.paid.yml ;;
    buy) COMPOSE_FILE+=:docker-compose.buy.yml ;;
    *) break ;;
  esac
  shift
done
export COMPOSE_FILE

DEPLOYER_KEY=0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
BZZ_TOKEN=0x5FbDB2315678afecb367f032d93F642f64180aa3
PORTS="21633 21635 21637 21639 21641 21643"

[ -f state/anvil-state.json ] || ./extract-state.sh

vm_skew() { echo $(( $(docker run --rm alpine date +%s) - $(date +%s) )); }
skew=$(vm_skew)
if [ "${skew#-}" -gt 5 ]; then
  echo "docker VM clock skewed by ${skew}s, resyncing"
  docker run --rm --privileged alpine hwclock -s
  skew=$(vm_skew)
  [ "${skew#-}" -le 5 ] || { echo "docker VM clock still skewed by ${skew}s" >&2; exit 1; }
fi

t0=$(date +%s)
docker compose up -d --wait "$@"

until addr=$(curl -sf localhost:21643/addresses | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["ethereum"])' 2>/dev/null); do sleep 1; done
balance=$(docker compose exec -T anvil cast balance "$addr")
if [ "$balance" = 0 ]; then
  echo "funding light node $addr"
  docker compose exec -T anvil cast send -q --private-key $DEPLOYER_KEY --value 1ether "$addr"
  docker compose exec -T anvil cast send -q --private-key $DEPLOYER_KEY $BZZ_TOKEN "mint(address,uint256)" "$addr" 1000000000000000000
fi

peers() { curl -sf "localhost:$1/peers" | python3 -I -c 'import json,sys; print(len(json.load(sys.stdin)["peers"] or []))' 2>/dev/null || echo 0; }
until [ "$(curl -s -o /dev/null -w '%{http_code}' localhost:21643/readiness)" = 200 ]; do sleep 1; done
MIN_PEERS=${MIN_PEERS:-4}
deadline=$(( $(date +%s) + ${MESH_TIMEOUT:-600} ))
meshed() { for p in $PORTS; do [ "$(peers $p)" -ge "$MIN_PEERS" ] || return 1; done; }
until meshed || [ "$(date +%s)" -ge "$deadline" ]; do sleep 1; done

if meshed; then
  echo "cluster ready, full mesh after $(( $(date +%s) - t0 ))s (>= $MIN_PEERS peers each)"
else
  echo "cluster up, mesh incomplete after $(( $(date +%s) - t0 ))s"
fi
for p in $PORTS; do echo "  :$p peers=$(peers $p)"; done
