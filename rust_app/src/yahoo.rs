use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::Deserialize;

use crate::state::AppState;

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

pub async fn fetch_stock_data(state: &AppState, symbol: &str) -> anyhow::Result<bool> {
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
