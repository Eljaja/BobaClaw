#!/usr/bin/env bash
# afterFileEdit: auto-format Rust after agent edits a .rs file.
set -euo pipefail

json_input="$(cat)"
file_path=""

if command -v python3 >/dev/null 2>&1; then
  file_path="$(printf '%s' "$json_input" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("file_path",""))' 2>/dev/null || true)"
elif command -v jq >/dev/null 2>&1; then
  set +e
  file_path="$(printf '%s' "$json_input" | jq -r '.file_path // ""' 2>/dev/null)"
  set -e
fi

if [[ -z "$file_path" ]] || [[ ! "$file_path" =~ \.rs$ ]]; then
  printf '%s\n' '{}'
  exit 0
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if ! "$script_dir/run-rust-fmt.sh" format >&2; then
  echo "rust-fmt-after-edit: cargo fmt failed for $file_path" >&2
fi

printf '%s\n' '{}'
exit 0
