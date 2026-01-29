---
name: fix-issue
description: Analyze and fix a GitHub issue. Provide the issue number as argument.
args: issue_number
---

# Fix GitHub Issue Skill

## Workflow

1. **Read the Issue**
   ```bash
   gh issue view {issue_number}
   ```

2. **Understand the Problem**
   - Parse issue title and description
   - Identify affected components
   - Determine the type: bug fix, feature, refactor

3. **Find Related Code**
   - Search for relevant files using keywords from the issue
   - Read and understand the current implementation
   - Identify the root cause (for bugs) or insertion point (for features)

4. **Plan the Fix**
   - List files that need to be modified
   - Outline the changes needed
   - Consider edge cases and tests

5. **Implement the Fix**
   - Make the necessary code changes
   - Follow project conventions (see CLAUDE.md)
   - Add or update tests as needed

6. **Verify the Fix**
   - Run tests: `cargo test`
   - Run linter: `cargo clippy --all-targets -- -D warnings`
   - Ensure no regressions

7. **Create Commit**
   - Stage changed files
   - Write descriptive commit message referencing the issue
   - Format: `fix: description (closes #issue_number)`

## Output Format

```
## Issue #{issue_number}: {title}

### Analysis
- Type: Bug/Feature/Refactor
- Affected files: [list]
- Root cause: [description]

### Changes Made
- [File]: [Description of changes]

### Tests
- [New/Modified tests]

### Verification
- Tests: PASS/FAIL
- Clippy: PASS/FAIL

### Commit
- Message: [commit message]
- Files: [list of committed files]
```

## Notes

- Requires GitHub CLI (`gh`) to be authenticated
- Always run tests before committing
- Reference the issue number in the commit message
