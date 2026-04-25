# anti-expect-lazy

> Don't use expect for recoverable errors

## Why It Matters

`.expect()` panics with a custom message, but it's still a panic. Using it for errors that could reasonably occur in production (network failures, file not found, invalid input) crashes the program instead of handling the error gracefully.

Reserve `.expect()` for programming errors where panic is appropriate.

## Bad

```rust
// Network failures are expected - don't panic
let response = client.get(url).await.expect("failed to fetch");

// Files might not exist
let config = fs::read_to_string("config.toml").expect("config not found");

// User input can be invalid
let age: u32 = input.parse().expect("invalid age");

// Database queries can fail
let user = db.find_user(id).await.expect("user not found");
```

## Good

```rust
// Handle recoverable errors properly
let response = client.get(url).await
    .context("failed to fetch URL")?;

// Return error if file doesn't exist
let config = fs::read_to_string("config.toml")
    .context("failed to read config file")?;

// Validate and return error
let age: u32 = input.parse()
    .map_err(|_| Error::InvalidInput("age must be a number"))?;

// Handle missing data
let user = db.find_user(id).await?
    .ok_or(Error::NotFound("user"))?;
```

## When expect() Is Appropriate (general guidance)

When `expect_used` is **not** denied, reserve `.expect()` for invariants that indicate bugs:

```rust
// Mutex poisoning indicates a bug elsewhere
let guard = mutex.lock().expect("mutex poisoned");

// Regex is known valid at compile time
let re = Regex::new(r"^\d{4}$").expect("invalid regex");
```

## With `expect_used = "deny"` (this project)

This project denies `expect_used` via clippy. Use type-system alternatives instead:

```rust
// Mutex: use unwrap_or_else with a recoverable strategy, or restructure
// to avoid shared mutable state (prefer message-passing)

// Static regex: use LazyLock
static RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d{4}$").unwrap_or_else(|e| panic!("invalid static regex: {e}"))
    // panic! in LazyLock init is acceptable for static invariants if
    // clippy::panic is not also denied — otherwise use a const-validated crate
});

// Invariant after validation: encode it structurally
// See err-expect-bugs-only.md for the typestate/newtype approach
```

## Decision Guide

| Situation                 | `expect_used` allowed             | `expect_used = "deny"`                    |
| ------------------------- | --------------------------------- | ----------------------------------------- |
| User input                | `?` with error                    | `?` with error                            |
| File/network I/O          | `?` with error                    | `?` with error                            |
| Database operations       | `?` with error                    | `?` with error                            |
| Parsed constants          | `.expect()`                       | `LazyLock` + type-safe init               |
| Post-validation invariant | `.expect()` with message          | Newtype / typestate pattern               |
| Never expected to fail    | `.expect()` documenting invariant | Restructure to be structurally impossible |

## See Also

- [err-expect-bugs-only](./err-expect-bugs-only.md) - When to use expect
- [err-no-unwrap-prod](./err-no-unwrap-prod.md) - Avoiding unwrap
- [anti-unwrap-abuse](./anti-unwrap-abuse.md) - Unwrap anti-pattern
