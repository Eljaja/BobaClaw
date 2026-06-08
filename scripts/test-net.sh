#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
WS=/tmp/bobaclaw-net-test
RUN=/tmp/bobaclaw-net-run
rm -rf "$WS" "$RUN"
mkdir -p "$WS" "$RUN"
bwrap --unshare-all --die-with-parent --new-session \
  --ro-bind /usr /usr --ro-bind /bin /bin --ro-bind /lib /lib \
  $(test -d /lib64 && echo --ro-bind /lib64 /lib64) \
  --bind "$WS" /workspace --bind "$RUN" /run --chdir /workspace --dev /dev \
  --share-net \
  --ro-bind /etc/resolv.conf /etc/resolv.conf \
  $(test -d /etc/ssl && echo --ro-bind /etc/ssl /etc/ssl) \
  -- /bin/bash -lc 'curl -sS --max-time 10 ifconfig.me; echo'
