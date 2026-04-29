---
name: rust
description: >
  Comprehensive Rust coding guidelines with 179 rules across 14 categories plus
  9 deep-dive reference chapters. Use when writing, reviewing, or refactoring
  Rust code. Covers ownership, error handling, async patterns, API design,
  memory optimization, performance, testing, and common anti-patterns.
license: MIT
compatibility: Rust 2024 edition, Cargo
metadata:
  author: leonardomso, apollographql
  version: "2.0.0"
  sources:
    - Rust API Guidelines
    - Rust Performance Book
    - Apollo GraphQL Rust Best Practices Handbook
    - ripgrep, tokio, serde, polars codebases
allowed-tools: Bash(cargo:*) Bash(rustc:*) Bash(rustfmt:*) Bash(clippy:*) Read Write Edit Glob Grep
---

# Rust Best Practices

Comprehensive guide for writing high-quality, idiomatic, and highly optimized Rust code. Contains 179 rules across 14 categories, prioritized by impact, plus 9 deep-dive reference chapters with detailed examples and nuanced explanations.

## Project Conventions

These overrides take precedence over the general rules below:

- **Rust edition**: Use the 2024 edition (`edition = "2024"`) for any new crates.
- **Adding dependencies**: Prefer `cargo add <crate>` over editing `Cargo.toml` by hand, so Cargo resolves the latest version instead of relying on potentially outdated training data. When you do specify versions manually, always use full semver — major, minor, _and_ patch (e.g. `dependency = "1.0.123"`, not `dependency = "1.0"`).
- **Error handling**: Use `eyre` / `color-eyre` instead of `anyhow`. Use `eyre::Result` and `.wrap_err()` / `.wrap_err_with()` instead of `.context()` / `.with_context()`.
- **Time**: Use the `time` crate instead of `chrono`.
- **Filesystem**: Use the `fs-err` crate instead of `std::fs`. For async code, enable the `tokio` feature on `fs-err` and use `fs_err::tokio` instead of `tokio::fs`.

### Lean into the type system

Default to making invalid states unrepresentable at compile time. Don't write "vanilla" Rust that passes primitives around when stronger types exist. Reach for these features proactively:

- **Newtypes over primitives**: Wrap domain concepts (`UserId(u64)`, `Pixels(f32)`, `Millimeters(f64)`) so the compiler catches unit/semantic mismatches. Derive arithmetic traits via `derive_more` when the newtype should support math.
- **Enums over booleans and strings**: `Mode::Dark` / `Mode::Light` beats `is_dark: bool`. If a function takes two booleans, that's a bug waiting to happen — use enums or newtypes.
- **Typestate for lifecycles**: If an API has phases (disconnected → connected, draft → published), encode them as type parameters so calling methods in the wrong state is a compile error, not a runtime panic.
- **Const generics**: Use `[T; N]` and `const N: usize` for fixed-size arrays, buffer sizes, and dimension parameters instead of runtime checks.
- **Associated types over generic params**: When a trait has exactly one meaningful impl per type, use an associated type (`type Output`) rather than a generic parameter — it simplifies call sites and makes the relationship clearer.
- **GATs (Generic Associated Types)**: Use for lending iterators, async traits returning borrowed data, and collection-generic code where the lifetime or type depends on the impl.
- **Sealed traits with marker types**: Combine sealed traits with zero-sized marker types to build type-level state machines that external code can use but not extend.
- **`TryFrom`/`TryInto` at boundaries**: Parse external input into validated types at the boundary. Once parsed, the inner code works with types that are valid by construction — no re-checking needed.
- **`#[non_exhaustive]`**: Use on public enums and structs to allow future variants/fields without breaking downstream.
- **Zero-sized types (ZSTs)**: Use for compile-time tags, permission tokens, and phantom state — they carry no runtime cost but prevent misuse at the type level.
- **Higher-ranked trait bounds**: Use `for<'a> F: Fn(&'a T) -> &'a U` when closures need to work with any lifetime, common in iterator adapters and callback APIs.
- **`where` clauses for readability**: Move complex bounds to `where` clauses — especially when combining multiple trait bounds, const generics, or lifetime constraints.

