mod db;
mod error;
mod models;
mod routes;
mod state;
mod stocks;
mod templates;
mod yahoo;

use std::sync::Arc;

use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::state::AppState;

pub(crate) const CACHE_MAX_AGE_SECS: i64 = 60 * 60; // 1 hour

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let conn = Connection::open(db::db_path())?;
    db::init_db(&conn)?;

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        env: Arc::new(templates::build_env()),
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?,
    };

    let app = routes::build_router(state);

    println!(
        "stock-tracker (Rust) v{} starting",
        env!("CARGO_PKG_VERSION")
    );
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 5000));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
