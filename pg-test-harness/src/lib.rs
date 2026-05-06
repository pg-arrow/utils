use std::fs;
use std::path::PathBuf;

use toml::Table;
use tokio_postgres::{Client, NoTls, Row};

// ── Config helpers ────────────────────────────────────────────────────────────

/// Read `pg-test-config.toml` from the given directory (typically `CARGO_MANIFEST_DIR`
/// of the calling crate, passed in by the test).
pub fn read_pg18_config(manifest_dir: &str) -> (String, String) {
    let config_path = PathBuf::from(manifest_dir).join("pg-test-config.toml");
    let src = fs::read_to_string(&config_path)
        .unwrap_or_else(|_| panic!("pg-test-config.toml not found at {config_path:?}"));
    let table: Table = src.parse().expect("invalid TOML");
    let pg18 = table
        .get("postgres")
        .and_then(|v| v.get("pg18"))
        .expect("[postgres.pg18] section missing");
    let data_dir = pg18
        .get("data_dir")
        .and_then(|v| v.as_str())
        .expect("data_dir missing")
        .to_owned();
    let bin_dir = pg18
        .get("bin_dir")
        .and_then(|v| v.as_str())
        .expect("bin_dir missing")
        .to_owned();
    (data_dir, bin_dir)
}

pub fn pg18_data_dir(manifest_dir: &str) -> String {
    read_pg18_config(manifest_dir).0
}

// ── Connection ─────────────────────────────────────────────────────────────

/// Connect to the local pg18 test instance (port 5432, db postgres, OS user).
pub async fn connect() -> Client {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "postgres".to_owned());
    let connstr = format!("host=localhost port=5432 dbname=postgres user={user}");
    let (client, conn) = tokio_postgres::connect(&connstr, NoTls)
        .await
        .expect("failed to connect to test postgres");

    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("pg connection error: {e}");
        }
    });

    client
}

// ── Database OID helpers ──────────────────────────────────────────────────

/// Query the OID of a named database from a live PostgreSQL connection.
pub async fn db_oid(client: &Client, datname: &str) -> usize {
    let row = client
        .query_one(
            "SELECT oid FROM pg_database WHERE datname = $1",
            &[&datname],
        )
        .await
        .unwrap_or_else(|_| panic!("database {datname:?} not found"));
    let oid: u32 = row.get(0);
    oid as usize
}

/// Blocking wrapper: connect, query the OID of `datname`, disconnect.
pub fn db_oid_blocking(datname: &str) -> usize {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async {
        let client = connect().await;
        db_oid(&client, datname).await
    })
}

// ── Decode test table ─────────────────────────────────────────────────────

/// Name of the decode test table.
pub const DECODE_TEST_TABLE: &str = "pg_arrow_decode_test";

/// Create the decode test table if it does not exist, insert seed rows, and
/// run CHECKPOINT so pg_arrow can read it via heap files.
///
/// Idempotent — safe to call from multiple tests.
pub async fn ensure_decode_test_table(client: &Client) {
    client
        .execute(
            &format!(
                "CREATE TABLE IF NOT EXISTS {DECODE_TEST_TABLE} (
                    id          serial PRIMARY KEY,
                    col_bool    boolean,
                    col_int2    smallint,
                    col_int4    integer,
                    col_int8    bigint,
                    col_float4  real,
                    col_float8  double precision,
                    col_text    text,
                    col_varchar varchar(64),
                    col_date    date,
                    col_ts      timestamp,
                    col_tstz    timestamptz,
                    col_bytea   bytea
                )"
            ),
            &[],
        )
        .await
        .expect("CREATE TABLE failed");

    let count: i64 = client
        .query_one(
            &format!("SELECT count(*) FROM {DECODE_TEST_TABLE}"),
            &[],
        )
        .await
        .expect("count failed")
        .get(0);

    if count == 0 {
        client
            .execute(
                &format!(
                    "INSERT INTO {DECODE_TEST_TABLE}
                     (col_bool, col_int2, col_int4, col_int8,
                      col_float4, col_float8, col_text, col_varchar,
                      col_date, col_ts, col_tstz, col_bytea)
                     VALUES
                     (true,  1,  100,  100000, 1.5,  2.5,  'hello', 'world',
                      '2024-03-15', '2024-03-15 10:30:00', '2024-03-15 10:30:00+00', '\\xDEADBEEF'),
                     (false, -1, -100, -100000, -1.5, -2.5, 'foo',   'bar',
                      '2000-01-01', '2000-01-01 00:00:00', '2000-01-01 00:00:00+00', '\\xCAFEBABE'),
                     (NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)"
                ),
                &[],
            )
            .await
            .expect("INSERT failed");
    }

    // Flush dirty pages so pg_arrow can read the table via heap files.
    client
        .execute("CHECKPOINT", &[])
        .await
        .expect("CHECKPOINT failed");
}

