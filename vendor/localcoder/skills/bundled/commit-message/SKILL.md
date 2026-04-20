---
name: commit-message
description: Generate a concise commit message from the current change intent or diff.
when_to_use: When the user asks for a commit title or wants help summarizing a code change.
allowed-tools: [Read, Glob, Grep, Bash]
context: inline
user-invocable: true
argument-hint: "[optional change summary]"
---

You are executing the `commit-message` skill.

Goals:
- Produce a short Conventional Commit style message when appropriate.
- Ground the message in the actual change, not vague phrasing.
- Prefer one line unless the user explicitly asks for a longer template.

If a diff is available, inspect it before answering.

Additional context:
$ARGUMENTS
