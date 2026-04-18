set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

mod agents 'just/agents.justfile'
mod handoffs 'just/handoffs.justfile'
mod pwf 'just/pwf.justfile'

_default:
    @just --list --unsorted --list-submodules

# Print the pwf engine's command reference.
help:
    @just pwf help

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

# Binary-e2e Rust suites plus ShellSpec conformance.
test-e2e:
    @just pwf test-e2e

# Everything: slim suite, then e2e + conformance.
test-all:
    @just pwf test-all

# Run only the ShellSpec conformance corpus.
test-conformance *args:
    @just pwf test-conformance {{ args }}

# List active handoffs from the invocation repo.
handoff *args:
    @just handoffs list {{ args }}

handoff-new title *args:
    @just handoffs new "{{ title }}" {{ args }}

handoff-done id *args:
    @just handoffs done "{{ id }}" {{ args }}

handoff-refresh *args:
    @just handoffs refresh {{ args }}

# One-time repo setup for cross-agent skills.
bootstrap *flags:
    @just agents bootstrap {{ flags }}
