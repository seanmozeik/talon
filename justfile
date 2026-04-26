set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    @just --list

run *ARGS:
    cargo run -p talon-cli -- {{ ARGS }}

run-release *ARGS:
    cargo run -p talon-cli --release -- {{ ARGS }}

search query="zettelkasten atomic notes" limit="10":
    cargo run -p talon-cli -- search "{{ query }}" --limit {{ limit }}

status:
    cargo run -p talon-cli -- status

skill:
    cargo run -p talon-cli -- --skill

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

check: fmt
    cargo check --workspace --all-targets --all-features --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

clippy:
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

test:
    cargo test --workspace --all-targets --locked

test-doc:
    cargo test --doc --workspace --locked

verify: fmt
    cargo check --workspace --all-targets --all-features --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test --workspace --all-targets --locked

build-release:
    cargo build -p talon-cli --release --locked

build-release-linux-x64:
    PATH="$HOME/.cargo/bin:$PATH" "$HOME/.cargo/bin/cargo" zigbuild -p talon-cli --release --target x86_64-unknown-linux-gnu --locked

build-release-linux-arm64:
    PATH="$HOME/.cargo/bin:$PATH" "$HOME/.cargo/bin/cargo" zigbuild -p talon-cli --release --target aarch64-unknown-linux-gnu --locked

zigbuild-target target:
    cargo zigbuild -p talon-cli --release --target {{ target }} --locked

cross-build:
    scripts/build-platform-packages.sh

cross-build-no-smoke:
    TALON_SKIP_SMOKE=1 scripts/build-platform-packages.sh

install:
    cargo install --path crates/talon-cli --locked
