#!/usr/bin/env bash
# Ensure ~/.bobaclaw/config.yaml has executor.network and sandbox_packages enabled.
set -euo pipefail
CFG="${BOBACLAW_HOME:-$HOME/.bobaclaw}/config.yaml"
mkdir -p "$(dirname "$CFG")"
if [[ ! -f "$CFG" ]]; then
  cat >"$CFG" <<'EOF'
default_agent_group: home
executor:
  network: true
  sandbox_packages: true
EOF
  echo "created $CFG"
  exit 0
fi
if grep -q 'sandbox_packages' "$CFG"; then
  sed -i 's/^\([[:space:]]*network:\).*/\1 true/' "$CFG" 2>/dev/null || true
  sed -i 's/^\([[:space:]]*sandbox_packages:\).*/\1 true/' "$CFG" 2>/dev/null || true
else
  sed -i '/^executor:/a\  sandbox_packages: true' "$CFG" 2>/dev/null || {
    printf '\nexecutor:\n  network: true\n  sandbox_packages: true\n' >>"$CFG"
  }
fi
if ! grep -q '^executor:' "$CFG"; then
  printf '\nexecutor:\n  network: true\n  sandbox_packages: true\n' >>"$CFG"
fi
if ! grep -q 'network:' "$CFG"; then
  sed -i '/^executor:/a\  network: true' "$CFG"
fi
echo "Updated $CFG — executor section:"
grep -A3 '^executor:' "$CFG" || true
