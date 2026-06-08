#!/usr/bin/env bash
# Run cargo fmt in the BobaClaw workspace root.
# Usage: run-rust-fmt.sh [format|check]
set -euo pipefail

mode="${1:-format}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

fmt_args=(--all)
if [[ "$mode" == "check" ]]; then
  fmt_args+=(-- --check)
fi

run_local() {
  (cd "$repo_root" && cargo fmt "${fmt_args[@]}")
}

run_wsl() {
  local wsl_root
  wsl_root="$(wsl wslpath -u "$repo_root")"
  wsl -e bash -lc "cd '$wsl_root' && cargo fmt ${fmt_args[*]}"
}

if command -v cargo >/dev/null 2>&1; then
  run_local
  exit $?
fi

if command -v wsl >/dev/null 2>&1; then
  run_wsl
  exit $?
fi

echo "run-rust-fmt: cargo not found (install Rust or use WSL)" >&2
exit 127