## When to Apply

Reference these guidelines when:

- Writing new Rust functions, structs, or modules
- Implementing error handling or async code
- Designing public APIs for libraries
- Reviewing code for ownership/borrowing issues
- Optimizing memory usage or reducing allocations
- Tuning performance for hot paths
- Refactoring existing Rust code

## Deep-Dive Reference Chapters

**You MUST read the relevant chapters below before writing or reviewing significant Rust code.** Don't rely on the quick-reference summaries alone — the chapters contain nuanced guidance, edge cases, and real examples that change how you apply the rules. Read them in parallel at the start of a task.

| When you're doing...               | Read these chapters first               |
| ---------------------------------- | --------------------------------------- |
| Writing new functions or structs   | Ch 1 (idioms), Ch 8 (docs)              |
| Error handling                     | Ch 4 (error handling)                   |
| Designing APIs or type hierarchies | Ch 6 (generics), Ch 7 (typestate)       |
| Performance-sensitive code         | Ch 3 (performance), Ch 9 (pointers)     |
| Writing tests                      | Ch 5 (testing)                          |
| Reviewing code or running clippy   | Ch 2 (linting), Ch 1 (idioms)           |
| Shared/concurrent data             | Ch 9 (pointers — Send/Sync, Arc, Mutex) |

- [Chapter 1 - Coding Styles and Idioms](references/chapter_01.md): Borrowing vs cloning, Copy trait, Option/Result handling, iterators, let-else patterns
- [Chapter 2 - Clippy and Linting](references/chapter_02.md): Workspace lint config, `#[expect]` vs `#[allow]`, critical lints to watch
- [Chapter 3 - Performance Mindset](references/chapter_03.md): Profiling first, stack vs heap, zero-cost abstractions, flamegraphs
- [Chapter 4 - Error Handling](references/chapter_04.md): Error hierarchies, `#[from]`/`#[source]`, async error bounds, `inspect_err` chains
- [Chapter 5 - Automated Testing](references/chapter_05.md): Test as documentation, snapshot testing trade-offs, doc-test advantages
- [Chapter 6 - Generics and Dispatch](references/chapter_06.md): Static vs dynamic dispatch trade-offs, object safety rules, when to box
- [Chapter 7 - Type State Pattern](references/chapter_07.md): Phantom types, builders with required fields, encoding protocols in types
- [Chapter 8 - Comments vs Documentation](references/chapter_08.md): Comments rot — use `///` doc comments, ADRs, and tests instead
- [Chapter 9 - Understanding Pointers](references/chapter_09.md): Pointer selection table, Send/Sync guarantees, Cell/OnceCell/OnceLock

Similarly, when a rule in the index below is relevant to your task, **read the linked rule file** in `rules/` — the one-line summary is not enough. The rule files contain code examples, edge cases, and rationale.

## Rule Categories by Priority

| Priority | Category              | Impact    | Prefix   | Rules |
| -------- | --------------------- | --------- | -------- | ----- |
| 1        | Ownership & Borrowing | CRITICAL  | `own-`   | 12    |
| 2        | Error Handling        | CRITICAL  | `err-`   | 12    |
| 3        | Memory Optimization   | CRITICAL  | `mem-`   | 15    |
| 4        | API Design            | HIGH      | `api-`   | 15    |
| 5        | Async/Await           | HIGH      | `async-` | 15    |
| 6        | Compiler Optimization | HIGH      | `opt-`   | 12    |
| 7        | Naming Conventions    | MEDIUM    | `name-`  | 16    |
| 8        | Type Safety           | MEDIUM    | `type-`  | 10    |
| 9        | Testing               | MEDIUM    | `test-`  | 13    |
| 10       | Documentation         | MEDIUM    | `doc-`   | 11    |
| 11       | Performance Patterns  | MEDIUM    | `perf-`  | 11    |
| 12       | Project Structure     | LOW       | `proj-`  | 11    |
| 13       | Clippy & Linting      | LOW       | `lint-`  | 11    |
| 14       | Anti-patterns         | REFERENCE | `anti-`  | 15    |

---

## Quick Reference

### Borrowing & Ownership

