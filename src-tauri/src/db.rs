use rusqlite::{params, Connection, Result as SqlResult};
use crate::models::{Thread, Message, DesignOutput};

pub fn init_db(db_path: &std::path::Path) -> SqlResult<Connection> {
    let conn = Connection::open(db_path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS threads (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            output TEXT,
            timestamp INTEGER NOT NULL,
            image_data TEXT,
            FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
        )",
        [],
    )?;
    // Attempt to add column for existing databases. Ignore error if it already exists.
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN image_data TEXT", []);

    Ok(conn)
}

pub fn get_all_threads(conn: &Connection) -> SqlResult<Vec<Thread>> {
    let mut stmt = conn.prepare("SELECT id, title, updated_at FROM threads ORDER BY updated_at DESC")?;
    let thread_iter = stmt.query_map([], |row| {
        Ok(Thread {
            id: row.get(0)?,
            title: row.get(1)?,
            updated_at: row.get::<_, i64>(2)? as u64,
            messages: vec![],
        })
    })?;

    let mut threads = Vec::new();
    for thread in thread_iter {
        let mut t = thread?;
        let mut msg_stmt = conn.prepare("SELECT id, role, content, output, timestamp, image_data FROM messages WHERE thread_id = ? ORDER BY timestamp ASC")?;
        let msg_iter = msg_stmt.query_map([&t.id], |row| {
            let output_str: Option<String> = row.get(3)?;
            let output: Option<DesignOutput> = output_str.and_then(|s| serde_json::from_str(&s).ok());
            Ok(Message {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                output,
                timestamp: row.get::<_, i64>(4)? as u64,
                image_data: row.get(5)?,
            })
        })?;
        for msg in msg_iter {
            t.messages.push(msg?);
        }
        threads.push(t);
    }
    Ok(threads)
}

pub fn create_or_update_thread(conn: &Connection, thread_id: &str, title: &str, updated_at: u64) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO threads (id, title, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET title=excluded.title, updated_at=excluded.updated_at",
        params![thread_id, title, updated_at as i64],
    )?;
    Ok(())
}

pub fn add_message(conn: &Connection, thread_id: &str, msg: &Message) -> SqlResult<()> {
    let output_str = msg.output.as_ref().and_then(|o| serde_json::to_string(o).ok());
    conn.execute(
        "INSERT INTO messages (id, thread_id, role, content, output, timestamp, image_data) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![msg.id, thread_id, msg.role, msg.content, output_str, msg.timestamp as i64, msg.image_data],
    )?;
    Ok(())
}

pub fn get_thread_messages(conn: &Connection, thread_id: &str) -> SqlResult<Vec<Message>> {
    let mut stmt = conn.prepare("SELECT id, role, content, output, timestamp, image_data FROM messages WHERE thread_id = ? ORDER BY timestamp ASC")?;
    let msg_iter = stmt.query_map([thread_id], |row| {
        let output_str: Option<String> = row.get(3)?;
        let output: Option<DesignOutput> = output_str.and_then(|s| serde_json::from_str(&s).ok());
        Ok(Message {
            id: row.get(0)?,
            role: row.get(1)?,
            content: row.get(2)?,
            output,
            timestamp: row.get::<_, i64>(4)? as u64,
            image_data: row.get(5)?,
        })
    })?;
    
    let mut messages = Vec::new();
    for msg in msg_iter {
        messages.push(msg?);
    }
    Ok(messages)
}

pub fn clear_history(conn: &Connection) -> SqlResult<()> {
    conn.execute("DELETE FROM threads", [])?;
    // CASCADE will delete messages
    Ok(())
}

pub fn delete_thread(conn: &Connection, id: &str) -> SqlResult<()> {
    conn.execute("DELETE FROM threads WHERE id = ?", [id])?;
    Ok(())
}