use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

const GGUF_MAGIC: &[u8; 4] = b"GGUF";
const ARTIFACT_MAGIC: &[u8; 8] = b"LFTOK1\0\0";
const ARTIFACT_VERSION: u32 = 1;

const GGUF_U8: u32 = 0;
const GGUF_I8: u32 = 1;
const GGUF_U16: u32 = 2;
const GGUF_I16: u32 = 3;
const GGUF_U32: u32 = 4;
const GGUF_I32: u32 = 5;
const GGUF_F32: u32 = 6;
const GGUF_BOOL: u32 = 7;
const GGUF_STRING: u32 = 8;
const GGUF_ARRAY: u32 = 9;
const GGUF_U64: u32 = 10;
const GGUF_I64: u32 = 11;
const GGUF_F64: u32 = 12;

#[derive(Default)]
struct TokenizerMetadata {
    model: Option<String>,
    pre: Option<String>,
    tokens: Option<Vec<String>>,
    token_types: Option<Vec<i32>>,
    merges: Option<Vec<String>>,
    bos: Option<u32>,
    eos: Option<u32>,
    pad: Option<u32>,
    add_bos: Option<bool>,
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Reader<'_> {
    fn take(&mut self, bytes: usize) -> Result<&[u8], String> {
        let end = self
            .offset
            .checked_add(bytes)
            .ok_or_else(|| "GGUF offset overflow".to_string())?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| "unexpected end of GGUF".to_string())?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| "u32".to_string())?))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().map_err(|_| "i32".to_string())?))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| "u64".to_string())?))
    }

    fn string(&mut self) -> Result<String, String> {
        let bytes =
            usize::try_from(self.u64()?).map_err(|_| "GGUF string is too large".to_string())?;
        String::from_utf8(self.take(bytes)?.to_vec())
            .map_err(|_| "GGUF string is not UTF-8".to_string())
    }

    fn string_array(&mut self, value_type: u32, key: &str) -> Result<Vec<String>, String> {
        if value_type != GGUF_ARRAY || self.u32()? != GGUF_STRING {
            return Err(format!("{key} is not a string array"));
        }
        let count =
            usize::try_from(self.u64()?).map_err(|_| format!("{key} array is too large"))?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.string()?);
        }
        Ok(values)
    }

    fn i32_array(&mut self, value_type: u32, key: &str) -> Result<Vec<i32>, String> {
        if value_type != GGUF_ARRAY || self.u32()? != GGUF_I32 {
            return Err(format!("{key} is not an i32 array"));
        }
        let count =
            usize::try_from(self.u64()?).map_err(|_| format!("{key} array is too large"))?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.i32()?);
        }
        Ok(values)
    }

    fn typed_string(&mut self, value_type: u32, key: &str) -> Result<String, String> {
        if value_type != GGUF_STRING {
            return Err(format!("{key} is not a string"));
        }
        self.string()
    }

    fn typed_u32(&mut self, value_type: u32, key: &str) -> Result<u32, String> {
        if value_type != GGUF_U32 {
            return Err(format!("{key} is not a u32"));
        }
        self.u32()
    }

    fn typed_bool(&mut self, value_type: u32, key: &str) -> Result<bool, String> {
        if value_type != GGUF_BOOL {
            return Err(format!("{key} is not a bool"));
        }
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(format!("{key} has an invalid bool")),
        }
    }

    fn skip_value(&mut self, value_type: u32) -> Result<(), String> {
        match value_type {
            GGUF_U8 | GGUF_I8 | GGUF_BOOL => {
                self.take(1)?;
            }
            GGUF_U16 | GGUF_I16 => {
                self.take(2)?;
            }
            GGUF_U32 | GGUF_I32 | GGUF_F32 => {
                self.take(4)?;
            }
            GGUF_U64 | GGUF_I64 | GGUF_F64 => {
                self.take(8)?;
            }
            GGUF_STRING => {
                let bytes = usize::try_from(self.u64()?)
                    .map_err(|_| "GGUF string is too large".to_string())?;
                self.take(bytes)?;
            }
            GGUF_ARRAY => {
                let element_type = self.u32()?;
                let count = self.u64()?;
                for _ in 0..count {
                    self.skip_value(element_type)?;
                }
            }
            other => return Err(format!("unsupported GGUF metadata type {other}")),
        }
        Ok(())
    }
}

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let gguf_path = args
        .next()
        .ok_or_else(|| "usage: lfm25-tokenizer-seal INPUT.gguf OUTPUT.bin".to_string())?;
    let output_path = args
        .next()
        .ok_or_else(|| "usage: lfm25-tokenizer-seal INPUT.gguf OUTPUT.bin".to_string())?;
    if args.next().is_some() {
        return Err("usage: lfm25-tokenizer-seal INPUT.gguf OUTPUT.bin".to_string());
    }

    let gguf = fs::read(&gguf_path).map_err(|error| format!("read {gguf_path}: {error}"))?;
    if gguf.len() != trueos_lfm25_model::lfm25::PINNED_GGUF_BYTES as usize {
        return Err(format!(
            "GGUF bytes {} != pinned {}",
            gguf.len(),
            trueos_lfm25_model::lfm25::PINNED_GGUF_BYTES
        ));
    }
    let metadata = parse_metadata(&gguf)?;
    let artifact = seal(metadata)?;
    validate_runtime_artifact(&artifact)?;
    let output = Path::new(&output_path);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    fs::write(output, &artifact).map_err(|error| format!("write {}: {error}", output.display()))?;
    println!("tokenizer={} bytes={} source={}", output.display(), artifact.len(), gguf_path);
    Ok(())
}

