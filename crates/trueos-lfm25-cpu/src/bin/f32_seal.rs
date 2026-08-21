use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::Path;

use trueos_lfm25_cpu::{
    F32_SIDECAR_BYTES, F32_SIDECAR_ELEMENT_COUNT, F32_SIDECAR_ENTRY_BYTES,
    F32_SIDECAR_HEADER_BYTES, F32_SIDECAR_MAGIC, F32_SIDECAR_PAYLOAD_OFFSET,
    F32_SIDECAR_TENSOR_COUNT, F32_SIDECAR_VERSION, F32Sidecar,
};
use trueos_lfm25_model::lfm25::{self, TensorFormat};

const GGUF_F32: u32 = 0;
const GGUF_Q8_0: u32 = 8;
const GGUF_U8: u32 = 0;
const GGUF_I8: u32 = 1;
const GGUF_U16: u32 = 2;
const GGUF_I16: u32 = 3;
const GGUF_U32: u32 = 4;
const GGUF_I32: u32 = 5;
const GGUF_METADATA_F32: u32 = 6;
const GGUF_BOOL: u32 = 7;
const GGUF_STRING: u32 = 8;
const GGUF_ARRAY: u32 = 9;
const GGUF_U64: u32 = 10;
const GGUF_I64: u32 = 11;
const GGUF_F64: u32 = 12;

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
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| "invalid u32".to_string())?,
        ))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| "invalid u64".to_string())?,
        ))
    }

    fn string(&mut self) -> Result<String, String> {
        let bytes =
            usize::try_from(self.u64()?).map_err(|_| "GGUF string is too large".to_string())?;
        String::from_utf8(self.take(bytes)?.to_vec())
            .map_err(|_| "GGUF string is not UTF-8".to_string())
    }

    fn skip_value(&mut self, value_type: u32) -> Result<(), String> {
        match value_type {
            GGUF_U8 | GGUF_I8 | GGUF_BOOL => {
                self.u8()?;
            }
            GGUF_U16 | GGUF_I16 => {
                self.take(2)?;
            }
            GGUF_U32 | GGUF_I32 | GGUF_METADATA_F32 => {
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

struct Tensor {
    name: String,
    dimensions: Vec<u64>,
    source_type: u32,
    relative_offset: usize,
}

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let input = args
        .next()
        .ok_or_else(|| "usage: lfm25-f32-seal INPUT.gguf OUTPUT.bin".to_string())?;
    let output = args
        .next()
        .ok_or_else(|| "usage: lfm25-f32-seal INPUT.gguf OUTPUT.bin".to_string())?;
    if args.next().is_some() {
        return Err("usage: lfm25-f32-seal INPUT.gguf OUTPUT.bin".to_string());
    }

    let gguf = fs::read(&input).map_err(|error| format!("read {input}: {error}"))?;
    if gguf.len() != lfm25::PINNED_GGUF_BYTES as usize {
        return Err(format!("GGUF bytes {} != pinned {}", gguf.len(), lfm25::PINNED_GGUF_BYTES));
    }
    let observed_gguf: [u8; 32] = Sha256::digest(&gguf).into();
    if observed_gguf != lfm25::PINNED_GGUF_SHA256 {
        return Err(format!("GGUF SHA-256 mismatch: {}", hex(&observed_gguf)));
    }
    let (data_offset, tensors) = parse_gguf(&gguf)?;
    let artifact = seal(&gguf, data_offset, &tensors)?;
    F32Sidecar::from_artifact(&artifact)
        .map_err(|error| format!("runtime rejected generated sidecar: {error:?}"))?;

    let output = Path::new(&output);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    fs::write(output, &artifact).map_err(|error| format!("write {}: {error}", output.display()))?;
    let artifact_hash: [u8; 32] = Sha256::digest(&artifact).into();
    println!(
        "f32_sidecar={} bytes={} elements={} sha256={}",
        output.display(),
        artifact.len(),
        F32_SIDECAR_ELEMENT_COUNT,
        hex(&artifact_hash)
    );
    Ok(())
}

fn parse_gguf(bytes: &[u8]) -> Result<(usize, Vec<Tensor>), String> {
    let mut reader = Reader { bytes, offset: 0 };
    if reader.take(4)? != b"GGUF" {
        return Err("bad GGUF magic".to_string());
    }
    let version = reader.u32()?;
    if version != 3 {
        return Err(format!("GGUF version {version}, expected 3"));
    }
    let tensor_count =
        usize::try_from(reader.u64()?).map_err(|_| "tensor count is too large".to_string())?;
    let metadata_count = reader.u64()?;
    if tensor_count != lfm25::MODEL_TENSOR_COUNT {
        return Err(format!("GGUF tensor count {tensor_count} != {}", lfm25::MODEL_TENSOR_COUNT));
    }
    let mut alignment = 32usize;
    for _ in 0..metadata_count {
        let key = reader.string()?;
        let value_type = reader.u32()?;
        if key == "general.alignment" {
            if value_type != GGUF_U32 {
                return Err("general.alignment is not u32".to_string());
            }
            alignment = reader.u32()? as usize;
        } else {
            reader.skip_value(value_type)?;
        }
    }
    if alignment == 0 || !alignment.is_power_of_two() {
        return Err(format!("invalid GGUF alignment {alignment}"));
    }

    let mut tensors = Vec::with_capacity(tensor_count);
    for _ in 0..tensor_count {
        let name = reader.string()?;
        let rank = reader.u32()? as usize;
        if !(1..=4).contains(&rank) {
            return Err(format!("{name} has unsupported rank {rank}"));
        }
        let mut dimensions = Vec::with_capacity(rank);
        for _ in 0..rank {
            dimensions.push(reader.u64()?);
        }
        let source_type = reader.u32()?;
        if !matches!(source_type, GGUF_F32 | GGUF_Q8_0) {
            return Err(format!("{name} has unsupported type {source_type}"));
        }
        let relative_offset =
            usize::try_from(reader.u64()?).map_err(|_| format!("{name} offset is too large"))?;
        tensors.push(Tensor {
            name,
            dimensions,
            source_type,
            relative_offset,
        });
    }
    let data_offset = reader
        .offset
        .checked_add(alignment - 1)
        .ok_or_else(|| "GGUF data alignment overflow".to_string())?
        & !(alignment - 1);
    Ok((data_offset, tensors))
}

fn seal(bytes: &[u8], data_offset: usize, tensors: &[Tensor]) -> Result<Vec<u8>, String> {
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
    artifact[32..64].copy_from_slice(&lfm25::PINNED_GGUF_SHA256);
    artifact[64..96].copy_from_slice(&lfm25::PINNED_NATIVE_IMAGE_SHA256);
    artifact[96..128].copy_from_slice(&lfm25::generated::MODEL_SEAL.tensor_table_sha256);

    let mut entry_index = 0usize;
    let mut payload_offset = F32_SIDECAR_PAYLOAD_OFFSET;
    for (tensor_id, ((tensor, name), descriptor)) in tensors
        .iter()
        .zip(lfm25::generated::TENSOR_NAMES)
        .zip(lfm25::generated::TENSORS)
        .enumerate()
    {
        let expected_type = match TensorFormat::from_raw(descriptor.format) {
            Some(TensorFormat::Bf16Le) => GGUF_F32,
            Some(TensorFormat::Q8_0) => GGUF_Q8_0,
            None => return Err(format!("tensor {tensor_id} has invalid generated format")),
        };
        let expected_dimensions = if descriptor.rank == 1 {
            vec![descriptor.ggml_ne0 as u64]
        } else {
            vec![descriptor.ggml_ne0 as u64, descriptor.ggml_ne1 as u64]
        };
        if descriptor.tensor_id as usize != tensor_id
            || tensor.name != name
            || tensor.dimensions != expected_dimensions
            || tensor.source_type != expected_type
        {
            return Err(format!(
                "tensor {tensor_id} mismatch: {} {:?} type {}",
                tensor.name, tensor.dimensions, tensor.source_type
            ));
        }
        if expected_type != GGUF_F32 {
            continue;
        }

        let elements = descriptor.ggml_ne0 as usize * descriptor.ggml_ne1 as usize;
        let source_offset = data_offset
            .checked_add(tensor.relative_offset)
            .ok_or_else(|| format!("{} source offset overflow", tensor.name))?;
        let source_bytes = elements
            .checked_mul(4)
            .ok_or_else(|| format!("{} byte count overflow", tensor.name))?;
        let source_end = source_offset
            .checked_add(source_bytes)
            .ok_or_else(|| format!("{} source range overflow", tensor.name))?;
        let source = bytes
            .get(source_offset..source_end)
            .ok_or_else(|| format!("{} extends past GGUF", tensor.name))?;

        let entry = F32_SIDECAR_HEADER_BYTES + entry_index * F32_SIDECAR_ENTRY_BYTES;
        artifact[entry..entry + 2].copy_from_slice(&descriptor.tensor_id.to_le_bytes());
        artifact[entry + 4..entry + 8].copy_from_slice(&(elements as u32).to_le_bytes());
        artifact[entry + 8..entry + 12].copy_from_slice(&(payload_offset as u32).to_le_bytes());
        artifact[entry + 12..entry + 16].copy_from_slice(&(source_bytes as u32).to_le_bytes());
        artifact[payload_offset..payload_offset + source_bytes].copy_from_slice(source);
        payload_offset += source_bytes;
        entry_index += 1;
    }
    if entry_index != F32_SIDECAR_TENSOR_COUNT || payload_offset != F32_SIDECAR_BYTES {
        return Err(format!("sidecar totals tensors={entry_index} bytes={payload_offset}"));
    }
    let payload_hash: [u8; 32] = Sha256::digest(&artifact[F32_SIDECAR_PAYLOAD_OFFSET..]).into();
    artifact[128..160].copy_from_slice(&payload_hash);
    Ok(artifact)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}
