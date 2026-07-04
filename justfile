set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

mod agents 'just/agents.justfile'
mod pwf 'just/pwf.justfile'

_default:
    @just --list --unsorted --list-submodules

# Build the release binary.
build:
    @just pwf build

# First-time setup of the global pwf shim.
install:
    @just pwf install

# Refresh the global pwf shim after a rebuild.
update *args:
    @just pwf update {{ args }}

# Format Rust sources and Markdown in place.
fmt:
    @just pwf fmt

# Check formatting + clippy -D warnings + Markdown; non-zero on drift.
fmt-check:
    @just pwf fmt-check

# Apply clippy's machine-applicable fixes, then reformat.
fix *args:
    @just pwf fix {{ args }}

# Report production Rust error-handling smells.
smell-check-errors:
    @just pwf smell-check-errors

# Test gate. Default: slim unit+integration. --e2e: binary suites. --all: both. --verbose: full output.
test *flags:
    @just pwf test {{ flags }}

# One-time repo setup for cross-agent skills.
bootstrap *flags:
    @just agents bootstrap {{ flags }}

# Read-only readiness check: required tools/versions + optional health (see doctor.toml).
doctor *args:
    doctor-rs {{ args }}
