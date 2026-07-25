set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set windows-shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set working-directory := '..'

# Build the release binary at target/release/pwf.
[group('build')]
build:
    cargo build --release

# First-time setup of the global pwf shim.
[group('build')]
install:
    cargo run --quiet -p xtask -- install

# Refresh the global pwf shim after a rebuild.
[group('build')]
update *args:
    cargo run --quiet -p xtask -- update {{ args }}

# Run the full test preflight and build the release binary.
[group('build')]
ship:
    cargo run --quiet -p xtask -- ship

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

# Apply machine-applicable fixes, then format.
[group('quality')]
fix *args:
    cargo run --quiet -p xtask -- fix {{ args }}

# Run unit, end-to-end, or all tests.
[group('quality')]
test *args:
    cargo run --quiet -p xtask -- test {{ args }}
