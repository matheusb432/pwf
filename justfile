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

# Slim default: in-process unit + integration tests.
test:
    @just pwf test

# Binary-e2e Rust suites.
test-e2e:
    @just pwf test-e2e

# Everything: slim suite, then binary e2e.
test-all:
    @just pwf test-all

# One-time repo setup for cross-agent skills.
bootstrap *flags:
    @just agents bootstrap {{ flags }}
