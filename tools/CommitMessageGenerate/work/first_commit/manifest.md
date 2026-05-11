# Git Story Harness Manifest

- repo: `/home/t4ce/REPOS/TRUEOS`
- revs: `HEAD`
- commits: `1`
- batch_size: `200`
- batches: `1`
- max_diff_bytes: `120000`
- excludes: `(none)`

## Batch Files

- `batches/batch_001.jsonl`

## Agent Contract

For each JSONL record, preserve `original_message` exactly.
Fill `generated_message` with a diff-grounded commit message.
Use `story_notes` for cross-commit narrative context or uncertainty.
Do not rewrite Git history from these files until the generated output is reviewed.
