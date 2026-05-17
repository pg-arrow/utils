# turbohex

> **Status:** Work in progress. Decoders are added on demand as the `pg_arrow` / `pgfusion` family needs them.

Hex viewer with pluggable decoders for PostgreSQL's on-disk binary formats. Useful for inspecting raw heap files, debugging tuple layouts, and verifying the structures `pg_arrow` parses against the actual bytes.

Each decoder is a separate `cdylib` plugin under `decoders/` — turbohex loads them at runtime and prints a structured JSON view alongside the raw hex dump.

## Install

```bash
cargo install turbohex
```

To use a decoder, build it from this repo (decoders are not published to crates.io):

```bash
cd utilities/turbohex
cargo build --release
# pg_heap_page decoder lands in target/release/libpg_heap_page.{dylib,so}
```

## Usage

```bash
# Inspect a heap file page-by-page using the pg_heap_page decoder
turbohex \
    --decoder target/release/libpg_heap_page.dylib \
    /path/to/pgdata/base/<db_oid>/<relfilenode>
```

The decoder reads each 8192-byte PostgreSQL page, emits the parsed `PageHeaderData`, line pointers, and tuple headers as JSON, and the raw bytes are still shown on the side so you can spot-check offsets.

## Available decoders

| Decoder | What it decodes |
|---|---|
| `pg_heap_page` | PostgreSQL heap page headers, line pointers, and tuple headers |

More decoders (e.g. index pages, WAL records, FSM, VM) will land here as the parent projects grow.

## Layout

```
turbohex/
├── Cargo.toml                  # workspace root; members are the decoder crates
└── decoders/
    └── pg_heap_page/
        └── src/lib.rs          # heap-page decoder (depends on pg_arrow via git)
```

## Building

Release builds use `opt-level = "s"` and `lto = true` for compact plugin binaries:

```bash
cargo build --release
```

Decoders depend on `pg_arrow` via a public git source (`https://github.com/pg-arrow/pg_arrow.git`) — no SSH key needed.

## Adding a decoder

1. Create `decoders/<name>/` with `Cargo.toml` (crate-type = `cdylib`) and `src/lib.rs`.
2. Add it as a workspace member in the root `Cargo.toml`.
3. Implement the turbohex decoder ABI (see `pg_heap_page` for a reference implementation).
4. `cargo build --release` produces a loadable plugin under `target/release/`.
