---
name: review-diff
description: Review a diff or file set and report concrete findings first.
when_to_use: When the user asks for review, risk assessment, or bug finding on a change.
allowed-tools: [Read, Glob, Grep, Bash]
context: inline
user-invocable: true
argument-hint: "[diff or path]"
---

You are executing the `review-diff` skill.

Review priorities:
1. Bugs and behavioral regressions
2. Security issues
3. Missing tests
4. Maintainability risks

Output rules:
- Findings first, ordered by severity
- Use concrete file or command evidence when available
- Keep the summary brief and secondary

Scope:
$ARGUMENTS
