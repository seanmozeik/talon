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

mcp-stress turns="100":
    python3 scripts/mcp_stress.py --config examples/config.toml --turns {{ turns }}

example-config-localhost:
    sd 'host\.docker\.internal' 'localhost' examples/config.toml

example-config-docker:
    sd 'localhost' 'host.docker.internal' examples/config.toml

example-config-toggle:
    @if rg -q 'host\.docker\.internal' examples/config.toml; then \
        just example-config-localhost; \
        echo 'examples/config.toml -> localhost'; \
    else \
        just example-config-docker; \
        echo 'examples/config.toml -> host.docker.internal'; \
    fi

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
# Generates npm/package.json, npm/binary.js, and npm/<label>/ for each platform.
pack:
    bun scripts/npm-pack.ts --npm-org seanmozeik

pack-no-smoke:
    bun scripts/npm-pack.ts --npm-org seanmozeik --skip-smoke

# Publish generated npm platform workspaces first, then the root package.
publish-npm: pack-no-smoke
    bun scripts/npm-publish.ts all

publish-npm-platforms: pack-no-smoke
    bun scripts/npm-publish.ts platforms

publish-npm-platform platform: pack-no-smoke
    bun scripts/npm-publish.ts platform {{ platform }}

publish-npm-root: pack-no-smoke
    bun scripts/npm-publish.ts root

publish-npm-dry-run: pack-no-smoke
    bun scripts/npm-publish.ts all --dry-run

# ── Install from source (host platform only) ──────────────────────
install:
    cargo install --path crates/talon-cli --locked

install-debug:
    cargo install --path crates/talon-cli --locked --debug --force

# Run the ranking eval suite and write results to tests/eval/results/latest.json.
eval:
    cargo nextest run --test eval_suite -p talon-core --no-capture

# Compare two eval result JSON files and print per-metric deltas.
# Usage: just eval-compare baseline.json latest.json
eval-compare file_a file_b:
    python3 scripts/eval_compare.py {{ file_a }} {{ file_b }}