- Prefer `&T` over `.clone()` unless ownership transfer is required
- Use `&str` over `String`, `&[T]` over `Vec<T>` in function parameters
- Small `Copy` types (<=24 bytes) can be passed by value
- Use `Cow<'_, T>` when ownership is ambiguous

### Error Handling

- Return `Result<T, E>` for fallible operations; avoid `panic!` in production
- Never use `unwrap()`/`expect()` outside tests
- Use `thiserror` for library errors, `anyhow` for binaries only
- Prefer `?` operator over match chains for error propagation

### Performance

- Always benchmark with `--release` flag
- Run `cargo clippy -- -D clippy::perf` for performance hints
- Avoid cloning in loops; use `.iter()` instead of `.into_iter()` for Copy types
- Prefer iterators over manual loops; avoid intermediate `.collect()` calls

### Linting

Run regularly: `cargo clippy --all-targets --all-features --locked -- -D warnings`

Key lints to watch:

- `redundant_clone` - unnecessary cloning
- `large_enum_variant` - oversized variants (consider boxing)
- `needless_collect` - premature collection

Use `#[expect(clippy::lint)]` over `#[allow(...)]` with justification comment.

### Testing

- Name tests descriptively: `process_should_return_error_when_input_empty()`
- One assertion per test when possible
- Use doc tests (`///`) for public API examples
- Consider `cargo insta` for snapshot testing generated output

### Generics & Dispatch

- Prefer generics (static dispatch) for performance-critical code
- Use `dyn Trait` only when heterogeneous collections are needed
- Box at API boundaries, not internally

### Type State Pattern

Encode valid states in the type system to catch invalid operations at compile time:

```rust
struct Connection<State> { /* ... */ _state: PhantomData<State> }
struct Disconnected;
struct Connected;

impl Connection<Connected> {
    fn send(&self, data: &[u8]) { /* only connected can send */ }
}
```

### Documentation

- `//` comments explain _why_ (safety, workarounds, design rationale)
- `///` doc comments explain _what_ and _how_ for public APIs
- Every `TODO` needs a linked issue: `// TODO(#42): ...`
- Enable `#![deny(missing_docs)]` for libraries

---

## Rule Index

### 1. Ownership & Borrowing (CRITICAL)

- [`own-borrow-over-clone`](rules/own-borrow-over-clone.md) - Prefer `&T` borrowing over `.clone()`
- [`own-slice-over-vec`](rules/own-slice-over-vec.md) - Accept `&[T]` not `&Vec<T>`, `&str` not `&String`
- [`own-cow-conditional`](rules/own-cow-conditional.md) - Use `Cow<'a, T>` for conditional ownership
- [`own-arc-shared`](rules/own-arc-shared.md) - Use `Arc<T>` for thread-safe shared ownership
- [`own-rc-single-thread`](rules/own-rc-single-thread.md) - Use `Rc<T>` for single-threaded sharing
- [`own-refcell-interior`](rules/own-refcell-interior.md) - Use `RefCell<T>` for interior mutability (single-thread)
- [`own-mutex-interior`](rules/own-mutex-interior.md) - Use `Mutex<T>` for interior mutability (multi-thread)
- [`own-rwlock-readers`](rules/own-rwlock-readers.md) - Use `RwLock<T>` when reads dominate writes
- [`own-copy-small`](rules/own-copy-small.md) - Derive `Copy` for small, trivial types
- [`own-clone-explicit`](rules/own-clone-explicit.md) - Make `Clone` explicit, avoid implicit copies
- [`own-move-large`](rules/own-move-large.md) - Move large data instead of cloning
- [`own-lifetime-elision`](rules/own-lifetime-elision.md) - Rely on lifetime elision when possible

### 2. Error Handling (CRITICAL)

