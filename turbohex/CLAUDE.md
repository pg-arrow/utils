# turbohex

Hex viewer with pluggable PostgreSQL page decoders. Decoders are compiled as `cdylib` plugins.

## Structure

```
turbohex/
├── Cargo.toml                        # Workspace root (members: decoders/*)
└── decoders/
    └── pg_heap_page/
        └── src/lib.rs                # Decodes PostgreSQL heap page headers and tuples to JSON
```

## Key Conventions

- Each decoder is a separate `cdylib` crate under `decoders/`
- Decoders depend on `pg_arrow` via git source (`ssh://git@github.com/pg-arrow/pg_arrow.git`)
- Release profile uses `opt-level = "s"` and `lto = true` for small binary size

## Building

```bash
cd utilities/turbohex
cargo build --release
```

## Commit Message Format

Use a short lowercase prefix followed by a colon and a brief description:

```
feat: add new feature
fix: correct a bug
refactor: restructure code without behavior change
bench: benchmark setup, runs, or results
test: add or update tests
chore: tooling, config, CI, dependency updates
docs: documentation changes
```

- Subject line: lowercase, no trailing period, imperative mood
- Keep it concise (under 72 characters)
