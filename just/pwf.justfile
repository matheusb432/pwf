set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set working-directory := '..'

# Build the release binary at target/release/pwf.
build:
    cargo build --release

# First-time setup of the global pwf shim.
install:
    cargo run --quiet -p xtask -- install

# Refresh the global pwf shim after a rebuild.
update *args:
    cargo run --quiet -p xtask -- update {{ args }}

# Format Rust sources + Markdown in place.
fmt:
    cargo run --quiet -p xtask -- fmt

# Check Rust formatting + clippy -D warnings + Markdown; non-zero on drift.
fmt-check:
    cargo run --quiet -p xtask -- fmt-check

# Apply clippy's machine-applicable fixes, then reformat.
fix *args:
    cargo run --quiet -p xtask -- fix {{ args }}

# Report production Rust error-handling smells.
smell-check-errors:
    cargo run --quiet -p xtask -- smell-check-errors

# Test gate (see root recipe for flags).
test *flags:
    cargo run --quiet -p xtask -- test {{ flags }}
