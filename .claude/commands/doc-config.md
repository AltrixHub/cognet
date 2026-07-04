# Doc Config

Update Claude Code configuration files (agents, commands).

## Input

$ARGUMENTS

Optional: Specific area to update (e.g., `agents`, `commands`).
If not provided, checks all configuration files.

## Instructions

### 1. Analyze Current State

Check for updates needed in `.claude/` directory:

```bash
ls -la .claude/agents/
ls -la .claude/commands/
ls -la .claude/rules/
```

### 2. Verify Configuration

1. Verify agent scopes match current project structure
2. Ensure commands use correct agent names
3. Identify outdated patterns or examples

### 3. Common Update Triggers

Update configs when:

| Trigger | What to Update |
|---------|----------------|
| New crate added | Update agent scopes |
| Directory restructured | Update agent scopes |
| New patterns established | Update code examples in agents |
| Workflow changed | Update command instructions |

### 4. Report Changes

After updates, display summary:

```markdown
## Config Update Summary

### Updated Files
- `.claude/agents/quality-checker.md`
  - Updated scope to include new crate

### No Changes Needed
- `.claude/commands/git-commit.md` - Already up to date

### New Files Created
- `.claude/agents/new-agent.md` - For new module

### Recommendations
- Consider adding agent for {new topic}
```

## Examples

```bash
# Update all configs
/doc-config

# Update only agents
/doc-config agents

# Update after adding new crate
/doc-config Added new crate geolis_tessellation
```

## Notes

- Run this after significant project structure changes
- Keeps agent/command definitions in sync with codebase
- Does not modify source code, only `.claude/` configuration
