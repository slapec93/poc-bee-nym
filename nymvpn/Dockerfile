FROM debian:trixie-slim
ARG VERSION=2026.15.0
ARG VPND_SHA256=cdf638426987708a4193444918eb5ad699c0f15a8fa77c7529d8749f93b4ab40
ARG VPNC_SHA256=f991821ce51652ee6094b4226eaf93d5aa906699ec0ec4f1f489678bf544bbb3
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl iproute2 iptables nftables libdbus-1-3 libmnl0 libnftnl11 \
 && base=https://github.com/nymtech/nym-vpn-client/releases/download/nym-vpn-v${VERSION} \
 && curl -fsSL -o /tmp/vpnd.deb ${base}/nym-vpnd_${VERSION}_arm64.deb \
 && curl -fsSL -o /tmp/vpnc.deb ${base}/nym-vpnc_${VERSION}_arm64.deb \
 && echo "${VPND_SHA256}  /tmp/vpnd.deb" | sha256sum -c - \
 && echo "${VPNC_SHA256}  /tmp/vpnc.deb" | sha256sum -c - \
 && dpkg-deb -x /tmp/vpnd.deb / && dpkg-deb -x /tmp/vpnc.deb / \
 && rm -rf /tmp/*.deb /var/lib/apt/lists/*
COPY entrypoint.sh /entrypoint.sh
ENTRYPOINT ["/entrypoint.sh"]
