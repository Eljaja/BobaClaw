#!/usr/bin/env bash
# stop: verify rustfmt before handoff; auto-follow-up when formatting drifts.
set -euo pipefail

json_input="$(cat)"
status="completed"

if command -v python3 >/dev/null 2>&1; then
  status="$(printf '%s' "$json_input" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("status","completed"))' 2>/dev/null || echo "completed")"
elif command -v jq >/dev/null 2>&1; then
  set +e
  status="$(printf '%s' "$json_input" | jq -r '.status // "completed"' 2>/dev/null)"
  set -e
fi

if [[ "$status" == "aborted" ]]; then
  printf '%s\n' '{}'
  exit 0
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
output_file="$(mktemp)"
set +e
"$script_dir/run-rust-fmt.sh" check >"$output_file" 2>&1
exit_code=$?
set -e
output="$(cat "$output_file")"
rm -f "$output_file"

if [[ "$exit_code" -eq 0 ]]; then
  printf '%s\n' '{}'
  exit 0
fi

printf '%s' "$output" | head -c 12000 | python3 -c '
import json, sys
out = sys.stdin.read()
msg = (
    "The **stop** hook ran `cargo fmt --all -- --check` after your last turn "
    "(same check as CI).\n\n"
    "**Result:** formatting drift detected.\n\n"
    "Run `cargo fmt --all` (or `make fmt` in WSL), then continue.\n\n"
    "```text\n" + out + "\n```\n"
)
print(json.dumps({"followup_message": msg}, ensure_ascii=False))
'
exit 0