- [`err-thiserror-lib`](rules/err-thiserror-lib.md) - Use `thiserror` for library error types
- [`err-anyhow-app`](rules/err-anyhow-app.md) - Use `anyhow` for application error handling
- [`err-result-over-panic`](rules/err-result-over-panic.md) - Return `Result`, don't panic on expected errors
- [`err-context-chain`](rules/err-context-chain.md) - Add context with `.context()` or `.with_context()`
- [`err-no-unwrap-prod`](rules/err-no-unwrap-prod.md) - Never use `.unwrap()` in production code
- [`err-expect-bugs-only`](rules/err-expect-bugs-only.md) - Use `.expect()` only for programming errors; if `expect_used = "deny"`, use typestate/newtype patterns instead
- [`err-question-mark`](rules/err-question-mark.md) - Use `?` operator for clean propagation
- [`err-from-impl`](rules/err-from-impl.md) - Use `#[from]` for automatic error conversion
- [`err-source-chain`](rules/err-source-chain.md) - Use `#[source]` to chain underlying errors
- [`err-lowercase-msg`](rules/err-lowercase-msg.md) - Error messages: lowercase, no trailing punctuation
- [`err-doc-errors`](rules/err-doc-errors.md) - Document errors with `# Errors` section
- [`err-custom-type`](rules/err-custom-type.md) - Create custom error types, not `Box<dyn Error>`

### 3. Memory Optimization (CRITICAL)

- [`mem-with-capacity`](rules/mem-with-capacity.md) - Use `with_capacity()` when size is known
- [`mem-smallvec`](rules/mem-smallvec.md) - Use `SmallVec` for usually-small collections
- [`mem-arrayvec`](rules/mem-arrayvec.md) - Use `ArrayVec` for bounded-size collections
- [`mem-box-large-variant`](rules/mem-box-large-variant.md) - Box large enum variants to reduce type size
- [`mem-boxed-slice`](rules/mem-boxed-slice.md) - Use `Box<[T]>` instead of `Vec<T>` when fixed
- [`mem-thinvec`](rules/mem-thinvec.md) - Use `ThinVec` for often-empty vectors
- [`mem-clone-from`](rules/mem-clone-from.md) - Use `clone_from()` to reuse allocations
- [`mem-reuse-collections`](rules/mem-reuse-collections.md) - Reuse collections with `clear()` in loops
- [`mem-avoid-format`](rules/mem-avoid-format.md) - Avoid `format!()` when string literals work
- [`mem-write-over-format`](rules/mem-write-over-format.md) - Use `write!()` instead of `format!()`
- [`mem-arena-allocator`](rules/mem-arena-allocator.md) - Use arena allocators for batch allocations
- [`mem-zero-copy`](rules/mem-zero-copy.md) - Use zero-copy patterns with slices and `Bytes`
- [`mem-compact-string`](rules/mem-compact-string.md) - Use `CompactString` for small string optimization
- [`mem-smaller-integers`](rules/mem-smaller-integers.md) - Use smallest integer type that fits
- [`mem-assert-type-size`](rules/mem-assert-type-size.md) - Assert hot type sizes to prevent regressions

### 4. API Design (HIGH)

- [`api-builder-pattern`](rules/api-builder-pattern.md) - Use Builder pattern for complex construction
- [`api-builder-must-use`](rules/api-builder-must-use.md) - Add `#[must_use]` to builder types
- [`api-newtype-safety`](rules/api-newtype-safety.md) - Use newtypes for type-safe distinctions
- [`api-typestate`](rules/api-typestate.md) - Use typestate for compile-time state machines
- [`api-sealed-trait`](rules/api-sealed-trait.md) - Seal traits to prevent external implementations
- [`api-extension-trait`](rules/api-extension-trait.md) - Use extension traits to add methods to foreign types
- [`api-parse-dont-validate`](rules/api-parse-dont-validate.md) - Parse into validated types at boundaries
- [`api-impl-into`](rules/api-impl-into.md) - Accept `impl Into<T>` for flexible string inputs
- [`api-impl-asref`](rules/api-impl-asref.md) - Accept `impl AsRef<T>` for borrowed inputs
- [`api-must-use`](rules/api-must-use.md) - Add `#[must_use]` to `Result` returning functions
- [`api-non-exhaustive`](rules/api-non-exhaustive.md) - Use `#[non_exhaustive]` for future-proof enums/structs
- [`api-from-not-into`](rules/api-from-not-into.md) - Implement `From`, not `Into` (auto-derived)
- [`api-default-impl`](rules/api-default-impl.md) - Implement `Default` for sensible defaults
- [`api-common-traits`](rules/api-common-traits.md) - Implement `Debug`, `Clone`, `PartialEq` eagerly
- [`api-serde-optional`](rules/api-serde-optional.md) - Gate `Serialize`/`Deserialize` behind feature flag

