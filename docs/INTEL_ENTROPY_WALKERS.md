# Intel entropy walkers: kernel-owned compression research lane

Status: research architecture. This document freezes the memory, math, proof,
and artifact boundaries before TRUEOS admits a new production AOT kernel.

The target is the Intel integrated GT already owned by TRUEOS: caller PPGTT,
GuC scheduling, direct RCS walkers, shared system DRAM, and a storage handoff
that can end in the existing USB UAS path. This is not a portable GPU codec API
and it is not an OpenCL runtime proposal.

## Objective

The research question is stronger than "make compression faster":

> For independent kernel-owned chunks, how close can TRUEOS get to the shortest
> representation justified by an explicit model, while retaining a bit-exact,
> independently testable decoder and a GPU execution shape that matches Xe-LP?

There are three different meanings of "least" and they must not be mixed:

1. **Uncomputable least** -- Kolmogorov complexity is the north star, not an
   implementable target.
2. **Exact least inside a finite class** -- enumerative coding can prove that a
   fixed-weight binary sequence needs `ceil(log2(C(n,k)))` payload bits to name
   one member of that class.
3. **Near-entropy least for an explicit probability model** -- CTW/KT models and
   ANS can be judged against `-log2(P(x))`, with model-description overhead kept
   visible.

The first production experiment should optimize (2) and (3), not invent a new
LZ dictionary.

## Ping/pong is the kernel data path

```text
              CPU / SCSI / UAS producer
                       |
                       v
              +-----------------+
              | PPGTT arena A   |  FILLING -> READY
              +-----------------+
                       |
                       | generation N
                       v
              +-----------------+
              | GuC RCS walkers |  RUNNING
              +-----------------+
                       |
                       v
              +-----------------+
              | PPGTT arena B   |  COMPLETE
              +-----------------+
                       |
                       v
                 UAS DMA drain
                       |
                       +------ swap A/B ------+
```

Both arenas are mapped before submission. Runtime dispatch should mutate
contents and descriptors, not rebuild the GPU address-space contract for every
chunk. The final GPU release is a generation-tagged completion cache line;
"the surface changed" is never an ownership signal.

The new `EntropyStreamBatch`, `EntropyStreamChunk`, and
`EntropyStreamCompletion` records are each one cache line. They intentionally
contain no Rust pointers and no compiler/runtime object handles.

### RP0 window

The batch ABI reserves a `REQUEST_GT_RP0_WINDOW` flag because compression is a
natural short GT burst. This PR does **not** program GT frequency registers.
The future submitter may request the highest admitted GT performance state
immediately before a sufficiently large walker wave and release that request
on retirement. PCODE/thermal limits remain authoritative. Frequency control
must be benchmarked separately from codec correctness.

## Chunk contract

The initial size to benchmark is 256 KiB, not a format commitment. Every result
must also be measured at 64 KiB, 1 MiB, and 4 MiB.

Each chunk is independently decodable. That costs some cross-chunk modeling
power but buys parallel walkers, bounded corruption domains, random access,
independent proof/test vectors, direct ping/pong storage ownership, and a raw
fallback for weakly compressible input.

A future on-disk chunk header should carry at least:

```text
magic / version
uncompressed length
compressed length
model/codec id
model metadata length
checksum
32-state rANS terminal states or enumerative parameters
```

No on-disk format is frozen by this PR.

## Mathematical ladder

### Raw

Raw is a real candidate, not an error path. If model metadata plus entropy
payload exceeds the input, the chunk is stored verbatim.

### Exact enumerative bitplanes

For one bitplane of `n` bytes with exactly `k` set bits, there are exactly
`C(n,k)` possible members. A lexicographic rank therefore needs exactly
`ceil(log2(C(n,k)))` payload bits to identify the member when `n` and `k` are
known.

This is the cleanest first formal target:

```text
rank(x) < C(n,k)
unrank(n,k,rank(x)) == x
```

The host oracle implements both operations with arbitrary-precision integers.
The GPU experiment should begin with fixed small tiles where binomial values fit
an admitted integer representation, then move to multi-limb rank arithmetic or
hierarchical enumerative blocks.

Bitplane enumerative coding is attractive for sparse flags, bitmaps, masks,
indexes, and transformed data. A uniform random bitplane should fall back to
raw/rANS.

### CTW / KT universal modeling

Context Tree Weighting is the ratio oracle for binary context modeling. For a
bounded context tree it recursively mixes a KT estimator at a node with the
product of its children. The classic CTW result gives a finite-sequence
redundancy bound for the model family.

