# Dev Implement

Implement a feature or change in the Geolis codebase.

## Input

$ARGUMENTS

Required: File path, directory, or description of what to implement.

## Instructions

### 1. Analyze Target

Determine which area of the codebase the implementation targets:

| Target Area | Description |
|-------------|-------------|
| Geometry | Curves, surfaces, points (NURBS, Bezier, etc.) |
| Topology | BRep structure (Vertex, Edge, Loop, Face, Shell, Solid) |
| Operations | Shape manipulation (Extrude, Boolean, Offset, etc.) |
| Tessellation | Mesh generation for display |
| Math | Foundation math utilities (nalgebra-based) |

If the target spans multiple areas, determine the primary area and note dependencies.

### 2. Implement

Based on the analysis:

1. Understand the implementation task description
2. Identify specific files to modify (if known)
3. Follow the layered architecture (Math -> Geometry -> Topology -> Operations)
4. Implement in dependency order

### 3. Post-Implementation

After implementation:

1. **Run `quality-checker`** to verify:
   - Build succeeds
   - Tests pass
   - No clippy warnings

2. **Ask user about documentation**:
   > Implementation complete. Would you like to update documentation?

## Examples

```bash
# Implement geometry
/dev-implement Add NURBS curve evaluation

# Implement topology
/dev-implement Add half-edge data structure for BRep

# Implement operation
/dev-implement Add extrude operation for Face to Solid

# Implement with file path
/dev-implement src/geometry/curve/nurbs.rs
```

## Notes

- If the target is unclear, ask the user for clarification
- For cross-layer changes, implement in dependency order (math -> geometry -> topology -> operations)
- Follow Rust coding rules from `.claude/rules/rust-coding.md`