fn validate_runtime_artifact(artifact: &[u8]) -> Result<(), String> {
    let tokenizer = trueos_lfm25_cpu::Lfm25Tokenizer::from_artifact(artifact)
        .map_err(|error| format!("runtime artifact rejected: {error:?}"))?;
    let cases: &[(&str, &[u32])] = &[
        ("hello how are you", &[52_572, 1_531, 938, 1_010]),
        ("Hello, world! I'm fine.", &[36_309, 521, 2_031, 510, 859, 6_217, 7_471, 523]),
        (
            "Numbers 1234567 and symbols: []{}.",
            &[
                555, 20_661, 730, 10_293, 26_178, 532, 810, 18_162, 535, 1_607, 7_108, 5_350,
            ],
        ),
        ("line one\nline two", &[1_922, 1_235, 708, 1_922, 1_547]),
        ("Grüße, 世界 👋", &[10_706, 1_142, 7_017, 521, 730, 11_370, 23_805, 749, 743]),
    ];
    for &(source, expected) in cases {
        let observed = tokenizer
            .encode(source)
            .map_err(|error| format!("runtime tokenize failed: {error:?}"))?;
        if observed != expected {
            return Err(format!(
                "pinned tokenization mismatch source={source:?} observed={observed:?} expected={expected:?}"
            ));
        }
    }
    let chat = tokenizer
        .encode_user_turn("hello how are you")
        .map_err(|error| format!("runtime chat tokenize failed: {error:?}"))?;
    let expected = [
        1, 6, 6_423, 708, 52_572, 1_531, 938, 1_010, 7, 708, 6, 64_015, 708,
    ];
    if chat != expected {
        return Err(format!("pinned chat tokenization mismatch: {chat:?}"));
    }
    let followup = tokenizer
        .encode_followup_user_turn("hello how are you")
        .map_err(|error| format!("runtime followup tokenize failed: {error:?}"))?;
    let expected_followup = [
        708, 6, 6_423, 708, 52_572, 1_531, 938, 1_010, 7, 708, 6, 64_015, 708,
    ];
    if followup != expected_followup {
        return Err(format!("pinned followup tokenization mismatch: {followup:?}"));
    }
    let decoded = tokenizer
        .decode(cases[0].1, true)
        .map_err(|error| format!("runtime detokenize failed: {error:?}"))?;
    if decoded != b"hello how are you" {
        return Err(format!(
            "pinned text detokenization mismatch: {:?}",
            String::from_utf8_lossy(&decoded)
        ));
    }
    Ok(())
}

