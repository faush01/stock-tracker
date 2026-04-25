pub mod portfolio;
pub mod symbols;

use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(symbols::index))
        .route("/symbols", post(symbols::add_symbol))
        .route("/symbols/:symbol", get(symbols::detail))
        .route("/symbols/:symbol/delete", post(symbols::delete_symbol))
        .route("/api/symbols/:symbol", get(symbols::api_symbol_data))
        .route(
            "/portfolio",
            get(portfolio::portfolio).post(portfolio::add_portfolio_entry),
        )
        .route(
            "/portfolio/:id/delete",
            post(portfolio::delete_portfolio_entry),
        )
        .layer(middleware::from_fn(log_requests))
        .with_state(state)
}

async fn log_requests(req: Request, next: Next) -> Response {
    println!("{} {}", req.method(), req.uri().path());
    next.run(req).await
}
