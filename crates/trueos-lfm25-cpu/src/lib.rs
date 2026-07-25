#![no_std]

//! Scalar, deterministic LFM2.5 CPU kernels for TRUEOS's hybrid decoder.
//!
//! This crate deliberately owns numerical primitives only. Model I/O, token
//! scheduling, FPGA submission, and cache ownership remain in TRUEOS.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use half::{bf16, f16};
use sha2::{Digest, Sha256};

pub const RMS_EPSILON: f32 = 1.0e-5;
pub const ROPE_FREQUENCY_BASE: f32 = 1_000_000.0;
pub const Q8_BLOCK_VALUES: usize = trueos_fpga_abi::lfm25::Q8_0_BLOCK_VALUES;
pub const Q8_BLOCK_BYTES: usize = trueos_fpga_abi::lfm25::Q8_0_BLOCK_BYTES;
pub const HEAD_DIMENSION: usize = trueos_fpga_abi::lfm25::MODEL_HEAD_DIMENSION as usize;
pub const HALF_HEAD_DIMENSION: usize = HEAD_DIMENSION / 2;
pub const PACKED_Q8X16_ROWS: usize = 16;
pub const PACKED_Q8X16_BLOCKS_PER_PAIR: usize = 2;
pub const PACKED_Q8X16_WORDS_PER_BLOCK: usize = Q8_BLOCK_VALUES / 4;
pub const PACKED_Q8X16_PAIR_BYTES: usize = PACKED_Q8X16_BLOCKS_PER_PAIR
    * (PACKED_Q8X16_ROWS * core::mem::size_of::<u16>() + PACKED_Q8X16_ROWS * Q8_BLOCK_VALUES);
pub const PACKED_Q8X16_TENSOR_COUNT: usize = 93;
pub const PACKED_Q8X16_BLOCK_TILES: u64 = 692_224;
pub const PACKED_Q8X16_QUANTIZED_VALUES: u64 = 354_418_688;
pub const PACKED_Q8X16_SUBNORMAL_SCALES: u64 = 25_994;
pub const PACKED_Q8X16_IMAGE_SHA256: [u8; 32] = [
    0x90, 0x87, 0x6f, 0x02, 0xe0, 0xcc, 0x22, 0x4f, 0xe2, 0x3e, 0x01, 0xc8, 0x73, 0x9d, 0xcb, 0xb9,
    0x4d, 0x7b, 0xcc, 0x8f, 0xbf, 0xa3, 0xd3, 0x62, 0x04, 0xc6, 0x26, 0x7a, 0x44, 0x0f, 0x5f, 0xd8,
];

const _: () = assert!(PACKED_Q8X16_PAIR_BYTES == 1_088);
const _: () = assert!(PACKED_Q8X16_PAIR_BYTES.is_multiple_of(64));

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedQ8x16Stats {
    pub tensor_count: usize,
    pub block_tiles: u64,
    pub quantized_values: u64,
    pub subnormal_scales: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Shape,
    Encoding,
    Artifact,
    Vocabulary,
    Allocation,
    NonFinite,
}

const TOKENIZER_MAGIC: [u8; 8] = *b"LFTOK1\0\0";
const TOKENIZER_VERSION: u32 = 1;
const TOKENIZER_HEADER_BYTES: usize = 72;

pub const F32_SIDECAR_MAGIC: [u8; 8] = *b"LFMF32V1";
pub const F32_SIDECAR_VERSION: u32 = 1;
pub const F32_SIDECAR_HEADER_BYTES: usize = 160;
pub const F32_SIDECAR_TENSOR_COUNT: usize = 55;
pub const F32_SIDECAR_ENTRY_BYTES: usize = 16;
pub const F32_SIDECAR_ELEMENT_COUNT: usize = 65_280;
pub const F32_SIDECAR_PAYLOAD_OFFSET: usize =
    F32_SIDECAR_HEADER_BYTES + F32_SIDECAR_TENSOR_COUNT * F32_SIDECAR_ENTRY_BYTES;
pub const F32_SIDECAR_BYTES: usize = F32_SIDECAR_PAYLOAD_OFFSET + F32_SIDECAR_ELEMENT_COUNT * 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct F32SidecarEntry {
    tensor_id: u16,
    element_offset: usize,
    elements: usize,
}

/// Validated software-only view of every source-F32 tensor used by the hybrid
/// decoder. The tensor IDs and order are the generated native-image IDs, even
/// though the fixed native image stores these particular tensors as BF16.
pub struct F32Sidecar {
    entries: Vec<F32SidecarEntry>,
    values: Vec<f32>,
}

impl F32Sidecar {
    pub fn from_artifact(artifact: &[u8]) -> Result<Self, Error> {
        if artifact.len() != F32_SIDECAR_BYTES
            || artifact.get(..8) != Some(F32_SIDECAR_MAGIC.as_slice())
            || artifact_u32(artifact, 8)? != F32_SIDECAR_VERSION
            || artifact_u32(artifact, 12)? as usize != F32_SIDECAR_HEADER_BYTES
            || artifact_u32(artifact, 16)? as usize != F32_SIDECAR_TENSOR_COUNT
            || artifact_u32(artifact, 20)? as usize != F32_SIDECAR_ENTRY_BYTES
            || artifact_u32(artifact, 24)? as usize != F32_SIDECAR_ELEMENT_COUNT
            || artifact_u32(artifact, 28)? as usize != F32_SIDECAR_PAYLOAD_OFFSET
            || artifact.get(32..64) != Some(trueos_fpga_abi::lfm25::PINNED_GGUF_SHA256.as_slice())
            || artifact.get(64..96)
                != Some(trueos_fpga_abi::lfm25::PINNED_NATIVE_IMAGE_SHA256.as_slice())
            || artifact.get(96..128)
                != Some(
                    trueos_fpga_abi::lfm25::generated::MODEL_SEAL
                        .tensor_table_sha256
                        .as_slice(),
                )
        {
            return Err(Error::Artifact);
        }
        let payload = artifact
            .get(F32_SIDECAR_PAYLOAD_OFFSET..)
            .ok_or(Error::Artifact)?;
        let observed_payload_hash: [u8; 32] = Sha256::digest(payload).into();
        if artifact.get(128..160) != Some(observed_payload_hash.as_slice()) {
            return Err(Error::Artifact);
        }

        let expected = trueos_fpga_abi::lfm25::generated::TENSORS
            .iter()
            .filter(|descriptor| {
                trueos_fpga_abi::lfm25::TensorFormat::from_raw(descriptor.format)
                    == Some(trueos_fpga_abi::lfm25::TensorFormat::Bf16Le)
            });
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(F32_SIDECAR_TENSOR_COUNT)
            .map_err(|_| Error::Allocation)?;
        let mut next_payload = F32_SIDECAR_PAYLOAD_OFFSET;
        let mut total_elements = 0usize;
        for (entry_index, descriptor) in expected.enumerate() {
            if entry_index >= F32_SIDECAR_TENSOR_COUNT {
                return Err(Error::Artifact);
            }
            let offset = F32_SIDECAR_HEADER_BYTES + entry_index * F32_SIDECAR_ENTRY_BYTES;
            let tensor_id = artifact_u16(artifact, offset)?;
            let reserved = artifact_u16(artifact, offset + 2)?;
            let elements = artifact_u32(artifact, offset + 4)? as usize;
            let payload_offset = artifact_u32(artifact, offset + 8)? as usize;
            let payload_bytes = artifact_u32(artifact, offset + 12)? as usize;
            let expected_elements = (descriptor.ggml_ne0 as usize)
                .checked_mul(descriptor.ggml_ne1 as usize)
                .ok_or(Error::Artifact)?;
            if tensor_id != descriptor.tensor_id
                || reserved != 0
                || elements != expected_elements
                || payload_offset != next_payload
                || payload_bytes != elements.checked_mul(4).ok_or(Error::Artifact)?
            {
                return Err(Error::Artifact);
            }
            entries.push(F32SidecarEntry {
                tensor_id,
                element_offset: total_elements,
                elements,
            });
            total_elements = total_elements
                .checked_add(elements)
                .ok_or(Error::Artifact)?;
            next_payload = next_payload
                .checked_add(payload_bytes)
                .ok_or(Error::Artifact)?;
        }
        if entries.len() != F32_SIDECAR_TENSOR_COUNT
            || total_elements != F32_SIDECAR_ELEMENT_COUNT
            || next_payload != artifact.len()
        {
            return Err(Error::Artifact);
        }

        let mut values = Vec::new();
        values
            .try_reserve_exact(F32_SIDECAR_ELEMENT_COUNT)
            .map_err(|_| Error::Allocation)?;
        for word in payload.chunks_exact(4) {
            values.push(f32::from_bits(u32::from_le_bytes(
                word.try_into().map_err(|_| Error::Artifact)?,
            )));
        }
        if values.len() != F32_SIDECAR_ELEMENT_COUNT
            || values.iter().any(|value| !value.is_finite())
        {
            return Err(Error::Artifact);
        }
        Ok(Self { entries, values })
    }

