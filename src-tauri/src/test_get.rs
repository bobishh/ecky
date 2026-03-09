use rusqlite::Connection;
use crate::db;

pub fn run() {
    let db_path = std::path::Path::new("/Users/bogdan/Library/Application Support/com.alcoholics-audacious.ecky-cad/history.sqlite");
    let conn = Connection::open(db_path).unwrap();
    let threads = db::get_all_threads(&conn).unwrap();
    println!("Found {} threads", threads.len());
    for t in threads {
        println!("- {} ({} msg)", t.title, t.messages.len());
    }
}
