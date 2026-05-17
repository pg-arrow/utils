# pg-test-harness

> **Status:** Work in progress. Used by [`pg_arrow`](https://github.com/pg-arrow/pg_arrow) and [`pgfusion`](https://github.com/pg-arrow/pgfusion); APIs may evolve as those projects do.

Shared test harness for `pg_arrow` and `pgfusion`. It does two things:

1. **Rust library** — connection helpers, value-decoding helpers, decode-test-table seeding, and MVCC snapshot helpers for tests that need a live PostgreSQL.
2. **Setup scripts** — build PostgreSQL from source, initialize a local cluster, load test data, and manage pgbackrest backups. Used by every consumer's `just pg-setup` recipe.

## Contents

- [Library usage](#usage) — Rust dev-dependency
- [Configuration](#configuration) — `pg-test-config.toml`
- [API](#api) — connection, snapshots, decoders, value helpers
- [Environment variables](#environment-variables)
- [`setup-postgres.sh`](#scriptssetup-postgressh)
- [`pgbackrest-backup.sh`](#scriptspgbackrest-backupsh)

## Usage

Add to your crate's `[dev-dependencies]`:

```toml
pg-test-harness = { path = "../utilities/pg-test-harness" }
```

## Configuration

Place a `pg-test-config.toml` in your crate root (next to `Cargo.toml`):

```toml
[postgres.pg18]
data_dir = "/path/to/testdata/postgres-pg18/data"
bin_dir  = "/path/to/testdata/postgres-pg18/install/bin"
```

Use `setup-postgres.sh` (see [Scripts](#scripts)) to generate this file automatically.

## API

### Config

```rust
let cfg = pg_test_harness::read_pg_config(env!("CARGO_MANIFEST_DIR"), "pg18");
// cfg.data_dir, cfg.bin_dir, cfg.port, cfg.socket_dir
```

### Connection

```rust
// Async — Unix socket first, TCP fallback
let client = pg_test_harness::connect_to(&cfg, "postgres").await;

// Blocking wrapper
let client = pg_test_harness::connect_to_blocking(&cfg, "postgres");
```

### Decode test table

```rust
pg_test_harness::ensure_decode_test_table(&client).await;
// Creates pg_arrow_decode_test with 3 seed rows and issues CHECKPOINT
```

### Snapshot (MVCC)

```rust
let snap = pg_test_harness::acquire_snapshot(&client).await;
// snap.xmin, snap.xmax, snap.xip
pg_test_harness::release_snapshot(&client).await;
```

### Value helpers

```rust
pg_test_harness::pg_bool(&row, "col_bool")   // Option<bool>
pg_test_harness::pg_i32(&row, "col_int4")    // Option<i32>
pg_test_harness::pg_str(&row, "col_text")    // Option<String>
pg_test_harness::pg_bytes(&row, "col_bytea") // Option<Vec<u8>>
// also: pg_i16, pg_i64, pg_f32, pg_f64, pg_date_days, pg_ts_us
```

### Checkpoint control

```rust
// Skip CHECKPOINT for speed in read-only tests:
// PG_TEST_NO_CHECKPOINT=1 cargo test

if pg_test_harness::skip_if_no_checkpoint() {
    return; // test requires flushed pages — skip
}
```

## Environment Variables

| Variable | Effect |
|---|---|
| `PG_TEST_NO_CHECKPOINT` | Set to `1` to skip `CHECKPOINT` calls in `ensure_decode_test_table` and `checkpoint()` |

## Scripts

### `scripts/setup-postgres.sh`

Clones the PostgreSQL source, builds it, initializes a cluster, and seeds test databases. Writes `pg-test-config.toml` into `TARGET_DIR`.

```bash
# Build pg18 from source, init cluster, seed test data
TARGET_DIR=/path/to/project bash scripts/setup-postgres.sh -b pg18 -B -i -t

# Simple single-table schema instead of e-commerce schema
bash scripts/setup-postgres.sh -b pg18 -B -i -t -s

# Add a pgbench database (scale factor 10)
PGBENCH_SCALE=10 bash scripts/setup-postgres.sh -b pg18 -p
```

**Key flags:**

| Flag | Description |
|---|---|
| `-b VERSION` | Version/branch: `pg18`, `pg17`, `pg16`, `latest` |
| `-B` | Build PostgreSQL from source |
| `-i` | Initialize database cluster (requires `-B`) |
| `-t` | Seed test database with sample data |
| `-s` | Use simple `test_types` schema (instead of e-commerce) |
| `-p` | Create pgbench database |

**Environment variables:**

| Variable | Default | Description |
|---|---|---|
| `TARGET_DIR` | `$PWD` | Where `pg-test-config.toml` is written |
| `TESTDATA_DIR` | `$TARGET_DIR/testdata` | Where PG source/build/data live |
| `PGBENCH_SCALE` | `1` | Scale factor for `--pgbench` |
| `PGBENCH_DBNAME` | `pgbench_test` | Database name for pgbench data |

---

### `scripts/pgbackrest-backup.sh`

Manages WAL archiving and point-in-time backups for a local test PostgreSQL instance using [pgbackrest](https://pgbackrest.org/).

```bash
# First-time setup (configures archive_mode, creates stanza)
bash scripts/pgbackrest-backup.sh setup

# Take a full backup
bash scripts/pgbackrest-backup.sh full

# Incremental / differential
bash scripts/pgbackrest-backup.sh incr
bash scripts/pgbackrest-backup.sh diff

# List backups
bash scripts/pgbackrest-backup.sh info

# Restore to a target directory
bash scripts/pgbackrest-backup.sh restore -t /tmp/pg-restore
```

**Environment variables:**

| Variable | Default | Description |
|---|---|---|
| `TESTDATA_DIR` | `$PWD/testdata` | Root of PG source/build/data |
| `PG_VERSION` | `pg18` | Version key matching `pg-test-config.toml` |
| `PGBACKREST` | `/opt/homebrew/bin/pgbackrest` | Path to pgbackrest binary |
| `STANZA` | `$PG_VERSION` | pgbackrest stanza name |
