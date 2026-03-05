use rusqlite::{params, Connection, OptionalExtension, Result as SqlResult};
use crate::models::{Thread, Message, DesignOutput, ThreadReference};

pub fn init_db(db_path: &std::path::Path) -> SqlResult<Connection> {
    let conn = Connection::open(db_path)?;
    
    // Enable WAL mode for better concurrency and prevent "database is locked" errors
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;"
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS threads (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            summary TEXT NOT NULL DEFAULT '',
            updated_at INTEGER NOT NULL,
            genie_traits TEXT
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'success',
            output TEXT,
            timestamp INTEGER NOT NULL,
            image_data TEXT,
            FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS thread_references (
            id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            source_message_id TEXT,
            ordinal INTEGER NOT NULL DEFAULT 0,
            kind TEXT NOT NULL,
            name TEXT NOT NULL,
            content TEXT NOT NULL DEFAULT '',
            summary TEXT NOT NULL DEFAULT '',
            pinned INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL,
            FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_thread_references_source_ordinal_kind
         ON thread_references(source_message_id, ordinal, kind)",
        [],
    )?;
    
    // Migrations for existing databases
    let _ = conn.execute("ALTER TABLE threads ADD COLUMN summary TEXT NOT NULL DEFAULT ''", []);
    let _ = conn.execute("ALTER TABLE threads ADD COLUMN genie_traits TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN image_data TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN status TEXT NOT NULL DEFAULT 'success'", []);

    Ok(conn)
}

pub fn get_all_threads(conn: &Connection) -> SqlResult<Vec<Thread>> {
    let mut stmt = conn.prepare("
        SELECT id, title, summary, updated_at, genie_traits,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND output IS NOT NULL) as v_count
        FROM threads ORDER BY updated_at DESC
    ")?;
    let thread_iter = stmt.query_map([], |row| {
        let traits_str: Option<String> = row.get(4)?;
        let genie_traits: Option<serde_json::Value> = traits_str.and_then(|s| serde_json::from_str(&s).ok());
        Ok(Thread {
            id: row.get(0)?,
            title: row.get(1)?,
            summary: row.get(2)?,
            updated_at: row.get::<_, i64>(3)? as u64,
            messages: vec![], // Messages are now lazy-loaded
            genie_traits,
            version_count: row.get::<_, i64>(5)? as usize,
        })
    })?;

    let mut threads = Vec::new();
    for thread in thread_iter {
        threads.push(thread?);
    }
    Ok(threads)
}

pub fn create_or_update_thread(conn: &Connection, thread_id: &str, title: &str, updated_at: u64, genie_traits: Option<&serde_json::Value>) -> SqlResult<()> {
    let traits_str = genie_traits.and_then(|t| serde_json::to_string(t).ok());
    conn.execute(
        "INSERT INTO threads (id, title, updated_at, genie_traits) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET title=excluded.title, updated_at=excluded.updated_at, genie_traits=COALESCE(excluded.genie_traits, threads.genie_traits)",
        params![thread_id, title, updated_at as i64, traits_str],
    )?;
    Ok(())
}

pub fn update_thread_summary(conn: &Connection, thread_id: &str, summary: &str) -> SqlResult<()> {
    conn.execute(
        "UPDATE threads SET summary = ?1 WHERE id = ?2",
        params![summary, thread_id],
    )?;
    Ok(())
}

pub fn get_thread_title(conn: &Connection, thread_id: &str) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT title FROM threads WHERE id = ?1",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_thread_summary(conn: &Connection, thread_id: &str) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT summary FROM threads WHERE id = ?1",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn add_message(conn: &Connection, thread_id: &str, msg: &Message) -> SqlResult<()> {
    let output_str = msg.output.as_ref().and_then(|o| serde_json::to_string(o).ok());
    conn.execute(
        "INSERT INTO messages (id, thread_id, role, content, status, output, timestamp, image_data) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![msg.id, thread_id, msg.role, msg.content, msg.status, output_str, msg.timestamp as i64, msg.image_data],
    )?;
    Ok(())
}

pub fn add_thread_reference(conn: &Connection, reference: &ThreadReference) -> SqlResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO thread_references
         (id, thread_id, source_message_id, ordinal, kind, name, content, summary, pinned, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            reference.id,
            reference.thread_id,
            reference.source_message_id,
            reference.ordinal,
            reference.kind,
            reference.name,
            reference.content,
            reference.summary,
            if reference.pinned { 1 } else { 0 },
            reference.created_at as i64
        ],
    )?;
    Ok(())
}

pub fn get_thread_messages(conn: &Connection, thread_id: &str) -> SqlResult<Vec<Message>> {
    let mut stmt = conn.prepare("SELECT id, role, content, status, output, timestamp, image_data FROM messages WHERE thread_id = ? ORDER BY timestamp ASC")?;
    let msg_iter = stmt.query_map([thread_id], |row| {
        let output_str: Option<String> = row.get(4)?;
        let output: Option<DesignOutput> = output_str.and_then(|s| serde_json::from_str(&s).ok());
        Ok(Message {
            id: row.get(0)?,
            role: row.get(1)?,
            content: row.get(2)?,
            status: row.get(3)?,
            output,
            timestamp: row.get::<_, i64>(5)? as u64,
            image_data: row.get(6)?,
        })
    })?;
    
    let mut messages = Vec::new();
    for msg in msg_iter {
        messages.push(msg?);
    }
    Ok(messages)
}

