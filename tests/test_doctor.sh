#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Unknown section is rejected
"$ROOT/bin/cc-doctor" unknown-section 2>/dev/null && { echo "should reject unknown section"; exit 1; } || true

# --help exits 0
"$ROOT/bin/cc-doctor" --help >/dev/null || { echo "--help should exit 0"; exit 1; }

echo "ok"
