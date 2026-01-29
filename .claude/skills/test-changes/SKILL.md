---
name: test-changes
description: Run Rust tests, analyze failures, and fix issues. Use after making code changes.
---

# Test Changes Skill

## Workflow

1. **Run All Tests**
   ```bash
   cargo test 2>&1
   ```

2. **If Tests Fail**
   - Analyze the failure output
   - Identify the root cause
   - Fix the failing code
   - Re-run tests to verify

3. **Run Clippy for Additional Checks**
   ```bash
   cargo clippy --all-targets --all-features -- -D warnings 2>&1
   ```

4. **Report Results**
   - Number of tests passed/failed
   - Any warnings from clippy
   - Summary of fixes made (if any)

## Expected Output

```
## Test Results

### Summary
- Tests: X passed, Y failed
- Clippy: Z warnings

### Failed Tests (if any)
- `test_name`: Brief description of failure and fix

### Actions Taken
- [List of fixes applied]

### Status: PASS/FAIL
```

## Notes

- Tests require DynamoDB Local running on `http://dynamodb-local:8000`
- If DynamoDB connection fails, suggest starting the container:
  ```bash
  docker compose -f .devcontainer/docker-compose.yaml up -d dynamodb-local
  ```
