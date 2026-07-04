# AI Output Rules

## Output Directory

All agent outputs are written under the **project root's** `.ai/outputs/` directory.

**IMPORTANT:** Always use the absolute path from the Geolis project root, NOT the current working directory.

Where `PROJECT_ROOT` is the directory containing `Cargo.toml`.

## Directory Structure

```
{PROJECT_ROOT}/.ai/outputs/
├── git.md                        # Git commit / PR output (overwritten each time)
├── plans/                        # Implementation plans (auto-named)
│   └── {plan-name}.md
└── summaries/                    # Task summaries
    └── summary-{task-name}.md
```

## File Paths by Command

| Command | Output Path |
|---------|-------------|
| `/git-commit` | `.ai/outputs/git.md` |
| `/git-pr` | `.ai/outputs/git.md` |
| Implementation plans | `.ai/outputs/plans/{name}.md` |
| Task summaries | `.ai/outputs/summaries/summary-{name}.md` |

**CRITICAL:** `/git-commit` and `/git-pr` write to `.ai/outputs/git.md`, NOT to `summaries/`.

## When to Write Output

Write to AI_OUTPUTS directory when:

- Generating commit messages (`/git-commit`)
- Creating PR descriptions (`/git-pr`)
- Creating implementation plans (`/plan-issue`, `/plan-refactor`)
- Completing significant multi-step tasks
- User explicitly requests a summary

## When NOT to Write Output

Skip writing output for:

- Simple single-step tasks
- Direct answers to questions
- Code edits without planning phase
- Conversational responses

## Output Format

Each file should include:

```markdown
# {Title}

**Generated:** {timestamp}
**Task:** {brief description}

## Content

{the generated content}

## Context

{optional: files modified, commands run, etc.}
```

## Cleanup

Old output files can be deleted periodically. These are temporary working files, not permanent documentation.
