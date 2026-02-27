---
name: refactor
---

# Refactor Agent

Performs code refactoring across the Geolis CAD kernel codebase.

## Scope

All crates in the workspace.

## Before Starting

1. Understand current code structure
2. Identify all affected files
3. Plan the refactoring strategy
4. Verify doc-code consistency - Check if implementation matches documentation

## Current Architecture

The codebase follows a layered architecture for the CAD kernel:

### Layers

| Layer | Responsibility | Description |
|-------|----------------|-------------|
| Geometry Layer | Mathematical representation | NURBS curves/surfaces, points, vectors |
| Topology Layer | Connectivity management | Vertex, Edge, Loop, Face, Shell, Solid (BRep) |
| Operations Layer | Shape manipulation | Extrude, Revolve, Trim, Boolean, Offset |
| Math Foundation | Linear algebra | nalgebra-based vectors and matrices |

### Key Design Principles

- **Separation of Geometry and Topology** - Mathematical shape definitions are separate from connectivity
- **Incremental API** - Use only the features you need
- **Reliability-focused** - Numerical stability and topology consistency validation

## Refactoring Types

### Rename

- Variables, functions, types, modules
- Update all references across crates
- Update documentation if needed

### Extract

- Extract function/method from large code blocks
- Extract module from large files
- Extract trait from common behavior

### Move

- Move types between modules/crates
- Update imports and visibility
- Ensure proper module hierarchy

### Simplify

- Remove dead code
- Reduce complexity
- Consolidate duplicate logic

### Restructure

- Reorganize module structure
- Split or merge files
- Improve code organization

## Process

1. **Analyze**: Identify all affected locations

   ```bash
   cargo check --workspace --all-targets 2>&1
   ```

2. **Plan**: List all changes to be made

3. **Execute**: Make changes systematically

4. **Verify**: Ensure build and tests pass

   ```bash
   cargo build --workspace --all-targets
   cargo test --workspace
   cargo clippy --workspace --all-targets
   ```

## Principles

- **Loose coupling** - Make code testable with clear boundaries
- **Single Responsibility** - Each module/type has one reason to change
- **Proper file organization** - Split files appropriately by concern
- **Clear naming** - Use descriptive, intention-revealing names
- **Remove redundancy** - Eliminate duplicate or dead code
- **Optimize performance** - Improve efficiency where possible

### Rust-Specific

- **Error handling** - Use proper error types, `Result`, avoid `unwrap`/`expect`
- **Type safety** - Leverage Rust's type system, use newtype pattern
- **Lifetime management** - Reduce unnecessary `.clone()`, use proper borrowing
- **Visibility** - Use appropriate `pub`/`pub(crate)`/private visibility

## Guidelines

- Make atomic, focused changes
- Preserve existing behavior (no functional changes)
- Update tests if signatures change
- Keep commit history clean (logical commits)
- Verify documentation matches implementation before refactoring
- Suggest additional improvements if identified

## After Completion

1. Run quality-checker agent to verify:

   - Build succeeds
   - All tests pass
   - No clippy warnings

2. Add test cases for refactored code
