#!/usr/bin/env bash
# Test-only Bee build with mixnet-tolerant timeouts; never upstream.
set -euo pipefail
cd "$(dirname "$0")"

TAG=v2.8.2

if [ ! -d .src ]; then
  git clone -q --depth 1 --branch "$TAG" https://github.com/ethersphere/bee.git .src
fi
git -C .src checkout -q -- .
git -C .src apply ../bee-nym-timeouts.patch

mkdir -p bin
docker run --rm -v "$PWD/.src":/src -v "$PWD/bin":/out -v bee-fork-gomod:/go/pkg/mod -v bee-fork-gocache:/root/.cache/go-build -w /src \
  golang:1.26 sh -c 'git config --global --add safe.directory /src && make binary VERSION=2.8.2 REACHABILITY_OVERRIDE_PUBLIC=true && cp dist/bee /out/bee'

echo "built $(pwd)/bin/bee"
