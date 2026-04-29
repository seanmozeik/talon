# err-expect-bugs-only

> Use `expect()` only for invariants that indicate bugs — or eliminate it entirely with `#[deny(clippy::expect_used)]`

## Why It Matters

`expect()` is better than `unwrap()` because it provides context, but it still panics. Reserve it for situations where failure indicates a bug in your code—a violated invariant, not a user error or external failure.

**Project override:** If your workspace sets `expect_used = "deny"` (as this project does), `.expect()` is not permitted anywhere in production code. Use the typestate pattern or `ok_or`/`ok_or_else` with `?` to express invariants without panicking. See the alternatives section below.

## Bad

```rust
// User input can legitimately fail - don't expect
fn parse_user_input(input: &str) -> Config {
    serde_json::from_str(input)
        .expect("Invalid JSON")  // User error, not a bug!
}

// Network can fail - don't expect
fn fetch_data(url: &str) -> Data {
    reqwest::get(url)
        .expect("Network request failed")  // External failure!
        .json()
        .expect("Invalid response")
}

// File might not exist - don't expect
fn load_config() -> Config {
    let content = fs::read_to_string("config.json")
        .expect("Config file missing");  // Environment issue!
}
```

## Good (when `expect_used` is permitted)

```rust
// Invariant: after insert, key exists
fn cache_and_get(&mut self, key: String, value: Value) -> &Value {
    self.cache.insert(key.clone(), value);
    self.cache.get(&key)
        .expect("BUG: key must exist immediately after insert")
}

// Invariant: regex is compile-time constant
fn create_parser() -> Regex {
    Regex::new(r"^\d{4}-\d{2}-\d{2}$")
        .expect("BUG: date regex is invalid - this is a compile-time constant")
}
```

## Better: Encode Invariants in the Type System (works with `expect_used = "deny"`)

When `.expect()` is denied, make invariants structurally impossible to violate rather than asserting them at runtime:

```rust
// Instead of: .expect("BUG: key must exist after insert")
// Return the reference directly from the insert operation:
fn cache_and_get(&mut self, key: String, value: Value) -> &Value {
    self.cache.entry(key).or_insert(value)
}

// Instead of: Regex::new(...).expect("BUG: static regex")
// Use once_cell or std::sync::LazyLock for compile-adjacent initialization:
static DATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap_or_else(|_| unreachable!())
    // or use a const-validated crate like `regex-lite` with const new()
});

// Instead of: .expect("BUG: ValidatedEmail must contain @")
// Encode the structure in the type so the invariant can't be broken:
struct ValidatedEmail { local: String, domain: String }

impl ValidatedEmail {
    pub fn new(email: &str) -> Result<Self, EmailError> {
        let (local, domain) = email.split_once('@').ok_or(EmailError::MissingAt)?;
        Ok(Self { local: local.to_owned(), domain: domain.to_owned() })
    }
    pub fn domain(&self) -> &str { &self.domain }  // no panic possible
}
```

## Alternatives When expect() Is Wrong

```rust
// Don't: expect on user data
let port: u16 = input.parse().expect("Invalid port");

// Do: Return Result
let port: u16 = input.parse().map_err(|_| ConfigError::InvalidPort)?;

// Do: Provide default
let port: u16 = input.parse().unwrap_or(8080);

// Do: Handle explicitly
let port: u16 = match input.parse() {
    Ok(p) => p,
    Err(_) => {
        log::warn!("Invalid port '{}', using default", input);
        8080
    }
};
```

## See Also

- [err-no-unwrap-prod](./err-no-unwrap-prod.md) - Avoiding unwrap in production
- [err-result-over-panic](./err-result-over-panic.md) - When to return Result
- [api-parse-dont-validate](./api-parse-dont-validate.md) - Type-driven validation