    pub fn tensor(&self, tensor_id: u16) -> Result<&[f32], Error> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.tensor_id == tensor_id)
            .ok_or(Error::Artifact)?;
        self.values
            .get(entry.element_offset..entry.element_offset + entry.elements)
            .ok_or(Error::Artifact)
    }

    pub fn tensor_ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.entries.iter().map(|entry| entry.tensor_id)
    }

    pub const fn element_count(&self) -> usize {
        self.values.len()
    }
}

/// Compact no_std view of the exact LFM2.5 GPT-2/Llama-3 BPE vocabulary.
///
/// The companion host binary extracts this from the pinned GGUF. TRUEOS loads
/// the resulting artifact beside the native tensor image, so tokenization
/// remains ordinary CPU code and never becomes FPGA or kernel compiler state.
pub struct Lfm25Tokenizer {
    pieces: Vec<Vec<u8>>,
    token_types: Vec<u8>,
    token_to_id: BTreeMap<Vec<u8>, u32>,
    merges: BTreeMap<(u32, u32), (u32, u32)>,
    byte_tokens: [u32; 256],
    bos: u32,
    eos: u32,
    pad: u32,
    im_start: u32,
    im_end: u32,
}

impl Lfm25Tokenizer {
    pub fn from_artifact(artifact: &[u8]) -> Result<Self, Error> {
        if artifact.len() < TOKENIZER_HEADER_BYTES || artifact[..8] != TOKENIZER_MAGIC {
            return Err(Error::Artifact);
        }
        let version = artifact_u32(artifact, 8)?;
        let vocabulary = artifact_u32(artifact, 12)? as usize;
        let merge_count = artifact_u32(artifact, 16)? as usize;
        let bos = artifact_u32(artifact, 20)?;
        let eos = artifact_u32(artifact, 24)?;
        let pad = artifact_u32(artifact, 28)?;
        let im_start = artifact_u32(artifact, 32)?;
        let im_end = artifact_u32(artifact, 36)?;
        if version != TOKENIZER_VERSION
            || vocabulary != trueos_fpga_abi::lfm25::MODEL_VOCABULARY_SIZE as usize
            || artifact[40..72] != trueos_fpga_abi::lfm25::PINNED_GGUF_SHA256
        {
            return Err(Error::Artifact);
        }

        let mut pieces = Vec::new();
        let mut token_types = Vec::new();
        let mut token_to_id = BTreeMap::new();
        pieces
            .try_reserve_exact(vocabulary)
            .map_err(|_| Error::Allocation)?;
        token_types
            .try_reserve_exact(vocabulary)
            .map_err(|_| Error::Allocation)?;
        let mut cursor = TOKENIZER_HEADER_BYTES;
        for token in 0..vocabulary {
            let token_type = *artifact.get(cursor).ok_or(Error::Artifact)?;
            cursor = cursor.checked_add(1).ok_or(Error::Artifact)?;
            let bytes = artifact_u32(artifact, cursor)? as usize;
            cursor = cursor.checked_add(4).ok_or(Error::Artifact)?;
            let end = cursor.checked_add(bytes).ok_or(Error::Artifact)?;
            let piece = artifact.get(cursor..end).ok_or(Error::Artifact)?.to_vec();
            cursor = end;
            let token = u32::try_from(token).map_err(|_| Error::Artifact)?;
            if token_to_id.insert(piece.clone(), token).is_some() {
                return Err(Error::Vocabulary);
            }
            pieces.push(piece);
            token_types.push(token_type);
        }

        let mut merges = BTreeMap::new();
        for rank in 0..merge_count {
            let left = artifact_u32(artifact, cursor)?;
            let right = artifact_u32(artifact, cursor + 4)?;
            let merged = artifact_u32(artifact, cursor + 8)?;
            cursor = cursor.checked_add(12).ok_or(Error::Artifact)?;
            if left as usize >= vocabulary
                || right as usize >= vocabulary
                || merged as usize >= vocabulary
                || merges
                    .insert(
                        (left, right),
                        (u32::try_from(rank).map_err(|_| Error::Artifact)?, merged),
                    )
                    .is_some()
            {
                return Err(Error::Vocabulary);
            }
        }
        if cursor != artifact.len()
            || [bos, eos, pad, im_start, im_end]
                .iter()
                .any(|&token| token as usize >= vocabulary)
        {
            return Err(Error::Artifact);
        }

        let mut byte_tokens = [u32::MAX; 256];
        for byte in 0u16..=255 {
            if let Some(&token) = token_to_id.get(&vec![byte as u8]) {
                byte_tokens[byte as usize] = token;
            }
        }
        if byte_tokens.iter().any(|&token| token == u32::MAX) {
            return Err(Error::Vocabulary);
        }

        Ok(Self {
            pieces,
            token_types,
            token_to_id,
            merges,
            byte_tokens,
            bos,
            eos,
            pad,
            im_start,
            im_end,
        })
    }

    pub const fn bos_id(&self) -> u32 {
        self.bos
    }

    pub const fn eos_id(&self) -> u32 {
        self.eos
    }

    pub const fn pad_id(&self) -> u32 {
        self.pad
    }

    pub const fn im_start_id(&self) -> u32 {
        self.im_start
    }

    pub const fn im_end_id(&self) -> u32 {
        self.im_end
    }

    pub fn is_stop(&self, token: u32) -> bool {
        token == self.eos || token == self.im_end || token == self.im_start
    }

    pub fn encode(&self, text: &str) -> Result<Vec<u32>, Error> {
        let mut output = Vec::new();
        for piece in llama3_pieces(text) {
            self.encode_piece(piece.as_bytes(), &mut output)?;
        }
        Ok(output)
    }

    /// Exact single-user Liquid chat envelope used by the pinned GGUF.
    pub fn encode_user_turn(&self, prompt: &str) -> Result<Vec<u32>, Error> {
        let mut tokens = Vec::new();
        tokens
            .try_reserve_exact(prompt.len().saturating_add(10))
            .map_err(|_| Error::Allocation)?;
        tokens.push(self.bos);
        tokens.push(self.im_start);
        tokens.extend(self.encode("user\n")?);
        tokens.extend(self.encode(prompt)?);
        tokens.push(self.im_end);
        tokens.extend(self.encode("\n")?);
        tokens.push(self.im_start);
        tokens.extend(self.encode("assistant\n")?);
        Ok(tokens)
    }

    /// Exact Liquid chat envelope with one system instruction and one user turn.
    ///
    /// Stateful callers feed this only for the first turn. Follow-up turns use
    /// [`Self::encode_followup_user_turn`] so the system prefix remains resident
    /// in the existing model state without being replayed.
    pub fn encode_system_user_turn(&self, system: &str, prompt: &str) -> Result<Vec<u32>, Error> {
        let mut tokens = Vec::new();
        tokens
            .try_reserve_exact(system.len().saturating_add(prompt.len()).saturating_add(18))
            .map_err(|_| Error::Allocation)?;
        tokens.push(self.bos);
        tokens.push(self.im_start);
        tokens.extend(self.encode("system\n")?);
        tokens.extend(self.encode(system)?);
        tokens.push(self.im_end);
        tokens.extend(self.encode("\n")?);
        tokens.push(self.im_start);
        tokens.extend(self.encode("user\n")?);
        tokens.extend(self.encode(prompt)?);
        tokens.push(self.im_end);
        tokens.extend(self.encode("\n")?);
        tokens.push(self.im_start);
        tokens.extend(self.encode("assistant\n")?);
        Ok(tokens)
    }

    /// Exact continuation envelope after the preceding assistant terminator
    /// has been consumed by the same decode session.
    ///
    /// Unlike [`Self::encode_user_turn`], this deliberately omits BOS and the
    /// previous assistant `<|im_end|>` token. The resident caller feeds the
    /// actual generated terminator first, then this suffix, preserving the
    /// existing KV/short-convolution state without replaying chat history.
    pub fn encode_followup_user_turn(&self, prompt: &str) -> Result<Vec<u32>, Error> {
        let mut tokens = Vec::new();
        tokens
            .try_reserve_exact(prompt.len().saturating_add(9))
            .map_err(|_| Error::Allocation)?;
        tokens.extend(self.encode("\n")?);
        tokens.push(self.im_start);
        tokens.extend(self.encode("user\n")?);
        tokens.extend(self.encode(prompt)?);
        tokens.push(self.im_end);
        tokens.extend(self.encode("\n")?);
        tokens.push(self.im_start);
        tokens.extend(self.encode("assistant\n")?);
        Ok(tokens)
    }