// ── Value extraction helpers ───────────────────────────────────────────────

pub fn pg_bool(row: &Row, col: &str) -> Option<bool> { row.get(col) }
pub fn pg_i16(row: &Row, col: &str) -> Option<i16>   { row.get(col) }
pub fn pg_i32(row: &Row, col: &str) -> Option<i32>   { row.get(col) }
pub fn pg_i64(row: &Row, col: &str) -> Option<i64>   { row.get(col) }
pub fn pg_f32(row: &Row, col: &str) -> Option<f32>   { row.get(col) }
pub fn pg_f64(row: &Row, col: &str) -> Option<f64>   { row.get(col) }
pub fn pg_str(row: &Row, col: &str) -> Option<String> { row.get(col) }
pub fn pg_bytes(row: &Row, col: &str) -> Option<Vec<u8>> { row.get(col) }

/// Get a date column as days since 1970-01-01 (the column must be cast to `::text` in the query).
pub fn pg_date_days(row: &Row, col: &str) -> Option<i32> {
    let s: Option<String> = row.try_get::<_, Option<String>>(col).ok().flatten();
    s.and_then(|s| {
        let parts: Vec<i32> = s.split('-').filter_map(|p| p.parse().ok()).collect();
        if parts.len() == 3 {
            Some(days_since_epoch(parts[0], parts[1] as u32, parts[2] as u32))
        } else {
            None
        }
    })
}

/// Get a timestamp column as µs since 1970-01-01 (the column must be cast to `::text` in the query).
pub fn pg_ts_us(row: &Row, col: &str) -> Option<i64> {
    let s: Option<String> = row.try_get::<_, Option<String>>(col).ok().flatten();
    s.and_then(|s| parse_ts_to_us(&s))
}

// ── Internal date/time helpers ─────────────────────────────────────────────

fn days_since_epoch(year: i32, month: u32, day: u32) -> i32 {
    let jdn = julian_day(year, month, day);
    let epoch_jdn = julian_day(1970, 1, 1);
    (jdn - epoch_jdn) as i32
}

fn julian_day(year: i32, month: u32, day: u32) -> i64 {
    let a = (14 - month as i64) / 12;
    let y = year as i64 + 4800 - a;
    let m = month as i64 + 12 * a - 3;
    day as i64 + (153 * m + 2) / 5 + 365 * y + y / 4 - y / 100 + y / 400 - 32045
}

fn parse_ts_to_us(s: &str) -> Option<i64> {
    let s = s.trim_end_matches("+00").trim_end_matches(" UTC");
    let (date_part, time_part) = s.split_once(' ')?;
    let date_parts: Vec<i32> = date_part.split('-').filter_map(|p| p.parse().ok()).collect();
    if date_parts.len() != 3 {
        return None;
    }
    let days = days_since_epoch(date_parts[0], date_parts[1] as u32, date_parts[2] as u32);
    let (time_no_frac, frac_us) = if let Some((t, f)) = time_part.split_once('.') {
        let mut frac_str = f.to_owned();
        frac_str.truncate(6);
        while frac_str.len() < 6 { frac_str.push('0'); }
        (t, frac_str.parse::<i64>().ok().unwrap_or(0))
    } else {
        (time_part, 0)
    };
    let time_parts: Vec<i64> = time_no_frac
        .split(':')
        .filter_map(|p| p.parse().ok())
        .collect();
    if time_parts.len() != 3 { return None; }
    let day_us = days as i64 * 86_400_000_000;
    let time_us = time_parts[0] * 3_600_000_000
        + time_parts[1] * 60_000_000
        + time_parts[2] * 1_000_000;
    Some(day_us + time_us + frac_us)
}
