# Git PR Description

Generate a PR description based on the current branch's linked issue and commits.

## Instructions

### 1. Get Current Branch and Issue

Extract the issue number from the current branch name.

```bash
# Get current branch name
git branch --show-current
```

Branch naming pattern: `{issue-number}-{description}` (e.g., `12-feat-geometry-nurbs-curve`)

### 2. Fetch Issue Details

Use `gh issue view` to get the issue content:

```bash
gh issue view {issue-number} --json title,body,labels
```

### 3. Get Commit History

Get all commits on this branch that are not in the base branch (main):

```bash
git log main..HEAD --oneline
git log main..HEAD --pretty=format:"%s%n%b"
```

### 4. Analyze and Generate PR Description

Based on the issue and commits, generate a PR description following this template:

```markdown
# Description

Closes #{issue-number}

## PR Type

What kind of change does this PR introduce?

- [ ] Bugfix
- [ ] Feature
- [ ] Code style update (formatting, local variables)
- [ ] Refactoring (no functional changes, no api changes)
- [ ] Build related changes
- [ ] CI related changes
- [ ] Documentation content changes
- [ ] Tests
- [ ] Other

{Check the appropriate type based on issue labels and commit messages}

## What's new?

{Summarize the changes based on:
1. The issue description and requirements
2. The actual commits made on this branch
Write 2-5 bullet points describing the key changes}

## Implementation Details

{If there are significant technical details worth mentioning:
- Architecture decisions
- Key files changed
- Notable implementation choices}

## Screenshots

{If UI changes, mention "Add screenshots here" otherwise "N/A"}

## Testing

{Describe how this was tested:
- [ ] Unit tests added/updated
- [ ] Manual testing performed
- [ ] Build passes}
```

### 5. Output

Write the output to `.ai/outputs/git.md` file (overwrite if exists).

Format:

```markdown
# PR Output

**Generated:** {timestamp}
**Issue:** #{issue-number}

## Title

{issue title - typically in format "type(scope): description"}

## Description

{the generated markdown description - note: remove the top-level "# Description" heading from the template since we use "## Description" here}
```

After writing the file, display a brief summary to the user:

```text
Wrote PR description to .ai/outputs/git.md

- Issue: #{issue-number} {title}
- Commits: {number of commits}
- Type: {detected PR type}
```

## Notes

- If branch name doesn't contain an issue number, ask the user for it
- Determine PR type from:
  - Issue labels (enhancement = Feature, bug = Bugfix)
  - Branch name prefix (feat = Feature, fix = Bugfix, refactor = Refactoring)
  - Commit messages
- Keep the description concise but informative
- Focus on the "what" and "why", not the "how" (code details are in the diff)
- **IMPORTANT**: Use only ASCII characters in the output. Avoid Unicode symbols (arrows, special characters) as they may get corrupted. Use ASCII alternatives like `->`, `|`, `v` instead of `->`, `|`, `v`, etc.