### 5. Async/Await (HIGH)

- [`async-tokio-runtime`](rules/async-tokio-runtime.md) - Use Tokio for production async runtime
- [`async-no-lock-await`](rules/async-no-lock-await.md) - Never hold `Mutex`/`RwLock` across `.await`
- [`async-spawn-blocking`](rules/async-spawn-blocking.md) - Use `spawn_blocking` for CPU-intensive work
- [`async-tokio-fs`](rules/async-tokio-fs.md) - Use `tokio::fs` not `std::fs` in async code
- [`async-cancellation-token`](rules/async-cancellation-token.md) - Use `CancellationToken` for graceful shutdown
- [`async-join-parallel`](rules/async-join-parallel.md) - Use `tokio::join!` for parallel operations
- [`async-try-join`](rules/async-try-join.md) - Use `tokio::try_join!` for fallible parallel ops
- [`async-select-racing`](rules/async-select-racing.md) - Use `tokio::select!` for racing/timeouts
- [`async-bounded-channel`](rules/async-bounded-channel.md) - Use bounded channels for backpressure
- [`async-mpsc-queue`](rules/async-mpsc-queue.md) - Use `mpsc` for work queues
- [`async-broadcast-pubsub`](rules/async-broadcast-pubsub.md) - Use `broadcast` for pub/sub patterns
- [`async-watch-latest`](rules/async-watch-latest.md) - Use `watch` for latest-value sharing
- [`async-oneshot-response`](rules/async-oneshot-response.md) - Use `oneshot` for request/response
- [`async-joinset-structured`](rules/async-joinset-structured.md) - Use `JoinSet` for dynamic task groups
- [`async-clone-before-await`](rules/async-clone-before-await.md) - Clone data before await, release locks

### 6. Compiler Optimization (HIGH)

- [`opt-inline-small`](rules/opt-inline-small.md) - Use `#[inline]` for small hot functions
- [`opt-inline-always-rare`](rules/opt-inline-always-rare.md) - Use `#[inline(always)]` sparingly
- [`opt-inline-never-cold`](rules/opt-inline-never-cold.md) - Use `#[inline(never)]` for cold paths
- [`opt-cold-unlikely`](rules/opt-cold-unlikely.md) - Use `#[cold]` for error/unlikely paths
- [`opt-likely-hint`](rules/opt-likely-hint.md) - Use `likely()`/`unlikely()` for branch hints
- [`opt-lto-release`](rules/opt-lto-release.md) - Enable LTO in release builds
- [`opt-codegen-units`](rules/opt-codegen-units.md) - Use `codegen-units = 1` for max optimization
- [`opt-pgo-profile`](rules/opt-pgo-profile.md) - Use PGO for production builds
- [`opt-target-cpu`](rules/opt-target-cpu.md) - Set `target-cpu=native` for local builds
- [`opt-bounds-check`](rules/opt-bounds-check.md) - Use iterators to avoid bounds checks
- [`opt-simd-portable`](rules/opt-simd-portable.md) - Use portable SIMD for data-parallel ops
- [`opt-cache-friendly`](rules/opt-cache-friendly.md) - Design cache-friendly data layouts (SoA)

### 7. Naming Conventions (MEDIUM)

