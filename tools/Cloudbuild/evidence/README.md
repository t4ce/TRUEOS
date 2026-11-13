# Measurement and check evidence

`upload/TrueOS-0.0.235(1).lz4` is the exact user-supplied release, preserved by
SHA-256. `analysis/` contains the inspection helpers and reports from the
conversation. The scripts never execute the extracted ELF modules. The outer
archive inspector locates the upload through the relative `../upload/` path.
The app inspector expects the decoded AppDB as `frame-14615049.raw`; the
reference report records its compressed offset, length and hashes. These are
snapshot-specific inspection helpers, not a generic redb migration tool.

## Reproduce release and app measurements

The helpers require Python 3 and a system `liblz4` discoverable by
`ctypes.util.find_library('lz4')`. Work on a copy of `evidence/`, because the
inspection scripts regenerate JSON reports and extracted files there.
From that copy's `analysis/` directory:

```sh
python3 analyze_release.py
python3 - <<'PY'
import json
from analyze_release import ROOT, decode_frame, sha

frames = json.loads((ROOT / 'embedded-frames.json').read_text())
frame = next(item for item in frames if item['offset'] == 14615049)
kernel = (ROOT / 'extracted/TRUEOS.elf').read_bytes()
start = frame['offset']
packed = kernel[start:start + frame['packed_bytes']]
if len(packed) != frame['packed_bytes'] or sha(packed) != frame['sha256']:
    raise ValueError('Embedded AppDB frame does not match the reference')
raw, consumed = decode_frame(packed, max_output=frame['raw_bytes'])
if (consumed != len(packed) or len(raw) != frame['raw_bytes']
        or sha(raw) != frame['raw_sha256']):
    raise ValueError('Decoded AppDB does not match the reference')
(ROOT / f'frame-{start}.raw').write_bytes(raw)
print(f'Verified {len(packed):,} packed bytes and {len(raw):,} decoded bytes')
PY
python3 inspect_buildins.py
```

This reproduces the ELF/package lengths, hashes and import listings. The
historical Unix/POSIX classification still depends on the classifier at the
recorded TRUEOS source reference; the helper does not relabel those imports
using a newer classifier. The supplied app report was also enriched with
rounded kB and classified import counts; regeneration produces the raw
inspection fields only. The export-date recheck confirmed that all of those
raw fields match the supplied report for all 12 apps.

## Selector checks

`ui-checks/` contains the historical DOM/API reports and their check scripts.
Only three setup lines were adapted for portability: import `jsdom` normally,
read the checkout from `TRUEOS_REPOSITORY` (or the working directory), and use
`TRUEOS_QA_OUTPUT_DIR` (or the script directory) for temporary outputs. The
assertions are unchanged. Dependency version: jsdom 26.1.0. Earlier scripts
record earlier implementation stages; use `check-size-estimates-dom.cjs` for
the final size-panel checks. No browser layout assertion is claimed.

Optional final UI check, after importing the branch into a complete checkout:

```sh
cd /path/to/handoff/evidence/ui-checks
npm install --ignore-scripts
mkdir -p /path/to/qa-output
TRUEOS_REPOSITORY=/path/to/TRUEOS TRUEOS_QA_OUTPUT_DIR=/path/to/qa-output \
  node check-size-estimates-dom.cjs
```

Runtime queue state, bearer tokens, generated identity keys, extracted copies
of every binary, and node_modules are not part of this archive. Results of the
export's patch/tree, bundle and ZIP checks are under `../validation/`.
