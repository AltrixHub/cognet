# Git PR Description

Generate a PR description based on commits (and optionally a linked issue).

## Usage

- `/git-pr` — Generate from commits only (default, no issue lookup)
- `/git-pr 61` — Generate from commits + issue #61

## Instructions

### 1. Get Current Branch

```bash
git branch --show-current
```

### 2. Determine Issue Number (Optional)

- If an issue number was passed as `$ARGUMENTS`, use it.
- Otherwise, skip issue lookup entirely. Do NOT extract or guess an issue number from the branch name.

If an issue number is available, fetch details:

```bash
gh issue view {issue-number} --json title,body,labels
```

### 3. Get Commit History

Get all commits on this branch that are not in the base branch (develop):

```bash
git log develop..HEAD --oneline
git log develop..HEAD --pretty=format:"%s%n%b"
```

### 4. Analyze and Generate PR Description

Based on commits (and issue if provided), generate a PR description following this template:

```markdown
# Description

{If issue number is available: "Closes #{issue-number}", otherwise omit this line}

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
{If issue number is available: "**Issue:** #{issue-number}", otherwise omit}

## Title

{Derive title from commit messages; if issue is available, use issue title}

## Description

{the generated markdown description - note: remove the top-level "# Description" heading from the template since we use "## Description" here}
```

After writing the file, display a brief summary to the user:

```text
Wrote PR description to .ai/outputs/git.md

{If issue: "- Issue: #{issue-number} {title}"}
- Commits: {number of commits}
- Type: {detected PR type}
```

## Notes

- **Do NOT ask for an issue number** unless the user explicitly passes one
- Determine PR type from:
  - Issue labels (enhancement = Feature, bug = Bugfix)
  - Branch name prefix (feat = Feature, fix = Bugfix, refactor = Refactoring)
  - Commit messages
- Keep the description concise but informative
- Focus on the "what" and "why", not the "how" (code details are in the diff)
- **IMPORTANT**: Use only ASCII characters in the output. Avoid Unicode symbols (arrows, special characters) as they may get corrupted. Use ASCII alternatives like `->`, `|`, `v` instead of `→`, `↓`, etc.