    pub fn decode(&self, tokens: &[u32], skip_special: bool) -> Result<Vec<u8>, Error> {
        let mut output = Vec::new();
        for &token in tokens {
            let index = token as usize;
            let piece = self.pieces.get(index).ok_or(Error::Vocabulary)?;
            let token_type = *self.token_types.get(index).ok_or(Error::Vocabulary)?;
            if skip_special && matches!(token_type, 3 | 4 | 5) {
                continue;
            }
            output
                .try_reserve(piece.len())
                .map_err(|_| Error::Allocation)?;
            output.extend_from_slice(piece);
        }
        clean_tokenizer_spaces(&mut output);
        Ok(output)
    }

    fn encode_piece(&self, piece: &[u8], output: &mut Vec<u32>) -> Result<(), Error> {
        if piece.is_empty() {
            return Ok(());
        }
        // LFM2's `ignore_merges` policy emits a complete regex piece directly
        // when the vocabulary owns it.
        if let Some(&token) = self.token_to_id.get(piece) {
            output.push(token);
            return Ok(());
        }

        let mut symbols = Vec::new();
        symbols
            .try_reserve_exact(piece.len())
            .map_err(|_| Error::Allocation)?;
        for &byte in piece {
            symbols.push(self.byte_tokens[byte as usize]);
        }
        while symbols.len() > 1 {
            let mut best: Option<(u32, usize, u32)> = None;
            for index in 0..symbols.len() - 1 {
                let Some(&(rank, merged)) = self.merges.get(&(symbols[index], symbols[index + 1]))
                else {
                    continue;
                };
                if best.is_none_or(|(best_rank, best_index, _)| {
                    rank < best_rank || (rank == best_rank && index < best_index)
                }) {
                    best = Some((rank, index, merged));
                }
            }
            let Some((_, index, merged)) = best else {
                break;
            };
            symbols[index] = merged;
            symbols.remove(index + 1);
        }
        output
            .try_reserve(symbols.len())
            .map_err(|_| Error::Allocation)?;
        output.extend(symbols);
        Ok(())
    }
}

fn artifact_u32(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let end = offset.checked_add(4).ok_or(Error::Artifact)?;
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..end)
            .ok_or(Error::Artifact)?
            .try_into()
            .map_err(|_| Error::Artifact)?,
    ))
}

fn artifact_u16(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    let end = offset.checked_add(2).ok_or(Error::Artifact)?;
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..end)
            .ok_or(Error::Artifact)?
            .try_into()
            .map_err(|_| Error::Artifact)?,
    ))
}

fn is_letter(ch: char) -> bool {
    ch.is_alphabetic()
}

fn is_number(ch: char) -> bool {
    ch.is_numeric()
}

fn is_symbol(ch: char) -> bool {
    !ch.is_whitespace() && !is_letter(ch) && !is_number(ch)
}

fn next_char(text: &str, offset: usize) -> Option<(char, usize)> {
    let ch = text.get(offset..)?.chars().next()?;
    Some((ch, offset + ch.len_utf8()))
}

fn contraction_bytes(text: &str, offset: usize) -> Option<usize> {
    let tail = text.get(offset..)?;
    ["'re", "'ve", "'ll", "'s", "'t", "'m", "'d"]
        .iter()
        .find_map(|suffix| {
            let candidate = tail.get(..suffix.len())?;
            candidate
                .eq_ignore_ascii_case(suffix)
                .then_some(offset + suffix.len())
        })
}

/// Deterministic scanner equivalent to LFM2's single Llama-3 pre-tokenizer
/// expression. Returned slices preserve the original UTF-8 bytes.
fn llama3_pieces(text: &str) -> Vec<&str> {
    let mut output = Vec::new();
    let mut offset = 0usize;
    while offset < text.len() {
        if let Some(end) = contraction_bytes(text, offset) {
            output.push(&text[offset..end]);
            offset = end;
            continue;
        }
        let Some((first, first_end)) = next_char(text, offset) else {
            break;
        };

        let letter_start = if is_letter(first) {
            Some(first_end)
        } else if first != '\r' && first != '\n' && !is_number(first) {
            next_char(text, first_end)
                .filter(|(next, _)| is_letter(*next))
                .map(|(_, end)| end)
        } else {
            None
        };
        if let Some(mut end) = letter_start {
            while let Some((next, next_end)) = next_char(text, end) {
                if !is_letter(next) {
                    break;
                }
                end = next_end;
            }
            output.push(&text[offset..end]);
            offset = end;
            continue;
        }

        if is_number(first) {
            let mut end = first_end;
            let mut count = 1usize;
            while count < 3 {
                let Some((next, next_end)) = next_char(text, end) else {
                    break;
                };
                if !is_number(next) {
                    break;
                }
                end = next_end;
                count += 1;
            }
            output.push(&text[offset..end]);
            offset = end;
            continue;
        }

        let symbol_start = if first == ' ' {
            next_char(text, first_end)
                .filter(|(next, _)| is_symbol(*next))
                .map(|(_, end)| end)
        } else if is_symbol(first) {
            Some(first_end)
        } else {
            None
        };
        if let Some(mut end) = symbol_start {
            while let Some((next, next_end)) = next_char(text, end) {
                if !is_symbol(next) {
                    break;
                }
                end = next_end;
            }
            while let Some((next, next_end)) = next_char(text, end) {
                if next != '\r' && next != '\n' {
                    break;
                }
                end = next_end;
            }
            output.push(&text[offset..end]);
            offset = end;
            continue;
        }

        if first.is_whitespace() {
            let mut end = first_end;
            let mut contains_newline = first == '\r' || first == '\n';
            while let Some((next, next_end)) = next_char(text, end) {
                if !next.is_whitespace() {
                    break;
                }
                contains_newline |= next == '\r' || next == '\n';
                end = next_end;
            }
            if contains_newline || end == text.len() {
                output.push(&text[offset..end]);
                offset = end;
                continue;
            }
            output.push(&text[offset..end]);
            offset = end;
            continue;
        }

        output.push(&text[offset..first_end]);
        offset = first_end;
    }
    output
}

fn clean_tokenizer_spaces(bytes: &mut Vec<u8>) {
    let mut write = 0usize;
    for read in 0..bytes.len() {
        let byte = bytes[read];
        if write > 0 && bytes[write - 1] == b' ' && matches!(byte, b'?' | b'!' | b'.' | b',') {
            write -= 1;
        }
        bytes[write] = byte;
        write += 1;
    }
    bytes.truncate(write);
}

#[inline]
pub fn bf16_from_le_bytes(bytes: &[u8]) -> Result<f32, Error> {
    let word: [u8; 2] = bytes.try_into().map_err(|_| Error::Encoding)?;
    Ok(bf16::from_bits(u16::from_le_bytes(word)).to_f32())
}

