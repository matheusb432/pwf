set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set windows-shell := ["bash", "-eu", "-o", "pipefail", "-c"]

mod agents 'just/agents.just'
mod project 'just/project.just'

_default:
    @just --list --unsorted

# One-time repository setup for cross-agent skills.
[group('build')]
bootstrap *args: (agents::bootstrap args)

# Build the release binary.
[group('build')]
build: project::build

# First-time setup of the global pwf shim.
[group('build')]
install: project::install

# Refresh the global pwf shim after a rebuild.
[group('build')]
update *args: (project::update args)

# Run the full test preflight and build the release binary.
[group('build')]
ship: project::ship

# Apply every repository formatter.
[group('quality')]
fmt: project::fmt

# Check formatting without modifying files.
[group('quality')]
fmt-check: project::fmt-check

# Run repository linters.
[group('quality')]
lint: project::lint

# Refresh or verify the committed SQLx checked-query cache.
prepare *args: (project::prepare args)

# Run the complete read-only formatting and lint gate.
[group('quality')]
check: project::check

# Apply machine-applicable fixes and reformat.
[group('quality')]
fix *args: (project::fix args)

# Collect Rust test line coverage via cargo-llvm-cov. Default prints the per-file summary table; pass --show-missing-lines for uncovered-line detail, or any other cargo-llvm-cov flag.
[group('quality')]
cov *args:
    cargo llvm-cov --workspace {{ args }}

# Run the selected test scope.
[group('quality')]
test *args: (project::test args)

# Check development-host readiness.
doctor *args:
    doctor-rs {{ args }}
