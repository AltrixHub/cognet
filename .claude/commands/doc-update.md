# Doc Update

Manually trigger documentation update based on recent code changes.

## Input

$ARGUMENTS

Optional: Specify a commit range or branch to analyze (e.g., `HEAD~5`, `feature-branch`).
If not provided, analyzes changes from the last commit.

## Instructions

1. Analyze code changes in the specified scope
2. Identify affected documentation (README, design docs, API docs)
3. Update documentation to match code
4. Report what was updated

## Usage Examples

```bash
# Update docs based on last commit
/doc-update

# Update docs for last 5 commits
/doc-update HEAD~5

# Update docs for changes in a branch
/doc-update feature/nurbs-curves
```
