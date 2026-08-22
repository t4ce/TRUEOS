# trueos-kokoro-lexicon

Allocation-free, `no_std` access to the high-quality Misaki US pronunciation
overlay used by TRUEOS Kokoro. JSON never enters the kernel: the offline
compiler emits a canonical KLEX v1 image, and the runtime validates its whole
header/payload SHA-256 seal before doing zero-copy binary searches.

The sealed runtime path is `models/kokoro/misaki-us.klex`. The current artifact
contains 389,904 sorted unique default pronunciations and 41 non-null
POS/tag variants in 15,844,468 bytes. Its SHA-256 is
`df5e2a52110c70c3b04a722bb24fc4fa59f2457dcb7b4b3a5c110ff60a4ca03b`.
`Lexicon` implements `trueos_kokoro_g2p::PronunciationLookup`, so these stressed
Kokoro-native pronunciations take priority and the compact G2P2 model remains
the out-of-vocabulary fallback.

## Deterministic source profile

The compiler accepts only these exact inputs from
[`misaki-rs`](https://github.com/MicheleYin/misaki-rs) commit
`7bbe06cacd9102d8a0d9e338a3711ae7208de0ad`:

- `data/us_silver.json`: 299,704 entries, SHA-256
  `57cae2a1a9d73ce219ad9142b0d904914a0228cb1babce20e5bfd4e1b1307ee4`;
- `data/us_gold.json`: 90,201 entries, SHA-256
  `bb83c899d8dbfa160fa05661bea052bacfeece9b639851662334e85002ee8ad9`;
- `LICENSE`: SHA-256
  `1bea4b79e660b7477ea5919bed5944d970c86531b508bd1d538309c0d12e8858`.

Silver is loaded first. Gold then replaces the default for an identical key
(the pinned sources overlap once). Gold objects must contain a string
`DEFAULT`; all 41 non-null non-`DEFAULT` values are retained in the side table
and exposed through `get_variant`. Null POS markers are intentionally omitted.
Both tables are sorted by their UTF-8 bytes and reject duplicates.

From the TRUEOS root, regenerate and byte-verify the local model asset with:

```bash
python3 tools/ttstt/compile_misaki_lexicon.py \
  --silver /path/to/misaki-rs/data/us_silver.json \
  --gold /path/to/misaki-rs/data/us_gold.json \
  --license /path/to/misaki-rs/LICENSE \
  --output tools/trueos-ttstt/.ttstt/models/kokoro/misaki-us.klex

python3 tools/ttstt/compile_misaki_lexicon.py \
  --silver /path/to/misaki-rs/data/us_silver.json \
  --gold /path/to/misaki-rs/data/us_gold.json \
  --license /path/to/misaki-rs/LICENSE \
  --output tools/trueos-ttstt/.ttstt/models/kokoro/misaki-us.klex --check
```

## Format and validation

KLEX v1 has a 256-byte little-endian header, 12-byte default records, 16-byte
variant records, then one canonical packed string pool. Records contain only
checked offsets and lengths. Parsing rejects unknown versions/flags, record
size or offset drift, nonzero reserved bytes, count/length limits, arithmetic
overflow, bad UTF-8, gaps/overlaps/trailing bytes, unsorted or duplicate keys,
invalid variant references, missing provenance, and any seal mismatch.

The direct `misaki-rs` port is MIT-licensed; its exact notice is included as
`LICENSE-MISAKI-RS`. The underlying dictionary originates in
[`hexgrad/misaki`](https://github.com/hexgrad/misaki), licensed under Apache
2.0; the license text is included as `LICENSE-APACHE`. TRUEOS's parser/compiler
code is offered under MIT OR Apache-2.0. This crate ports data, not the
`misaki-rs` runtime or its `std`, regex, JSON, POS-tagger, or espeak stack.