pub fn get_thread_references(conn: &Connection, thread_id: &str) -> SqlResult<Vec<ThreadReference>> {
    let mut stmt = conn.prepare(
        "SELECT id, thread_id, source_message_id, ordinal, kind, name, content, summary, pinned, created_at
         FROM thread_references
         WHERE thread_id = ?1 AND pinned = 1
         ORDER BY created_at ASC, ordinal ASC"
    )?;
    let iter = stmt.query_map([thread_id], |row| {
        Ok(ThreadReference {
            id: row.get(0)?,
            thread_id: row.get(1)?,
            source_message_id: row.get(2)?,
            ordinal: row.get(3)?,
            kind: row.get(4)?,
            name: row.get(5)?,
            content: row.get(6)?,
            summary: row.get(7)?,
            pinned: row.get::<_, i64>(8)? != 0,
            created_at: row.get::<_, i64>(9)? as u64,
        })
    })?;
    let mut refs = Vec::new();
    for item in iter {
        refs.push(item?);
    }
    Ok(refs)
}

pub fn clear_history(conn: &Connection) -> SqlResult<()> {
    conn.execute("DELETE FROM threads", [])?;
    Ok(())
}

pub fn delete_thread(conn: &Connection, id: &str) -> SqlResult<()> {
    conn.execute("DELETE FROM threads WHERE id = ?", [id])?;
    Ok(())
}

pub fn delete_message(conn: &Connection, id: &str) -> SqlResult<()> {
    conn.execute("DELETE FROM messages WHERE id = ?", [id])?;
    // We also delete associated thread references that originated from this message
    conn.execute("DELETE FROM thread_references WHERE source_message_id = ?", [id])?;
    Ok(())
}

pub fn update_message_ui_spec(conn: &Connection, message_id: &str, ui_spec: &serde_json::Value) -> SqlResult<()> {
    let output_str: Option<String> = conn.query_row(
        "SELECT output FROM messages WHERE id = ?1",
        [message_id],
        |row| row.get(0),
    )?;

    if let Some(json_str) = output_str {
        if let Ok(mut output) = serde_json::from_str::<serde_json::Value>(&json_str) {
            output["ui_spec"] = ui_spec.clone();
            let updated = serde_json::to_string(&output).unwrap();
            conn.execute("UPDATE messages SET output = ?1 WHERE id = ?2", params![updated, message_id])?;
        }
    }
    Ok(())
}

pub fn update_message_parameters(conn: &Connection, message_id: &str, parameters: &serde_json::Value) -> SqlResult<()> {
    let output_str: Option<String> = conn.query_row(
        "SELECT output FROM messages WHERE id = ?1",
        [message_id],
        |row| row.get(0),
    )?;

    if let Some(json_str) = output_str {
        if let Ok(mut output) = serde_json::from_str::<serde_json::Value>(&json_str) {
            output["initial_params"] = parameters.clone();
            let updated = serde_json::to_string(&output).unwrap();
            conn.execute("UPDATE messages SET output = ?1 WHERE id = ?2", params![updated, message_id])?;
        }
    }
    Ok(())
}

pub fn get_message_output_and_thread(conn: &Connection, message_id: &str) -> SqlResult<Option<(DesignOutput, String)>> {
    let row: Option<(Option<String>, String)> = conn
        .query_row(
            "SELECT output, thread_id FROM messages WHERE id = ?1",
            [message_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    let Some((output_str, thread_id)) = row else {
        return Ok(None);
    };

    let Some(json_str) = output_str else {
        return Ok(None);
    };

    let Ok(output) = serde_json::from_str::<DesignOutput>(&json_str) else {
        return Ok(None);
    };

    Ok(Some((output, thread_id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn init_db_internal(conn: &Connection) -> SqlResult<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS threads (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                summary TEXT NOT NULL DEFAULT '',
                updated_at INTEGER NOT NULL,
                genie_traits TEXT
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'success',
                output TEXT,
                timestamp INTEGER NOT NULL,
                image_data TEXT,
                FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
            )",
            [],
        )?;
        Ok(())
    }

    #[test]
    fn test_update_ui_spec_and_params() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "test-thread";
        let msg_id = "test-msg";
        let now = 123456789;

        create_or_update_thread(&conn, thread_id, "Test Thread", now, None).unwrap();

        let initial_output = DesignOutput {
            title: "Test".to_string(),
            version_name: "V1".to_string(),
            response: "".to_string(),
            interaction_mode: "design".to_string(),
            macro_code: "print('hi')".to_string(),
            ui_spec: json!({"fields": []}),
            initial_params: json!({"x": 10}),
        };

        let msg = Message {
            id: msg_id.to_string(),
            role: "assistant".to_string(),
            content: "Hello".to_string(),
            status: "success".to_string(),
            output: Some(initial_output),
            timestamp: now,
            image_data: None,
        };

        add_message(&conn, thread_id, &msg).unwrap();

        // Update UI Spec
        let new_spec = json!({"fields": [{"key": "y", "type": "number"}]});
        update_message_ui_spec(&conn, msg_id, &new_spec).unwrap();

        // Update Params
        let new_params = json!({"x": 20, "y": 5});
        update_message_parameters(&conn, msg_id, &new_params).unwrap();

        // Verify
        let (output, tid) = get_message_output_and_thread(&conn, msg_id).unwrap().unwrap();
        assert_eq!(tid, thread_id);
        assert_eq!(output.ui_spec, new_spec);
        assert_eq!(output.initial_params, new_params);
    }
}
