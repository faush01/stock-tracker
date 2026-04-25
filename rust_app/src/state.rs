use std::sync::Arc;

use minijinja::Environment;
use rusqlite::Connection;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub env: Arc<Environment<'static>>,
    pub http: reqwest::Client,
}
