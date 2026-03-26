#!/usr/bin/env python3

from __future__ import annotations

import argparse
import shutil
import sqlite3
from pathlib import Path


DEFAULT_SOURCE = Path.home() / "Library/Application Support/com.alcoholics-audacious.ecky-cad/history.sqlite"
DEFAULT_DEST = Path(__file__).resolve().parents[1] / "src-tauri/tests/fixtures/mcp_regression_fixture.sqlite"
DEFAULT_THREADS = [
    "29c64fc4-803b-4d75-bac0-e0f656304881",  # Panelka Constructor
    "5c45fa6b-7457-4722-870d-dbc63e7e02fb",  # Reinforced Tie-Down Ears
    "4453c35d-b7c6-4aa8-8577-c1d8cae3697d",  # Pot frame
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build a trimmed MCP regression SQLite fixture.")
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--dest", type=Path, default=DEFAULT_DEST)
    parser.add_argument(
        "--thread",
        dest="threads",
        action="append",
        help="Thread id to keep in the fixture. Can be repeated.",
    )
    return parser.parse_args()


def sql_in_placeholders(count: int) -> str:
    return ",".join("?" for _ in range(count))


def main() -> int:
    args = parse_args()
    threads = args.threads or DEFAULT_THREADS
    if not args.source.exists():
        raise SystemExit(f"Source database does not exist: {args.source}")

    args.dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(args.source, args.dest)

    conn = sqlite3.connect(args.dest)
    try:
        conn.execute("PRAGMA foreign_keys = OFF")
        thread_placeholders = sql_in_placeholders(len(threads))

        keep_sessions = {
            row[0]
            for row in conn.execute(
                f"SELECT DISTINCT session_id FROM agent_sessions WHERE thread_id IN ({thread_placeholders})",
                threads,
            )
        }
        keep_sessions.update(
            row[0]
            for row in conn.execute(
                f"SELECT DISTINCT session_id FROM agent_session_trace WHERE thread_id IN ({thread_placeholders})",
                threads,
            )
        )

        conn.execute(
            f"DELETE FROM messages WHERE thread_id NOT IN ({thread_placeholders})",
            threads,
        )
        conn.execute(
            f"DELETE FROM thread_references WHERE thread_id NOT IN ({thread_placeholders})",
            threads,
        )
        conn.execute(
            f"DELETE FROM target_leases WHERE thread_id NOT IN ({thread_placeholders})",
            threads,
        )

        if keep_sessions:
            session_placeholders = sql_in_placeholders(len(keep_sessions))
            keep_session_values = list(keep_sessions)
            conn.execute(
                f"DELETE FROM agent_sessions WHERE session_id NOT IN ({session_placeholders})",
                keep_session_values,
            )
            conn.execute(
                f"DELETE FROM agent_session_trace WHERE session_id NOT IN ({session_placeholders})",
                keep_session_values,
            )
        else:
            conn.execute("DELETE FROM agent_sessions")
            conn.execute("DELETE FROM agent_session_trace")

        conn.execute(
            f"DELETE FROM threads WHERE id NOT IN ({thread_placeholders})",
            threads,
        )

        # Strip heavyweight user screenshots from the fixture while keeping CAD history intact.
        conn.execute("UPDATE messages SET image_data = NULL WHERE image_data IS NOT NULL")

        conn.commit()
        conn.execute("VACUUM")
    finally:
        conn.close()

    print(f"Wrote trimmed MCP fixture to {args.dest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
