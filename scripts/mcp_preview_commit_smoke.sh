#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ $# -lt 2 ]]; then
  echo "Usage: $0 <thread-id> <ecky-source-file> [mcp-url]" >&2
  echo "Example: $0 thread-123 model-runtime/examples/film-scanning-adapter-helicoid.ecky http://127.0.0.1:39249/mcp" >&2
  exit 1
fi

THREAD_ID="$1"
SOURCE_FILE="$2"
MCP_URL="${3:-http://127.0.0.1:39249/mcp}"
GEOMETRY_BACKEND="${GEOMETRY_BACKEND:-build123d}"
VERSION_NAME="${VERSION_NAME:-MCP smoke $(date +%Y-%m-%dT%H:%M:%S)}"

if [[ ! -f "$SOURCE_FILE" ]]; then
  echo "Source file not found: $SOURCE_FILE" >&2
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl required" >&2
  exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
  echo "jq required" >&2
  exit 1
fi

"$ROOT/scripts/guard_no_direct_db_write.sh" || {
  echo "Direct DB write guard failed." >&2
  exit 1
}

SOURCE_CODE="$(cat "$SOURCE_FILE")"
SOURCE_JSON="$(printf '%s' "$SOURCE_CODE" | jq -Rs .)"

init_headers="$(mktemp)"
init_body="$(mktemp)"
trap 'rm -f "$init_headers" "$init_body"' EXIT

curl -sS \
  -D "$init_headers" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"mcp-smoke","version":"1.0"}}}' \
  "$MCP_URL" >"$init_body"

SESSION_ID="$(awk 'tolower($1) == "mcp-session-id:" {print $2}' "$init_headers" | tr -d '\r\n' | tail -n1)"
if [[ -z "$SESSION_ID" ]]; then
  echo "Mcp-Session-Id header missing from initialize response." >&2
  cat "$init_body" >&2
  exit 1
fi

rpc_call() {
  local id="$1"
  local tool_name="$2"
  local arguments_json="$3"
  curl -sS \
    -H 'Content-Type: application/json' \
    -H "Mcp-Session-Id: $SESSION_ID" \
    -d "{\"jsonrpc\":\"2.0\",\"id\":$id,\"method\":\"tools/call\",\"params\":{\"name\":\"$tool_name\",\"arguments\":$arguments_json}}" \
    "$MCP_URL"
}

borrow_resp="$(rpc_call 2 thread_borrow "{\"threadId\":$(jq -Rn --arg v "$THREAD_ID" '$v')}" )"
echo "$borrow_resp" | jq -e 'if .error then false else true end' >/dev/null

preview_args="$(jq -cn \
  --argjson macroCode "$SOURCE_JSON" \
  --arg backend "$GEOMETRY_BACKEND" \
  '{macroCode:$macroCode, geometryBackend:$backend}')"
preview_resp="$(rpc_call 3 macro_preview_render "$preview_args")"
echo "$preview_resp" | jq -e 'if .error then false else true end' >/dev/null

commit_args="$(jq -cn --arg versionName "$VERSION_NAME" '{versionName:$versionName}')"
commit_resp="$(rpc_call 4 commit_preview_version "$commit_args")"
echo "$commit_resp" | jq -e 'if .error then false else true end' >/dev/null

echo "Preview response:"
echo "$preview_resp" | jq '{result: .result, error: .error}'
echo
echo "Commit response:"
echo "$commit_resp" | jq '{result: .result, error: .error}'
