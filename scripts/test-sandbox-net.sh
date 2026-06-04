#!/usr/bin/env bash
# Quick check: bwrap sandbox with network + package dirs (matches bobaclaw defaults).
set -euo pipefail
ROOT="$(mktemp -d)"
WS="$ROOT/ws"
RUN="$ROOT/run"
mkdir -p "$WS" "$RUN"
SANDBOX="$WS/.bobaclaw-sandbox"
for d in usr-local var-cache-apt var-lib-apt var-lib-dpkg home; do
  mkdir -p "$SANDBOX/$d"
done

BWRAP="${BWRAP:-/usr/bin/bwrap}"
command -v "$BWRAP" >/dev/null || BWRAP=bwrap

LIB64=()
if [[ -d /lib64 ]]; then
  LIB64=(--ro-bind /lib64 /lib64)
fi

exec "$BWRAP" \
  --unshare-all --die-with-parent --new-session \
  --ro-bind /usr /usr --ro-bind /bin /bin --ro-bind /lib /lib \
  "${LIB64[@]}" \
  --bind "$WS" /workspace --bind "$RUN" /capsule \
  --chdir /workspace --dev /dev \
  --share-net --ro-bind /etc/resolv.conf /etc/resolv.conf \
  --proc /proc --tmpfs /tmp \
  --bind "$SANDBOX/usr-local" /usr/local \
  --bind "$SANDBOX/var-cache-apt" /var/cache/apt \
  --bind "$SANDBOX/var-lib-apt" /var/lib/apt \
  --bind "$SANDBOX/var-lib-dpkg" /var/lib/dpkg \
  --bind "$SANDBOX/home" /home/sandbox \
  --ro-bind /etc/apt /etc/apt \
  --ro-bind /etc/passwd /etc/passwd \
  --ro-bind /etc/group /etc/group \
  --setenv HOME /home/sandbox \
  -- /bin/bash -lc '
    echo "=== curl ==="
    curl -fsS --max-time 15 https://example.com | head -c 120
    echo
    echo "=== pip ==="
    python3 -m pip --version
    echo "=== write usr-local ==="
    echo ok > /usr/local/.bobaclaw-write-test && cat /usr/local/.bobaclaw-write-test
  '

rm -rf "$ROOT"
echo "sandbox network + packages: OK"
