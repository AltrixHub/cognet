# Impl Verify

Verify the current implementation against the plan. Check if the actual code matches what was planned.

## Input

$ARGUMENTS

**Arguments:**

| Argument | Description |
|----------|-------------|
| `{plan-file}` | Plan file path (optional, auto-detected if not specified) |
| `--ja` or `--jp` | Output in Japanese |

**Default language:** English

## Instructions

### 1. Check Staged Files

First, identify what files are staged for commit:

```bash
# Get list of staged files
git diff --cached --name-only

# Get staged changes summary
git diff --cached --stat
```

If no staged files, report and exit:
> No staged files found. Please stage your changes first with `git add`.

### 2. Auto-detect Matching Plan File

Find the plan that matches the staged changes:

1. **List all plan files:**
   ```bash
   ls -t {PROJECT_ROOT}/.ai/outputs/plans/*.md
   ```

2. **Read each plan file** and extract:
   - Target files mentioned in the plan
   - Implementation steps and descriptions
   - Component/module names

3. **Compare against staged files:**
   - Match file paths in plan vs staged files
   - Match component/module names
   - Calculate match score

4. **Select the best match** and confirm with user:
   ```
   Detected plan file: {plan-file-path}
   Match confidence: {high/medium/low}

   Is this correct? (y/n)
   ```

If no match or user says no, list available plans and ask to select.

### 3. Read the Plan

Read the selected plan file and extract:

- Implementation steps
- Expected changes per file
- Design decisions and approach
- Acceptance criteria (if any)

### 4. Read Actual Implementation

For each file mentioned in the plan that is also staged:

1. **Read the current file content** (not just the diff):
   - Use the Read tool to read the actual file
   - Understand the full implementation context

2. **Read the staged changes**:
   ```bash
   git diff --cached -- {file_path}
   ```

3. **Analyze the implementation**:
   - Does the code structure match the plan?
   - Are the planned functions/types implemented?
   - Does the approach match what was planned?

### 5. Compare Plan vs Actual Implementation

For each planned item, verify:

| Check | Description |
|-------|-------------|
| File exists | Was the planned file created/modified? |
| Structure | Does code structure match the plan? |
| Approach | Is the implementation approach as planned? |
| Completeness | Are all planned items implemented? |
| Extras | Any unplanned additions? |

### 6. Report: Plan vs Implementation

#### English (Default)

```markdown
## Plan vs Implementation Report

### Detected Plan

**File:** {plan file path}
**Match confidence:** {high/medium/low}

### Staged Files

{list of staged files}

### Verification Results

#### Matches Plan

{Items where implementation matches the plan exactly}

- {file}: {what matches}

#### Deviations

| Planned | Actual Implementation | Assessment |
|---------|----------------------|------------|
| {what was planned} | {what was actually implemented} | {OK/Needs Review/Issue} |

#### Missing from Plan

{Items that were staged but not mentioned in the plan}

- {file}: {what was added}

#### Not Yet Implemented

{Items in the plan that are not in staged changes}

- {planned item}: {status}

### Code Review Notes

{Any observations about the implementation quality, potential issues, or suggestions}

### Summary

{Overall assessment: Does the implementation match the plan?}

- Plan adherence: {percentage or qualitative}
- Recommendation: {proceed with commit / review needed / revise implementation}
```

#### Japanese (--ja / --jp)

```markdown
## 計画と実装の検証レポート

### 検出された計画

**ファイル:** {plan file path}
**マッチ度:** {高/中/低}

### ステージングされたファイル

{ステージングされたファイル一覧}

### 検証結果

#### 計画通り

{計画通りに実装された項目}

- {file}: {一致している内容}

#### 乖離

| 計画 | 実際の実装 | 評価 |
|------|-----------|------|
| {計画された内容} | {実際の実装内容} | {OK/要確認/問題あり} |

#### 計画外の追加

{計画になかったがステージングされた項目}

- {file}: {追加された内容}

#### 未実装

{計画にあるがステージングされていない項目}

- {計画項目}: {状況}

### コードレビューノート

{実装品質、潜在的な問題、提案などの所見}

### まとめ

{全体評価：実装は計画と一致しているか？}

- 計画準拠度: {パーセンテージまたは定性評価}
- 推奨: {コミット可 / 要レビュー / 実装修正}
```

### 7. Offer Next Actions

#### English (Default)

```markdown
### Next Actions

1. **Proceed to commit** - Implementation matches plan, ready to commit
2. **Review specific files** - Look at specific deviations in detail
3. **Update plan** - Sync plan with actual implementation
4. **Revise implementation** - Adjust code to match plan
5. **Done** - Review complete
```

#### Japanese (--ja / --jp)

```markdown
### 次のアクション

1. **コミットへ進む** - 実装が計画と一致、コミット可能
2. **特定ファイルを確認** - 乖離箇所を詳細に確認
3. **計画を更新** - 計画を実装に合わせて更新
4. **実装を修正** - 計画に合わせてコードを修正
5. **完了** - レビュー終了
```

## Examples

```bash
# Verify staged changes against auto-detected plan (English)
/impl-verify

# Verify in Japanese
/impl-verify --ja

# Specify plan file directly
/impl-verify .ai/outputs/plans/cognet-interaction.md
```

## Notes

- This command reads **actual file contents**, not just diffs
- Compares the real implementation against planned design
- Helps catch design drift before committing
- Run before `/git-commit` to ensure plan adherence
- If no staged files, prompts to stage changes first
