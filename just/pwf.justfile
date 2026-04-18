set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set working-directory := '..'

[private]
_binname := if os() == "windows" { "pwf.exe" } else { "pwf" }
[private]
_bin := "target/release" / _binname
[private]
_manifest := "pwf.json"

_preflight:
    @[ -f "{{ _bin }}" ] || cargo build --release

[private]
_require-rg:
    @command -v rg >/dev/null 2>&1 || { printf '%s\n' "ripgrep (rg) is required for error smell checks." >&2; exit 127; }

# Print the pwf engine's command reference.
help: _preflight
    @"{{ _bin }}" --help

# Build the release binary at target/release/pwf.exe.
build:
    cargo build --release

# First-time setup of the global pwf shim.
[unix]
install:
    @printf '%s\n' "No scoop on Linux - run 'just update' (builds, symlinks ~/.local/bin/pwf, and wires PATH)."

# First-time setup of the global pwf shim.
[windows]
install:
    scoop install "{{ _manifest }}"

# Refresh the global pwf shim after a rebuild.
[unix]
update *args: build
    #!/usr/bin/env bash
    set -euo pipefail
    args="{{ args }}"
    case "$args" in
        ""|--dry|--dry-run) ;;
        *)
            printf '%s\n' "just update: unknown args '$args' (use --dry, or no args to refresh)." >&2
            exit 1
            ;;
    esac

    target="$(pwd)/{{ _bin }}"
    link="$HOME/.local/bin/pwf"
    dir="$(dirname "$link")"

    if [ "$args" != "" ]; then
        printf '%s\n' "DRY-RUN: would ensure symlink $link -> $target. No global changes made."
        "$target" --help
        exit 0
    fi

    mkdir -p "$dir"
    if [ -L "$link" ] && [ "$(readlink "$link")" = "$target" ]; then
        printf '%s\n' "Global pwf shim already current ($link -> $target); rebuilt binary is live."
    else
        rm -f "$link"
        ln -s "$target" "$link"
        printf '%s\n' "Linked global pwf shim -> $target"
    fi

    case ":$PATH:" in
        *":$dir:"*) ;;
        *)
            rc="$HOME/.bashrc"
            if [ -f "$rc" ] && grep -F "$dir" "$rc" >/dev/null 2>&1; then
                printf '%s\n' "Warning: $dir not on PATH yet (already in $rc - open a new shell)." >&2
            else
                printf '\nexport PATH="%s:$PATH"\n' "$dir" >> "$rc"
                printf '%s\n' "Added $dir to PATH in $rc (open a new shell)."
            fi
            ;;
    esac

# Refresh the global pwf shim after a rebuild.
[windows]
update *args: build
    #!/usr/bin/env bash
    set -euo pipefail
    args="{{ args }}"
    case "$args" in
        ""|--dry|--dry-run) ;;
        *)
            printf '%s\n' "just update: unknown args '$args' (use --dry, or no args to refresh)." >&2
            exit 1
            ;;
    esac

    dest="$HOME/scoop/apps/pwf/current/pwf.exe"
    if [ "$args" != "" ]; then
        printf '%s\n' "DRY-RUN: would copy {{ _bin }} -> $dest. No global changes made."
        "{{ _bin }}" --help
        exit 0
    fi

    [ -f "$dest" ] || {
        printf '%s\n' "pwf not installed via scoop yet - run 'just install' first." >&2
        exit 1
    }
    cp -f "{{ _bin }}" "$dest"
    printf '%s\n' "Refreshed global pwf shim -> $dest"

# Format the code and lint with clippy.
format:
    cargo fmt
    cargo clippy --all-targets -- -D warnings

