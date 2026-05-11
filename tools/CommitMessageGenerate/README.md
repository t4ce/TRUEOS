# Git Story Harness

This harness exports commit history into deterministic JSONL batches so multiple
agents can generate replacement or appended commit messages without touching Git
history.

## Smoke Test

```sh
python3 tools/gitstory_harness.py export --limit 10 --out .gitstory-smoke
python3 tools/gitstory_harness.py validate --limit 10 --out .gitstory-smoke
```

## Full Export

```sh
python3 tools/gitstory_harness.py export --out .gitstory --batch-size 200
python3 tools/gitstory_harness.py validate --out .gitstory
```

That produces:

- `.gitstory/commits.jsonl`: every commit in oldest-to-newest order
- `.gitstory/batches/batch_001.jsonl`: agent-sized work packets
- `.gitstory/manifest.md`: coverage and instructions

Each record keeps `original_message` exactly as written. Agents should fill:

- `generated_message`: the diff-grounded real commit message
- `story_notes`: optional continuity notes, uncertainty, or cross-commit context

For very large history entries, `diff_truncated` is set to `true`. Re-run with a
larger `--max-diff-bytes` or inspect that commit directly before finalizing it.

## Optional Noise Reduction

To reduce vendored or generated-code noise while drafting the story:

```sh
python3 tools/gitstory_harness.py export \
  --out .gitstory \
  --batch-size 200 \
  --exclude 'vendor/**' \
  --exclude 'tgt/**' \
  --exclude 'bld/**'
```

Use the unexcluded full export before a final rewrite so the generated messages
still account for every commit.
