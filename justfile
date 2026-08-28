set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set windows-shell := ["bash", "-eu", "-o", "pipefail", "-c"]

_default:
    @just --list --unsorted

# Build the release binary.
[group('build')]
build:
    cargo build --release

# First-time setup of the global pwf binary.
[group('build')]
install:
    cargo run --quiet -p xtask -- install

# Converge the Ubuntu development environment and install pwf.
[group('build')]
bootstrap *args:
    mise bootstrap --yes {{ args }}

# Refresh the global pwf binary after a rebuild.
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
    cargo fmt --all

# Check formatting without modifying files.
[group('quality')]
fmt-check:
    cargo fmt --all --check

# Run repository linters.
[group('quality')]
lint:
    buf lint
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy -p pwf-cli --test binary --test e2e -- -D warnings

# Refresh or verify the committed SQLx checked-query cache.
prepare *args:
    mise exec cargo:sqlx-cli -- cargo run --quiet -p xtask -- prepare {{ args }}

# Apply pending SQLite migrations through the dedicated process.
migrate:
    cargo run --quiet -p pwf-migrator

# Run the complete read-only formatting and lint gate.
[group('quality')]
check:
    just fmt-check
    just lint
    ast-grep scan
    just prepare --check

# Apply machine-applicable fixes and reformat.
[group('quality')]
fix *args:
    cargo clippy --fix --workspace --all-targets --allow-dirty {{ args }}
    just fmt

# Report missing mise state without changing the host.
[group('quality')]
doctor:
    @mise ls --local --missing --locked --no-header

# Run tests, or use `just test coverage`; coverage defaults to quiet and forwards cargo-llvm-cov arguments.
[group('quality')]
test *args:
    @cargo run --quiet -p xtask -- test {{ args }}

# Compare Criterion benchmarks against the local baseline. Use --update to replace it.
[arg("benchmark", help="Benchmark target or all", pattern="all|prompt-lanes|obsidian-frontmatter-read|obsidian-markdown-file|obsidian-store-io")]
[arg("case", help="Exact Criterion benchmark case")]
[arg("update", long="update", value="--save-baseline local", help="Compare and replace the local baseline")]
[arg("quick", long="quick", value="--quick", help="Stop once Criterion reaches statistical significance")]
[group('performance')]
bench benchmark="all" case="" update="--baseline local" quick="":
    CRITERION_HOME="{{ justfile_directory() }}/.artifacts/benchmarks/criterion" cargo bench --locked {{ if benchmark == "all" { "--workspace --benches" } else if benchmark == "prompt-lanes" { "-p prompt-lanes --bench prompt_lanes" } else { "-p pwf-infra --bench " + replace(benchmark, "-", "_") } }} -- {{ if case == "" { "" } else { quote(case) + " --exact" } }} {{ update }} {{ quick }}

# Compare deterministic allocation reports against their local baselines.
[arg("update", long="update", value="--update", help="Compare and replace the local baselines")]
[group('performance')]
bench-allocations update="":
    cargo run --quiet --locked --release -p pwf-infra --example obsidian_store_allocations -- {{ update }}
    cargo run --quiet --locked --release -p pwf-infra --example obsidian_markdown_file_allocations -- {{ update }}
    cargo run --quiet --locked --release -p pwf-infra --example obsidian_frontmatter_read_allocations -- {{ update }}
