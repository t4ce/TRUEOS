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

pub const RMS_EPSILON: f32 = 1.0e-5;
pub const ROPE_FREQUENCY_BASE: f32 = 1_000_000.0;
pub const Q8_BLOCK_VALUES: usize = trueos_fpga_abi::lfm25::Q8_0_BLOCK_VALUES;
pub const Q8_BLOCK_BYTES: usize = trueos_fpga_abi::lfm25::Q8_0_BLOCK_BYTES;
pub const HEAD_DIMENSION: usize = trueos_fpga_abi::lfm25::MODEL_HEAD_DIMENSION as usize;
pub const HALF_HEAD_DIMENSION: usize = HEAD_DIMENSION / 2;

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
    let mut sum = 0.0f32;
    for (weight, activation) in row.chunks_exact(Q8_BLOCK_BYTES).zip(input) {
        let weight_scale = f16::from_bits(u16::from_le_bytes([weight[0], weight[1]])).to_f32();
        let activation_scale =
            f16::from_bits(u16::from_le_bytes([activation[0], activation[1]])).to_f32();
        if !weight_scale.is_finite() || !activation_scale.is_finite() {
            return Err(Error::NonFinite);
        }
        let mut dot = 0i32;
        for (&weight, &activation) in weight[2..].iter().zip(&activation[2..]) {
            dot += i32::from(weight as i8) * i32::from(activation as i8);
        }
        sum += weight_scale * activation_scale * dot as f32;
    }
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
    let mut sum_squares = 0.0f32;
    for value in input {
        sum_squares += value * value;
    }
    let inverse_rms = 1.0 / libm::sqrtf(sum_squares / input.len() as f32 + RMS_EPSILON);
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
    let mut sum_squares = 0.0f32;
    for value in head.iter() {
        sum_squares += value * value;
    }
    let inverse_rms = 1.0 / libm::sqrtf(sum_squares / HEAD_DIMENSION as f32 + RMS_EPSILON);
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
    for pair in 0..HALF_HEAD_DIMENSION {
        let exponent = pair as f32 / HALF_HEAD_DIMENSION as f32;
        let angle = position as f32 / libm::powf(ROPE_FREQUENCY_BASE, exponent);
        let cosine = libm::cosf(angle);
        let sine = libm::sinf(angle);
        let low = head[pair];
        let high = head[pair + HALF_HEAD_DIMENSION];
        head[pair] = low * cosine - high * sine;
        head[pair + HALF_HEAD_DIMENSION] = low * sine + high * cosine;
    }
    Ok(())
}

pub fn softmax_in_place(values: &mut [f32]) -> Result<(), Error> {
    if values.is_empty() {
        return Err(Error::Shape);
    }
    let maximum = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0f32;
    for value in values.iter_mut() {
        *value = libm::expf(*value - maximum);
        sum += *value;
    }
    if !sum.is_finite() || sum <= 0.0 {
        return Err(Error::NonFinite);
    }
    let inverse = 1.0 / sum;
    for value in values {
        *value *= inverse;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

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
}
