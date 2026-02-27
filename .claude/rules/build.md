# Build Verification Rules

## Workspace Build Commands

This project is a Cargo workspace with multiple crates and examples. Use the appropriate commands to verify builds.

### Quick Check (Recommended)

```bash
# Check all targets without building (fastest)
cargo check --workspace --all-targets
```

### Full Build Verification

```bash
# Build all crates and examples
cargo build --workspace --all-targets
```

### Lint Check

```bash
# Clippy on all targets (catches more issues)
cargo clippy --workspace --all-targets
```

### With All Features (Comprehensive)

```bash
# Check with all features enabled
cargo check --workspace --all-targets --all-features

# Build with all features enabled
cargo build --workspace --all-targets --all-features

# Clippy with all features enabled
cargo clippy --workspace --all-targets --all-features
```

## What `--all-targets` Includes

- `--lib` - Library crates
- `--bins` - Binary crates
- `--examples` - Example programs
- `--tests` - Test code
- `--benches` - Benchmarks

## What `--all-features` Includes

Enables all optional features across the workspace. Features will be updated as the project develops and new crates are added.

## When to Use Each Command

| Situation | Command |
|-----------|---------|
| Quick syntax check | `cargo check --workspace --all-targets` |
| Verify full build | `cargo build --workspace --all-targets` |
| Before commit | `cargo clippy --workspace --all-targets` |
| Comprehensive check | `cargo check --workspace --all-targets --all-features` |
| PR/CI verification | `cargo clippy --workspace --all-targets --all-features` |
| Single example | `cargo build --example <name>` |
| Run tests | `cargo test --workspace` |
| Run tests (all features) | `cargo test --workspace --all-features` |

## Common Mistakes to Avoid

- `cargo build` alone only builds the root package
- `cargo build --workspace` misses examples and tests
- Always use `--all-targets` for complete verification
- Without `--all-features`, feature-gated code is not checked
