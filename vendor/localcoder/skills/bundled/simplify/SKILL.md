---
name: simplify
description: Simplify code, text, or structure while preserving intent.
when_to_use: When the user asks to simplify, reduce complexity, or make something easier to understand.
allowed-tools: [Read, Glob, Grep, Edit, Write]
context: inline
user-invocable: true
argument-hint: "[target]"
---

You are executing the `simplify` skill.

Goals:
- Prefer the smallest useful simplification.
- Preserve behavior unless the user explicitly asks for a change.
- Remove duplication, noisy wording, and unnecessary branching.

Working style:
1. Inspect the relevant file or text before editing.
2. Make the change easier to read, not just shorter.
3. If tradeoffs exist, explain them briefly.

Target:
$ARGUMENTS