A literal serial CTW decoder has a causality problem for a SIMD32 GPU wave: the
probability of the next bit can depend on bits decoded immediately before it.
TRUEOS should therefore compare three variants rather than pretending this
problem does not exist:

- **CTW-serial** -- strongest research oracle, CPU/reference first;
- **CTW32-lag** -- a context for wave `w` may only use history retired before
  that 32-symbol wave, so all 32 probabilities are known before SIMD decode;
- **static-context** -- a first pass builds context statistics and the encoded
  chunk carries a quantized static model.

Record more than compressed bytes:

```text
ideal model bits       = -log2(P_model(x))
actual coder bits
model metadata bits
coder redundancy       = actual - ideal
parallelism penalty    = CTW32-lag - CTW-serial
```

### 32-state rANS

rANS is the first entropy coder to target for real walkers. One integer state
per lane gives a natural 32-state decomposition. With a static model, symbols
can be decoded in 32-wide waves without a single arithmetic-coder interval
becoming the serialization point.

The host oracle uses the byte-rANS recurrence with 12-bit normalized
frequencies and 32 states. It stores the 32 terminal states followed by the
renormalization stream. The model table is deliberately not hidden inside the
reported payload size.

Two production packing choices should be benchmarked:

1. **one interleaved renormalization stream** -- best packing, requires prefix
   allocation/compaction discipline;
2. **32 lane slices** -- each lane owns a bounded output slice and the header
   stores 32 lengths. This wastes a few hundred bytes per chunk but eliminates
   output collisions and is very friendly to raw walkers and parallel decode.

At 256 KiB, even 256 bytes of lane-state/length metadata is about 0.1% before
model metadata. That may be a good trade for a kernel primitive.

### CTW + rANS

CTW is a model and rANS is a coder; keep them separate. The model produces
probabilities/frequencies. The coder is judged against those probabilities.

A production candidate is only admitted if the decoder has the same causal
information as the encoder. A perfect encoder-side model that requires future
bytes is an oracle with missing metadata, not a codec.

### NML / MDL

Normalized Maximum Likelihood and Minimum Description Length belong in the
offline scoring lane first. Exact NML normalization is often intractable for
interesting model classes, but MDL provides the right discipline: model bytes
are bytes. A 20-bit probability improvement is a regression if it needs a
100-bit model description.

### Bits-back ANS

Bits-back ANS is horizon research, not a base storage codec. It becomes
relevant only when TRUEOS has a useful latent model whose inference cost can
stay on the GT and whose model artifact is already part of the OS. It should
have to beat the much smaller enumerative/CTW/rANS trusted base on real OS
data.

## Walker decomposition

The first useful GPU pipeline is multi-pass on purpose:

```text
walker 0: probe/statistics
    histogram[256]
    bitplane weights[8]
    2-bit and 4-bit binary context counts
    cheap lower-bound scores

CPU or tiny GPU selector:
    raw / enumerative / static-rANS / research CTW variant

walker 1..N:
    transform/model stage if selected

entropy walker:
    32 rANS states or enumerative rank tiles

release packet:
    flush/order output writes
    publish generation/completed_chunks/error_code
```

The supplied WGSL `entropy_probe` is intentionally only walker 0. It gives us
one portable, inspectable source that can be compiled to SPIR-V and compared
against an IGC-native implementation. It does **not** claim that WGSL is the
runtime API.

The production ADL-S path should follow the repository's existing pattern:

```text
source used for audit/reproduction
  -> pinned compiler lane
  -> IGC/ocloc target artifact
  -> ELF/.ze_info/ABI audit
  -> checked-in binary + manifest + Rust contract
  -> caller-PPGTT admission
  -> hand-authored direct-RCS payload / walker
  -> GuC scheduling
```

If direct IGC/GEN ISA is eventually retained instead of a higher-level source,
the same rule applies: the binary and its exact ABI/provenance are the artifact;
the kernel never grows an ambient compiler dependency.

## Why WGSL and raw IGC both belong here

WGSL is useful as a readable mathematical reference, as an input to the
repository's Naga validator/SPIR-V tool, and for differential testing against
the target-specific artifact.

Raw/native IGC is useful for the real experiment because TRUEOS already owns
walker state, PPGTT, payload layout, cache policy, and GuC scheduling. There is
little value in putting a generic compute runtime back between those pieces.

The rule is:

