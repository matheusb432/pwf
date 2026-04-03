set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set working-directory := '..'

[private]
_binname := if os() == "windows" { "pwf.exe" } else { "pwf" }
[private]
_bin := "target/release" / _binname

_preflight:
    @[ -f "{{ _bin }}" ] || cargo build --release

# List active handoffs from the invocation repo.
list *args: _preflight
    @"{{ _bin }}" handoff list --repo-root "{{ invocation_directory() }}" {{ args }}

new title *args: _preflight
    @"{{ _bin }}" handoff new --repo-root "{{ invocation_directory() }}" --title "{{ title }}" {{ args }}

done id *args: _preflight
    @"{{ _bin }}" handoff done --repo-root "{{ invocation_directory() }}" --id "{{ id }}" {{ args }}

refresh *args: _preflight
    @"{{ _bin }}" handoff refresh --repo-root "{{ invocation_directory() }}" {{ args }}
