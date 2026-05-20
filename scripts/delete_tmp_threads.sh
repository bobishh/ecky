#!/usr/bin/env bash
set -euo pipefail

DB="${1:-/Users/bogdan/Library/Application Support/com.alcoholics-audacious.ecky-cad/history.sqlite}"

if [[ ! -f "$DB" ]]; then
  echo "DB not found: $DB" >&2
  exit 1
fi

TMP_IDS="$(sqlite3 -noheader "$DB" <<'SQL'
SELECT id
FROM threads
WHERE title LIKE 'TMP%'
   OR title LIKE 'Fan holder native MCP test%';
SQL
)"

if [[ -z "${TMP_IDS//$'\n'/}" ]]; then
  echo "No matching junk threads."
  exit 0
fi

echo "Deleting thread ids:"
printf '%s\n' "$TMP_IDS"

IN_LIST="$(printf "%s\n" "$TMP_IDS" | sed "s/'/''/g; s/.*/'&'/" | paste -sd, -)"

sqlite3 "$DB" <<SQL
PRAGMA foreign_keys=OFF;
BEGIN;

DELETE FROM target_leases
WHERE thread_id IN ($IN_LIST);

DELETE FROM agent_drafts
WHERE thread_id IN ($IN_LIST);

DELETE FROM agent_sessions
WHERE thread_id IN ($IN_LIST);

DELETE FROM thread_references
WHERE thread_id IN ($IN_LIST);

DELETE FROM thread_window_layouts
WHERE thread_id IN ($IN_LIST);

DELETE FROM messages
WHERE thread_id IN ($IN_LIST);

DELETE FROM threads
WHERE id IN ($IN_LIST);

COMMIT;
PRAGMA wal_checkpoint(FULL);
VACUUM;
SQL

echo "Done."