> WGSL specifies behavior; the admitted native artifact specifies execution.

They must agree on test vectors before the native path may own storage output.

## Formal verification target

Do not try to verify the whole Intel command streamer first. Keep the trusted
mathematical core small.

### Tier A: pure mathematics

Prove or mechanically check:

- enumerative `rank < C(n,k)`;
- `unrank(rank(x)) = x` for the fixed-weight class;
- one rANS encode step and decode step are inverses under admitted frequency
  bounds;
- rANS renormalization preserves the admitted state interval;
- normalized frequencies are positive for every emitted symbol and sum exactly
  to `2^R`;
- chunk framing is prefix/length unambiguous.

### Tier B: executable scalar oracle

The Python reference exhaustively checks enumerative inversion for all binary
strings through length 12 and round-trips representative/random rANS32 blocks.
Replace or augment this with a Rust no_std oracle once the container ABI is
stable.

### Tier C: GPU equivalence

For every admitted artifact require:

- same input + same model -> byte-identical encoded payload;
- GPU decode -> original input;
- CPU oracle decodes GPU output;
- GPU decoder decodes CPU-oracle output;
- descriptor canaries and guard pages remain unchanged;
- completion generation is published only after output visibility.

The existing artifact hash/ABI admission then binds the tested machine code to
what the kernel will actually dispatch.

## WGSL probe output

`entropy_probe.wgsl` uses 320 u32 words per chunk:

```text
0..15      summary
16..271    byte histogram[256]
272..279   bitplane ones[8]
280..287   2-bit-context {zero,one} counts[4][2]
288..319   4-bit-context {zero,one} counts[16][2]
```

Summary words:

```text
0   magic "EPR1"
1   byte count
2   occupied byte symbols
3   zero-byte count
4   0xff-byte count
5   raw bit count
6   empirical byte H0 bits, f32 bit pattern
7   bitplane Bernoulli bound, f32 bit pattern
8   2-bit static Markov bound, f32 bit pattern
9   4-bit static Markov bound, f32 bit pattern
10  candidate mask
11  caller flags
12  first byte
13  last byte
14  wrapping byte sum
15  byte xor
```

The f32 scores are selectors only. Exact enumerative size and the CTW oracle
live in `tools/entropy-research/reference.py`.

## Promotion sequence

1. Run the scalar oracle across representative TRUEOS data: kernel images,
   shader artifacts, logs, filesystem metadata, package archives, media, and
   already-compressed input.
2. Compile/inspect the WGSL probe and compare its statistics with a CPU
   implementation.
3. Port the chosen probe/codec to the pinned Intel C++-for-OpenCL/IGC lane or a
   separately reviewed raw IGC artifact.
4. Publish the normal artifact quartet and generated Rust ABI contract.
5. Add a direct-RCS encoder beside the existing subset-sum and 2D walkers.
6. Add physical hardware logs that demonstrate exact completion and no PPGTT
   guard damage.
7. Only then connect ping/pong retirement to UAS ownership and benchmark the
   temporary RP0 window.

Do not add a `.clcpp` file to the production kernel source set without its
matching checked-in artifact publication; the existing compiler-free verifier
is intentionally stricter than this research directory.

## Measurements that decide whether this survives

For every corpus/chunk size record:

```text
raw bytes
encoded payload bytes
model/header bytes
compression ratio
probe ns/byte
encode ns/byte
decode ns/byte
DDR bytes read/written if measurable
GT requested/actual frequency
package + graphics energy delta
UAS end-to-end throughput
```

Compare against at least raw copy, LZ4, zstd-fast, and the current 7z archive
workflow where appropriate. A mathematically elegant model that loses the UAS
pipeline is a research result, not a kernel default.

## Primary references

- F. Willems, Y. Shtarkov, T. Tjalkens, *The context-tree weighting method:
  basic properties*, IEEE Transactions on Information Theory 41(3), 1995,
  DOI `10.1109/18.382012`.
- T. Cover, *Enumerative source encoding*, IEEE Transactions on Information
  Theory 19(1), 1973, DOI `10.1109/TIT.1973.1054929`.
- J. Duda, *Asymmetric numeral systems: entropy coding combining speed of
  Huffman coding with compression rate of arithmetic coding*, arXiv:1311.2540.
- J. Townsend, T. Bird, D. Barber, *Practical Lossless Compression with Latent
  Variables using Bits Back Coding*, arXiv:1901.04866 / ICLR 2019.
