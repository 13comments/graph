use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use duckdb::{params, Connection};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tower_http::services::ServeDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
}

#[derive(Serialize)]
struct Candle {
    timestamp: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

#[derive(Serialize)]
struct IndicatorPoint {
    timestamp: String,
    sma_14: Option<f64>,
    ema_14: Option<f64>,
    rsi_14: Option<f64>,
}

#[derive(Serialize)]
struct FibLevels {
    low: f64,
    high: f64,
    levels: Vec<FibLevel>,
}

#[derive(Serialize)]
struct FibLevel {
    ratio: f64,
    value: f64,
}

#[derive(Deserialize)]
struct CandleQuery {
    limit: Option<u32>,
}

#[derive(Deserialize)]
struct RangeQuery {
    start: Option<String>,
    end: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "graph=debug,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let db_path = Path::new("data/data.duckdb");
    let csv_path = Path::new("data/stocks.csv");
    let conn = Connection::open(db_path).context("open DuckDB")?;
    initialize_db(&conn, csv_path).context("init DuckDB")?;

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
    };

    let app = Router::new()
        .route("/api/candles", get(get_candles))
        .route("/api/indicators", get(get_indicators))
        .route("/api/fib", get(get_fib))
        .nest_service("/", ServeDir::new("static"))
        .with_state(state);

    let addr: SocketAddr = "0.0.0.0:8000".parse()?;
    tracing::info!("listening on {addr}");
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}

fn initialize_db(conn: &Connection, csv_path: &Path) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS candles (
            timestamp TIMESTAMP,
            open DOUBLE,
            high DOUBLE,
            low DOUBLE,
            close DOUBLE,
            volume DOUBLE
        );",
    )?;

    let existing: i64 = conn.query_row("SELECT COUNT(*) FROM candles", [], |row| row.get(0))?;
    if existing == 0 {
        let csv_str = csv_path
            .to_str()
            .context("CSV path not valid UTF-8")?
            .replace('\\', "/");
        let sql = format!(
            "COPY candles FROM '{}' (HEADER, AUTO_DETECT TRUE);",
            csv_str
        );
        conn.execute_batch(&sql)?;
    }
    Ok(())
}

async fn get_candles(
    State(state): State<AppState>,
    Query(query): Query<CandleQuery>,
) -> Result<Json<Vec<Candle>>, (StatusCode, String)> {
    let limit = query.limit.unwrap_or(500) as i64;
    let conn = state.db.lock().await;
    let mut stmt = conn
        .prepare(
            "SELECT
                strftime(timestamp, '%Y-%m-%d %H:%M:%S') AS ts,
                open, high, low, close, volume
             FROM candles
             ORDER BY timestamp
             LIMIT ?",
        )
        .map_err(internal_error)?;
    let mut rows = stmt.query([limit]).map_err(internal_error)?;
    let mut candles = Vec::new();
    while let Some(row) = rows.next().map_err(internal_error)? {
        candles.push(Candle {
            timestamp: row.get(0).map_err(internal_error)?,
            open: row.get(1).map_err(internal_error)?,
            high: row.get(2).map_err(internal_error)?,
            low: row.get(3).map_err(internal_error)?,
            close: row.get(4).map_err(internal_error)?,
            volume: row.get(5).map_err(internal_error)?,
        });
    }
    Ok(Json(candles))
}

async fn get_indicators(
    State(state): State<AppState>,
) -> Result<Json<Vec<IndicatorPoint>>, (StatusCode, String)> {
    let conn = state.db.lock().await;
    let sql = r#"
        WITH ordered AS (
            SELECT
                row_number() OVER (ORDER BY timestamp) AS rn,
                timestamp,
                close
            FROM candles
        ),
        ema AS (
            SELECT rn, timestamp, close, close AS ema
            FROM ordered
            WHERE rn = 1
            UNION ALL
            SELECT o.rn, o.timestamp, o.close,
                   (o.close * 0.133333333333) + (e.ema * 0.866666666667) AS ema
            FROM ordered o
            JOIN ema e ON o.rn = e.rn + 1
        ),
        deltas AS (
            SELECT
                timestamp,
                close,
                close - lag(close) OVER (ORDER BY timestamp) AS delta
            FROM candles
        ),
        gains AS (
            SELECT
                timestamp,
                close,
                CASE WHEN delta > 0 THEN delta ELSE 0 END AS gain,
                CASE WHEN delta < 0 THEN -delta ELSE 0 END AS loss
            FROM deltas
        ),
        rsi_calc AS (
            SELECT
                timestamp,
                close,
                avg(gain) OVER (ORDER BY timestamp ROWS BETWEEN 13 PRECEDING AND CURRENT ROW) AS avg_gain,
                avg(loss) OVER (ORDER BY timestamp ROWS BETWEEN 13 PRECEDING AND CURRENT ROW) AS avg_loss
            FROM gains
        )
        SELECT
            strftime(candles.timestamp, '%Y-%m-%d %H:%M:%S') AS ts,
            avg(candles.close) OVER (ORDER BY candles.timestamp ROWS BETWEEN 13 PRECEDING AND CURRENT ROW) AS sma_14,
            ema.ema AS ema_14,
            CASE
                WHEN rsi_calc.avg_loss = 0 THEN NULL
                ELSE 100 - (100 / (1 + (rsi_calc.avg_gain / rsi_calc.avg_loss)))
            END AS rsi_14
        FROM candles
        LEFT JOIN ema ON ema.timestamp = candles.timestamp
        LEFT JOIN rsi_calc ON rsi_calc.timestamp = candles.timestamp
        ORDER BY candles.timestamp
    "#;
    let mut stmt = conn.prepare(sql).map_err(internal_error)?;
    let mut rows = stmt.query([]).map_err(internal_error)?;
    let mut points = Vec::new();
    while let Some(row) = rows.next().map_err(internal_error)? {
        points.push(IndicatorPoint {
            timestamp: row.get(0).map_err(internal_error)?,
            sma_14: row.get(1).map_err(internal_error)?,
            ema_14: row.get(2).map_err(internal_error)?,
            rsi_14: row.get(3).map_err(internal_error)?,
        });
    }
    Ok(Json(points))
}

async fn get_fib(
    State(state): State<AppState>,
    Query(query): Query<RangeQuery>,
) -> Result<Json<FibLevels>, (StatusCode, String)> {
    let conn = state.db.lock().await;
    let (low, high): (f64, f64) = match (&query.start, &query.end) {
        (Some(start), Some(end)) => conn
            .query_row(
                "SELECT min(low), max(high) FROM candles WHERE timestamp BETWEEN ? AND ?",
                params![start, end],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(internal_error)?,
        _ => conn
            .query_row(
                "SELECT min(low), max(high) FROM candles",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(internal_error)?,
    };

    let levels = [0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0]
        .into_iter()
        .map(|ratio| FibLevel {
            ratio,
            value: high - (high - low) * ratio,
        })
        .collect();

    Ok(Json(FibLevels { low, high, levels }))
}

fn internal_error(error: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}
