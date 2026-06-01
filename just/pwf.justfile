set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set working-directory := '..'

[private]
_binname := if os() == "windows" { "pwf.exe" } else { "pwf" }
[private]
_bin := "target/release" / _binname
[private]
_manifest := "pwf.json"

[private]
_require-rg:
    @command -v rg >/dev/null 2>&1 || { printf '%s\n' "ripgrep (rg) is required for error smell checks." >&2; exit 127; }

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
    bash scripts/smell-check-errors.sh

# Test gate. Default: slim in-process unit + integration (cargo test). --e2e: binary suites (cli_e2e, help_cli). --all: both. --verbose: nocapture.
test *flags:
    #!/usr/bin/env bash
    set -euo pipefail

    scope=default
    verbose=0
    for flag in {{ flags }}; do
        case "$flag" in
            --e2e) scope=e2e ;;
            --all) scope=all ;;
            --verbose) verbose=1 ;;
            *)
                printf '%s\n' "just test: unknown flag '$flag' (use --e2e, --all, --verbose)." >&2
                exit 1
                ;;
        esac
    done

    nocapture=()
    [ "$verbose" -eq 1 ] && nocapture=(-- --nocapture)

    run_slim() { cargo test "${nocapture[@]}"; }
    run_e2e() {
        [ -f "{{ _bin }}" ] || cargo build --release
        cargo test --test cli_e2e --test help_cli "${nocapture[@]}"
    }

    case "$scope" in
        default) run_slim ;;
        e2e) run_e2e ;;
        all) run_slim; run_e2e ;;
    esac
