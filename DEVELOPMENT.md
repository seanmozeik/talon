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

## Ranking Quality Eval

Talon ships a golden-set evaluation harness to detect silent ranking drift in CI.

### Running the eval

```bash
cargo test --test eval_suite -p talon-core -- --nocapture
```

Results are written to `crates/talon-core/tests/eval/results/latest.json`.

### Updating the baseline

After a real quality improvement (new chunker, better RRF weights, etc.), update
the committed baseline with the new results:

```bash
cp crates/talon-core/tests/eval/results/latest.json \
   crates/talon-core/tests/eval/baseline.json
```

Then raise the floor constants in `crates/talon-core/tests/ranking_regression.rs`
to match the new baseline minus ~10%.

**Thresholds are raised never lowered.** If a PR lowers metrics, investigate before
merging — do not lower the floor to make the test pass.

### Comparing two runs

```bash
just eval-compare tests/eval/baseline.json tests/eval/results/latest.json
```

This prints a per-metric delta table between two result JSON files.

### Floor calibration

The floor constants in `ranking_regression.rs` were set to measured baseline
minus 10%:

| Mode             | nDCG@5 | MRR   | Hit@5 | Recall@10 |
|------------------|--------|-------|-------|-----------|
| fast (BM25+vec)  | 0.83   | 0.90  | 0.95  | —         |
| default (hybrid) | 0.45   | 0.45  | 0.90  | —         |
| golden set       | 0.85   | 0.85  | 0.95  | 0.90      |

Default mode nDCG is lower than fast because the mock expansion emits fixed
off-topic queries, which injects RRF noise. Real LLM expansion improves this.
