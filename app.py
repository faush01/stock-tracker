import sqlite3
import os
from datetime import datetime, timedelta, timezone

import yfinance as yf
from flask import Flask, g, render_template, request, redirect, url_for, abort, jsonify

app = Flask(__name__)
app.secret_key = os.urandom(24)

DATABASE = os.path.join(app.root_path, "data", "stocks.db")
os.makedirs(os.path.dirname(DATABASE), exist_ok=True)
CACHE_MAX_AGE = timedelta(hours=1)


# ---------------------------------------------------------------------------
# Database helpers
# ---------------------------------------------------------------------------

def get_db():
    if "db" not in g:
        g.db = sqlite3.connect(DATABASE)
        g.db.row_factory = sqlite3.Row
    return g.db


@app.teardown_appcontext
def close_db(exc):
    db = g.pop("db", None)
    if db is not None:
        db.close()


def init_db():
    db = get_db()
    db.executescript("""
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
    """)
    db.commit()


with app.app_context():
    init_db()


# ---------------------------------------------------------------------------
# yfinance helpers
# ---------------------------------------------------------------------------

def fetch_stock_data(symbol):
    """Fetch daily time series from Yahoo Finance and cache in SQLite."""
    print(f"Fetching data for {symbol}...")
    ticker = yf.Ticker(symbol)
    df = ticker.history(period="3mo")

    if df.empty:
        return False

    db = get_db()
    now = datetime.now(timezone.utc).isoformat()
    counter = 0
    for date, row in df.iterrows():
        counter += 1
        db.execute(
            """INSERT OR REPLACE INTO daily_data
               (symbol, date, open, high, low, close, volume, fetched_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
            (
                symbol,
                date.strftime("%Y-%m-%d"),
                float(row["Open"]),
                float(row["High"]),
                float(row["Low"]),
                float(row["Close"]),
                int(row["Volume"]),
                now,
            ),
        )
    db.commit()
    print(f"Data for {symbol} fetched and cached, {counter} rows.")

    return True


def get_stock_data(symbol):
    """Return cached daily data, refreshing from API if stale."""
    db = get_db()
    row = db.execute(
        "SELECT fetched_at FROM daily_data WHERE symbol = ? ORDER BY fetched_at DESC LIMIT 1",
        (symbol,),
    ).fetchone()

    time_now = datetime.now(timezone.utc)
    if not row or time_now - datetime.fromisoformat(row["fetched_at"]).replace(tzinfo=timezone.utc) >= CACHE_MAX_AGE:
        fetch_stock_data(symbol)

    rows = db.execute(
        "SELECT date, open, high, low, close, volume FROM daily_data WHERE symbol = ? ORDER BY date",
        (symbol,),
    ).fetchall()
    return [dict(r) for r in rows]


# ---------------------------------------------------------------------------
# Routes
# ---------------------------------------------------------------------------

@app.route("/")
def index():
    db = get_db()
    symbols = db.execute("SELECT symbol FROM symbols ORDER BY symbol").fetchall()

    # need to refresh data for all symbols to get latest close prices for change calculations
    for s in symbols:
        get_stock_data(s["symbol"])

    # Build per-symbol change dicts keyed by date
    all_dates = set()
    raw = {}
    for s in symbols:
        sym = s["symbol"]
        rows = db.execute(
            "SELECT date, close FROM daily_data WHERE symbol = ? ORDER BY date DESC LIMIT 10",
            (sym,),
        ).fetchall()
        changes = {}
        for i in range(len(rows) - 1):
            if i >= 5:
                break
            prev_close = rows[i + 1]["close"]
            cur_close = rows[i]["close"]
            if prev_close and cur_close is not None:
                pct = (cur_close - prev_close) / prev_close * 100
            else:
                pct = None
            changes[rows[i]["date"]] = {"pct": pct, "close": cur_close}
            all_dates.add(rows[i]["date"])
        raw[sym] = changes

    # Unified sorted date columns (last 5 days only)
    dates = sorted(all_dates)[-5:]

    symbol_data = []
    for s in symbols:
        sym = s["symbol"]
        changes = [{"date": d, "pct": raw[sym].get(d, {}).get("pct"), "close": raw[sym].get(d, {}).get("close")} for d in dates]
        symbol_data.append({"symbol": sym, "changes": changes})

    return render_template("list.html", symbols=symbol_data)


@app.route("/symbols", methods=["POST"])
def add_symbol():
    symbol = request.form.get("symbol", "").strip().upper()
    if not symbol or not all(c.isalnum() or c == '.' for c in symbol):
        return redirect(url_for("index"))

    db = get_db()
    try:
        db.execute("INSERT INTO symbols (symbol) VALUES (?)", (symbol,))
        db.commit()
    except sqlite3.IntegrityError:
        pass  # already exists

    # Trigger initial fetch so data is ready when user clicks through
    fetch_stock_data(symbol)

    return redirect(url_for("index"))


@app.route("/symbols/<path:symbol>/delete", methods=["POST"])
def delete_symbol(symbol):
    symbol = symbol.upper()
    db = get_db()
    db.execute("DELETE FROM symbols WHERE symbol = ?", (symbol,))
    db.execute("DELETE FROM daily_data WHERE symbol = ?", (symbol,))
    db.commit()
    return redirect(url_for("index"))


@app.route("/api/symbols/<path:symbol>")
def api_symbol_data(symbol):
    symbol = symbol.upper()
    db = get_db()
    exists = db.execute("SELECT 1 FROM symbols WHERE symbol = ?", (symbol,)).fetchone()
    if not exists:
        abort(404)

    data = get_stock_data(symbol)
    return jsonify(data)


@app.route("/symbols/<path:symbol>")
def detail(symbol):
    symbol = symbol.upper()
    db = get_db()
    exists = db.execute("SELECT 1 FROM symbols WHERE symbol = ?", (symbol,)).fetchone()
    if not exists:
        abort(404)

    return render_template("detail.html", symbol=symbol)


# ---------------------------------------------------------------------------
# Portfolio
# ---------------------------------------------------------------------------

def get_latest_price(symbol):
    """Return the most recent close price for a symbol, refreshing if stale."""
    data = get_stock_data(symbol)
    if data:
        return data[-1]["close"]
    return None


@app.route("/portfolio")
def portfolio():
    db = get_db()
    entries = db.execute(
        "SELECT id, symbol, shares, price_paid FROM portfolio ORDER BY symbol, created_at"
    ).fetchall()
    entries = [dict(e) for e in entries]

    total_cost = 0.0
    total_value = 0.0
    price_cache = {}

    for entry in entries:
        sym = entry["symbol"]
        if sym not in price_cache:
            price_cache[sym] = get_latest_price(sym)
        current_price = price_cache[sym]
        entry["current_price"] = current_price

        cost = entry["shares"] * entry["price_paid"]
        entry["cost"] = cost
        total_cost += cost

        if current_price is not None:
            value = entry["shares"] * current_price
            entry["value"] = value
            entry["pl"] = value - cost
            entry["pl_pct"] = (entry["pl"] / cost * 100) if cost else 0
            total_value += value
        else:
            entry["value"] = None
            entry["pl"] = None
            entry["pl_pct"] = None

    total_pl = total_value - total_cost
    total_pl_pct = (total_pl / total_cost * 100) if total_cost else 0

    summary = {
        "total_cost": total_cost,
        "total_value": total_value,
        "total_pl": total_pl,
        "total_pl_pct": total_pl_pct,
        "has_data": any(e["current_price"] is not None for e in entries) if entries else False,
    }

    return render_template("portfolio.html", entries=entries, summary=summary)


@app.route("/portfolio", methods=["POST"])
def add_portfolio_entry():
    symbol = request.form.get("symbol", "").strip().upper()
    if not symbol or not all(c.isalnum() or c == '.' for c in symbol):
        return redirect(url_for("portfolio"))

    try:
        shares = float(request.form.get("shares", ""))
        price_paid = float(request.form.get("price_paid", ""))
    except (ValueError, TypeError):
        return redirect(url_for("portfolio"))

    if shares <= 0 or price_paid < 0:
        return redirect(url_for("portfolio"))

    db = get_db()
    db.execute(
        "INSERT INTO portfolio (symbol, shares, price_paid) VALUES (?, ?, ?)",
        (symbol, shares, price_paid),
    )
    db.commit()

    # Ensure symbol is tracked so price data is available
    try:
        db.execute("INSERT INTO symbols (symbol) VALUES (?)", (symbol,))
        db.commit()
        fetch_stock_data(symbol)
    except sqlite3.IntegrityError:
        pass

    return redirect(url_for("portfolio"))


@app.route("/portfolio/<int:entry_id>/delete", methods=["POST"])
def delete_portfolio_entry(entry_id):
    db = get_db()
    db.execute("DELETE FROM portfolio WHERE id = ?", (entry_id,))
    db.commit()
    return redirect(url_for("portfolio"))


# ---------------------------------------------------------------------------
# Run
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    app.run(debug=True)