- [`name-types-camel`](rules/name-types-camel.md) - Use `UpperCamelCase` for types, traits, enums
- [`name-variants-camel`](rules/name-variants-camel.md) - Use `UpperCamelCase` for enum variants
- [`name-funcs-snake`](rules/name-funcs-snake.md) - Use `snake_case` for functions, methods, modules
- [`name-consts-screaming`](rules/name-consts-screaming.md) - Use `SCREAMING_SNAKE_CASE` for constants/statics
- [`name-lifetime-short`](rules/name-lifetime-short.md) - Use short lowercase lifetimes: `'a`, `'de`, `'src`
- [`name-type-param-single`](rules/name-type-param-single.md) - Use single uppercase for type params: `T`, `E`, `K`, `V`
- [`name-as-free`](rules/name-as-free.md) - `as_` prefix: free reference conversion
- [`name-to-expensive`](rules/name-to-expensive.md) - `to_` prefix: expensive conversion
- [`name-into-ownership`](rules/name-into-ownership.md) - `into_` prefix: ownership transfer
- [`name-no-get-prefix`](rules/name-no-get-prefix.md) - No `get_` prefix for simple getters
- [`name-is-has-bool`](rules/name-is-has-bool.md) - Use `is_`, `has_`, `can_` for boolean methods
- [`name-iter-convention`](rules/name-iter-convention.md) - Use `iter`/`iter_mut`/`into_iter` for iterators
- [`name-iter-method`](rules/name-iter-method.md) - Name iterator methods consistently
- [`name-iter-type-match`](rules/name-iter-type-match.md) - Iterator type names match method
- [`name-acronym-word`](rules/name-acronym-word.md) - Treat acronyms as words: `Uuid` not `UUID`
- [`name-crate-no-rs`](rules/name-crate-no-rs.md) - Crate names: no `-rs` suffix

### 8. Type Safety (MEDIUM)

- [`type-newtype-ids`](rules/type-newtype-ids.md) - Wrap IDs in newtypes: `UserId(u64)`
- [`type-newtype-validated`](rules/type-newtype-validated.md) - Newtypes for validated data: `Email`, `Url`
- [`type-enum-states`](rules/type-enum-states.md) - Use enums for mutually exclusive states
- [`type-option-nullable`](rules/type-option-nullable.md) - Use `Option<T>` for nullable values
- [`type-result-fallible`](rules/type-result-fallible.md) - Use `Result<T, E>` for fallible operations
- [`type-phantom-marker`](rules/type-phantom-marker.md) - Use `PhantomData<T>` for type-level markers
- [`type-never-diverge`](rules/type-never-diverge.md) - Use `!` type for functions that never return
- [`type-generic-bounds`](rules/type-generic-bounds.md) - Add trait bounds only where needed
- [`type-no-stringly`](rules/type-no-stringly.md) - Avoid stringly-typed APIs, use enums/newtypes
- [`type-repr-transparent`](rules/type-repr-transparent.md) - Use `#[repr(transparent)]` for FFI newtypes

### 9. Testing (MEDIUM)

- [`test-cfg-test-module`](rules/test-cfg-test-module.md) - Use `#[cfg(test)] mod tests { }`
- [`test-use-super`](rules/test-use-super.md) - Use `use super::*;` in test modules
- [`test-integration-dir`](rules/test-integration-dir.md) - Put integration tests in `tests/` directory
- [`test-descriptive-names`](rules/test-descriptive-names.md) - Use descriptive test names
- [`test-arrange-act-assert`](rules/test-arrange-act-assert.md) - Structure tests as arrange/act/assert
- [`test-proptest-properties`](rules/test-proptest-properties.md) - Use `proptest` for property-based testing
- [`test-mockall-mocking`](rules/test-mockall-mocking.md) - Use `mockall` for trait mocking
- [`test-mock-traits`](rules/test-mock-traits.md) - Use traits for dependencies to enable mocking
- [`test-fixture-raii`](rules/test-fixture-raii.md) - Use RAII pattern (Drop) for test cleanup
- [`test-tokio-async`](rules/test-tokio-async.md) - Use `#[tokio::test]` for async tests
- [`test-should-panic`](rules/test-should-panic.md) - Use `#[should_panic]` for panic tests
- [`test-criterion-bench`](rules/test-criterion-bench.md) - Use `criterion` for benchmarking
- [`test-doctest-examples`](rules/test-doctest-examples.md) - Keep doc examples as executable tests

### 10. Documentation (MEDIUM)

