# Development

## Cross-Building Release Binaries

Talon publishes through the npm wrapper in `ts/` plus one prebuilt binary package per supported platform. The cross-build path uses `cargo-zigbuild` so Linux can produce GNU Linux, macOS, and Windows GNU release artifacts from one script.

Install the release build tools:

```bash
cargo install cargo-zigbuild --locked
brew install zig
```

On Linux, install Zig with your package manager or from <https://ziglang.org/download/> if the packaged version is too old for `cargo-zigbuild`.

Build and package every platform binary:

```bash
just cross-build
```

This runs:

```bash
cargo zigbuild -p talon-cli --release --target aarch64-apple-darwin --locked
cargo zigbuild -p talon-cli --release --target x86_64-apple-darwin --locked
cargo zigbuild -p talon-cli --release --target x86_64-unknown-linux-gnu --locked
cargo zigbuild -p talon-cli --release --target aarch64-unknown-linux-gnu --locked
cargo zigbuild -p talon-cli --release --target x86_64-pc-windows-gnu --locked
```

The script writes platform npm packages under `ts/npm/<platform>-<arch>/`. Each package contains only `package.json` and `bin/talon`; there is no JavaScript in the platform packages. The main `@seanmozeik/talon` package resolves those optional dependencies at runtime.

Each packaged binary must be smaller than 30 MiB. The release profile has `strip = true`, and the script fails if any `bin/talon` exceeds `31,457,280` bytes. Override only for local diagnostics with `TALON_MAX_BINARY_BYTES=<bytes>`.

The script also smoke-runs `talon --version` when a target runtime is available:

- native Linux and macOS targets run directly on matching hosts;
- Linux arm64 can run through `qemu-aarch64`;
- Windows x64 can run through `wine`.

Set `TALON_REQUIRE_TARGET_SMOKE=1` in CI when the runner is expected to have the right native runtime, QEMU, or Wine installed. Use `TALON_SKIP_SMOKE=1` only when checking packaging mechanics locally.
