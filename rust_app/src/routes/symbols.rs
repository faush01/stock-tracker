use std::collections::{HashMap, HashSet};

use axum::extract::{Form, Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Json;
use chrono::{Duration, Local};
use minijinja::context;
use rusqlite::params;
use serde::Deserialize;

use crate::error::{valid_symbol, AppError};
use crate::models::{DayChange, SymbolRow};
use crate::state::AppState;
use crate::stocks::get_stock_data;
use crate::templates::render;
use crate::yahoo::fetch_stock_data;

pub async fn index(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let symbols: Vec<String> = {
        let conn = state.db.lock().await;
        let mut stmt = conn.prepare("SELECT symbol FROM symbols ORDER BY symbol")?;
        let it = stmt.query_map([], |r| r.get::<_, String>(0))?;
        it.collect::<Result<_, _>>()?
    };

    // refresh data for all symbols
    for sym in &symbols {
        let _ = get_stock_data(&state, sym).await;
    }

    // build per-symbol changes keyed by date (last 5 changes)
    let mut raw: HashMap<String, HashMap<String, (Option<f64>, Option<f64>)>> = HashMap::new();
    let mut all_dates: HashSet<String> = HashSet::new();

    {
        let conn = state.db.lock().await;
        for sym in &symbols {
            let mut stmt = conn.prepare(
                "SELECT date, close FROM daily_data WHERE symbol = ? ORDER BY date DESC LIMIT 14",
            )?;
            let rows: Vec<(String, Option<f64>)> = stmt
                .query_map(params![sym], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;

            let mut changes: HashMap<String, (Option<f64>, Option<f64>)> = HashMap::new();
            for i in 0..rows.len().saturating_sub(1) {
                if i >= 5 {
                    break;
                }
                let prev_close = rows[i + 1].1;
                let cur_close = rows[i].1;
                let pct = match (prev_close, cur_close) {
                    (Some(p), Some(c)) if p != 0.0 => Some((c - p) / p * 100.0),
                    _ => None,
                };
                changes.insert(rows[i].0.clone(), (pct, cur_close));
                all_dates.insert(rows[i].0.clone());
            }
            raw.insert(sym.clone(), changes);
        }
    }

    // build last 7 calendar days
    let today = Local::now().date_naive();
    let dates: Vec<String> = (0..7)
        .rev()
        .map(|i| (today - Duration::days(i)).format("%Y-%m-%d").to_string())
        .collect();

    let symbol_data: Vec<SymbolRow> = symbols
        .iter()
        .map(|sym| {
            let m = raw.get(sym).cloned().unwrap_or_default();
            let changes = dates
                .iter()
                .map(|d| {
                    let (pct, close) = m.get(d).cloned().unwrap_or((None, None));
                    DayChange {
                        date: d.clone(),
                        pct,
                        close,
                    }
                })
                .collect();
            SymbolRow {
                symbol: sym.clone(),
                changes,
            }
        })
        .collect();

    render(&state.env, "list.html", context! { symbols => symbol_data })
}

#[derive(Deserialize)]
pub struct SymbolForm {
    symbol: Option<String>,
}

pub async fn add_symbol(
    State(state): State<AppState>,
    Form(form): Form<SymbolForm>,
) -> Result<Redirect, AppError> {
    let symbol = form.symbol.unwrap_or_default().trim().to_uppercase();
    if !valid_symbol(&symbol) {
        return Ok(Redirect::to("/"));
    }
    {
        let conn = state.db.lock().await;
        let _ = conn.execute("INSERT INTO symbols (symbol) VALUES (?)", params![symbol]);
    }
    let _ = fetch_stock_data(&state, &symbol).await;
    Ok(Redirect::to("/"))
}

pub async fn delete_symbol(
    State(state): State<AppState>,
    Path(symbol): Path<String>,
) -> Result<Redirect, AppError> {
    let symbol = symbol.to_uppercase();
    let conn = state.db.lock().await;
    conn.execute("DELETE FROM symbols WHERE symbol = ?", params![symbol])?;
    conn.execute("DELETE FROM daily_data WHERE symbol = ?", params![symbol])?;
    Ok(Redirect::to("/"))
}

pub async fn api_symbol_data(
    State(state): State<AppState>,
    Path(symbol): Path<String>,
) -> Result<Response, AppError> {
    let symbol = symbol.to_uppercase();
    let exists: bool = {
        let conn = state.db.lock().await;
        conn.query_row(
            "SELECT 1 FROM symbols WHERE symbol = ?",
            params![symbol],
            |_| Ok(true),
        )
        .unwrap_or(false)
    };
    if !exists {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let data = get_stock_data(&state, &symbol).await?;
    Ok(Json(data).into_response())
}

pub async fn detail(
    State(state): State<AppState>,
    Path(symbol): Path<String>,
) -> Result<Response, AppError> {
    let symbol = symbol.to_uppercase();
    let exists: bool = {
        let conn = state.db.lock().await;
        conn.query_row(
            "SELECT 1 FROM symbols WHERE symbol = ?",
            params![symbol],
            |_| Ok(true),
        )
        .unwrap_or(false)
    };
    if !exists {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    Ok(render(&state.env, "detail.html", context! { symbol => symbol })?.into_response())
}
