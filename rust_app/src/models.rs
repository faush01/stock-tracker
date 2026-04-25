use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Daily {
    pub date: String,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub close: Option<f64>,
    pub volume: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayChange {
    pub date: String,
    pub pct: Option<f64>,
    pub close: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolRow {
    pub symbol: String,
    pub changes: Vec<DayChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortfolioEntry {
    pub id: i64,
    pub symbol: String,
    pub shares: f64,
    pub price_paid: f64,
    pub current_price: Option<f64>,
    pub cost: f64,
    pub value: Option<f64>,
    pub pl: Option<f64>,
    pub pl_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortfolioSummary {
    pub total_cost: f64,
    pub total_value: f64,
    pub total_pl: f64,
    pub total_pl_pct: f64,
    pub has_data: bool,
}
