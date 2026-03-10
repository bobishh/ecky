use std::sync::Mutex;

pub use crate::contracts::*;

pub struct AppState {
    pub config: Mutex<Config>,
    pub last_snapshot: Mutex<Option<LastDesignSnapshot>>,
    pub db: tokio::sync::Mutex<rusqlite::Connection>,
    pub render_lock: tokio::sync::Mutex<()>,
}
