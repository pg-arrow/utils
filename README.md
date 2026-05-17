# utilities

Shared tooling for the [`pg_arrow`](https://github.com/pg-arrow/pg_arrow) / [`pgfusion`](https://github.com/pg-arrow/pgfusion) family.

> **Status:** Work in progress. Tested on macOS and Linux; Windows is not currently supported.

## Crates

| Path | Purpose |
|---|---|
| [`pg-test-harness/`](pg-test-harness/) | Rust test harness (connection, snapshot, value helpers) + setup scripts that build PostgreSQL from source, init a local cluster, and seed test data. |
| [`turbohex/`](turbohex/) | Hex viewer with pluggable decoders for PostgreSQL on-disk formats. Currently ships a heap-page decoder. |

Each subdirectory has its own `README.md` with detailed usage.
