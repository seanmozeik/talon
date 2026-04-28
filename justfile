set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    @just --list

run *ARGS:
    cargo run -q -p talon-cli -- {{ ARGS }}

run-release *ARGS:
    cargo run -p talon-cli --release -- {{ ARGS }}

search query="zettelkasten atomic notes" limit="10":
    cargo run -p talon-cli -- search "{{ query }}" --limit {{ limit }}

status:
    cargo run -p talon-cli -- status

skill:
    cargo run -p talon-cli -- --skill

fmt:
    rtk cargo fmt --all

fmt-check:
    cargo fmt --all --check

# Fail if Rust source files grow beyond the maintainability budget.
rust-line-counts:
    @fd --type f --extension rs . crates -X wc -l \
        | sort -nr \
        | awk 'BEGIN { limit = 350; red = "\033[31m"; yellow = "\033[33m"; bold = "\033[1m"; reset = "\033[0m" } $2 != "total" && $1 > limit { if (!bad) { printf "%s%sRust file line-count violations%s\n", bold, red, reset > "/dev/stderr"; printf "%sLimit:%s %d lines\n\n", yellow, reset, limit > "/dev/stderr" } printf "  %s%5d%s  %s\n", red, $1, reset, $2 > "/dev/stderr"; bad = 1 } END { if (bad) { printf "\n%sFound oversized Rust files. Split modules or add a narrow exception.%s\n", yellow, reset > "/dev/stderr" } exit bad + 0 }'

check: fmt
    rtk cargo check --workspace --all-targets --all-features --locked
    rtk cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    just rust-line-counts

clippy:
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

test:
    rtk cargo nextest run --workspace --all-targets --locked

test-doc:
    cargo test --doc --workspace --locked

verify: fmt
    rtk cargo check --workspace --all-targets --all-features --locked
    rtk cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    rtk cargo nextest run --workspace --all-targets --locked
    rtk cargo test --doc --workspace --locked
    just rust-line-counts

# ── Build (into target/ directory) ────────────────────────────────
# Each target builds independently. Run individually or all at once.

build-release:
    cargo build -p talon-cli --release --locked

build-release-darwin-arm64:
    cargo build -p talon-cli --release --target aarch64-apple-darwin --locked

build-release-darwin-x64:
    PATH="$HOME/.cargo/bin:$PATH" "$HOME/.cargo/bin/cargo" zigbuild -p talon-cli --release --target x86_64-apple-darwin --locked

build-release-linux-x64:
    PATH="$HOME/.cargo/bin:$PATH" "$HOME/.cargo/bin/cargo" zigbuild -p talon-cli --release --target x86_64-unknown-linux-gnu --locked

build-release-linux-arm64:
    PATH="$HOME/.cargo/bin:$PATH" "$HOME/.cargo/bin/cargo" zigbuild -p talon-cli --release --target aarch64-unknown-linux-gnu --locked

build-release-win32-x64:
    PATH="$HOME/.cargo/bin:$PATH" "$HOME/.cargo/bin/cargo" zigbuild -p talon-cli --release --target x86_64-pc-windows-gnu --locked

# Build all 5 platform targets
build-all:
    just build-release-darwin-arm64
    just build-release-darwin-x64
    just build-release-linux-x64
    just build-release-linux-arm64
    just build-release-win32-x64

# ── NPM packaging (takes already-built binaries from target/) ─────
# Run `just build-all` first, then `just pack`. No building happens here.
pack:
    bun scripts/npm-pack.ts --npm-org seanmozeik --outdir ./ts/npm

pack-no-smoke:
    bun scripts/npm-pack.ts --npm-org seanmozeik --outdir ./ts/npm --skip-smoke

# ── Install from source (host platform only) ──────────────────────
install:
    cargo install --path crates/talon-cli --locked

# Run the ranking eval suite and write results to tests/eval/results/latest.json.
eval:
    cargo nextest run --test eval_suite -p talon-core --no-capture

# Compare two eval result JSON files and print per-metric deltas.
# Usage: just eval-compare baseline.json latest.json
eval-compare file_a file_b:
    python3 scripts/eval_compare.py {{ file_a }} {{ file_b }}