fn parse_metadata(bytes: &[u8]) -> Result<TokenizerMetadata, String> {
    let mut reader = Reader { bytes, offset: 0 };
    if reader.take(4)? != GGUF_MAGIC {
        return Err("bad GGUF magic".to_string());
    }
    let version = reader.u32()?;
    if !(2..=3).contains(&version) {
        return Err(format!("unsupported GGUF version {version}"));
    }
    let _tensor_count = reader.u64()?;
    let metadata_count = reader.u64()?;
    let mut metadata = TokenizerMetadata::default();
    for _ in 0..metadata_count {
        let key = reader.string()?;
        let value_type = reader.u32()?;
        match key.as_str() {
            "tokenizer.ggml.model" => metadata.model = Some(reader.typed_string(value_type, &key)?),
            "tokenizer.ggml.pre" => metadata.pre = Some(reader.typed_string(value_type, &key)?),
            "tokenizer.ggml.tokens" => {
                metadata.tokens = Some(reader.string_array(value_type, &key)?)
            }
            "tokenizer.ggml.token_type" => {
                metadata.token_types = Some(reader.i32_array(value_type, &key)?)
            }
            "tokenizer.ggml.merges" => {
                metadata.merges = Some(reader.string_array(value_type, &key)?)
            }
            "tokenizer.ggml.bos_token_id" => {
                metadata.bos = Some(reader.typed_u32(value_type, &key)?)
            }
            "tokenizer.ggml.eos_token_id" => {
                metadata.eos = Some(reader.typed_u32(value_type, &key)?)
            }
            "tokenizer.ggml.padding_token_id" => {
                metadata.pad = Some(reader.typed_u32(value_type, &key)?)
            }
            "tokenizer.ggml.add_bos_token" => {
                metadata.add_bos = Some(reader.typed_bool(value_type, &key)?)
            }
            _ => reader.skip_value(value_type)?,
        }
    }
    Ok(metadata)
}

