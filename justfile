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

# Format the code and lint with clippy.
format:
    @just pwf format

# Report production Rust error-handling smells.
smell-check-errors:
    @just pwf smell-check-errors

# Test gate. Default: slim in-process unit + integration (quiet). --e2e: binary suites. --all: both. --verbose: full per-test output + nocapture.
test *flags:
    @just pwf test {{ flags }}

# One-time repo setup for cross-agent skills.
bootstrap *flags:
    @just agents bootstrap {{ flags }}

# Read-only readiness check: required tools/versions + optional health (see doctor.toml). Run first on a new clone. --json for machine output.
doctor *args:
    doctor-rs {{ args }}
