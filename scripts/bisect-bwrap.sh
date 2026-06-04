#!/usr/bin/env bash
set -e
base=(--ro-bind /usr /usr --ro-bind /bin /bin --ro-bind /lib /lib --dev /dev)
try() {
  echo "=== $*"
  if bwrap "${base[@]}" "$@" -- /bin/bash -c 'echo ok'; then
    echo OK
  else
    echo FAIL
  fi
}
try
try --proc /proc
try --share-net
try --unshare-all
try --tmpfs /tmp
try --unshare-all --share-net --proc /proc --tmpfs /tmp
