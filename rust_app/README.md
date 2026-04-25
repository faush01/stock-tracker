# Stock Tracker (Rust)

A Rust port of the Flask stock tracker app. Built with:

- [axum](https://github.com/tokio-rs/axum) — web framework
- [rusqlite](https://github.com/rusqlite/rusqlite) — SQLite (bundled)
- [reqwest](https://github.com/seanmonstar/reqwest) — fetches daily data from the Yahoo Finance chart API
- [minijinja](https://github.com/mitsuhiko/minijinja) — Jinja2-compatible templates

The on-disk schema and HTTP routes mirror the Flask app, so the same `data/stocks.db` works with both.

## Stock symbol examples
```
MSFT
BHP.AX
```

## Run Locally
```
cargo run --release
```

The server listens on `http://0.0.0.0:5000`.

## Build and Run Docker
```
docker build -t stock-tracker-rust .

docker run --rm -p 5000:5000 -v stock_data:/app/data --name stock-tracker-rust stock-tracker-rust

docker run -d -p 5000:5000 -v stock_data:/app/data --name stock-tracker-rust stock-tracker-rust
```

## Pull from GitHub
```
docker pull ghcr.io/faush01/stock-tracker-rust:main

docker run --rm -p 5000:5000 -v stock_data:/app/data --name stock-tracker-rust ghcr.io/faush01/stock-tracker-rust:main

docker run -d -p 5000:5000 -v stock_data:/app/data --name stock-tracker-rust ghcr.io/faush01/stock-tracker-rust:main
```
