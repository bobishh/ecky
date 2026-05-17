#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v rg >/dev/null 2>&1; then
  echo "rg required for direct DB write guard" >&2
  exit 1
fi

matches="$(
  rg -n --no-heading -S \
    -g '!scripts/build_mcp_regression_fixture.py' \
    -g '!scripts/guard_no_direct_db_write.sh' \
    -e 'sqlite3\.connect' \
    -e 'INSERT INTO' \
    -e 'UPDATE\s+[A-Za-z_]+' \
    "$ROOT/scripts" || true
)"

if [[ -n "$matches" ]]; then
  echo "WARNING: potential direct DB write in dev scripts. Use MCP tools instead." >&2
  echo "$matches" >&2
  exit 2
fi

echo "DB write guard passed: no direct SQLite write patterns in scripts/."
