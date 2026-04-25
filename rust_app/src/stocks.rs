use chrono::{DateTime, Duration, Utc};
use rusqlite::params;

use crate::models::Daily;
use crate::state::AppState;
use crate::yahoo::fetch_stock_data;
use crate::CACHE_MAX_AGE_SECS;

pub async fn get_stock_data(state: &AppState, symbol: &str) -> anyhow::Result<Vec<Daily>> {
    let need_refresh = {
        let conn = state.db.lock().await;
        let last: Option<String> = conn
            .query_row(
                "SELECT fetched_at FROM daily_data WHERE symbol = ? ORDER BY fetched_at DESC LIMIT 1",
                params![symbol],
                |r| r.get(0),
            )
            .ok();
        match last {
            None => true,
            Some(s) => match DateTime::parse_from_rfc3339(&s) {
                Ok(t) => Utc::now().signed_duration_since(t.with_timezone(&Utc))
                    >= Duration::seconds(CACHE_MAX_AGE_SECS),
                Err(_) => true,
            },
        }
    };

    if need_refresh {
        let _ = fetch_stock_data(state, symbol).await;
    }

    let conn = state.db.lock().await;
    let mut stmt = conn.prepare(
        "SELECT date, open, high, low, close, volume FROM daily_data WHERE symbol = ? ORDER BY date",
    )?;
    let rows = stmt.query_map(params![symbol], |r| {
        Ok(Daily {
            date: r.get(0)?,
            open: r.get(1)?,
            high: r.get(2)?,
            low: r.get(3)?,
            close: r.get(4)?,
            volume: r.get(5)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub async fn get_latest_price(state: &AppState, symbol: &str) -> Option<f64> {
    let data = get_stock_data(state, symbol).await.ok()?;
    data.last().and_then(|d| d.close)
}
