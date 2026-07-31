# trueos-kokoro-g2p

Kernel-ready, `no_std + alloc` English text preparation for the resident
Kokoro v1.0 model. The crate provides:

- a bounded, fallible parser for the `G2P2` binary model format;
- exact lexicon lookup and pair-joint n-gram beam decoding;
- a primary pronunciation-override hook for a future Kokoro/Misaki lexicon;
- English acronym and integer normalization while retaining punctuation and
  whitespace boundaries;
- Kokoro IPA canonicalization and the fixed v1.0 character-to-token mapping;
- deterministic model ranges targeting 175–250 tokens, searching as far as
  450 tokens for a natural boundary, with 510 as the graph's final hard limit.

The 450-token fallback is only a lexical chunking opportunity, never a decoder
admission guarantee. The backend must resolve the duration scalar at the
requested speed and reject, split, and retry before phase-one allocation when
the result exceeds the sealed `F=2560` ceiling. In particular, a slow request
such as speed 0.5 can require a split well below 450 tokens.

`FrontendOutput::token_ids` and its `chunks` deliberately exclude Kokoro's
boundary token. For each returned range the backend forms
`[0, token_ids[range], 0]`; therefore the 510-token hard limit becomes at most
512 model-input tokens. This keeps chunk selection independent from the caller's
tensor storage while making the model's BOS/EOS contract explicit.

The parser borrows all strings from the resident model image. N-gram and
backoff keys use fixed six-ID records in contiguous vectors instead of one
heap allocation per key.

## Upstream and license

The model format, backoff scoring, and beam-decoder behavior are adapted from
[`g2p2-core` 0.2.0](https://github.com/jqueguiner/g2p2), copyright © 2026
jqueguiner. Upstream code is offered under **MIT OR Apache-2.0**; the matching
license texts are included in this crate. TRUEOS changes make parsing bounded
and fallible, replace `std` hash maps and boxed keys with borrowed/contiguous
indexes, and add the Kokoro-specific text layer.

The English `.g2p` model is not embedded in this crate. Its standardized
runtime path is `models/kokoro/en.g2p`. The pinned artifact is 6,691,149 bytes
with SHA-256
`091347d375e494b5542202201a24a0f724738a3b18c38d56a87022970c70aa9c`.
Model-data licensing and attribution remain those documented by the upstream
g2p2 project (including its WikiPron/Kaikki source notices).

For that sealed artifact on TRUEOS's x86-64 target, the parsed indexes occupy
exactly 11,587,064 bytes in five contiguous allocations. Together with the
borrowed 6,691,149-byte model image, the deterministic resident payload is
18,278,213 bytes; word decoding then uses only bounded short-lived allocations.
