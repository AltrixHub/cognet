# Rust Coding Rules

## Error Handling

### Never Use `unwrap()` or `expect()` in Production Code

| Allowed | Forbidden |
|---------|-----------|
| Tests (`#[cfg(test)]`) | Library code |
| Examples | Application code |
| Prototyping (temporary) | Any committed code |

```rust
// Bad
let value = some_option.unwrap();
let result = some_result.expect("should work");

// Good
let value = some_option.ok_or(MyError::NotFound)?;
let result = some_result?;
```

### Use the `?` Operator

Propagate errors with `?` instead of manual matching:

```rust
// Bad
fn read_config() -> Result<Config, MyError> {
    let content = match std::fs::read_to_string("config.toml") {
        Ok(c) => c,
        Err(e) => return Err(MyError::Io(e)),
    };
    // ...
}

// Good
fn read_config() -> Result<Config, MyError> {
    let content = std::fs::read_to_string("config.toml")?;
    // ...
}
```

### Custom Error Types with `thiserror`

Define domain-specific errors using `thiserror`:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MyError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error at line {line}: {message}")]
    Parse { line: usize, message: String },

    #[error("Not found: {0}")]
    NotFound(String),
}
```

### Error Handling Patterns

| Pattern | Use Case |
|---------|----------|
| `?` operator | Propagate error to caller |
| `.ok_or(err)?` | Convert Option to Result |
| `.map_err()?` | Transform error type |
| `if let Err(e) = ...` | Handle error locally |
| `.unwrap_or(default)` | Provide fallback value |
| `.unwrap_or_else(\|\| ...)` | Lazy fallback computation |

## Safety

### Minimize `unsafe` Usage

- Avoid `unsafe` unless absolutely necessary
- When required, isolate in small, well-documented functions
- Add `// SAFETY:` comments explaining invariants

```rust
// SAFETY: We verified that ptr is valid and aligned
// in the constructor, and the lifetime is bounded by 'a.
unsafe { &*self.ptr }
```

### Prefer Safe Abstractions

| Unsafe | Safe Alternative |
|--------|------------------|
| Raw pointers | References, `Box`, `Rc`, `Arc` |
| `transmute` | `From`/`Into` traits |
| Manual memory | RAII patterns |

## Performance

### Ownership and Borrowing

```rust
// Bad - unnecessary clone
fn process(data: Vec<u8>) {
    let copy = data.clone();
    // ...
}

// Good - borrow when you don't need ownership
fn process(data: &[u8]) {
    // ...
}

// Good - take ownership only when needed
fn consume(data: Vec<u8>) -> ProcessedData {
    // data is moved and consumed
}
```

### Smart Pointer Guidelines

| Type | Use Case |
|------|----------|
| `Box<T>` | Heap allocation, trait objects |
| `Rc<T>` | Single-threaded shared ownership |
| `Arc<T>` | Multi-threaded shared ownership |
| `Cow<'a, T>` | Clone-on-write optimization |

### Iterator Preference

```rust
// Bad - manual indexing
for i in 0..vec.len() {
    process(&vec[i]);
}

// Good - iterator
for item in &vec {
    process(item);
}

// Good - iterator with transformation
let results: Vec<_> = items.iter().map(transform).collect();
```

### Avoid Unnecessary Allocations

```rust
// Bad - allocates intermediate String
let s = format!("{}", value);
log::info!("{}", s);

// Good - format directly
log::info!("{}", value);

// Bad - creates new Vec
let filtered = vec.iter().filter(|x| x > &0).cloned().collect::<Vec<_>>();
for item in filtered { ... }

// Good - lazy iteration
for item in vec.iter().filter(|x| **x > 0) { ... }
```

## Code Style

### Naming Conventions

| Item | Convention | Example |
|------|------------|---------|
| Types | PascalCase | `MyStruct`, `ErrorKind` |
| Functions | snake_case | `get_value`, `process_data` |
| Constants | SCREAMING_SNAKE_CASE | `MAX_SIZE`, `DEFAULT_TIMEOUT` |
| Modules | snake_case | `my_module` |
| Type parameters | Single uppercase | `T`, `E`, `K`, `V` |
| Lifetimes | Short lowercase | `'a`, `'b`, `'ctx` |

### Documentation

Document public APIs with `///`:

```rust
/// Processes the input data and returns the result.
///
/// # Arguments
///
/// * `input` - The raw input data to process
///
/// # Returns
///
/// The processed output, or an error if processing fails.
///
/// # Examples
///
/// ```
/// let result = process_data(&input)?;
/// ```
pub fn process_data(input: &[u8]) -> Result<Output, MyError> {
    // ...
}
```

### Visibility

Minimize public surface area:

```rust
// Prefer private by default
struct InternalState { ... }

// Only expose what's needed
pub struct PublicApi {
    state: InternalState,  // private field
}

// Use pub(crate) for internal sharing
pub(crate) fn internal_helper() { ... }
```

## Patterns

### Builder Pattern

For types with many optional fields:

```rust
pub struct Config {
    timeout: Duration,
    retries: u32,
    // ...
}

impl Config {
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }
}

#[derive(Default)]
pub struct ConfigBuilder {
    timeout: Option<Duration>,
    retries: Option<u32>,
}

impl ConfigBuilder {
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn retries(mut self, retries: u32) -> Self {
        self.retries = Some(retries);
        self
    }

    pub fn build(self) -> Result<Config, ConfigError> {
        Ok(Config {
            timeout: self.timeout.unwrap_or(Duration::from_secs(30)),
            retries: self.retries.unwrap_or(3),
        })
    }
}
```

### Newtype Pattern

Wrap primitives for type safety:

```rust
// Bad - easy to confuse
fn process(user_id: u64, order_id: u64) { ... }

// Good - distinct types
pub struct UserId(pub u64);
pub struct OrderId(pub u64);

fn process(user_id: UserId, order_id: OrderId) { ... }
```

### Type State Pattern

Encode state in the type system:

```rust
pub struct Connection<S> {
    inner: TcpStream,
    _state: PhantomData<S>,
}

pub struct Disconnected;
pub struct Connected;
pub struct Authenticated;

impl Connection<Disconnected> {
    pub fn connect(addr: &str) -> Result<Connection<Connected>, Error> { ... }
}

impl Connection<Connected> {
    pub fn authenticate(self, creds: &Credentials) -> Result<Connection<Authenticated>, Error> { ... }
}

impl Connection<Authenticated> {
    pub fn send(&mut self, data: &[u8]) -> Result<(), Error> { ... }
}
```

## Common Derive Traits

Implement standard traits appropriately:

| Trait | When to Derive |
|-------|----------------|
| `Debug` | Almost always (except sensitive data) |
| `Clone` | When copying makes sense |
| `Copy` | Small, trivially copyable types |
| `PartialEq`, `Eq` | For comparison/testing |
| `Hash` | For use in HashMap/HashSet |
| `Default` | When a sensible default exists |
| `Send`, `Sync` | Usually automatic; verify for custom types |

## Clippy Enforcement

The following Clippy lints are enforced:

```rust
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
```

## Testing

### Test Organization

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_happy_path() {
        let result = process(&valid_input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_error_case() {
        let result = process(&invalid_input);
        assert!(matches!(result, Err(MyError::InvalidInput(_))));
    }
}
```

### In Tests, `unwrap()` is Acceptable

```rust
#[test]
fn test_parsing() {
    // OK in tests - failure will show in test output
    let config = Config::from_str(INPUT).unwrap();
    assert_eq!(config.name, "test");
}
```