- [`doc-all-public`](rules/doc-all-public.md) - Document all public items with `///`
- [`doc-module-inner`](rules/doc-module-inner.md) - Use `//!` for module-level documentation
- [`doc-examples-section`](rules/doc-examples-section.md) - Include `# Examples` with runnable code
- [`doc-errors-section`](rules/doc-errors-section.md) - Include `# Errors` for fallible functions
- [`doc-panics-section`](rules/doc-panics-section.md) - Include `# Panics` for panicking functions
- [`doc-safety-section`](rules/doc-safety-section.md) - Include `# Safety` for unsafe functions
- [`doc-question-mark`](rules/doc-question-mark.md) - Use `?` in examples, not `.unwrap()`
- [`doc-hidden-setup`](rules/doc-hidden-setup.md) - Use `# ` prefix to hide example setup code
- [`doc-intra-links`](rules/doc-intra-links.md) - Use intra-doc links: `[Vec]`
- [`doc-link-types`](rules/doc-link-types.md) - Link related types and functions in docs
- [`doc-cargo-metadata`](rules/doc-cargo-metadata.md) - Fill `Cargo.toml` metadata

### 11. Performance Patterns (MEDIUM)

- [`perf-iter-over-index`](rules/perf-iter-over-index.md) - Prefer iterators over manual indexing
- [`perf-iter-lazy`](rules/perf-iter-lazy.md) - Keep iterators lazy, collect() only when needed
- [`perf-collect-once`](rules/perf-collect-once.md) - Don't `collect()` intermediate iterators
- [`perf-entry-api`](rules/perf-entry-api.md) - Use `entry()` API for map insert-or-update
- [`perf-drain-reuse`](rules/perf-drain-reuse.md) - Use `drain()` to reuse allocations
- [`perf-extend-batch`](rules/perf-extend-batch.md) - Use `extend()` for batch insertions
- [`perf-chain-avoid`](rules/perf-chain-avoid.md) - Avoid `chain()` in hot loops
- [`perf-collect-into`](rules/perf-collect-into.md) - Use `collect_into()` for reusing containers
- [`perf-black-box-bench`](rules/perf-black-box-bench.md) - Use `black_box()` in benchmarks
- [`perf-release-profile`](rules/perf-release-profile.md) - Optimize release profile settings
- [`perf-profile-first`](rules/perf-profile-first.md) - Profile before optimizing

### 12. Project Structure (LOW)

- [`proj-lib-main-split`](rules/proj-lib-main-split.md) - Keep `main.rs` minimal, logic in `lib.rs`
- [`proj-mod-by-feature`](rules/proj-mod-by-feature.md) - Organize modules by feature, not type
- [`proj-flat-small`](rules/proj-flat-small.md) - Keep small projects flat
- [`proj-mod-rs-dir`](rules/proj-mod-rs-dir.md) - Use `mod.rs` for multi-file modules
- [`proj-pub-crate-internal`](rules/proj-pub-crate-internal.md) - Use `pub(crate)` for internal APIs
- [`proj-pub-super-parent`](rules/proj-pub-super-parent.md) - Use `pub(super)` for parent-only visibility
- [`proj-pub-use-reexport`](rules/proj-pub-use-reexport.md) - Use `pub use` for clean public API
- [`proj-prelude-module`](rules/proj-prelude-module.md) - Create `prelude` module for common imports
- [`proj-bin-dir`](rules/proj-bin-dir.md) - Put multiple binaries in `src/bin/`
- [`proj-workspace-large`](rules/proj-workspace-large.md) - Use workspaces for large projects
- [`proj-workspace-deps`](rules/proj-workspace-deps.md) - Use workspace dependency inheritance

### 13. Clippy & Linting (LOW)

- [`lint-deny-correctness`](rules/lint-deny-correctness.md) - `#![deny(clippy::correctness)]`
- [`lint-warn-suspicious`](rules/lint-warn-suspicious.md) - `#![warn(clippy::suspicious)]`
- [`lint-warn-style`](rules/lint-warn-style.md) - `#![warn(clippy::style)]`
- [`lint-warn-complexity`](rules/lint-warn-complexity.md) - `#![warn(clippy::complexity)]`
- [`lint-warn-perf`](rules/lint-warn-perf.md) - `#![warn(clippy::perf)]`
- [`lint-pedantic-selective`](rules/lint-pedantic-selective.md) - Enable `clippy::pedantic` selectively
- [`lint-missing-docs`](rules/lint-missing-docs.md) - `#![warn(missing_docs)]`
- [`lint-unsafe-doc`](rules/lint-unsafe-doc.md) - `#![warn(clippy::undocumented_unsafe_blocks)]`
- [`lint-cargo-metadata`](rules/lint-cargo-metadata.md) - `#![warn(clippy::cargo)]` for published crates
- [`lint-rustfmt-check`](rules/lint-rustfmt-check.md) - Run `cargo fmt --check` in CI
- [`lint-workspace-lints`](rules/lint-workspace-lints.md) - Configure lints at workspace level

