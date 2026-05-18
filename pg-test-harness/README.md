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
pg-test-harness = { path = "../utils/pg-test-harness" }
```

## Configuration

The harness directory is the single source of truth: testdata and the config live next to the scripts. Point `PG_HARNESS_DIR` at it once and everything else follows.

```bash
git clone https://github.com/pg-arrow/utils /path/to/utils
export PG_HARNESS_DIR=/path/to/utils/pg-test-harness
```

`setup-postgres.sh` writes `$PG_HARNESS_DIR/pg-test-config.toml` automatically (see [Scripts](#scriptssetup-postgressh)). Paths inside it are stored **relative** to the harness directory; `read_pg_config()` resolves them at load time.

```toml
[postgres.pg18]
data_dir = "testdata/postgres-pg18/data"
bin_dir  = "testdata/postgres-pg18/install/bin"
```

To point at a different config file, set `PG_ARROW_TEST_CONFIG=/abs/path/to/file.toml`.

## API

### Config

```rust
// First argument is ignored — kept for source compat with older callers.
// Set PG_HARNESS_DIR before calling.
let cfg = pg_test_harness::read_pg_config("", "pg18");
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

Clones the PostgreSQL source, builds it, initializes a cluster, and seeds test databases. Writes `pg-test-config.toml` and `testdata/` next to the script (`$PG_HARNESS_DIR`).

```bash
# Build pg18 from source, init cluster, seed test data
bash scripts/setup-postgres.sh -b pg18 -B -i -t

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
| `PG_HARNESS_DIR` | parent of `scripts/` | Drives every other path. Both consumers read this. |
| `TARGET_DIR` | `$PG_HARNESS_DIR` | Where `pg-test-config.toml` is written. Override only for non-standard layouts. |
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
| `TESTDATA_DIR` | `$PG_HARNESS_DIR/testdata` | Root of PG source/build/data |
| `PG_VERSION` | `pg18` | Version key matching `pg-test-config.toml` |
| `PGBACKREST` | `/opt/homebrew/bin/pgbackrest` | Path to pgbackrest binary |
| `STANZA` | `$PG_VERSION` | pgbackrest stanza name |
