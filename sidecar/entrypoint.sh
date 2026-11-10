#!/bin/sh
set -eu

SUBNET=${SUBNET:-10.0.0.0/24}
BEE_P2P_PORT=${BEE_P2P_PORT:-1634}
REDIRECT_PORT=${REDIRECT_PORT:-4001}

rm -f "/peers/$NODE_IP"

if [ -z "${INGRESS_ONLY:-}" ]; then
  iptables -t nat -A OUTPUT -p tcp -d "$SUBNET" --dport "$BEE_P2P_PORT" -j REDIRECT --to-ports "$REDIRECT_PORT"
  # leak guard: Bee peers must only arrive through the mixnet
  iptables -A INPUT -p tcp -s "$SUBNET" --dport "$BEE_P2P_PORT" -j DROP
fi

exec nym-sidecar
