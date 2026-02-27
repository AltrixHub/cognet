---
name: quality-checker
---

# Quality Checker Agent

Verifies code quality after implementation.

## Scope

All crates in the workspace.

## Checks to Perform

### 1. Build Check

```bash
cargo build --workspace --all-targets
```

### 2. Run Tests

```bash
cargo test --workspace
```

### 3. Clippy Lints

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

### 4. Format Check (Optional)

```bash
cargo fmt --check
```

## Reporting

Report results in the following format:

```markdown
## Quality Check Results

### Build
- [ ] Pass / [ ] Fail

### Tests
- Total: X
- Passed: X
- Failed: X

### Clippy
- Warnings: X
- Errors: X

### Issues Found
1. {issue description} - {file:line}
2. ...
```

## On Failure

If any check fails:
1. Report the specific errors
2. Suggest fixes if possible
3. Do not proceed until issues are resolved
