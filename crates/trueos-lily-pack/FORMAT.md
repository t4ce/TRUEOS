# LILYPK01 experimental format

All integers are little endian. This version assumes a 256x256 canvas and an
exact 32-entry RGBA palette, entry zero transparent and all others opaque.
Frame pixels remain palette indices 0..31 until converted to RGBA8 at runtime.
CRC32 uses IEEE polynomial 0xedb88320.

## Outer header, 32 bytes

| Offset | Bytes | Meaning |
| --- | --- | --- |
| 0 | 8 | ASCII `LILYPK01` |
| 8 | 1 | Codec: 0=zlib DEFLATE, 1=Zstandard, 2=XZ/LZMA2, 3=lossless H.264 experiment |
| 9 | 1 | Transform, see below |
| 10 | 2 | Tile side for transform 5, otherwise 0 |
| 12 | 4 | Frame count |
| 16 | 8 | Uncompressed body length |
| 24 | 4 | CRC32 of complete uncompressed body |
| 28 | 4 | Reserved, must be zero |

The compressed body follows immediately. Codec 3 has a special payload described
below. A pack is self-contained, including frame names and all alignment data.

## Uncompressed body

128 bytes: 32 straight RGBA8 palette entries, transparent entry `[0,0,0,0]`.
Then the following directory entry repeated frame-count times:

- u16: path byte length
- UTF8 path bytes, relative to the asset root, slash separated
- 4 x u16: crop x, y, width, height in the original top-left 256x256 canvas
- u32: reference frame index, or 0xffffffff for an all-zero reference
- u32: CRC32 of reconstructed 65536 index bytes for this aligned frame

All pixel data follows the complete directory. Within each frame, order is
row-major left-to-right, top-to-bottom. Frame order is directory order.
Reconstruction must retain this order even when exposing assets by path.

## Transforms

- 0: 65536 index bytes per frame.
- 1: 65536 XOR-residual bytes per frame. XOR each byte with the corresponding
  reconstructed reference-frame byte. A zero reference leaves it unchanged.
- 2: Five bitplanes per frame, least significant index bit first. Each plane
  stores 8192 bytes. Within each byte the first pixel is the least significant bit.
- 3: Same bitplane layout as 2, applied to XOR residual bytes. Unpack planes,
  then XOR against reconstructed reference frames.
- 4: 65536 modulo-32 residual bytes per frame. Add the reference-frame byte,
  then mask with 31. A zero reference leaves the residual unchanged.
- 5: u32 dictionary tile count; dictionary tiles as tightly packed tile-side
  by tile-side index bytes; then u32 dictionary indices, frame-major, tile-row-major.
  Tile side is 4, 8, or 16, so a frame is always an integral grid of tiles.
- 6: Five-bit indices concatenated LSB-first, spanning bytes as needed; frame-major.

References point to earlier frames only. Non-residual transforms use no reference.
All reconstructed frame checksums must match before use.

## Video experiment (codec 3, transform 0)

The payload is u32 metadata length, that many bytes containing the body palette
and directory, then an elementary lossless H.264 stream of grayscale index frames.
The external video decoder must return 65536 exact 8-bit index samples per frame,
without YUV scaling. Concatenate the metadata and decoded samples to reconstruct
the uncompressed body and verify its length, CRC32 and per-frame checksums.

## Restoring images

An index frame already includes transparent canvas padding. Expand each byte
through the shared palette to obtain 256x256 straight RGBA8. To reproduce the
cropped source PNG, select `[x,x+w)` in rows `[y,y+h)`. Animation durations, state
transitions and endpoint reuse are not inferred; the pack retains existing image
assets, not their external application playback semantics.