# Report production Rust error-handling smells.
smell-check-errors: _require-rg
    #!/usr/bin/env bash
    set -euo pipefail

    status=0

    allow_none() {
        rg '.'
    }

    # String-error compatibility is allowed only at public/final boundaries kept
    # for callers that consume the legacy Result<T, String> API.
    allow_result_string_boundaries() {
        local unmatched=0
        while IFS= read -r line; do
            case "$line" in
                src/main.rs:*'fn run_parsed(parsed: command::ParsedCommand) -> Result<String, String> {'|\
                src/engines/mod.rs:*'pub fn run(engine: &str, args: &Args) -> Result<String, String> {'|\
                src/engines/migrate.rs:*'pub fn run(args: &Args) -> Result<String, String> {'|\
                src/engines/handoff.rs:*'pub fn repo_root(args: &Args) -> Result<PathBuf, String> {'|\
                src/engines/handoff.rs:*'pub fn refresh_ledger(root: &Path) -> Result<(PathBuf, usize), String> {'|\
                src/engines/handoff.rs:*'pub fn run(args: &Args) -> Result<String, String> {'|\
                src/engines/pending_work/query.rs:*'pub fn resolve_managed_project_name(cfg: &Config, name: &str) -> Result<String, String> {'|\
                src/engines/pending_work/query.rs:*'pub fn is_item_open(cfg: &Config, id: &str) -> Result<bool, String> {'|\
                src/engines/pending_work/run.rs:*'pub fn run(command: &PendingWorkCommand) -> Result<String, String> {'|\
                src/engines/pending_work/run.rs:*'pub fn run_args(args: &crate::cli::Args) -> Result<String, String> {'|\
                src/engines/pending_work/parse.rs:*'pub fn newest_handoff(repo: &str) -> Result<PathBuf, String> {' )
                    continue
                    ;;
                src/engines/clean.rs:*') -> Result<String, String> {' )
                    line_no="${line#src/engines/clean.rs:}"
                    line_no="${line_no%%:*}"
                    if awk -v line="$line_no" '
                        NR >= line { exit 1 }
                        /^[[:space:]]*pub fn run_clean\(/ { seen = 1; next }
                        /^[[:space:]]*(pub[[:space:]]+)?fn[[:space:]]+/ { seen = 0 }
                        END { exit seen ? 0 : 1 }
                    ' src/engines/clean.rs; then
                        continue
                    fi
                    ;;
            esac
            printf '%s\n' "$line"
            unmatched=1
        done
        [ "$unmatched" -eq 0 ] && return 1
        return 0
    }

    allow_string_boundary_conversions() {
        rg -v \
            -e '^src/engines/clean\.rs:[0-9]+:\s*\.map_err\(\|error\| error\.to_string\(\)\)' \
            -e '^src/engines/migrate\.rs:[0-9]+:\s*run_typed\(args\)\.map_err\(\|e\| e\.to_string\(\)\)' \
            -e '^src/engines/handoff\.rs:[0-9]+:\s*repo_root_typed\(args\)\.map_err\(\|e\| e\.to_string\(\)\)' \
            -e '^src/engines/handoff\.rs:[0-9]+:\s*refresh_ledger_typed\(root\)\.map_err\(\|e\| e\.to_string\(\)\)' \
            -e '^src/engines/handoff\.rs:[0-9]+:\s*run_typed\(args\)\.map_err\(\|e\| e\.to_string\(\)\)' \
            | rg '.'
    }

    allow_string_err_boundaries() {
        rg -v \
            -e '^src/engines/mod\.rs:[0-9]+:\s*Err\(\(\)\) => Err\(format!\("unknown engine: \{engine\}"\)\),' \
            -e '^src/engines/mod\.rs:[0-9]+:\s*Err\("a pw subcommand is required\."\.to_string\(\)\)' \
            -e '^src/engines/mod\.rs:[0-9]+:\s*Err\("unknown engine: bogus"\.to_string\(\)\)' \
            | rg '.'
    }

    allow_test_only_panics() {
        rg -v \
            -e '^src/command\.rs:[0-9]+:\s*panic!\("expected pending-work command"\);' \
            | rg '.'
    }

    check_rg() {
        label="$1"
        filter="$2"
        shift
        shift
        printf '== %s ==\n' "$label"
        if "$@" | rg -v '^[^:]+:[0-9]+:\s*//' | "$filter"; then
            status=1
        else
            printf '%s\n' "ok"
        fi
        printf '\n'
    }

    check_rg 'Result<_, String> production signatures' \
        allow_result_string_boundaries \
        rg -n 'Result<.*, *String\s*>' src -g '*.rs'
    check_rg 'stringly ok_or/ok_or_else errors' \
        allow_none \
        rg -n 'ok_or\("|ok_or_else\(\|\| ".*"\.to_string\(\)' src -g '*.rs'
    check_rg 'stringly map_err conversions' \
        allow_string_boundary_conversions \
        rg -n 'map_err\(\|[^|]*\| format!|map_err\(\|[^|]*\| [[:alnum:]_]+\.to_string\(\)\)' src -g '*.rs'
    check_rg 'stringly Err construction' \
        allow_string_err_boundaries \
        rg -n 'Err\(format!|Err\(".*"\.to_string\(\)' src -g '*.rs'
    check_rg 'broad error escape hatches' \
        allow_none \
        rg -n 'anyhow|Box<dyn Error>' src Cargo.toml
    check_rg 'production panic/todo/unreachable' \
        allow_test_only_panics \
        rg -n 'panic!|todo!|unreachable!' src -g '*.rs'

    if [ "$status" -ne 0 ]; then
        printf '%s\n' "error-handling smells found; classify or eliminate them before closing the typed-error migration." >&2
    fi
    exit "$status"

# Slim default: in-process unit + integration tests.
test:
    cargo test

# Binary-e2e Rust suites plus the ShellSpec conformance corpus.
test-e2e: _preflight
    cargo test --test cli_e2e --test help_cli
    just pwf test-conformance

# Everything: the slim suite, then the e2e + conformance suite.
test-all: test test-e2e

# Run only the ShellSpec conformance corpus. Extra args pass through to shellspec.
test-conformance *args: _preflight
    #!/usr/bin/env sh
    cd conformance
    exec shellspec {{ args }}
