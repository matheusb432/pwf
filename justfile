set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set windows-shell := ["bash", "-eu", "-o", "pipefail", "-c"]

_default:
    @just --list --unsorted

# Build the release binary.
[group('build')]
build:
    cargo build --release

# First-time setup of the global pwf shim.
[group('build')]
install:
    cargo run --quiet -p xtask -- install

# Converge the Ubuntu development environment and install pwf.
[group('build')]
bootstrap *args:
    mise bootstrap --yes {{ args }}

# Refresh the global pwf shim after a rebuild.
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
    cargo run --quiet -p xtask -- fmt

# Check formatting without modifying files.
[group('quality')]
fmt-check:
    cargo run --quiet -p xtask -- fmt-check

# Run repository linters.
[group('quality')]
lint:
    cargo run --quiet -p xtask -- lint

# Refresh or verify the committed SQLx checked-query cache.
prepare *args:
    cargo run --quiet -p xtask -- prepare {{ args }}

# Run the complete read-only formatting and lint gate.
[group('quality')]
check:
    cargo run --quiet -p xtask -- check

# Apply machine-applicable fixes and reformat.
[group('quality')]
fix *args:
    cargo run --quiet -p xtask -- fix {{ args }}

# Report missing mise state without changing the host.
[group('quality')]
doctor *args:
    mise bootstrap status --missing {{ args }}

# Run tests, or use `just test coverage` for cargo-llvm-cov. Run `just test --help` for options.
[group('quality')]
test *args:
    @cargo run --quiet -p xtask -- test {{ args }}
