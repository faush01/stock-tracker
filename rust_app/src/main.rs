use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Form, Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, Local, Utc};
use minijinja::{context, Environment, Value};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const CACHE_MAX_AGE_SECS: i64 = 60 * 60; // 1 hour

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    env: Arc<Environment<'static>>,
    http: reqwest::Client,
}

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct Daily {
    date: String,
    open: Option<f64>,
    high: Option<f64>,
    low: Option<f64>,
    close: Option<f64>,
    volume: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct DayChange {
    date: String,
    pct: Option<f64>,
    close: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
struct SymbolRow {
    symbol: String,
    changes: Vec<DayChange>,
}

#[derive(Debug, Clone, Serialize)]
struct PortfolioEntry {
    id: i64,
    symbol: String,
    shares: f64,
    price_paid: f64,
    current_price: Option<f64>,
    cost: f64,
    value: Option<f64>,
    pl: Option<f64>,
    pl_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
struct PortfolioSummary {
    total_cost: f64,
    total_value: f64,
    total_pl: f64,
    total_pl_pct: f64,
    has_data: bool,
}

// ---------------------------------------------------------------------------
// DB helpers
// ---------------------------------------------------------------------------

fn db_path() -> PathBuf {
    let p = PathBuf::from("data");
    std::fs::create_dir_all(&p).ok();
    p.join("stocks.db")
}

fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS symbols (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol TEXT UNIQUE NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS daily_data (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol TEXT NOT NULL,
            date TEXT NOT NULL,
            open REAL,
            high REAL,
            low REAL,
            close REAL,
            volume INTEGER,
            fetched_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(symbol, date)
        );
        CREATE TABLE IF NOT EXISTS portfolio (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol TEXT NOT NULL,
            shares REAL NOT NULL,
            price_paid REAL NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
}

// ---------------------------------------------------------------------------
// Yahoo Finance fetch
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct YahooChart {
    chart: YahooChartInner,
}
#[derive(Debug, Deserialize)]
struct YahooChartInner {
    result: Option<Vec<YahooResult>>,
}
#[derive(Debug, Deserialize)]
struct YahooResult {
    timestamp: Option<Vec<i64>>,
    indicators: YahooIndicators,
}
#[derive(Debug, Deserialize)]
struct YahooIndicators {
    quote: Vec<YahooQuote>,
}
#[derive(Debug, Deserialize)]
struct YahooQuote {
    open: Option<Vec<Option<f64>>>,
    high: Option<Vec<Option<f64>>>,
    low: Option<Vec<Option<f64>>>,
    close: Option<Vec<Option<f64>>>,
    volume: Option<Vec<Option<i64>>>,
}

async fn fetch_stock_data(state: &AppState, symbol: &str) -> anyhow::Result<bool> {
    println!("Fetching data for {}...", symbol);
    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?range=3mo&interval=1d",
        urlencoding(symbol)
    );
    let resp = state
        .http
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 stock-tracker-rust/0.1")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Ok(false);
    }
    let body: YahooChart = resp.json().await?;
    let Some(results) = body.chart.result else {
        return Ok(false);
    };
    let Some(result) = results.into_iter().next() else {
        return Ok(false);
    };
    let Some(timestamps) = result.timestamp else {
        return Ok(false);
    };
    let Some(quote) = result.indicators.quote.into_iter().next() else {
        return Ok(false);
    };
    if timestamps.is_empty() {
        return Ok(false);
    }

    let now_iso = Utc::now().to_rfc3339();
    let conn = state.db.lock().await;
    let tx = conn.unchecked_transaction()?;
    let mut counter = 0;
    for (i, ts) in timestamps.iter().enumerate() {
        let date = DateTime::<Utc>::from_timestamp(*ts, 0)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let open = quote.open.as_ref().and_then(|v| v.get(i).copied()).flatten();
        let high = quote.high.as_ref().and_then(|v| v.get(i).copied()).flatten();
        let low = quote.low.as_ref().and_then(|v| v.get(i).copied()).flatten();
        let close = quote.close.as_ref().and_then(|v| v.get(i).copied()).flatten();
        let volume = quote.volume.as_ref().and_then(|v| v.get(i).copied()).flatten();

        // skip rows where everything is null
        if open.is_none() && close.is_none() && high.is_none() && low.is_none() {
            continue;
        }
        tx.execute(
            "INSERT OR REPLACE INTO daily_data
             (symbol, date, open, high, low, close, volume, fetched_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![symbol, date, open, high, low, close, volume, now_iso],
        )?;
        counter += 1;
    }
    tx.commit()?;
    println!("Data for {} fetched and cached, {} rows.", symbol, counter);
    Ok(true)
}

fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

async fn get_stock_data(state: &AppState, symbol: &str) -> anyhow::Result<Vec<Daily>> {
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

async fn get_latest_price(state: &AppState, symbol: &str) -> Option<f64> {
    let data = get_stock_data(state, symbol).await.ok()?;
    data.last().and_then(|d| d.close)
}

// ---------------------------------------------------------------------------
// Helpers / errors
// ---------------------------------------------------------------------------

struct AppError(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(e.into())
    }
}
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        eprintln!("error: {:?}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {}", self.0)).into_response()
    }
}