pub fn decode_bf16_vector(bytes: &[u8]) -> Result<Vec<f32>, Error> {
    if bytes.len() % 2 != 0 {
        return Err(Error::Shape);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(bytes.len() / 2)
        .map_err(|_| Error::Allocation)?;
    for word in bytes.chunks_exact(2) {
        output.push(bf16_from_le_bytes(word)?);
    }
    Ok(output)
}

/// Round an attention-cache value exactly once to the pinned F16 storage type.
#[inline]
pub fn f16_cache_bits(value: f32) -> Result<u16, Error> {
    if !value.is_finite() {
        return Err(Error::NonFinite);
    }
    Ok(f16::from_f32(value).to_bits())
}

/// Consume one value from the pinned F16 attention cache as F32.
#[inline]
pub fn f16_cache_f32(bits: u16) -> f32 {
    f16::from_bits(bits).to_f32()
}

#[inline]
pub fn q8_row_bytes(elements: usize) -> Result<usize, Error> {
    if elements == 0 || elements % Q8_BLOCK_VALUES != 0 {
        return Err(Error::Shape);
    }
    elements
        .checked_div(Q8_BLOCK_VALUES)
        .and_then(|blocks| blocks.checked_mul(Q8_BLOCK_BYTES))
        .ok_or(Error::Shape)
}

fn packed_q8x16_admitted_shape(columns: usize, rows: usize) -> bool {
    if columns == trueos_fpga_abi::lfm25::MODEL_HIDDEN_SIZE as usize {
        matches!(rows, 512 | 1_024 | 3_072 | 4_608 | 65_536)
    } else {
        columns == trueos_fpga_abi::lfm25::MODEL_FEED_FORWARD_SIZE as usize && rows == 1_024
    }
}

fn packed_q8x16_scale(bits: u16) -> Result<bool, Error> {
    if bits & 0x8000 != 0 || bits & 0x7c00 == 0x7c00 {
        return Err(Error::NonFinite);
    }
    Ok(bits & 0x7c00 == 0 && bits & 0x03ff != 0)
}

fn pack_q8x16_tensor_in_place(
    matrix: &mut [u8],
    rows: usize,
    columns: usize,
    scratch: &mut [u8],
    stats: &mut PackedQ8x16Stats,
) -> Result<(), Error> {
    if !packed_q8x16_admitted_shape(columns, rows)
        || rows % PACKED_Q8X16_ROWS != 0
        || columns % (Q8_BLOCK_VALUES * PACKED_Q8X16_BLOCKS_PER_PAIR) != 0
    {
        return Err(Error::Shape);
    }
    let blocks = columns / Q8_BLOCK_VALUES;
    let pairs = blocks / PACKED_Q8X16_BLOCKS_PER_PAIR;
    let row_bytes = q8_row_bytes(columns)?;
    let tile_bytes = PACKED_Q8X16_ROWS
        .checked_mul(row_bytes)
        .ok_or(Error::Shape)?;
    if matrix.len() != rows.checked_mul(row_bytes).ok_or(Error::Shape)?
        || scratch.len() < tile_bytes
        || pairs.checked_mul(PACKED_Q8X16_PAIR_BYTES) != Some(tile_bytes)
    {
        return Err(Error::Shape);
    }

    let scale_tile_bytes = PACKED_Q8X16_ROWS * core::mem::size_of::<u16>();
    let quant_tile_bytes = PACKED_Q8X16_ROWS * Q8_BLOCK_VALUES;
    for row_tile in 0..rows / PACKED_Q8X16_ROWS {
        let tile_start = row_tile * tile_bytes;
        let tile_end = tile_start + tile_bytes;
        scratch[..tile_bytes].copy_from_slice(&matrix[tile_start..tile_end]);

        for pair in 0..pairs {
            let destination_pair = tile_start + pair * PACKED_Q8X16_PAIR_BYTES;
            for block_in_pair in 0..PACKED_Q8X16_BLOCKS_PER_PAIR {
                let block = pair * PACKED_Q8X16_BLOCKS_PER_PAIR + block_in_pair;
                let scale_destination = destination_pair + block_in_pair * scale_tile_bytes;
                let quant_destination = destination_pair
                    + PACKED_Q8X16_BLOCKS_PER_PAIR * scale_tile_bytes
                    + block_in_pair * quant_tile_bytes;

                for lane in 0..PACKED_Q8X16_ROWS {
                    let source_block = lane * row_bytes + block * Q8_BLOCK_BYTES;
                    let scale =
                        u16::from_le_bytes([scratch[source_block], scratch[source_block + 1]]);
                    if packed_q8x16_scale(scale)? {
                        stats.subnormal_scales = stats.subnormal_scales.saturating_add(1);
                    }
                    let scale_offset = scale_destination + lane * core::mem::size_of::<u16>();
                    matrix[scale_offset..scale_offset + 2].copy_from_slice(&scale.to_le_bytes());

                    for word in 0..PACKED_Q8X16_WORDS_PER_BLOCK {
                        let source = source_block + 2 + word * core::mem::size_of::<u32>();
                        let values = u32::from_le_bytes(
                            scratch[source..source + 4]
                                .try_into()
                                .map_err(|_| Error::Encoding)?,
                        );
                        if values.to_le_bytes().into_iter().any(|value| value == 0x80) {
                            return Err(Error::Encoding);
                        }
                        let destination = quant_destination
                            + word * PACKED_Q8X16_ROWS * core::mem::size_of::<u32>()
                            + lane * core::mem::size_of::<u32>();
                        matrix[destination..destination + 4].copy_from_slice(&values.to_le_bytes());
                    }
                }
            }
        }
    }

    stats.tensor_count = stats.tensor_count.saturating_add(1);
    stats.block_tiles = stats
        .block_tiles
        .saturating_add((rows / PACKED_Q8X16_ROWS * blocks) as u64);
    stats.quantized_values = stats
        .quantized_values
        .saturating_add((rows * columns) as u64);
    Ok(())
}

/// Repack the exact sealed LFM2.5 Q8 matrices in place for the SIMD16 DP4A
/// kernel. A complete sixteen-row native tile is copied to a small scratch
/// buffer before its bytes are overwritten, so resident model memory stays at
/// one image plus at most 78,336 bytes.
pub fn pack_q8x16_model_in_place(model: &mut [u8]) -> Result<PackedQ8x16Stats, Error> {
    if model.len() != trueos_fpga_abi::lfm25::PINNED_NATIVE_IMAGE_BYTES as usize {
        return Err(Error::Artifact);
    }

    let maximum_tile_bytes = trueos_fpga_abi::lfm25::generated::TENSORS
        .iter()
        .filter(|descriptor| {
            trueos_fpga_abi::lfm25::TensorFormat::from_raw(descriptor.format)
                == Some(trueos_fpga_abi::lfm25::TensorFormat::Q8_0)
        })
        .map(|descriptor| {
            q8_row_bytes(descriptor.ggml_ne0 as usize)
                .and_then(|bytes| bytes.checked_mul(PACKED_Q8X16_ROWS).ok_or(Error::Shape))
        })
        .try_fold(0usize, |maximum, bytes| bytes.map(|bytes| maximum.max(bytes)))?;
    let mut scratch = Vec::new();
    scratch
        .try_reserve_exact(maximum_tile_bytes)
        .map_err(|_| Error::Allocation)?;
    scratch.resize(maximum_tile_bytes, 0);

    let mut stats = PackedQ8x16Stats::default();
    for descriptor in trueos_fpga_abi::lfm25::generated::TENSORS {
        if trueos_fpga_abi::lfm25::TensorFormat::from_raw(descriptor.format)
            != Some(trueos_fpga_abi::lfm25::TensorFormat::Q8_0)
        {
            continue;
        }
        let columns = descriptor.ggml_ne0 as usize;
        let rows = descriptor.ggml_ne1 as usize;
        let start = descriptor.native_offset as usize;
        let end = start
            .checked_add(descriptor.native_bytes as usize)
            .ok_or(Error::Artifact)?;
        let expected_bytes = rows
            .checked_mul(q8_row_bytes(columns)?)
            .ok_or(Error::Artifact)?;
        if descriptor.rank != 2
            || descriptor.native_offset % 64 != 0
            || descriptor.native_bytes as usize != expected_bytes
        {
            return Err(Error::Artifact);
        }
        let tensor = model.get_mut(start..end).ok_or(Error::Artifact)?;
        pack_q8x16_tensor_in_place(tensor, rows, columns, &mut scratch, &mut stats)?;
    }
    if stats.tensor_count != PACKED_Q8X16_TENSOR_COUNT
        || stats.block_tiles != PACKED_Q8X16_BLOCK_TILES
        || stats.quantized_values != PACKED_Q8X16_QUANTIZED_VALUES
        || stats.subnormal_scales != PACKED_Q8X16_SUBNORMAL_SCALES
    {
        return Err(Error::Artifact);
    }
    Ok(stats)
}

pub fn packed_q8x16_activation_bytes(columns: usize) -> Result<usize, Error> {
    if columns != trueos_fpga_abi::lfm25::MODEL_HIDDEN_SIZE as usize
        && columns != trueos_fpga_abi::lfm25::MODEL_FEED_FORWARD_SIZE as usize
    {
        return Err(Error::Shape);
    }
    (columns / Q8_BLOCK_VALUES)
        .checked_mul(core::mem::size_of::<u32>() * (1 + PACKED_Q8X16_WORDS_PER_BLOCK))
        .ok_or(Error::Shape)
}

/// Convert native 34-byte Q8 blocks into `uint scale[blocks]` followed by
/// `uint qwords[blocks][8]`, exactly matching the packed C++ kernel ABI.
pub fn pack_q8x16_activation(
    native: &[u8],
    columns: usize,
    output: &mut [u8],
) -> Result<(), Error> {
    let blocks = columns.checked_div(Q8_BLOCK_VALUES).ok_or(Error::Shape)?;
    let native_bytes = q8_row_bytes(columns)?;
    let packed_bytes = packed_q8x16_activation_bytes(columns)?;
    if native.len() != native_bytes || output.len() != packed_bytes {
        return Err(Error::Shape);
    }
    output.fill(0);

    let qword_base = blocks * core::mem::size_of::<u32>();
    for (block, source) in native.chunks_exact(Q8_BLOCK_BYTES).enumerate() {
        let scale = u16::from_le_bytes([source[0], source[1]]);
        packed_q8x16_scale(scale)?;
        let scale_offset = block * core::mem::size_of::<u32>();
        output[scale_offset..scale_offset + 4].copy_from_slice(&(scale as u32).to_le_bytes());
        for word in 0..PACKED_Q8X16_WORDS_PER_BLOCK {
            let source_offset = 2 + word * core::mem::size_of::<u32>();
            let values: [u8; 4] = source[source_offset..source_offset + 4]
                .try_into()
                .map_err(|_| Error::Encoding)?;
            if values.into_iter().any(|value| value == 0x80) {
                return Err(Error::Encoding);
            }
            let destination = qword_base
                + (block * PACKED_Q8X16_WORDS_PER_BLOCK + word) * core::mem::size_of::<u32>();
            output[destination..destination + 4].copy_from_slice(&values);
        }
    }
    Ok(())
}

/// Read one row from a packed fixed-shape matrix. This is used only for the
/// tied token embedding; projections consume the same bytes directly on GPU.
pub fn dequantize_q8x16_row(
    matrix: &[u8],
    rows: usize,
    columns: usize,
    row: usize,
    output: &mut [f32],
) -> Result<(), Error> {
    if !packed_q8x16_admitted_shape(columns, rows)
        || row >= rows
        || output.len() != columns
        || matrix.len()
            != rows
                .checked_mul(q8_row_bytes(columns)?)
                .ok_or(Error::Shape)?
    {
        return Err(Error::Shape);
    }
    let blocks = columns / Q8_BLOCK_VALUES;
    let pairs = blocks / PACKED_Q8X16_BLOCKS_PER_PAIR;
    let row_tile = row / PACKED_Q8X16_ROWS;
    let lane = row % PACKED_Q8X16_ROWS;
    let scale_tile_bytes = PACKED_Q8X16_ROWS * core::mem::size_of::<u16>();
    let quant_tile_bytes = PACKED_Q8X16_ROWS * Q8_BLOCK_VALUES;

    for block in 0..blocks {
        let block_in_pair = block % PACKED_Q8X16_BLOCKS_PER_PAIR;
        let pair =
            (row_tile * pairs + block / PACKED_Q8X16_BLOCKS_PER_PAIR) * PACKED_Q8X16_PAIR_BYTES;
        let scale_offset =
            pair + block_in_pair * scale_tile_bytes + lane * core::mem::size_of::<u16>();
        let scale_bits = u16::from_le_bytes(
            matrix[scale_offset..scale_offset + 2]
                .try_into()
                .map_err(|_| Error::Encoding)?,
        );
        packed_q8x16_scale(scale_bits)?;
        let scale = f16::from_bits(scale_bits).to_f32();
        let quant_base = pair
            + PACKED_Q8X16_BLOCKS_PER_PAIR * scale_tile_bytes
            + block_in_pair * quant_tile_bytes
            + lane * core::mem::size_of::<u32>();
        for word in 0..PACKED_Q8X16_WORDS_PER_BLOCK {
            let source = quant_base + word * PACKED_Q8X16_ROWS * core::mem::size_of::<u32>();
            let values: [u8; 4] = matrix[source..source + 4]
                .try_into()
                .map_err(|_| Error::Encoding)?;
            for (byte, quant) in values.into_iter().enumerate() {
                if quant == 0x80 {
                    return Err(Error::Encoding);
                }
                output[block * Q8_BLOCK_VALUES + word * 4 + byte] = scale * f32::from(quant as i8);
            }
        }
    }
    Ok(())
}

pub fn dequantize_q8_row(row: &[u8], output: &mut [f32]) -> Result<(), Error> {
    if row.len() != q8_row_bytes(output.len())? {
        return Err(Error::Shape);
    }
    for (block, values) in row
        .chunks_exact(Q8_BLOCK_BYTES)
        .zip(output.chunks_exact_mut(Q8_BLOCK_VALUES))
    {
        let scale = f16::from_bits(u16::from_le_bytes([block[0], block[1]])).to_f32();
        if !scale.is_finite() {
            return Err(Error::NonFinite);
        }
        for (output, quant) in values.iter_mut().zip(&block[2..]) {
            *output = scale * f32::from(*quant as i8);
        }
    }
    Ok(())
}

pub fn q8_row_dot(row: &[u8], input: &[f32]) -> Result<f32, Error> {
    if row.len() != q8_row_bytes(input.len())? {
        return Err(Error::Shape);
    }
    let mut sum = 0.0f32;
    for (block, values) in row
        .chunks_exact(Q8_BLOCK_BYTES)
        .zip(input.chunks_exact(Q8_BLOCK_VALUES))
    {
        let scale = f16::from_bits(u16::from_le_bytes([block[0], block[1]])).to_f32();
        if !scale.is_finite() {
            return Err(Error::NonFinite);
        }
        let mut block_sum = 0.0f32;
        for (&quant, &value) in block[2..].iter().zip(values) {
            block_sum += f32::from(quant as i8) * value;
        }
        sum += scale * block_sum;
    }
    if sum.is_finite() {
        Ok(sum)
    } else {
        Err(Error::NonFinite)
    }
}

pub fn q8_row_dot_q8(row: &[u8], input: &[[u8; Q8_BLOCK_BYTES]]) -> Result<f32, Error> {
    if row.len()
        != input
            .len()
            .checked_mul(Q8_BLOCK_BYTES)
            .ok_or(Error::Shape)?
    {
        return Err(Error::Shape);
    }
    // The pinned one-thread llama.cpp build uses its AVX2 Q8_0 dot kernel:
    // eight F32 lanes, four integer products per lane, fused scale/add per
    // block, then its fixed horizontal reduction tree. Reproduce that order
    // explicitly so the no_std scalar backend is numerically invariant.
    let mut lanes = [0.0f32; 8];
    for (weight, activation) in row.chunks_exact(Q8_BLOCK_BYTES).zip(input) {
        let weight_scale = f16::from_bits(u16::from_le_bytes([weight[0], weight[1]])).to_f32();
        let activation_scale =
            f16::from_bits(u16::from_le_bytes([activation[0], activation[1]])).to_f32();
        if !weight_scale.is_finite() || !activation_scale.is_finite() {
            return Err(Error::NonFinite);
        }
        let scale = weight_scale * activation_scale;
        for (lane, accumulator) in lanes.iter_mut().enumerate() {
            let start = 2 + lane * 4;
            // AVX2 uses `_mm256_maddubs_epi16` before widening. Its two
            // adjacent byte products saturate to signed i16 independently;
            // this matters for the rare ±128/±127 Q8 pairs at the limit.
            let pair = |index: usize| {
                let sum = i32::from(weight[index] as i8) * i32::from(activation[index] as i8)
                    + i32::from(weight[index + 1] as i8) * i32::from(activation[index + 1] as i8);
                sum.clamp(i16::MIN as i32, i16::MAX as i32)
            };
            let dot = pair(start) + pair(start + 2);
            *accumulator = libm::fmaf(scale, dot as f32, *accumulator);
        }
    }
    let low_high = [
        lanes[0] + lanes[4],
        lanes[1] + lanes[5],
        lanes[2] + lanes[6],
        lanes[3] + lanes[7],
    ];
    let quarters = [low_high[0] + low_high[2], low_high[1] + low_high[3]];
    let sum = quarters[0] + quarters[1];
    if sum.is_finite() {
        Ok(sum)
    } else {
        Err(Error::NonFinite)
    }
}

pub fn q8_matrix_vector(
    matrix: &[u8],
    rows: usize,
    columns: usize,
    input: &[f32],
) -> Result<Vec<f32>, Error> {
    if input.len() != columns {
        return Err(Error::Shape);
    }
    let row_bytes = q8_row_bytes(columns)?;
    if matrix.len() != rows.checked_mul(row_bytes).ok_or(Error::Shape)? {
        return Err(Error::Shape);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(rows)
        .map_err(|_| Error::Allocation)?;
    for row in matrix.chunks_exact(row_bytes) {
        output.push(q8_row_dot(row, input)?);
    }
    Ok(output)
}

pub fn q8_matrix_vector_quantized(
    matrix: &[u8],
    rows: usize,
    columns: usize,
    input: &[f32],
) -> Result<Vec<f32>, Error> {
    if input.len() != columns {
        return Err(Error::Shape);
    }
    let quantized = quantize_q8(input)?;
    let row_bytes = q8_row_bytes(columns)?;
    if matrix.len() != rows.checked_mul(row_bytes).ok_or(Error::Shape)? {
        return Err(Error::Shape);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(rows)
        .map_err(|_| Error::Allocation)?;
    for row in matrix.chunks_exact(row_bytes) {
        output.push(q8_row_dot_q8(row, &quantized)?);
    }
    Ok(output)
}

pub fn quantize_q8(values: &[f32]) -> Result<Vec<[u8; Q8_BLOCK_BYTES]>, Error> {
    if values.is_empty() || values.len() % Q8_BLOCK_VALUES != 0 {
        return Err(Error::Shape);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(values.len() / Q8_BLOCK_VALUES)
        .map_err(|_| Error::Allocation)?;
    for values in values.chunks_exact(Q8_BLOCK_VALUES) {
        let mut maximum = 0.0f32;
        for value in values {
            if !value.is_finite() {
                return Err(Error::NonFinite);
            }
            maximum = maximum.max(value.abs());
        }
        let scale = maximum / 127.0;
        let inverse = if maximum == 0.0 { 0.0 } else { 127.0 / maximum };
        let mut block = [0u8; Q8_BLOCK_BYTES];
        block[..2].copy_from_slice(&f16::from_f32(scale).to_bits().to_le_bytes());
        for (quant, value) in block[2..].iter_mut().zip(values) {
            *quant = (libm::rintf(*value * inverse) as i8) as u8;
        }
        output.push(block);
    }
    Ok(output)
}

pub fn rms_norm(input: &[f32], weights: &[f32]) -> Result<Vec<f32>, Error> {
    if input.is_empty() || input.len() != weights.len() {
        return Err(Error::Shape);
    }
    // llama.cpp accumulates each already-rounded F32 square into ggml_float
    // (F64), then rounds the mean back to F32 before sqrtf.
    let mut sum_squares = 0.0f64;
    for value in input {
        sum_squares += f64::from(value * value);
    }
    let mean = (sum_squares / input.len() as f64) as f32;
    let inverse_rms = 1.0 / libm::sqrtf(mean + RMS_EPSILON);
    if !inverse_rms.is_finite() {
        return Err(Error::NonFinite);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(input.len())
        .map_err(|_| Error::Allocation)?;
    for (&value, &weight) in input.iter().zip(weights) {
        output.push(value * inverse_rms * weight);
    }
    Ok(output)
}

pub fn rms_norm_head_in_place(head: &mut [f32], weights: &[f32]) -> Result<(), Error> {
    if head.len() != HEAD_DIMENSION || weights.len() != HEAD_DIMENSION {
        return Err(Error::Shape);
    }
    let mut sum_squares = 0.0f64;
    for value in head.iter() {
        sum_squares += f64::from(*value * *value);
    }
    let mean = (sum_squares / HEAD_DIMENSION as f64) as f32;
    let inverse_rms = 1.0 / libm::sqrtf(mean + RMS_EPSILON);
    for (value, weight) in head.iter_mut().zip(weights) {
        *value = *value * inverse_rms * *weight;
    }
    Ok(())
}

/// Apply the pinned NEOX RoPE pairing `(i, i + 32)` to one normalized head.
pub fn rope_neox_in_place(head: &mut [f32], position: u32) -> Result<(), Error> {
    if head.len() != HEAD_DIMENSION {
        return Err(Error::Shape);
    }
    // ggml builds one cache row by repeatedly multiplying theta by a single
    // F32 scale. Computing each frequency independently with powf changes the
    // last bits and can cross an F16 cache boundary at later positions.
    let theta_scale = libm::powf(ROPE_FREQUENCY_BASE, -2.0 / HEAD_DIMENSION as f32);
    let mut angle = position as f32;
    for pair in 0..HALF_HEAD_DIMENSION {
        // The pinned host reference resolves glibc's correctly-rounded F32
        // sin/cos. Evaluate the same F32 argument at F64 precision before the
        // single narrowing step; the no_std libm `sinf`/`cosf` fast paths can
        // otherwise differ by one ULP.
        let cosine = libm::cos(f64::from(angle)) as f32;
        let sine = libm::sin(f64::from(angle)) as f32;
        let low = head[pair];
        let high = head[pair + HALF_HEAD_DIMENSION];
        head[pair] = low * cosine - high * sine;
        head[pair + HALF_HEAD_DIMENSION] = low * sine + high * cosine;
        angle *= theta_scale;
    }
    Ok(())
}

pub fn softmax_in_place(values: &mut [f32]) -> Result<(), Error> {
    if values.is_empty() {
        return Err(Error::Shape);
    }
    let maximum = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0f64;
    for value in values.iter_mut() {
        *value = libm::expf(*value - maximum);
        sum += f64::from(*value);
    }
    if !sum.is_finite() || sum <= 0.0 {
        return Err(Error::NonFinite);
    }
    let inverse = (1.0 / sum) as f32;
    for value in values {
        *value *= inverse;
    }
    Ok(())
}

/// F32 dot product in the fixed AVX2/FMA reduction order used by the pinned
/// llama.cpp CPU reference. TRUEOS expresses it scalarly to keep the result
/// independent of compiler auto-vectorization.
pub fn f32_dot_pinned(lhs: &[f32], rhs: &[f32]) -> Result<f32, Error> {
    if lhs.len() != rhs.len() {
        return Err(Error::Shape);
    }
    let vectorized = lhs.len() & !31usize;
    let mut lanes = [[0.0f32; 8]; 4];
    for base in (0..vectorized).step_by(32) {
        for register in 0..4 {
            for lane in 0..8 {
                let index = base + register * 8 + lane;
                lanes[register][lane] = libm::fmaf(lhs[index], rhs[index], lanes[register][lane]);
            }
        }
    }
    for lane in 0..8 {
        lanes[0][lane] += lanes[2][lane];
        lanes[1][lane] += lanes[3][lane];
        lanes[0][lane] += lanes[1][lane];
    }
    let low_high = [
        lanes[0][0] + lanes[0][4],
        lanes[0][1] + lanes[0][5],
        lanes[0][2] + lanes[0][6],
        lanes[0][3] + lanes[0][7],
    ];
    let pair = [low_high[0] + low_high[1], low_high[2] + low_high[3]];
    let mut sum = pair[0] + pair[1];
    for index in vectorized..lhs.len() {
        sum += lhs[index] * rhs[index];
    }
    if sum.is_finite() {
        Ok(sum)
    } else {
        Err(Error::NonFinite)
    }
}

/// AVX2/FMA dot product with an explicitly zero-padded physical row.
///
/// llama.cpp stores causal KQ/KQV rows in 256-position tiles. The logical
/// cache prefix may be shorter, but those zero lanes still determine the SIMD
/// accumulator and reduction layout.
pub fn f32_dot_pinned_padded(
    lhs: &[f32],
    rhs: &[f32],
    padded_elements: usize,
) -> Result<f32, Error> {
    if lhs.len() != rhs.len()
        || padded_elements < lhs.len()
        || padded_elements == 0
        || padded_elements % 32 != 0
    {
        return Err(Error::Shape);
    }
    let mut lanes = [[0.0f32; 8]; 4];
    for (index, (&lhs, &rhs)) in lhs.iter().zip(rhs).enumerate() {
        let within_block = index % 32;
        let register = within_block / 8;
        let lane = within_block % 8;
        lanes[register][lane] = libm::fmaf(lhs, rhs, lanes[register][lane]);
    }
    for lane in 0..8 {
        lanes[0][lane] += lanes[2][lane];
        lanes[1][lane] += lanes[3][lane];
        lanes[0][lane] += lanes[1][lane];
    }
    let low_high = [
        lanes[0][0] + lanes[0][4],
        lanes[0][1] + lanes[0][5],
        lanes[0][2] + lanes[0][6],
        lanes[0][3] + lanes[0][7],
    ];
    let pair = [low_high[0] + low_high[1], low_high[2] + low_high[3]];
    let sum = pair[0] + pair[1];
    if sum.is_finite() {
        Ok(sum)
    } else {
        Err(Error::NonFinite)
    }
}

/// Map one query head to its grouped-query K/V head.
pub fn gqa_kv_head(query_head: usize, query_heads: usize, kv_heads: usize) -> Result<usize, Error> {
    if query_heads == 0 || kv_heads == 0 || query_heads % kv_heads != 0 || query_head >= query_heads
    {
        return Err(Error::Shape);
    }
    Ok(query_head * kv_heads / query_heads)
}

/// One causal LFM2.5 short-convolution channel.
///
/// Kernel order is oldest, newest, current. The returned state is
/// `(previous_newest, b*x)`.
pub fn shortconv_channel(
    b: f32,
    c: f32,
    x: f32,
    state_oldest: f32,
    state_newest: f32,
    kernel: [f32; 3],
) -> Result<(f32, f32, f32), Error> {
    let bx = b * x;
    let convolution = kernel[0] * state_oldest + kernel[1] * state_newest + kernel[2] * bx;
    let output = c * convolution;
    if bx.is_finite() && output.is_finite() {
        Ok((output, state_newest, bx))
    } else {
        Err(Error::NonFinite)
    }
}

pub fn add(lhs: &[f32], rhs: &[f32]) -> Result<Vec<f32>, Error> {
    if lhs.len() != rhs.len() {
        return Err(Error::Shape);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(lhs.len())
        .map_err(|_| Error::Allocation)?;
    for (&lhs, &rhs) in lhs.iter().zip(rhs) {
        output.push(lhs + rhs);
    }
    Ok(output)
}

pub fn q30_to_f32(values: &[i64]) -> Result<Vec<f32>, Error> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(values.len())
        .map_err(|_| Error::Allocation)?;
    const SCALE: f32 = (1u64 << 30) as f32;
    for value in values {
        output.push(*value as f32 / SCALE);
    }
    Ok(output)
}

pub fn f32_to_q30(value: f32) -> Result<i64, Error> {
    if !value.is_finite() {
        return Err(Error::NonFinite);
    }
    let scaled = value as f64 * (1u64 << 30) as f64;
    if scaled >= i64::MAX as f64 {
        Ok(i64::MAX)
    } else if scaled <= i64::MIN as f64 {
        Ok(i64::MIN)
    } else {
        Ok(libm::rint(scaled) as i64)
    }
}

/// Pinned llama.cpp AVX2/FMA SiLU lane followed by the ordinary F32 multiply.
///
/// LFM2.5's 4,608-wide SwiGLU rows are wholly processed by the eight-lane
/// vector kernel in the reference build. Reproducing its exp approximation is
/// necessary at Q8 halfway boundaries even when the visible F32 error is only
/// one ULP.
pub fn silu_mul_f32_pinned(gate: f32, up: f32) -> Result<f32, Error> {
    if !gate.is_finite() || !up.is_finite() {
        return Err(Error::NonFinite);
    }
    let exponent = pinned_avx2_expf(-gate)?;
    let value = (gate / (1.0 + exponent)) * up;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Error::NonFinite)
    }
}

fn pinned_avx2_expf(value: f32) -> Result<f32, Error> {
    let r = f32::from_bits(0x4b40_0000);
    let z = libm::fmaf(value, f32::from_bits(0x3fb8_aa3b), r);
    let n = z - r;
    // All sealed LFM2.5 SwiGLU values are in this ordinary branch. Fail closed
    // instead of silently substituting a different scalar overflow path.
    if !n.is_finite() || n.abs() > 126.0 {
        return Err(Error::NonFinite);
    }
    let inner = libm::fmaf(-n, f32::from_bits(0x3f31_7200), value);
    let b = libm::fmaf(-n, f32::from_bits(0x35bf_be8e), inner);
    let exponent_bits = z.to_bits().wrapping_shl(23);
    let k = f32::from_bits(exponent_bits.wrapping_add(1.0f32.to_bits()));
    let u = b * b;
    let left = libm::fmaf(f32::from_bits(0x3c07_2010), b, f32::from_bits(0x3d2b_9f17));
    let right = libm::fmaf(f32::from_bits(0x3e2a_af33), b, f32::from_bits(0x3eff_fedb));
    let polynomial = libm::fmaf(left, u, right);
    let j = libm::fmaf(polynomial, u, f32::from_bits(0x3f7f_fff6) * b);
    let output = libm::fmaf(j, k, k);
    if output.is_finite() {
        Ok(output)
    } else {
        Err(Error::NonFinite)
    }
}

/// Evaluate the model's exact f32 `SiLU(gate) * up` from FPGA Q30
/// projections and return Q30 for the downstream quantizer.
///
/// The persisted TRUEGA row streamer rejects values outside the narrow
/// polynomial domain sealed by its original layer-0 fixture. Its gate and up
/// projections are nevertheless complete at that terminal point, so only
/// this activation needs CPU recovery.
pub fn silu_mul_q30(gate_q30: i64, up_q30: i64) -> Result<i64, Error> {
    const SCALE: f32 = (1u64 << 30) as f32;
    let gate = gate_q30 as f32 / SCALE;
    let up = up_q30 as f32 / SCALE;
    f32_to_q30(silu_mul_f32_pinned(gate, up)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn synthetic_f32_sidecar() -> Vec<u8> {
        let mut artifact = vec![0u8; F32_SIDECAR_BYTES];
        artifact[..8].copy_from_slice(&F32_SIDECAR_MAGIC);
        for (offset, value) in [
            (8, F32_SIDECAR_VERSION),
            (12, F32_SIDECAR_HEADER_BYTES as u32),
            (16, F32_SIDECAR_TENSOR_COUNT as u32),
            (20, F32_SIDECAR_ENTRY_BYTES as u32),
            (24, F32_SIDECAR_ELEMENT_COUNT as u32),
            (28, F32_SIDECAR_PAYLOAD_OFFSET as u32),
        ] {
            artifact[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        artifact[32..64].copy_from_slice(&trueos_fpga_abi::lfm25::PINNED_GGUF_SHA256);
        artifact[64..96].copy_from_slice(&trueos_fpga_abi::lfm25::PINNED_NATIVE_IMAGE_SHA256);
        artifact[96..128]
            .copy_from_slice(&trueos_fpga_abi::lfm25::generated::MODEL_SEAL.tensor_table_sha256);
        let mut payload_offset = F32_SIDECAR_PAYLOAD_OFFSET;
        let mut value_index = 0usize;
        for (entry_index, descriptor) in trueos_fpga_abi::lfm25::generated::TENSORS
            .iter()
            .filter(|descriptor| {
                trueos_fpga_abi::lfm25::TensorFormat::from_raw(descriptor.format)
                    == Some(trueos_fpga_abi::lfm25::TensorFormat::Bf16Le)
            })
            .enumerate()
        {
            let elements = descriptor.ggml_ne0 as usize * descriptor.ggml_ne1 as usize;
            let entry = F32_SIDECAR_HEADER_BYTES + entry_index * F32_SIDECAR_ENTRY_BYTES;
            artifact[entry..entry + 2].copy_from_slice(&descriptor.tensor_id.to_le_bytes());
            artifact[entry + 4..entry + 8].copy_from_slice(&(elements as u32).to_le_bytes());
            artifact[entry + 8..entry + 12].copy_from_slice(&(payload_offset as u32).to_le_bytes());
            artifact[entry + 12..entry + 16]
                .copy_from_slice(&((elements * 4) as u32).to_le_bytes());
            for _ in 0..elements {
                let value = value_index as f32 * 0.000_031_25 - 1.0;
                artifact[payload_offset..payload_offset + 4]
                    .copy_from_slice(&value.to_bits().to_le_bytes());
                payload_offset += 4;
                value_index += 1;
            }
        }
        let payload_hash: [u8; 32] = Sha256::digest(&artifact[F32_SIDECAR_PAYLOAD_OFFSET..]).into();
        artifact[128..160].copy_from_slice(&payload_hash);
        artifact
    }

    #[test]
    fn f32_sidecar_has_exact_generated_ids_elements_and_source_bits() {
        let artifact = synthetic_f32_sidecar();
        let sidecar = F32Sidecar::from_artifact(&artifact).unwrap();
        let expected_ids: Vec<u16> = trueos_fpga_abi::lfm25::generated::TENSORS
            .iter()
            .filter(|descriptor| {
                trueos_fpga_abi::lfm25::TensorFormat::from_raw(descriptor.format)
                    == Some(trueos_fpga_abi::lfm25::TensorFormat::Bf16Le)
            })
            .map(|descriptor| descriptor.tensor_id)
            .collect();
        assert_eq!(sidecar.tensor_ids().collect::<Vec<_>>(), expected_ids);
        assert_eq!(sidecar.element_count(), F32_SIDECAR_ELEMENT_COUNT);
        let first = sidecar.tensor(expected_ids[0]).unwrap()[0];
        let source_bits = u32::from_le_bytes(
            artifact[F32_SIDECAR_PAYLOAD_OFFSET..F32_SIDECAR_PAYLOAD_OFFSET + 4]
                .try_into()
                .unwrap(),
        );
        assert_eq!(first.to_bits(), source_bits);
    }

    #[test]
    fn f32_sidecar_rejects_truncation_corruption_and_reordering() {
        let artifact = synthetic_f32_sidecar();
        assert!(matches!(
            F32Sidecar::from_artifact(&artifact[..artifact.len() - 1]),
            Err(Error::Artifact)
        ));

        let mut corrupt = artifact.clone();
        corrupt[F32_SIDECAR_PAYLOAD_OFFSET + 17] ^= 0x80;
        assert!(matches!(F32Sidecar::from_artifact(&corrupt), Err(Error::Artifact)));

        let mut reordered = artifact;
        let first = F32_SIDECAR_HEADER_BYTES;
        let second = first + F32_SIDECAR_ENTRY_BYTES;
        for byte in 0..F32_SIDECAR_ENTRY_BYTES {
            reordered.swap(first + byte, second + byte);
        }
        assert!(matches!(F32Sidecar::from_artifact(&reordered), Err(Error::Artifact)));
    }

    #[test]
    fn f16_cache_commit_rounds_before_consumption() {
        let value = 1.000_7f32;
        let bits = f16_cache_bits(value).unwrap();
        assert_eq!(f16_cache_f32(bits).to_bits(), f16::from_f32(value).to_f32().to_bits());
        assert_ne!(f16_cache_f32(bits).to_bits(), value.to_bits());
    }

    #[test]
    fn silu_q30_covers_values_outside_the_sealed_fpga_polynomial_domain() {
        const ONE_Q30: i64 = 1i64 << 30;
        assert_eq!(silu_mul_q30(0, 3 * ONE_Q30), Ok(0));

        let positive = silu_mul_q30(2 * ONE_Q30, ONE_Q30).unwrap();
        let negative = silu_mul_q30(-2 * ONE_Q30, ONE_Q30).unwrap();
        assert!((positive - 1_891_497_322).abs() <= 256);
        assert!((negative + 255_986_326).abs() <= 256);
    }

    #[test]
    fn packed_q8x16_generated_contract_is_the_fixed_93_tensor_graph() {
        let mut tensors = 0usize;
        let mut block_tiles = 0u64;
        let mut quantized_values = 0u64;
        let mut maximum_tile_bytes = 0usize;
        for descriptor in trueos_fpga_abi::lfm25::generated::TENSORS {
            if trueos_fpga_abi::lfm25::TensorFormat::from_raw(descriptor.format)
                != Some(trueos_fpga_abi::lfm25::TensorFormat::Q8_0)
            {
                continue;
            }
            let rows = descriptor.ggml_ne1 as usize;
            let columns = descriptor.ggml_ne0 as usize;
            assert!(packed_q8x16_admitted_shape(columns, rows));
            assert_eq!(descriptor.rank, 2);
            assert_eq!(descriptor.native_offset % 64, 0);
            assert_eq!(descriptor.native_bytes as usize, rows * q8_row_bytes(columns).unwrap());
            tensors += 1;
            block_tiles += (rows / PACKED_Q8X16_ROWS * (columns / Q8_BLOCK_VALUES)) as u64;
            quantized_values += (rows * columns) as u64;
            maximum_tile_bytes =
                maximum_tile_bytes.max(PACKED_Q8X16_ROWS * q8_row_bytes(columns).unwrap());
        }
        assert_eq!(tensors, PACKED_Q8X16_TENSOR_COUNT);
        assert_eq!(block_tiles, PACKED_Q8X16_BLOCK_TILES);
        assert_eq!(quantized_values, PACKED_Q8X16_QUANTIZED_VALUES);
        assert_eq!(maximum_tile_bytes, 78_336);
    }

    #[test]
    fn packed_q8x16_in_place_rows_and_activation_match_native_bytes() {
        let rows = 512usize;
        let columns = 1_024usize;
        let row_bytes = q8_row_bytes(columns).unwrap();
        let mut native = vec![0u8; rows * row_bytes];
        for row in 0..rows {
            for block in 0..columns / Q8_BLOCK_VALUES {
                let offset = row * row_bytes + block * Q8_BLOCK_BYTES;
                let scale = f16::from_f32(0.000_5 + (row % 17) as f32 * 0.000_1);
                native[offset..offset + 2].copy_from_slice(&scale.to_bits().to_le_bytes());
                for quant in 0..Q8_BLOCK_VALUES {
                    native[offset + 2 + quant] =
                        (((row * 13 + block * 7 + quant) % 255) as i16 - 127) as i8 as u8;
                }
            }
        }

        let original = native.clone();
        let mut scratch = vec![0u8; PACKED_Q8X16_ROWS * row_bytes];
        let mut stats = PackedQ8x16Stats::default();
        pack_q8x16_tensor_in_place(&mut native, rows, columns, &mut scratch, &mut stats).unwrap();
        assert_eq!(
            stats,
            PackedQ8x16Stats {
                tensor_count: 1,
                block_tiles: 1_024,
                quantized_values: 524_288,
                subnormal_scales: 0,
            }
        );

        for row in [0usize, 15, 16, 255, 511] {
            let mut expected = vec![0.0f32; columns];
            let mut observed = vec![0.0f32; columns];
            dequantize_q8_row(&original[row * row_bytes..(row + 1) * row_bytes], &mut expected)
                .unwrap();
            dequantize_q8x16_row(&native, rows, columns, row, &mut observed).unwrap();
            assert_eq!(observed, expected);
        }

        let activation_native = &original[5 * row_bytes..6 * row_bytes];
        let mut activation = vec![0u8; packed_q8x16_activation_bytes(columns).unwrap()];
        pack_q8x16_activation(activation_native, columns, &mut activation).unwrap();
        let blocks = columns / Q8_BLOCK_VALUES;
        for block in 0..blocks {
            assert_eq!(
                &activation[block * 4..block * 4 + 4],
                &u32::from(u16::from_le_bytes([
                    activation_native[block * Q8_BLOCK_BYTES],
                    activation_native[block * Q8_BLOCK_BYTES + 1],
                ]))
                .to_le_bytes()
            );
            for word in 0..PACKED_Q8X16_WORDS_PER_BLOCK {
                let packed = blocks * 4 + (block * PACKED_Q8X16_WORDS_PER_BLOCK + word) * 4;
                let source = block * Q8_BLOCK_BYTES + 2 + word * 4;
                assert_eq!(&activation[packed..packed + 4], &activation_native[source..source + 4]);
            }
        }
    }

    #[test]
    fn q8_round_trip_is_finite_and_bounded() {
        let input: Vec<f32> = (0..64).map(|index| index as f32 / 17.0 - 1.7).collect();
        let blocks = quantize_q8(&input).unwrap();
        let bytes: Vec<u8> = blocks.into_iter().flatten().collect();
        let mut output = vec![0.0; input.len()];
        dequantize_q8_row(&bytes, &mut output).unwrap();
        for (&actual, &expected) in output.iter().zip(&input) {
            assert!((actual - expected).abs() < 0.02);
        }
    }

    #[test]
    fn position_zero_rope_is_identity() {
        let mut head: Vec<f32> = (0..HEAD_DIMENSION).map(|value| value as f32).collect();
        let expected = head.clone();
        rope_neox_in_place(&mut head, 0).unwrap();
        assert_eq!(head, expected);
    }

    #[test]
    fn causal_shortconv_shifts_state() {
        let (output, oldest, newest) =
            shortconv_channel(2.0, 3.0, 4.0, 5.0, 6.0, [0.5, 0.25, 0.125]).unwrap();
        assert_eq!((oldest, newest), (6.0, 8.0));
        assert_eq!(output, 15.0);
    }

    #[test]
    fn causal_shortconv_preserves_oldest_newest_order_across_three_positions() {
        let kernel = [1.0, 10.0, 100.0];
        let mut state = [0.0, 0.0];
        let mut output = Vec::new();
        for x in [1.0, 2.0, 3.0] {
            let (value, oldest, newest) =
                shortconv_channel(1.0, 1.0, x, state[0], state[1], kernel).unwrap();
            state = [oldest, newest];
            output.push(value);
        }
        assert_eq!(output, [100.0, 210.0, 321.0]);
        assert_eq!(state, [2.0, 3.0]);
    }

    #[test]
    fn nonzero_neox_rope_pairs_low_and_high_halves() {
        let mut head = vec![0.0; HEAD_DIMENSION];
        head[0] = 2.0;
        head[HALF_HEAD_DIMENSION] = 3.0;
        rope_neox_in_place(&mut head, 1).unwrap();
        let cosine = libm::cosf(1.0);
        let sine = libm::sinf(1.0);
        assert!((head[0] - (2.0 * cosine - 3.0 * sine)).abs() < 1.0e-6);
        assert!((head[HALF_HEAD_DIMENSION] - (2.0 * sine + 3.0 * cosine)).abs() < 1.0e-6);
    }

    #[test]
    fn multi_position_softmax_is_normalized_and_ordered() {
        let mut scores = [-2.0, 0.0, 1.5, -0.5];
        softmax_in_place(&mut scores).unwrap();
        assert!((scores.iter().sum::<f32>() - 1.0).abs() < 1.0e-6);
        assert!(scores[2] > scores[1] && scores[1] > scores[3] && scores[3] > scores[0]);
    }

    #[test]
    fn pinned_gqa_maps_two_query_heads_to_each_kv_head() {
        let observed: Vec<usize> = (0..16)
            .map(|query| gqa_kv_head(query, 16, 8).unwrap())
            .collect();
        assert_eq!(observed, [0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7]);
        assert_eq!(gqa_kv_head(16, 16, 8), Err(Error::Shape));
    }

    #[test]
    fn attention_dot_uses_zero_padded_vector_reduction() {
        let lhs = [1.0f32, -2.0, 3.0];
        let rhs = [4.0f32, 5.0, -6.0];
        assert_eq!(f32_dot_pinned_padded(&lhs, &rhs, 256).unwrap(), -24.0);
        assert_eq!(f32_dot_pinned_padded(&lhs, &rhs, 31), Err(Error::Shape));
    }

    #[test]
    fn q8_halfway_ties_use_nearest_even() {
        let mut input = vec![0.0; Q8_BLOCK_VALUES];
        input[0] = 1.0;
        input[1] = 0.5 / 127.0;
        input[2] = 1.5 / 127.0;
        input[3] = -0.5 / 127.0;
        input[4] = -1.5 / 127.0;
        let block = quantize_q8(&input).unwrap().remove(0);
        assert_eq!(
            [
                block[2] as i8,
                block[3] as i8,
                block[4] as i8,
                block[5] as i8,
                block[6] as i8
            ],
            [127, 0, 2, 0, -2]
        );
    }

    #[test]
    fn residual_addition_uses_matching_lanes() {
        let residual = [1.0, -2.0, 4.0];
        let branch = [0.25, 0.5, -8.0];
        assert_eq!(add(&residual, &branch).unwrap(), [1.25, -1.5, -4.0]);
    }
}
