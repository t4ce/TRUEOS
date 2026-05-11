# Commit Message Generate

This harness exports commit history into deterministic JSONL batches and
per-commit Markdown files so agents can generate a coherent replacement-message
catalog without touching Git history.

Use `origin/main` as the default source ref for this mission. That keeps the
catalog keyed to the original upstream commit hashes even if local `main` has
experimental rewritten SHAs.

## Smoke Test

```sh
python3 tools/CommitMessageGenerate/gitstory_harness.py scaffold \
  --limit 10 \
  --out tools/CommitMessageGenerate/work/smoke \
  --revs origin/main

python3 tools/CommitMessageGenerate/gitstory_harness.py validate \
  --limit 10 \
  --out tools/CommitMessageGenerate/work/smoke \
  --revs origin/main
```

## Full Catalog

```sh
python3 tools/CommitMessageGenerate/gitstory_harness.py scaffold \
  --out tools/CommitMessageGenerate/work/catalog_origin_main \
  --revs origin/main \
  --agents 5 \
  --context 12 \
  --batch-size 200

python3 tools/CommitMessageGenerate/gitstory_harness.py validate \
  --out tools/CommitMessageGenerate/work/catalog_origin_main \
  --revs origin/main
```

That produces:

- `commits.jsonl`: every commit in oldest-to-newest order
- `batches/batch_001.jsonl`: simple batch packets
- `ranges/agent_01_owned.jsonl`: one agent's write-owned commits
- `ranges/agent_01_context.jsonl`: neighboring read-only context
- `messages/<sha>.md`: one generated-message file per original commit hash
- `AGENT_RANGES.md`: the agent assignment plan
- `manifest.md`: coverage and instructions

Each record keeps `original_message` exactly as written. Agents should fill:

- `Generated Message`: the diff-grounded real commit message
- `Story Notes`: continuity notes, uncertainty, or cross-commit context

For very large history entries, `diff_truncated` is set to `true`. Re-run with a
larger `--max-diff-bytes` or inspect that commit directly before finalizing it.

## Agent Contract

Each agent gets an owned range and a neighboring context range.

- Read the context range first to understand the local story.
- Write only `messages/<sha>.md` files for owned commits.
- Preserve `Original Message` exactly.
- Prefer generated messages that explain the actual code movement, not generic
  summaries.
- Small `ok` commits should be interpreted with nearby commits when the diff is
  too small to stand alone.
- Do not rewrite Git history from this catalog.

## Optional Noise Reduction

To reduce vendored or generated-code noise while drafting the story:

```sh
python3 tools/CommitMessageGenerate/gitstory_harness.py scaffold \
  --out tools/CommitMessageGenerate/work/catalog_origin_main \
  --revs origin/main \
  --batch-size 200 \
  --exclude 'vendor/**' \
  --exclude 'tgt/**' \
  --exclude 'bld/**'
```

Use the unexcluded full export before a final rewrite so the generated messages
still account for every commit.
