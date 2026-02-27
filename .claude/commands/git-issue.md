# Git Issue

Create a new GitHub issue for a feature, bug fix, or other task.

## Input

$ARGUMENTS

## Instructions

Based on the input above, perform the following steps:

### 1. Parse Issue Details

Determine from the input:
- **Title**: Short title (e.g., "NURBS Curve Evaluation")
- **Type**: `feat`, `fix`, `refactor`, `perf`, `docs`, `test`
- **Scope**: `geometry`, `topology`, `operations`, `tessellation`, `math`
- **Description**: One-line description of what this issue addresses
- **Tasks**: Checklist of implementation subtasks (optional)
- **Labels**: `enhancement`, `bug`, `performance`, `refactor` (default: `enhancement`)

### 2. Create the Issue

Execute the following command to create the GitHub issue:

```bash
gh issue create \
  --title "{type}({scope}): {title}" \
  --label "{label}" \
  --body "$(cat <<'EOF'
## Description

{description}

## Tasks

- [ ] {task 1}
- [ ] {task 2}
...

## Related

- {related items if any}
EOF
)"
```

### 3. Output Summary

Output a brief summary:
- Issue number created
- Title
- Link to the issue

## Examples

```bash
# Create a feature issue
/git-issue feat geometry NURBS Curve Evaluation - Implement curve point evaluation

# Create a bug fix issue
/git-issue fix topology Half-edge twin pointer not set correctly

# Create a refactor issue
/git-issue refactor operations Simplify extrude operation error handling
```

Output:
- Creates issue with title "feat(geometry): NURBS Curve Evaluation"
- Returns issue number and link
