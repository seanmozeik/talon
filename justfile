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

# ── crates.io publish ─────────────────────────────────────────────
# Workspace publish order: talon-core, then talon-cli (cli depends on core).
# The sleep gives the crates.io index a moment to surface talon-core before
# talon-cli tries to resolve it. Bump if you see "no matching package".

publish-cargo-dry-run:
    cargo publish -p talon-core --dry-run --locked
    cargo publish -p talon-cli --dry-run --locked

publish-cargo:
    cargo publish -p talon-core --locked
    @echo "Waiting 30s for crates.io to index talon-core..."
    sleep 30
    cargo publish -p talon-cli --locked

# ── Homebrew tarballs ─────────────────────────────────────────────
# Repackage the multi-platform binaries from target/ into dist/ as
# brew-compatible tarballs (single executable at the root). Run after
# `just build-all`. Windows is npm-only — brew doesn't ship there.

dist:
    rm -rf dist && mkdir -p dist
    @bash -c ' \
        set -eu; \
        for t in aarch64-apple-darwin:darwin-arm64 x86_64-apple-darwin:darwin-x64 aarch64-unknown-linux-gnu:linux-arm64 x86_64-unknown-linux-gnu:linux-x64; do \
            triple="${t%%:*}"; label="${t##*:}"; \
            src="target/$triple/release/talon"; \
            if [ ! -f "$src" ]; then echo "missing $src — run just build-all first" >&2; exit 1; fi; \
            cp "$src" "dist/talon-$label"; \
            chmod 0755 "dist/talon-$label"; \
            tar -czf "dist/talon-$label.tar.gz" -C dist "talon-$label"; \
            rm "dist/talon-$label"; \
            echo "  packaged dist/talon-$label.tar.gz"; \
        done'

# ── Homebrew formula ──────────────────────────────────────────────
# Render Formula/talon.rb against the tarballs in dist/.

brew-formula VERSION:
    bun scripts/brew-formula.ts --version {{ VERSION }}

# Copy the rendered formula into the tap repo and push.
# Assumes ~/dev/tap is the seanmozeik/tap clone.
publish-brew VERSION: (brew-formula VERSION)
    cp Formula/talon.rb ~/dev/tap/Formula/talon.rb
    @bash -c 'cd ~/dev/tap && git add Formula/talon.rb && git commit -m "talon {{ VERSION }}" && git push'

# ── GitHub release ────────────────────────────────────────────────
# Create the GH release with the dist/ tarballs attached. Pulls the
# top-of-CHANGELOG section as the release notes.

release-github VERSION:
    @bash -c ' \
        set -eu; \
        if [ -f CHANGELOG.md ]; then \
            notes=$(awk "/^## \\[/{count++} count==1" CHANGELOG.md | tail -n +2); \
            gh release create "v{{ VERSION }}" dist/*.tar.gz \
                --title "v{{ VERSION }}" --notes "$notes"; \
        else \
            gh release create "v{{ VERSION }}" dist/*.tar.gz \
                --title "v{{ VERSION }}" --generate-notes; \
        fi'

# ── Umbrella release ──────────────────────────────────────────────
# Full release flow. Bump Cargo.toml workspace version + CHANGELOG.md
# manually first, commit + tag, then run `just release VERSION`.
#
#   build-all       → produce 5 platform binaries in target/
#   dist            → flat tarballs for brew (4 platforms; no win32)
#   release-github  → GH release with tarballs and changelog notes
#   publish-brew    → render formula, copy to ~/dev/tap, push
#   publish-cargo   → cargo publish talon-core then talon-cli
#   publish-npm     → existing npm flow (pack + publish all workspaces)

release VERSION:
    just build-all
    just dist
    just release-github {{ VERSION }}
    just publish-brew {{ VERSION }}
    just publish-cargo
    just publish-npm

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