fn render(env: &Environment, name: &str, ctx: Value) -> Result<Html<String>, AppError> {
    let tmpl = env.get_template(name).map_err(anyhow::Error::from)?;
    let s = tmpl.render(ctx).map_err(anyhow::Error::from)?;
    Ok(Html(s))
}

fn valid_symbol(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

async fn index(State(state): State<AppState>) -> Result<Html<String>, AppError> {
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
struct SymbolForm {
    symbol: Option<String>,
}

async fn add_symbol(
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

async fn delete_symbol(
    State(state): State<AppState>,
    Path(symbol): Path<String>,
) -> Result<Redirect, AppError> {
    let symbol = symbol.to_uppercase();
    let conn = state.db.lock().await;
    conn.execute("DELETE FROM symbols WHERE symbol = ?", params![symbol])?;
    conn.execute("DELETE FROM daily_data WHERE symbol = ?", params![symbol])?;
    Ok(Redirect::to("/"))
}

async fn api_symbol_data(
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

async fn detail(
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

async fn portfolio(State(state): State<AppState>) -> Result<Html<String>, AppError> {
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
struct PortfolioForm {
    symbol: Option<String>,
    shares: Option<String>,
    price_paid: Option<String>,
}

async fn add_portfolio_entry(
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

async fn delete_portfolio_entry(
    State(state): State<AppState>,
    Path(entry_id): Path<i64>,
) -> Result<Redirect, AppError> {
    let conn = state.db.lock().await;
    conn.execute("DELETE FROM portfolio WHERE id = ?", params![entry_id])?;
    Ok(Redirect::to("/portfolio"))
}

// ---------------------------------------------------------------------------
// Template helpers (custom filters)
// ---------------------------------------------------------------------------

fn fmt_with_thousands(v: f64, decimals: usize, signed: bool) -> String {
    let sign = if signed && v >= 0.0 { "+" } else if v < 0.0 { "-" } else { "" };
    let abs = v.abs();
    let formatted = format!("{:.*}", decimals, abs);
    let parts: Vec<&str> = formatted.splitn(2, '.').collect();
    let int_part = parts[0];
    let frac_part = parts.get(1).copied().unwrap_or("");
    // insert commas
    let bytes = int_part.as_bytes();
    let mut with_commas = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            with_commas.push(',');
        }
        with_commas.push(*b as char);
    }
    if frac_part.is_empty() {
        format!("{}{}", sign, with_commas)
    } else {
        format!("{}{}.{}", sign, with_commas, frac_part)
    }
}

fn build_env() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader("templates"));

    // |money -> "1,234.56"
    env.add_filter("money", |v: f64| fmt_with_thousands(v, 2, false));
    // |signed_money -> "+1,234.56"
    env.add_filter("signed_money", |v: f64| fmt_with_thousands(v, 2, true));
    // |pct2 -> "+1.23"
    env.add_filter("pct2", |v: f64| {
        if v >= 0.0 {
            format!("+{:.2}", v)
        } else {
            format!("{:.2}", v)
        }
    });
    // |fixed2 -> "1.23"
    env.add_filter("fixed2", |v: f64| format!("{:.2}", v));
    env
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let conn = Connection::open(db_path())?;
    init_db(&conn)?;

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        env: Arc::new(build_env()),
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?,
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/symbols", post(add_symbol))
        .route("/symbols/:symbol", get(detail))
        .route("/symbols/:symbol/delete", post(delete_symbol))
        .route("/api/symbols/:symbol", get(api_symbol_data))
        .route("/portfolio", get(portfolio).post(add_portfolio_entry))
        .route("/portfolio/:id/delete", post(delete_portfolio_entry))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 5000));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