fn seal(metadata: TokenizerMetadata) -> Result<Vec<u8>, String> {
    if metadata.model.as_deref() != Some("gpt2")
        || metadata.pre.as_deref() != Some("lfm2")
        || metadata.add_bos != Some(true)
    {
        return Err(format!(
            "unexpected tokenizer model={:?} pre={:?} add_bos={:?}",
            metadata.model, metadata.pre, metadata.add_bos
        ));
    }
    let encoded_tokens = metadata
        .tokens
        .ok_or_else(|| "missing tokenizer tokens".to_string())?;
    let token_types = metadata
        .token_types
        .ok_or_else(|| "missing tokenizer token types".to_string())?;
    let encoded_merges = metadata
        .merges
        .ok_or_else(|| "missing tokenizer merges".to_string())?;
    if encoded_tokens.len() != trueos_lfm25_model::lfm25::MODEL_VOCABULARY_SIZE as usize
        || token_types.len() != encoded_tokens.len()
    {
        return Err(format!(
            "token table shape tokens={} types={}",
            encoded_tokens.len(),
            token_types.len()
        ));
    }

    let mut encoded_to_id = BTreeMap::new();
    for (token, piece) in encoded_tokens.iter().enumerate() {
        if encoded_to_id
            .insert(
                piece.as_str(),
                u32::try_from(token).map_err(|_| "token id overflow".to_string())?,
            )
            .is_some()
        {
            return Err(format!("duplicate tokenizer piece {piece:?}"));
        }
    }
    let im_start = *encoded_to_id
        .get("<|im_start|>")
        .ok_or_else(|| "missing <|im_start|>".to_string())?;
    let im_end = *encoded_to_id
        .get("<|im_end|>")
        .ok_or_else(|| "missing <|im_end|>".to_string())?;
    let decoder = gpt2_byte_decoder()?;

    let mut raw_tokens = Vec::with_capacity(encoded_tokens.len());
    for token in &encoded_tokens {
        let mut raw = Vec::with_capacity(token.len());
        for ch in token.chars() {
            raw.push(
                *decoder
                    .get(&ch)
                    .ok_or_else(|| format!("token contains unmapped GPT-2 character {ch:?}"))?,
            );
        }
        raw_tokens.push(raw);
    }

    let mut merge_records = Vec::with_capacity(encoded_merges.len());
    for (rank, merge) in encoded_merges.iter().enumerate() {
        let (left, right) = merge
            .split_once(' ')
            .ok_or_else(|| format!("merge {rank} has no separator"))?;
        let left_id = *encoded_to_id
            .get(left)
            .ok_or_else(|| format!("merge {rank} left token missing"))?;
        let right_id = *encoded_to_id
            .get(right)
            .ok_or_else(|| format!("merge {rank} right token missing"))?;
        let mut joined = String::with_capacity(left.len() + right.len());
        joined.push_str(left);
        joined.push_str(right);
        let merged_id = *encoded_to_id
            .get(joined.as_str())
            .ok_or_else(|| format!("merge {rank} output token missing"))?;
        merge_records.push((left_id, right_id, merged_id));
    }

    let bos = metadata.bos.ok_or_else(|| "missing BOS id".to_string())?;
    let eos = metadata.eos.ok_or_else(|| "missing EOS id".to_string())?;
    let pad = metadata.pad.ok_or_else(|| "missing PAD id".to_string())?;
    let mut artifact = Vec::new();
    artifact.extend_from_slice(ARTIFACT_MAGIC);
    artifact.extend_from_slice(&ARTIFACT_VERSION.to_le_bytes());
    artifact.extend_from_slice(&(raw_tokens.len() as u32).to_le_bytes());
    artifact.extend_from_slice(&(merge_records.len() as u32).to_le_bytes());
    artifact.extend_from_slice(&bos.to_le_bytes());
    artifact.extend_from_slice(&eos.to_le_bytes());
    artifact.extend_from_slice(&pad.to_le_bytes());
    artifact.extend_from_slice(&im_start.to_le_bytes());
    artifact.extend_from_slice(&im_end.to_le_bytes());
    artifact.extend_from_slice(&trueos_lfm25_model::lfm25::PINNED_GGUF_SHA256);
    for (piece, token_type) in raw_tokens.iter().zip(token_types) {
        let token_type =
            u8::try_from(token_type).map_err(|_| "token type does not fit u8".to_string())?;
        artifact.push(token_type);
        artifact.extend_from_slice(&(piece.len() as u32).to_le_bytes());
        artifact.extend_from_slice(piece);
    }
    for (left, right, merged) in merge_records {
        artifact.extend_from_slice(&left.to_le_bytes());
        artifact.extend_from_slice(&right.to_le_bytes());
        artifact.extend_from_slice(&merged.to_le_bytes());
    }
    Ok(artifact)
}

fn gpt2_byte_decoder() -> Result<BTreeMap<char, u8>, String> {
    let mut bytes: Vec<u16> = (b'!'..=b'~')
        .chain(0xa1..=0xac)
        .chain(0xae..=0xff)
        .map(u16::from)
        .collect();
    let mut codepoints = bytes.clone();
    let mut extension = 0u16;
    for byte in 0u16..=255 {
        if bytes.contains(&byte) {
            continue;
        }
        bytes.push(byte);
        codepoints.push(256 + extension);
        extension += 1;
    }
    let mut decoder = BTreeMap::new();
    for (byte, codepoint) in bytes.into_iter().zip(codepoints) {
        let ch = char::from_u32(u32::from(codepoint))
            .ok_or_else(|| format!("invalid GPT-2 codepoint {codepoint}"))?;
        decoder.insert(ch, byte as u8);
    }
    Ok(decoder)
}
