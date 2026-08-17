set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set windows-shell := ["bash", "-eu", "-o", "pipefail", "-c"]

_default:
    @just --list --unsorted

# Build the release binary.
[group('build')]
build:
    cargo build --release

# First-time setup of the global pwf binary.
[group('build')]
install:
    cargo run --quiet -p xtask -- install

# Converge the Ubuntu development environment and install pwf.
[group('build')]
bootstrap *args:
    mise bootstrap --yes {{ args }}

# Refresh the global pwf binary after a rebuild.
[group('build')]
update *args:
    cargo run --quiet -p xtask -- update {{ args }}

# Run the release preflight and build the release binary. Use `--force` to skip tests.
[group('build')]
ship *args:
    cargo run --quiet -p xtask -- ship {{ args }}

# Apply every repository formatter.
[group('quality')]
fmt:
    cargo fmt --all

# Check formatting without modifying files.
[group('quality')]
fmt-check:
    cargo fmt --all --check

# Run repository linters.
[group('quality')]
lint:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy -p pwf-e2e --test e2e -- -D warnings

# Refresh or verify the committed SQLx checked-query cache.
prepare *args:
    mise exec cargo:sqlx-cli -- cargo run --quiet -p xtask -- prepare {{ args }}

# Apply pending SQLite migrations through the dedicated process.
migrate:
    cargo run --quiet -p pwf-migrator

# Run the complete read-only formatting and lint gate.
[group('quality')]
check:
    just fmt-check
    just lint
    ast-grep scan
    just prepare --check

# Apply machine-applicable fixes and reformat.
[group('quality')]
fix *args:
    cargo clippy --fix --workspace --all-targets --allow-dirty {{ args }}
    just fmt

# Report missing mise state without changing the host.
[group('quality')]
doctor:
    @mise ls --local --missing --locked --no-header

# Run tests, or use `just test coverage`; coverage defaults to quiet and forwards cargo-llvm-cov arguments.
[group('quality')]
test *args:
    @cargo run --quiet -p xtask -- test {{ args }}
