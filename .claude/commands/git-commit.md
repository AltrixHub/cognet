# Git Commit Message

Generate a commit message based on staged changes.

## Instructions

### 1. Check Staged Changes

```bash
# Show staged files
git diff --cached --name-only

# Show staged diff
git diff --cached
```

### 2. Analyze Changes

Based on the staged diff, determine:

- **Type**: What kind of change is this?
  - `feat`: New feature
  - `fix`: Bug fix
  - `refactor`: Code refactoring (no functional change)
  - `perf`: Performance improvement
  - `docs`: Documentation only
  - `test`: Adding or updating tests
  - `chore`: Build, CI, or other maintenance
  - `style`: Code style/formatting (no functional change)

- **Scope**: Which part of the codebase? (optional)
  - `geometry`, `topology`, `operations`, `tessellation`, `math`

- **Summary**: One-line description of what changed (imperative mood)

- **Body**: Detailed explanation if needed (optional)

### 3. Generate Commit Message

Output the commit message in this format:

```
<type>(<scope>): <summary>

<body (optional, explain why if not obvious)>
```

### 4. Output

Write the output to `.ai/outputs/git.md` file (overwrite if exists).

Format:

```markdown
# Commit Message

**Generated:** {timestamp}

## Message

{the generated message}

## Staged Files

{list of staged files}

## Command

\`\`\`bash
git commit -m "$(cat <<'EOF'
{message}
EOF
)"
\`\`\`
```

After writing the file, display a brief summary to the user:

```text
Wrote commit message to .ai/outputs/git.md

- Type: {type}
- Staged files: {count}
```

**IMPORTANT:** The `## Command` section MUST always be included in the output file. The command must use a HEREDOC format to correctly handle multi-line commit messages.

## Guidelines

- Summary should be 50 characters or less
- Use imperative mood ("Add feature" not "Added feature")
- Don't end summary with a period
- Body should wrap at 72 characters
- Focus on "what" and "why", not "how"
- If multiple unrelated changes are staged, suggest splitting into separate commits
- **DO NOT** include "Generated with [Claude Code]" or "Co-Authored-By: Claude" footer
- Output only the commit title and body, nothing else

## Examples

Single file change:

```text
feat(geometry): Add NURBS curve evaluation
```

Multiple related changes:

```text
feat(topology): Implement BRep half-edge structure

- Add HalfEdge type with twin/next/prev pointers
- Implement Face boundary traversal
- Add topology consistency validation
```

Refactoring:

```text
refactor(operations): Extract boolean operation into trait
```
