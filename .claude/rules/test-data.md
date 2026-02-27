# Test Data Generation Rules

## Token Limit Awareness

Claude Code has a 64000 output token limit. Geometric test data (hardcoded coordinates)
is extremely token-heavy. Follow these rules to avoid exceeding the limit.

## Rules

### 1. Never Write Large Files in One Shot

- Do NOT use the Write tool to create files with >200 lines of test data
- Instead: Write a skeleton first (imports, module structure, helpers), then use Edit
  to add test functions one at a time
- When modifying existing test files, always use Edit (not Write) to change only the
  relevant section

### 2. Extract Shared Shape Definitions

Define reusable shape constructors instead of duplicating coordinate arrays:

```rust
// GOOD: Define once, reuse everywhere
fn t_shape_points() -> Vec<Point3> {
    vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(10.0, 0.0, 0.0),
        // ...
    ]
}

#[test]
fn t_shape_inward_d03() {
    let result = PolylineOffset2D::new(t_shape_points(), 0.3, true).execute().unwrap();
    // ...
}

// BAD: Duplicating the same coordinates in every test
#[test]
fn t_shape_inward_d03() {
    let points = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0), ...];
    // ...
}
```

### 3. Parameterize Expected Values When Possible

When the expected result follows a formula, use a helper function:

```rust
// GOOD: Parameterized expected values
fn open_cross_expected(d: f64) -> Vec<Point3> {
    vec![
        Point3::new(-1.5, -d, 0.0),
        Point3::new(-1.5, d, 0.0),
        // ...
    ]
}

// BAD: Separate hardcoded arrays for each distance
```

### 4. Add Tests Incrementally

When creating multiple test functions:
1. First Edit: Add helper functions (shape constructors, assertion utilities)
2. Subsequent Edits: Add one or two `#[test]` functions per Edit call
3. Never try to add more than ~5 test functions in a single response

### 5. Keep Test Files Under 500 Lines

If a test module grows beyond ~500 lines, split into sub-modules:

```rust
#[cfg(test)]
mod tests {
    mod t_shape_tests;
    mod cross_tests;
    mod open_polyline_tests;
}
```

### 6. Debug Viewer Patterns

For `examples/debug/patterns/` and `examples/debug/test/`:
- Each pattern file should stay under 200 lines
- Reuse `register_stroke` and other shared utilities from `patterns/mod.rs`
- Group related shapes into a single pattern file rather than creating one file per shape
