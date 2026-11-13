#!/bin/sh
set -eu

SUBNET=${SUBNET:-10.0.0.0/24}
BEE_P2P_PORT=${BEE_P2P_PORT:-1634}

# leak guard: bee-6 may reach full nodes only via their public address through the tunnel
iptables -A OUTPUT -p tcp -d "$SUBNET" --dport "$BEE_P2P_PORT" -j DROP

nym-vpnd -v run-with-args &
daemon=$!

until nym-vpnc status >/dev/null 2>&1; do sleep 1; done
nym-vpnc tunnel set --two-hop on
nym-vpnc tunnel set --ipv6 off
nym-vpnc lan set allow

if ! nym-vpnc account get | grep -q "LoggedOut"; then
  nym-vpnc connect --wait || echo "connect failed; retry with: docker exec <container> nym-vpnc connect --wait"
else
  echo "no account; log in with: docker exec -it <container> nym-vpnc account set \"<mnemonic>\""
fi

wait "$daemon"
