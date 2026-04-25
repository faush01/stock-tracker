use std::collections::HashMap;

use axum::extract::{Form, Path, State};
use axum::response::{Html, Redirect};
use minijinja::context;
use rusqlite::params;
use serde::Deserialize;

use crate::error::{valid_symbol, AppError};
use crate::models::{PortfolioEntry, PortfolioSummary};
use crate::state::AppState;
use crate::stocks::get_latest_price;
use crate::templates::render;
use crate::yahoo::fetch_stock_data;

pub async fn portfolio(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let mut entries: Vec<PortfolioEntry> = {
        let conn = state.db.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, symbol, shares, price_paid FROM portfolio ORDER BY symbol, created_at",
        )?;
        let it = stmt.query_map([], |r| {
            Ok(PortfolioEntry {
                id: r.get(0)?,
                symbol: r.get(1)?,
                shares: r.get(2)?,
                price_paid: r.get(3)?,
                current_price: None,
                cost: 0.0,
                value: None,
                pl: None,
                pl_pct: None,
            })
        })?;
        it.collect::<Result<_, _>>()?
    };

    let mut price_cache: HashMap<String, Option<f64>> = HashMap::new();
    let mut total_cost = 0.0_f64;
    let mut total_value = 0.0_f64;

    for entry in &mut entries {
        let cur = if let Some(p) = price_cache.get(&entry.symbol) {
            *p
        } else {
            let p = get_latest_price(&state, &entry.symbol).await;
            price_cache.insert(entry.symbol.clone(), p);
            p
        };
        entry.current_price = cur;
        let cost = entry.shares * entry.price_paid;
        entry.cost = cost;
        total_cost += cost;
        if let Some(price) = cur {
            let value = entry.shares * price;
            entry.value = Some(value);
            let pl = value - cost;
            entry.pl = Some(pl);
            entry.pl_pct = Some(if cost != 0.0 { pl / cost * 100.0 } else { 0.0 });
            total_value += value;
        }
    }

    let total_pl = total_value - total_cost;
    let total_pl_pct = if total_cost != 0.0 {
        total_pl / total_cost * 100.0
    } else {
        0.0
    };
    let summary = PortfolioSummary {
        total_cost,
        total_value,
        total_pl,
        total_pl_pct,
        has_data: entries.iter().any(|e| e.current_price.is_some()),
    };

    render(
        &state.env,
        "portfolio.html",
        context! { entries => entries, summary => summary },
    )
}

#[derive(Deserialize)]
pub struct PortfolioForm {
    symbol: Option<String>,
    shares: Option<String>,
    price_paid: Option<String>,
}

pub async fn add_portfolio_entry(
    State(state): State<AppState>,
    Form(form): Form<PortfolioForm>,
) -> Result<Redirect, AppError> {
    let symbol = form.symbol.unwrap_or_default().trim().to_uppercase();
    if !valid_symbol(&symbol) {
        return Ok(Redirect::to("/portfolio"));
    }
    let shares: f64 = match form.shares.and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => return Ok(Redirect::to("/portfolio")),
    };
    let price_paid: f64 = match form.price_paid.and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => return Ok(Redirect::to("/portfolio")),
    };
    if shares <= 0.0 || price_paid < 0.0 {
        return Ok(Redirect::to("/portfolio"));
    }

    let inserted_symbol = {
        let conn = state.db.lock().await;
        conn.execute(
            "INSERT INTO portfolio (symbol, shares, price_paid) VALUES (?, ?, ?)",
            params![symbol, shares, price_paid],
        )?;
        conn.execute("INSERT INTO symbols (symbol) VALUES (?)", params![symbol])
            .is_ok()
    };
    if inserted_symbol {
        let _ = fetch_stock_data(&state, &symbol).await;
    }
    Ok(Redirect::to("/portfolio"))
}

pub async fn delete_portfolio_entry(
    State(state): State<AppState>,
    Path(entry_id): Path<i64>,
) -> Result<Redirect, AppError> {
    let conn = state.db.lock().await;
    conn.execute("DELETE FROM portfolio WHERE id = ?", params![entry_id])?;
    Ok(Redirect::to("/portfolio"))
}