### 14. Anti-patterns (REFERENCE)

- [`anti-unwrap-abuse`](rules/anti-unwrap-abuse.md) - Don't use `.unwrap()` in production code
- [`anti-expect-lazy`](rules/anti-expect-lazy.md) - Don't use `.expect()` for recoverable errors
- [`anti-clone-excessive`](rules/anti-clone-excessive.md) - Don't clone when borrowing works
- [`anti-lock-across-await`](rules/anti-lock-across-await.md) - Don't hold locks across `.await`
- [`anti-string-for-str`](rules/anti-string-for-str.md) - Don't accept `&String` when `&str` works
- [`anti-vec-for-slice`](rules/anti-vec-for-slice.md) - Don't accept `&Vec<T>` when `&[T]` works
- [`anti-index-over-iter`](rules/anti-index-over-iter.md) - Don't use indexing when iterators work
- [`anti-panic-expected`](rules/anti-panic-expected.md) - Don't panic on expected/recoverable errors
- [`anti-empty-catch`](rules/anti-empty-catch.md) - Don't use empty `if let Err(_) = ...` blocks
- [`anti-over-abstraction`](rules/anti-over-abstraction.md) - Don't over-abstract with excessive generics
- [`anti-premature-optimize`](rules/anti-premature-optimize.md) - Don't optimize before profiling
- [`anti-type-erasure`](rules/anti-type-erasure.md) - Don't use `Box<dyn Trait>` when `impl Trait` works
- [`anti-format-hot-path`](rules/anti-format-hot-path.md) - Don't use `format!()` in hot paths
- [`anti-collect-intermediate`](rules/anti-collect-intermediate.md) - Don't `collect()` intermediate iterators
- [`anti-stringly-typed`](rules/anti-stringly-typed.md) - Don't use strings for structured data

---

## Recommended Cargo.toml Settings

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true

[profile.bench]
inherits = "release"
debug = true
strip = false

[profile.dev]
opt-level = 0
debug = true

[profile.dev.package."*"]
opt-level = 3  # Optimize dependencies in dev
```

---

## How to Use

**Before writing code**, follow this workflow:

1. **Read the reference chapters** listed in the task table above — these are mandatory, not optional.
2. **Read the relevant rule files** by prefix (linked in the Rule Index). The one-line summaries are a lookup aid, not a substitute for the full rule.
3. **Prioritize** CRITICAL > HIGH > MEDIUM > LOW categories.
4. **Apply the Project Conventions** at the top of this file — they override general rules.

### Rule Categories by Task

| Task                | Primary rule prefixes   | Reference chapters |
| ------------------- | ----------------------- | ------------------ |
| New function        | `own-`, `err-`, `name-` | Ch 1, Ch 4, Ch 8   |
| New struct/API      | `api-`, `type-`, `doc-` | Ch 6, Ch 7, Ch 8   |
| Async code          | `async-`, `own-`        | Ch 9               |
| Error handling      | `err-`, `api-`          | Ch 4               |
| Memory optimization | `mem-`, `own-`, `perf-` | Ch 3, Ch 9         |
| Performance tuning  | `opt-`, `mem-`, `perf-` | Ch 3               |
| Code review         | `anti-`, `lint-`        | Ch 2, Ch 1         |

---

## Sources

This skill synthesizes best practices from:

- [Apollo GraphQL Rust Best Practices Handbook](https://github.com/apollographql/rust-best-practices)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Rust Design Patterns](https://rust-unofficial.github.io/patterns/)
- Production codebases: ripgrep, tokio, serde, polars, axum, deno
- Clippy lint documentation
- Community conventions (2024-2025)
