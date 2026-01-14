# graph

Local stock lab demo using Axum, DuckDB, and KLineChart.

## Run

```bash
cargo run
```

Open <http://localhost:8000>.

## Data

The app loads `data/stocks.csv` into `data/data.duckdb` on first run.

## Endpoints

- `GET /api/candles?limit=500`
- `GET /api/indicators`
- `GET /api/fib?start=YYYY-MM-DD HH:MM:SS&end=YYYY-MM-DD HH:MM:SS`
