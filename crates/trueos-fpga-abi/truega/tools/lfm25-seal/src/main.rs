use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::env;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use trueos_fpga_abi::lfm25::{
    LayerKind, ModelSeal, NativeTensorDescriptor, TensorFormat, TensorRole, LAYER_SCHEDULE,
    LAYER_SCHEDULE_BYTES, MODEL_ATTENTION_HEADS, MODEL_ATTENTION_MASK, MODEL_CONTRACT_MAGIC,
    MODEL_FEED_FORWARD_SIZE, MODEL_FLAG_TIED_OUTPUT, MODEL_GENERATION, MODEL_HEAD_DIMENSION,
    MODEL_HIDDEN_SIZE, MODEL_INITIAL_CONTEXT, MODEL_KV_HEADS, MODEL_LAYER_COUNT,
    MODEL_LAYOUT_VERSION, MODEL_SEAL_BYTES, MODEL_SHORTCONV_CACHE, MODEL_SOURCE_CONTEXT,
    MODEL_TENSOR_ALIGNMENT, MODEL_TENSOR_COUNT, MODEL_TENSOR_DESCRIPTOR_BYTES,
    MODEL_VOCABULARY_SIZE, PINNED_GGUF_BYTES, PINNED_GGUF_SHA256, PINNED_NATIVE_IMAGE_BYTES,
    PINNED_NATIVE_IMAGE_SHA256, Q8_0_BLOCK_BYTES, Q8_0_BLOCK_VALUES, TENSOR_FLAG_TIED_OUTPUT,
};

const GGML_TYPE_F32: u32 = 0;
const GGML_TYPE_Q8_0: u32 = 8;
const GGUF_VERSION: u32 = 3;

#[derive(Clone, Debug, Eq, PartialEq)]
struct TensorSpec {
    name: String,
    layer: u8,
    role: TensorRole,
    source_type: u32,
    rank: u8,
    ne0: u32,
    ne1: u32,
    flags: u16,
}

#[derive(Debug)]
struct GgufTensor {
    name: String,
    dimensions: Vec<u64>,
    source_type: u32,
    relative_offset: u64,
    source_bytes: u64,
}

#[derive(Default, Debug)]
struct GgufMetadata {
    architecture: Option<String>,
    alignment: Option<u32>,
    block_count: Option<u32>,
    context_length: Option<u32>,
    embedding_length: Option<u32>,
    feed_forward_length: Option<u32>,
    head_count: Option<u32>,
    head_count_kv: Option<Vec<i32>>,
    vocabulary_size: Option<u32>,
    shortconv_cache: Option<u32>,
}

#[derive(Debug)]
struct GgufModel {
    metadata: GgufMetadata,
    tensors: Vec<GgufTensor>,
    data_offset: u64,
    file_bytes: u64,
}

struct Outputs {
    contract: PathBuf,
    rust: PathBuf,
    rtl: PathBuf,
}

enum Command {
    Generate(Outputs),
    Pack {
        gguf: PathBuf,
        image: PathBuf,
        outputs: Outputs,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("lfm25-seal: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let command = parse_args()?;
    let specs = exact_catalogue();
    validate_catalogue(&specs)?;
    let descriptors = native_descriptors(&specs)?;
    let table = serialize_table(&descriptors);
    let table_sha: [u8; 32] = Sha256::digest(&table).into();
    let seal = exact_seal(table_sha);
    let contract = serialize_contract(&seal, &table);
    let contract_sha: [u8; 32] = Sha256::digest(&contract).into();
    let rust = emit_rust(&seal, contract_sha, &specs, &descriptors);
    let rtl = emit_verilog(&contract, contract_sha);

    match command {
        Command::Generate(outputs) => {
            publish_small_outputs(&outputs, &contract, rust.as_bytes(), rtl.as_bytes())?;
            println!(
                "generated model_contract={} rust={} rtl={} tensors={} contract_sha256={}",
                outputs.contract.display(),
                outputs.rust.display(),
                outputs.rtl.display(),
                descriptors.len(),
                hex(&contract_sha)
            );
        }
        Command::Pack {
            gguf,
            image,
            outputs,
        } => {
            let source_sha = sha256_file(&gguf)?;
            if source_sha != PINNED_GGUF_SHA256 {
                return Err(format!(
                    "source GGUF SHA-256 {} does not match pinned {}",
                    hex(&source_sha),
                    hex(&PINNED_GGUF_SHA256)
                ));
            }
            let model = parse_gguf(&gguf)?;
            validate_gguf(&model, &specs)?;
            pack_native_image(&gguf, &model, &descriptors, &image)?;
            publish_small_outputs(&outputs, &contract, rust.as_bytes(), rtl.as_bytes())?;
            println!(
                "sealed image={} bytes={} sha256={} contract={} contract_sha256={}",
                image.display(),
                PINNED_NATIVE_IMAGE_BYTES,
                hex(&PINNED_NATIVE_IMAGE_SHA256),
                outputs.contract.display(),
                hex(&contract_sha)
            );
        }
    }
    Ok(())
}

fn usage() -> String {
    "usage:\n  lfm25-seal generate --contract-out FILE --rust-out FILE --rtl-out FILE\n  lfm25-seal pack --gguf FILE --image-out FILE --contract-out FILE --rust-out FILE --rtl-out FILE".into()
}

fn parse_args() -> Result<Command, String> {
    let mut args = env::args().skip(1);
    let mode = args.next().ok_or_else(usage)?;
    if matches!(mode.as_str(), "-h" | "--help") {
        return Err(usage());
    }
    let mut gguf = None;
    let mut image = None;
    let mut contract = None;
    let mut rust = None;
    let mut rtl = None;
    while let Some(arg) = args.next() {
        let value = PathBuf::from(args.next().ok_or_else(|| format!("{arg} needs a value"))?);
        match arg.as_str() {
            "--gguf" => gguf = Some(value),
            "--image-out" => image = Some(value),
            "--contract-out" => contract = Some(value),
            "--rust-out" => rust = Some(value),
            "--rtl-out" => rtl = Some(value),
            _ => return Err(format!("unknown argument {arg:?}\n{}", usage())),
        }
    }
    let outputs = Outputs {
        contract: contract.ok_or_else(|| "--contract-out is required".to_string())?,
        rust: rust.ok_or_else(|| "--rust-out is required".to_string())?,
        rtl: rtl.ok_or_else(|| "--rtl-out is required".to_string())?,
    };
    match mode.as_str() {
        "generate" => {
            if gguf.is_some() || image.is_some() {
                return Err("generate does not accept --gguf or --image-out".into());
            }
            Ok(Command::Generate(outputs))
        }
        "pack" => Ok(Command::Pack {
            gguf: gguf.ok_or_else(|| "pack requires --gguf".to_string())?,
            image: image.ok_or_else(|| "pack requires --image-out".to_string())?,
            outputs,
        }),
        _ => Err(usage()),
    }
}

fn exact_catalogue() -> Vec<TensorSpec> {
    let mut tensors = Vec::with_capacity(MODEL_TENSOR_COUNT);
    tensors.push(spec(
        "token_embd.weight",
        0xff,
        TensorRole::TokenEmbedding,
        GGML_TYPE_Q8_0,
        2,
        1024,
        65_536,
        TENSOR_FLAG_TIED_OUTPUT,
    ));
    tensors.push(spec(
        "token_embd_norm.weight",
        0xff,
        TensorRole::TokenEmbeddingNorm,
        GGML_TYPE_F32,
        1,
        1024,
        1,
        0,
    ));
    for (layer, kind) in LAYER_SCHEDULE.iter().copied().enumerate() {
        let layer_start = tensors.len();
        let prefix = format!("blk.{layer}");
        tensors.push(spec(
            &format!("{prefix}.ffn_norm.weight"),
            layer as u8,
            TensorRole::FfnNorm,
            GGML_TYPE_F32,
            1,
            1024,
            1,
            0,
        ));
        tensors.push(spec(
            &format!("{prefix}.ffn_gate.weight"),
            layer as u8,
            TensorRole::FfnGate,
            GGML_TYPE_Q8_0,
            2,
            1024,
            4608,
            0,
        ));
        tensors.push(spec(
            &format!("{prefix}.ffn_down.weight"),
            layer as u8,
            TensorRole::FfnDown,
            GGML_TYPE_Q8_0,
            2,
            4608,
            1024,
            0,
        ));
        tensors.push(spec(
            &format!("{prefix}.ffn_up.weight"),
            layer as u8,
            TensorRole::FfnUp,
            GGML_TYPE_Q8_0,
            2,
            1024,
            4608,
            0,
        ));
        tensors.push(spec(
            &format!("{prefix}.attn_norm.weight"),
            layer as u8,
            TensorRole::OperatorNorm,
            GGML_TYPE_F32,
            1,
            1024,
            1,
            0,
        ));
        match kind {
            LayerKind::ShortConv => {
                tensors.push(spec(
                    &format!("{prefix}.shortconv.conv.weight"),
                    layer as u8,
                    TensorRole::ShortConvKernel,
                    GGML_TYPE_F32,
                    2,
                    3,
                    1024,
                    0,
                ));
                tensors.push(spec(
                    &format!("{prefix}.shortconv.in_proj.weight"),
                    layer as u8,
                    TensorRole::ShortConvInput,
                    GGML_TYPE_Q8_0,
                    2,
                    1024,
                    3072,
                    0,
                ));
                tensors.push(spec(
                    &format!("{prefix}.shortconv.out_proj.weight"),
                    layer as u8,
                    TensorRole::ShortConvOutput,
                    GGML_TYPE_Q8_0,
                    2,
                    1024,
                    1024,
                    0,
                ));
            }
            LayerKind::Attention => {
                tensors.push(spec(
                    &format!("{prefix}.attn_q_norm.weight"),
                    layer as u8,
                    TensorRole::QueryNorm,
                    GGML_TYPE_F32,
                    1,
                    64,
                    1,
                    0,
                ));
                tensors.push(spec(
                    &format!("{prefix}.attn_k_norm.weight"),
                    layer as u8,
                    TensorRole::KeyNorm,
                    GGML_TYPE_F32,
                    1,
                    64,
                    1,
                    0,
                ));
                tensors.push(spec(
                    &format!("{prefix}.attn_q.weight"),
                    layer as u8,
                    TensorRole::Query,
                    GGML_TYPE_Q8_0,
                    2,
                    1024,
                    1024,
                    0,
                ));
                tensors.push(spec(
                    &format!("{prefix}.attn_k.weight"),
                    layer as u8,
                    TensorRole::Key,
                    GGML_TYPE_Q8_0,
                    2,
                    1024,
                    512,
                    0,
                ));
                tensors.push(spec(
                    &format!("{prefix}.attn_v.weight"),
                    layer as u8,
                    TensorRole::Value,
                    GGML_TYPE_Q8_0,
                    2,
                    1024,
                    512,
                    0,
                ));
                tensors.push(spec(
                    &format!("{prefix}.attn_output.weight"),
                    layer as u8,
                    TensorRole::AttentionOutput,
                    GGML_TYPE_Q8_0,
                    2,
                    1024,
                    1024,
                    0,
                ));
            }
        }
        // The pinned GGUF groups layers numerically and sorts names within each layer.
        // Preserve that exact source-table order so native image v1 is reproducible.
        tensors[layer_start..].sort_by(|left, right| left.name.cmp(&right.name));
    }
    tensors
}

#[allow(clippy::too_many_arguments)]
fn spec(
    name: &str,
    layer: u8,
    role: TensorRole,
    source_type: u32,
    rank: u8,
    ne0: u32,
    ne1: u32,
    flags: u16,
) -> TensorSpec {
    TensorSpec {
        name: name.into(),
        layer,
        role,
        source_type,
        rank,
        ne0,
        ne1,
        flags,
    }
}

fn validate_catalogue(specs: &[TensorSpec]) -> Result<(), String> {
    if specs.len() != MODEL_TENSOR_COUNT {
        return Err(format!(
            "catalogue has {} tensors, expected {MODEL_TENSOR_COUNT}",
            specs.len()
        ));
    }
    let mut names = HashSet::new();
    let mut roles = HashSet::new();
    let mut q8 = 0;
    let mut f32_count = 0;
    for tensor in specs {
        if !names.insert(&tensor.name) {
            return Err(format!("duplicate tensor name {}", tensor.name));
        }
        if !roles.insert((tensor.layer, tensor.role as u8)) {
            return Err(format!(
                "duplicate tensor role layer={} role={:?}",
                tensor.layer, tensor.role
            ));
        }
        match tensor.source_type {
            GGML_TYPE_Q8_0 => {
                q8 += 1;
                if !(tensor.ne0 as usize).is_multiple_of(Q8_0_BLOCK_VALUES) {
                    return Err(format!("{} ne0 is not Q8_0 block aligned", tensor.name));
                }
            }
            GGML_TYPE_F32 => f32_count += 1,
            other => return Err(format!("{} has unsupported GGML type {other}", tensor.name)),
        }
    }
    if (q8, f32_count) != (93, 55) {
        return Err(format!("catalogue type totals are q8_0={q8} f32={f32_count}, expected 93/55"));
    }
    Ok(())
}

fn native_descriptors(specs: &[TensorSpec]) -> Result<Vec<NativeTensorDescriptor>, String> {
    let mut offset = 0u64;
    let mut descriptors = Vec::with_capacity(specs.len());
    for (index, tensor) in specs.iter().enumerate() {
        offset = align_up(offset, MODEL_TENSOR_ALIGNMENT as u64)?;
        let elements = u64::from(tensor.ne0) * u64::from(tensor.ne1);
        let (format, bytes) = match tensor.source_type {
            GGML_TYPE_F32 => (TensorFormat::Bf16Le, elements * 2),
            GGML_TYPE_Q8_0 => {
                (TensorFormat::Q8_0, elements / Q8_0_BLOCK_VALUES as u64 * Q8_0_BLOCK_BYTES as u64)
            }
            _ => unreachable!(),
        };
        let descriptor = NativeTensorDescriptor {
            tensor_id: index as u16,
            layer: tensor.layer,
            role: tensor.role as u8,
            format: format as u8,
            rank: tensor.rank,
            flags: tensor.flags,
            ggml_ne0: tensor.ne0,
            ggml_ne1: tensor.ne1,
            native_offset: u32::try_from(offset)
                .map_err(|_| "native image offset exceeds u32".to_string())?,
            native_bytes: u32::try_from(bytes)
                .map_err(|_| format!("{} exceeds u32", tensor.name))?,
        };
        offset = offset
            .checked_add(bytes)
            .ok_or_else(|| "native image size overflow".to_string())?;
        descriptors.push(descriptor);
    }
    offset = align_up(offset, MODEL_TENSOR_ALIGNMENT as u64)?;
    if offset != u64::from(PINNED_NATIVE_IMAGE_BYTES) {
        return Err(format!(
            "native layout is {offset:#x} bytes, expected {PINNED_NATIVE_IMAGE_BYTES:#x}"
        ));
    }
    Ok(descriptors)
}

fn exact_seal(table_sha256: [u8; 32]) -> ModelSeal {
    ModelSeal {
        magic: MODEL_CONTRACT_MAGIC,
        layout_version: MODEL_LAYOUT_VERSION,
        seal_bytes: MODEL_SEAL_BYTES as u16,
        descriptor_bytes: MODEL_TENSOR_DESCRIPTOR_BYTES as u16,
        tensor_count: MODEL_TENSOR_COUNT as u16,
        flags: MODEL_FLAG_TIED_OUTPUT,
        model_generation: MODEL_GENERATION,
        tensor_alignment: MODEL_TENSOR_ALIGNMENT as u32,
        source_context: MODEL_SOURCE_CONTEXT,
        initial_context: MODEL_INITIAL_CONTEXT,
        hidden_size: MODEL_HIDDEN_SIZE,
        feed_forward_size: MODEL_FEED_FORWARD_SIZE,
        vocabulary_size: MODEL_VOCABULARY_SIZE,
        layer_count: MODEL_LAYER_COUNT as u16,
        attention_heads: MODEL_ATTENTION_HEADS,
        kv_heads: MODEL_KV_HEADS,
        head_dimension: MODEL_HEAD_DIMENSION,
        shortconv_cache: MODEL_SHORTCONV_CACHE,
        attention_mask: MODEL_ATTENTION_MASK,
        source_gguf_bytes: PINNED_GGUF_BYTES,
        native_image_bytes: PINNED_NATIVE_IMAGE_BYTES,
        source_gguf_sha256: PINNED_GGUF_SHA256,
        native_image_sha256: PINNED_NATIVE_IMAGE_SHA256,
        tensor_table_sha256: table_sha256,
        layer_schedule: LAYER_SCHEDULE_BYTES,
        reserved: [0; 12],
    }
}

fn serialize_table(descriptors: &[NativeTensorDescriptor]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(descriptors.len() * MODEL_TENSOR_DESCRIPTOR_BYTES);
    for descriptor in descriptors {
        bytes.extend_from_slice(&descriptor.tensor_id.to_le_bytes());
        bytes.push(descriptor.layer);
        bytes.push(descriptor.role);
        bytes.push(descriptor.format);
        bytes.push(descriptor.rank);
        bytes.extend_from_slice(&descriptor.flags.to_le_bytes());
        bytes.extend_from_slice(&descriptor.ggml_ne0.to_le_bytes());
        bytes.extend_from_slice(&descriptor.ggml_ne1.to_le_bytes());
        bytes.extend_from_slice(&descriptor.native_offset.to_le_bytes());
        bytes.extend_from_slice(&descriptor.native_bytes.to_le_bytes());
    }
    bytes
}

fn serialize_seal(seal: &ModelSeal) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(MODEL_SEAL_BYTES);
    bytes.extend_from_slice(&seal.magic);
    bytes.extend_from_slice(&seal.layout_version.to_le_bytes());
    bytes.extend_from_slice(&seal.seal_bytes.to_le_bytes());
    bytes.extend_from_slice(&seal.descriptor_bytes.to_le_bytes());
    bytes.extend_from_slice(&seal.tensor_count.to_le_bytes());
    bytes.extend_from_slice(&seal.flags.to_le_bytes());
    bytes.extend_from_slice(&seal.model_generation.to_le_bytes());
    bytes.extend_from_slice(&seal.tensor_alignment.to_le_bytes());
    bytes.extend_from_slice(&seal.source_context.to_le_bytes());
    bytes.extend_from_slice(&seal.initial_context.to_le_bytes());
    bytes.extend_from_slice(&seal.hidden_size.to_le_bytes());
    bytes.extend_from_slice(&seal.feed_forward_size.to_le_bytes());
    bytes.extend_from_slice(&seal.vocabulary_size.to_le_bytes());
    bytes.extend_from_slice(&seal.layer_count.to_le_bytes());
    bytes.extend_from_slice(&seal.attention_heads.to_le_bytes());
    bytes.extend_from_slice(&seal.kv_heads.to_le_bytes());
    bytes.extend_from_slice(&seal.head_dimension.to_le_bytes());
    bytes.extend_from_slice(&seal.shortconv_cache.to_le_bytes());
    bytes.extend_from_slice(&seal.attention_mask.to_le_bytes());
    bytes.extend_from_slice(&seal.source_gguf_bytes.to_le_bytes());
    bytes.extend_from_slice(&seal.native_image_bytes.to_le_bytes());
    bytes.extend_from_slice(&seal.source_gguf_sha256);
    bytes.extend_from_slice(&seal.native_image_sha256);
    bytes.extend_from_slice(&seal.tensor_table_sha256);
    bytes.extend_from_slice(&seal.layer_schedule);
    bytes.extend_from_slice(&seal.reserved);
    assert_eq!(bytes.len(), MODEL_SEAL_BYTES);
    bytes
}

fn serialize_contract(seal: &ModelSeal, table: &[u8]) -> Vec<u8> {
    let mut bytes = serialize_seal(seal);
    bytes.extend_from_slice(table);
    assert_eq!(bytes.len(), MODEL_SEAL_BYTES + MODEL_TENSOR_COUNT * MODEL_TENSOR_DESCRIPTOR_BYTES);
    bytes
}

fn parse_gguf(path: &Path) -> Result<GgufModel, String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let file_bytes = file
        .metadata()
        .map_err(|e| format!("stat {}: {e}", path.display()))?
        .len();
    let mut reader = BufReader::new(file);
    let mut magic = [0; 4];
    reader
        .read_exact(&mut magic)
        .map_err(io_error("read GGUF magic"))?;
    if &magic != b"GGUF" {
        return Err(format!("bad GGUF magic {magic:?}"));
    }
    let version = read_u32(&mut reader)?;
    if version != GGUF_VERSION {
        return Err(format!("GGUF version {version}, expected {GGUF_VERSION}"));
    }
    let tensor_count = read_u64(&mut reader)?;
    let metadata_count = read_u64(&mut reader)?;
    if tensor_count != MODEL_TENSOR_COUNT as u64 {
        return Err(format!("GGUF has {tensor_count} tensors, expected {MODEL_TENSOR_COUNT}"));
    }
    let mut metadata = GgufMetadata::default();
    for _ in 0..metadata_count {
        let key = read_string(&mut reader)?;
        let value_type = read_u32(&mut reader)?;
        match key.as_str() {
            "general.architecture" => {
                metadata.architecture = Some(read_typed_string(&mut reader, value_type, &key)?)
            }
            "general.alignment" => {
                metadata.alignment = Some(read_typed_u32(&mut reader, value_type, &key)?)
            }
            "lfm2.block_count" => {
                metadata.block_count = Some(read_typed_u32(&mut reader, value_type, &key)?)
            }
            "lfm2.context_length" => {
                metadata.context_length = Some(read_typed_u32(&mut reader, value_type, &key)?)
            }
            "lfm2.embedding_length" => {
                metadata.embedding_length = Some(read_typed_u32(&mut reader, value_type, &key)?)
            }
            "lfm2.feed_forward_length" => {
                metadata.feed_forward_length = Some(read_typed_u32(&mut reader, value_type, &key)?)
            }
            "lfm2.attention.head_count" => {
                metadata.head_count = Some(read_typed_u32(&mut reader, value_type, &key)?)
            }
            "lfm2.attention.head_count_kv" => {
                metadata.head_count_kv = Some(read_typed_i32_array(&mut reader, value_type, &key)?)
            }
            "lfm2.vocab_size" => {
                metadata.vocabulary_size = Some(read_typed_u32(&mut reader, value_type, &key)?)
            }
            "lfm2.shortconv.l_cache" => {
                metadata.shortconv_cache = Some(read_typed_u32(&mut reader, value_type, &key)?)
            }
            _ => skip_value(&mut reader, value_type)?,
        }
    }
    let mut tensors = Vec::with_capacity(tensor_count as usize);
    for _ in 0..tensor_count {
        let name = read_string(&mut reader)?;
        let rank = read_u32(&mut reader)?;
        if !(1..=4).contains(&rank) {
            return Err(format!("tensor {name} has unsupported rank {rank}"));
        }
        let mut dimensions = Vec::with_capacity(rank as usize);
        for _ in 0..rank {
            dimensions.push(read_u64(&mut reader)?);
        }
        let source_type = read_u32(&mut reader)?;
        let relative_offset = read_u64(&mut reader)?;
        let elements = dimensions
            .iter()
            .try_fold(1u64, |n, d| n.checked_mul(*d).ok_or(()))
            .map_err(|_| format!("tensor {name} element count overflows"))?;
        let source_bytes = match source_type {
            GGML_TYPE_F32 => elements.checked_mul(4),
            GGML_TYPE_Q8_0 if elements % Q8_0_BLOCK_VALUES as u64 == 0 => elements
                .checked_div(Q8_0_BLOCK_VALUES as u64)
                .and_then(|n| n.checked_mul(Q8_0_BLOCK_BYTES as u64)),
            GGML_TYPE_Q8_0 => return Err(format!("tensor {name} has partial Q8_0 block")),
            other => return Err(format!("tensor {name} has unsupported GGML type {other}")),
        }
        .ok_or_else(|| format!("tensor {name} byte size overflows"))?;
        tensors.push(GgufTensor {
            name,
            dimensions,
            source_type,
            relative_offset,
            source_bytes,
        });
    }
    let alignment = u64::from(metadata.alignment.unwrap_or(32));
    let position = reader
        .stream_position()
        .map_err(io_error("find GGUF data offset"))?;
    let data_offset = align_up(position, alignment)?;
    for tensor in &tensors {
        let end = data_offset
            .checked_add(tensor.relative_offset)
            .and_then(|n| n.checked_add(tensor.source_bytes))
            .ok_or_else(|| format!("tensor {} source range overflows", tensor.name))?;
        if end > file_bytes {
            return Err(format!("tensor {} extends past end of GGUF", tensor.name));
        }
    }
    Ok(GgufModel {
        metadata,
        tensors,
        data_offset,
        file_bytes,
    })
}

fn validate_gguf(model: &GgufModel, specs: &[TensorSpec]) -> Result<(), String> {
    if model.file_bytes != u64::from(PINNED_GGUF_BYTES) {
        return Err(format!("GGUF size {} != pinned {PINNED_GGUF_BYTES}", model.file_bytes));
    }
    let meta = &model.metadata;
    exact_meta("general.architecture", meta.architecture.as_deref(), "lfm2")?;
    exact_meta("lfm2.block_count", meta.block_count, MODEL_LAYER_COUNT as u32)?;
    exact_meta("lfm2.context_length", meta.context_length, MODEL_SOURCE_CONTEXT)?;
    exact_meta("lfm2.embedding_length", meta.embedding_length, MODEL_HIDDEN_SIZE)?;
    exact_meta("lfm2.feed_forward_length", meta.feed_forward_length, MODEL_FEED_FORWARD_SIZE)?;
    exact_meta("lfm2.attention.head_count", meta.head_count, MODEL_ATTENTION_HEADS as u32)?;
    exact_meta("lfm2.vocab_size", meta.vocabulary_size, MODEL_VOCABULARY_SIZE)?;
    exact_meta("lfm2.shortconv.l_cache", meta.shortconv_cache, MODEL_SHORTCONV_CACHE as u32)?;
    let expected_kv: Vec<i32> = LAYER_SCHEDULE
        .iter()
        .map(|kind| {
            if *kind == LayerKind::Attention {
                MODEL_KV_HEADS as i32
            } else {
                0
            }
        })
        .collect();
    if meta.head_count_kv.as_deref() != Some(expected_kv.as_slice()) {
        return Err(format!(
            "lfm2.attention.head_count_kv {:?} != {:?}",
            meta.head_count_kv, expected_kv
        ));
    }
    if model.tensors.len() != specs.len() {
        return Err(format!(
            "GGUF tensor count {} != catalogue {}",
            model.tensors.len(),
            specs.len()
        ));
    }
    for (index, (actual, expected)) in model.tensors.iter().zip(specs).enumerate() {
        let expected_dimensions: Vec<u64> = if expected.rank == 1 {
            vec![u64::from(expected.ne0)]
        } else {
            vec![u64::from(expected.ne0), u64::from(expected.ne1)]
        };
        if actual.name != expected.name
            || actual.dimensions != expected_dimensions
            || actual.source_type != expected.source_type
        {
            return Err(format!(
                "tensor {index} mismatch: got {} {:?} type {}, expected {} {:?} type {}",
                actual.name,
                actual.dimensions,
                actual.source_type,
                expected.name,
                expected_dimensions,
                expected.source_type
            ));
        }
    }
    Ok(())
}

fn pack_native_image(
    gguf_path: &Path,
    model: &GgufModel,
    descriptors: &[NativeTensorDescriptor],
    output: &Path,
) -> Result<(), String> {
    let stage = stage_path(output)?;
    let result = (|| {
        let source =
            File::open(gguf_path).map_err(|e| format!("open {}: {e}", gguf_path.display()))?;
        let mut source = BufReader::new(source);
        let destination = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&stage)
            .map_err(|e| format!("create {}: {e}", stage.display()))?;
        let mut destination = BufWriter::new(destination);
        let mut hasher = Sha256::new();
        let mut written = 0u64;
        let mut copy_buffer = vec![0u8; 1024 * 1024];
        let zeros = [0u8; MODEL_TENSOR_ALIGNMENT];
        for (tensor, descriptor) in model.tensors.iter().zip(descriptors) {
            let target = u64::from(descriptor.native_offset);
            if target < written {
                return Err(format!("native offset for {} overlaps prior tensor", tensor.name));
            }
            write_hashed(&mut destination, &mut hasher, &zeros[..(target - written) as usize])?;
            written = target;
            source
                .seek(SeekFrom::Start(model.data_offset + tensor.relative_offset))
                .map_err(io_error("seek GGUF tensor"))?;
            match tensor.source_type {
                GGML_TYPE_Q8_0 => {
                    copy_exact_hashed(
                        &mut source,
                        &mut destination,
                        &mut hasher,
                        tensor.source_bytes,
                        &mut copy_buffer,
                    )?;
                }
                GGML_TYPE_F32 => {
                    let elements = tensor.source_bytes / 4;
                    let mut f32_bytes = [0u8; 4];
                    for _ in 0..elements {
                        source
                            .read_exact(&mut f32_bytes)
                            .map_err(io_error("read F32 tensor"))?;
                        let bf16 = f32_to_bf16_rne(u32::from_le_bytes(f32_bytes)).to_le_bytes();
                        write_hashed(&mut destination, &mut hasher, &bf16)?;
                    }
                }
                _ => unreachable!(),
            }
            written += u64::from(descriptor.native_bytes);
        }
        let final_bytes = u64::from(PINNED_NATIVE_IMAGE_BYTES);
        write_hashed(&mut destination, &mut hasher, &zeros[..(final_bytes - written) as usize])?;
        destination
            .flush()
            .map_err(io_error("flush native image"))?;
        destination
            .get_ref()
            .sync_all()
            .map_err(io_error("sync native image"))?;
        let digest: [u8; 32] = hasher.finalize().into();
        validate_native_seal(final_bytes, digest)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&stage);
        return Err(error);
    }
    fs::rename(&stage, output).map_err(|e| format!("publish {}: {e}", output.display()))?;
    Ok(())
}

fn f32_to_bf16_rne(bits: u32) -> u16 {
    let rounding_bias = 0x7fff + ((bits >> 16) & 1);
    (bits.wrapping_add(rounding_bias) >> 16) as u16
}

fn validate_native_seal(bytes: u64, digest: [u8; 32]) -> Result<(), String> {
    if bytes != u64::from(PINNED_NATIVE_IMAGE_BYTES) {
        return Err(format!("native image is {bytes} bytes, expected {PINNED_NATIVE_IMAGE_BYTES}"));
    }
    if digest != PINNED_NATIVE_IMAGE_SHA256 {
        return Err(format!(
            "native image SHA-256 {} does not match pinned {}",
            hex(&digest),
            hex(&PINNED_NATIVE_IMAGE_SHA256)
        ));
    }
    Ok(())
}

fn emit_rust(
    seal: &ModelSeal,
    contract_sha: [u8; 32],
    specs: &[TensorSpec],
    descriptors: &[NativeTensorDescriptor],
) -> String {
    let mut rust = String::from("// @generated by TRUEGA tools/lfm25-seal; do not hand-edit.\n\nuse super::{ModelSeal, NativeTensorDescriptor};\n\n");
    rust.push_str("pub const MODEL_SEAL: ModelSeal = ModelSeal {\n");
    writeln!(rust, "    magic: *b\"TGALFM25\",").unwrap();
    writeln!(rust, "    layout_version: {},", seal.layout_version).unwrap();
    writeln!(rust, "    seal_bytes: {},", seal.seal_bytes).unwrap();
    writeln!(rust, "    descriptor_bytes: {},", seal.descriptor_bytes).unwrap();
    writeln!(rust, "    tensor_count: {},", seal.tensor_count).unwrap();
    writeln!(rust, "    flags: 0x{:08x},", seal.flags).unwrap();
    writeln!(rust, "    model_generation: {},", seal.model_generation).unwrap();
    writeln!(rust, "    tensor_alignment: {},", seal.tensor_alignment).unwrap();
    writeln!(rust, "    source_context: {},", seal.source_context).unwrap();
    writeln!(rust, "    initial_context: {},", seal.initial_context).unwrap();
    writeln!(rust, "    hidden_size: {},", seal.hidden_size).unwrap();
    writeln!(rust, "    feed_forward_size: {},", seal.feed_forward_size).unwrap();
    writeln!(rust, "    vocabulary_size: {},", seal.vocabulary_size).unwrap();
    writeln!(rust, "    layer_count: {},", seal.layer_count).unwrap();
    writeln!(rust, "    attention_heads: {},", seal.attention_heads).unwrap();
    writeln!(rust, "    kv_heads: {},", seal.kv_heads).unwrap();
    writeln!(rust, "    head_dimension: {},", seal.head_dimension).unwrap();
    writeln!(rust, "    shortconv_cache: {},", seal.shortconv_cache).unwrap();
    writeln!(rust, "    attention_mask: 0x{:04x},", seal.attention_mask).unwrap();
    writeln!(rust, "    source_gguf_bytes: {},", seal.source_gguf_bytes).unwrap();
    writeln!(rust, "    native_image_bytes: 0x{:08x},", seal.native_image_bytes).unwrap();
    writeln!(rust, "    source_gguf_sha256: {},", rust_bytes(&seal.source_gguf_sha256)).unwrap();
    writeln!(rust, "    native_image_sha256: {},", rust_bytes(&seal.native_image_sha256)).unwrap();
    writeln!(rust, "    tensor_table_sha256: {},", rust_bytes(&seal.tensor_table_sha256)).unwrap();
    writeln!(rust, "    layer_schedule: {:?},", seal.layer_schedule).unwrap();
    rust.push_str("    reserved: [0; 12],\n};\n\n");
    writeln!(rust, "pub const MODEL_CONTRACT_SHA256: [u8; 32] = {};", rust_bytes(&contract_sha))
        .unwrap();
    rust.push_str("\npub const TENSOR_NAMES: [&str; super::MODEL_TENSOR_COUNT] = [\n");
    for spec in specs {
        writeln!(rust, "    {:?},", spec.name).unwrap();
    }
    rust.push_str(
        "];\n\npub const TENSORS: [NativeTensorDescriptor; super::MODEL_TENSOR_COUNT] = [\n",
    );
    for descriptor in descriptors {
        rust.push_str("    NativeTensorDescriptor {\n");
        writeln!(rust, "        tensor_id: {}, layer: 0x{:02x}, role: {}, format: {}, rank: {}, flags: 0x{:04x},", descriptor.tensor_id, descriptor.layer, descriptor.role, descriptor.format, descriptor.rank, descriptor.flags).unwrap();
        writeln!(
            rust,
            "        ggml_ne0: {}, ggml_ne1: {}, native_offset: 0x{:08x}, native_bytes: 0x{:08x},",
            descriptor.ggml_ne0,
            descriptor.ggml_ne1,
            descriptor.native_offset,
            descriptor.native_bytes
        )
        .unwrap();
        rust.push_str("    },\n");
    }
    rust.push_str("];\n");
    rust
}

fn emit_verilog(contract: &[u8], contract_sha: [u8; 32]) -> String {
    let words = contract.len() / 4;
    let mut rtl = format!("// @generated by TRUEGA tools/lfm25-seal; do not hand-edit.\n// Contract SHA-256: {}\n// Deliberately uninstantiated in the heartbeat firmware.\nmodule truega_lfm25_model_rom(clk, word_index, data);\n    input wire clk;\n    input wire [9:0] word_index;\n    output reg [31:0] data;\n    always @(posedge clk) begin\n        case (word_index)\n", hex(&contract_sha));
    for (index, bytes) in contract.chunks_exact(4).enumerate() {
        let word = u32::from_le_bytes(bytes.try_into().unwrap());
        writeln!(rtl, "            10'd{index}: data <= 32'h{word:08X};").unwrap();
    }
    rtl.push_str(
        "            default: data <= 32'h00000000;\n        endcase\n    end\nendmodule\n",
    );
    assert_eq!(words, 936);
    rtl
}

fn publish_small_outputs(
    outputs: &Outputs,
    contract: &[u8],
    rust: &[u8],
    rtl: &[u8],
) -> Result<(), String> {
    write_atomic(&outputs.contract, contract)?;
    write_atomic(&outputs.rust, rust)?;
    write_atomic(&outputs.rtl, rtl)?;
    Ok(())
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    if fs::read(path).ok().as_deref() == Some(contents) {
        return Ok(());
    }
    let stage = stage_path(path)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&stage)
            .map_err(|e| format!("create {}: {e}", stage.display()))?;
        file.write_all(contents)
            .map_err(|e| format!("write {}: {e}", stage.display()))?;
        file.sync_all()
            .map_err(|e| format!("sync {}: {e}", stage.display()))?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&stage);
        return Err(error);
    }
    fs::rename(&stage, path).map_err(|e| format!("publish {}: {e}", path.display()))
}

fn stage_path(path: &Path) -> Result<PathBuf, String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("invalid output path {}", path.display()))?;
    Ok(parent.join(format!(".{name}.{}.tmp", std::process::id())))
}

fn copy_exact_hashed<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    hasher: &mut Sha256,
    mut bytes: u64,
    buffer: &mut [u8],
) -> Result<(), String> {
    while bytes != 0 {
        let count = usize::try_from(bytes.min(buffer.len() as u64)).unwrap();
        reader
            .read_exact(&mut buffer[..count])
            .map_err(io_error("read Q8_0 tensor"))?;
        write_hashed(writer, hasher, &buffer[..count])?;
        bytes -= count as u64;
    }
    Ok(())
}

fn write_hashed<W: Write>(writer: &mut W, hasher: &mut Sha256, bytes: &[u8]) -> Result<(), String> {
    writer
        .write_all(bytes)
        .map_err(io_error("write native image"))?;
    hasher.update(bytes);
    Ok(())
}

fn sha256_file(path: &Path) -> Result<[u8; 32], String> {
    let mut reader =
        BufReader::new(File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().into())
}

fn read_typed_u32<R: Read>(reader: &mut R, ty: u32, key: &str) -> Result<u32, String> {
    if ty != 4 {
        return Err(format!("metadata {key} has type {ty}, expected u32"));
    }
    read_u32(reader)
}

fn read_typed_string<R: Read>(reader: &mut R, ty: u32, key: &str) -> Result<String, String> {
    if ty != 8 {
        return Err(format!("metadata {key} has type {ty}, expected string"));
    }
    read_string(reader)
}

fn read_typed_i32_array<R: Read + Seek>(
    reader: &mut R,
    ty: u32,
    key: &str,
) -> Result<Vec<i32>, String> {
    if ty != 9 {
        return Err(format!("metadata {key} has type {ty}, expected array"));
    }
    let element_type = read_u32(reader)?;
    if element_type != 5 {
        return Err(format!("metadata {key} array has element type {element_type}, expected i32"));
    }
    let count = read_u64(reader)?;
    let mut values = Vec::with_capacity(count as usize);
    for _ in 0..count {
        values.push(read_u32(reader)? as i32);
    }
    Ok(values)
}

fn skip_value<R: Read + Seek>(reader: &mut R, ty: u32) -> Result<(), String> {
    let fixed = match ty {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        _ => None,
    };
    if let Some(bytes) = fixed {
        reader
            .seek(SeekFrom::Current(bytes))
            .map_err(io_error("skip GGUF metadata"))?;
        return Ok(());
    }
    match ty {
        8 => {
            let bytes = read_u64(reader)?;
            reader
                .seek(SeekFrom::Current(
                    i64::try_from(bytes).map_err(|_| "GGUF string too large".to_string())?,
                ))
                .map_err(io_error("skip GGUF string"))?;
        }
        9 => {
            let element_type = read_u32(reader)?;
            let count = read_u64(reader)?;
            for _ in 0..count {
                skip_value(reader, element_type)?;
            }
        }
        _ => return Err(format!("unknown GGUF metadata type {ty}")),
    }
    Ok(())
}

fn read_string<R: Read>(reader: &mut R) -> Result<String, String> {
    let length = read_u64(reader)?;
    let length =
        usize::try_from(length).map_err(|_| "GGUF string length exceeds usize".to_string())?;
    let mut bytes = vec![0; length];
    reader
        .read_exact(&mut bytes)
        .map_err(io_error("read GGUF string"))?;
    String::from_utf8(bytes).map_err(|e| format!("GGUF string is not UTF-8: {e}"))
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32, String> {
    let mut bytes = [0; 4];
    reader
        .read_exact(&mut bytes)
        .map_err(io_error("read GGUF u32"))?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64, String> {
    let mut bytes = [0; 8];
    reader
        .read_exact(&mut bytes)
        .map_err(io_error("read GGUF u64"))?;
    Ok(u64::from_le_bytes(bytes))
}

fn io_error(context: &'static str) -> impl FnOnce(io::Error) -> String {
    move |error| format!("{context}: {error}")
}

fn exact_meta<T: std::fmt::Debug + PartialEq>(
    name: &str,
    actual: Option<T>,
    expected: T,
) -> Result<(), String> {
    if actual.as_ref() != Some(&expected) {
        return Err(format!("metadata {name} is {actual:?}, expected {expected:?}"));
    }
    Ok(())
}

fn align_up(value: u64, alignment: u64) -> Result<u64, String> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return Err(format!("invalid alignment {alignment}"));
    }
    value
        .checked_add(alignment - 1)
        .map(|v| v & !(alignment - 1))
        .ok_or_else(|| "alignment overflow".to_string())
}

fn rust_bytes(bytes: &[u8]) -> String {
    let mut text = String::from("[");
    for (index, byte) in bytes.iter().enumerate() {
        if index != 0 {
            text.push_str(", ");
        }
        write!(text, "0x{byte:02x}").unwrap();
    }
    text.push(']');
    text
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(text, "{byte:02x}").unwrap();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_catalogue_has_stable_shape_and_roles() {
        let specs = exact_catalogue();
        validate_catalogue(&specs).unwrap();
        assert_eq!(specs.len(), 148);
        assert_eq!(
            specs
                .iter()
                .filter(|t| t.source_type == GGML_TYPE_Q8_0)
                .count(),
            93
        );
        assert_eq!(
            specs
                .iter()
                .filter(|t| t.source_type == GGML_TYPE_F32)
                .count(),
            55
        );
        assert_eq!(specs.first().unwrap().name, "token_embd.weight");
        assert_eq!(specs.last().unwrap().name, "blk.15.shortconv.out_proj.weight");
    }

    #[test]
    fn native_layout_is_aligned_non_overlapping_and_exact_size() {
        let descriptors = native_descriptors(&exact_catalogue()).unwrap();
        for pair in descriptors.windows(2) {
            assert_eq!(pair[0].native_offset as usize % MODEL_TENSOR_ALIGNMENT, 0);
            assert!(pair[0].native_offset + pair[0].native_bytes <= pair[1].native_offset);
        }
        let last = descriptors.last().unwrap();
        assert_eq!(
            align_up(u64::from(last.native_offset + last.native_bytes), 256).unwrap(),
            u64::from(PINNED_NATIVE_IMAGE_BYTES)
        );
    }

    #[test]
    fn bf16_rounding_is_nearest_even() {
        assert_eq!(f32_to_bf16_rne(1.0f32.to_bits()), 0x3f80);
        assert_eq!(f32_to_bf16_rne(0x3f80_8000), 0x3f80);
        assert_eq!(f32_to_bf16_rne(0x3f81_8000), 0x3f82);
        assert_eq!(f32_to_bf16_rne((-2.0f32).to_bits()), 0xc000);
    }

    #[test]
    fn binary_contract_and_verilog_rom_are_identical() {
        let descriptors = native_descriptors(&exact_catalogue()).unwrap();
        let table = serialize_table(&descriptors);
        let seal = exact_seal(Sha256::digest(&table).into());
        let contract = serialize_contract(&seal, &table);
        assert_eq!(contract.len(), 3744);
        assert_eq!(&contract[..8], b"TGALFM25");
        assert_eq!(&contract[192..194], &0u16.to_le_bytes());
        let rtl = emit_verilog(&contract, Sha256::digest(&contract).into());
        for (index, bytes) in contract.chunks_exact(4).enumerate() {
            let word = u32::from_le_bytes(bytes.try_into().unwrap());
            assert!(rtl.contains(&format!("10'd{index}: data <= 32'h{word:08X};")));
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let specs = exact_catalogue();
        let descriptors = native_descriptors(&specs).unwrap();
        let table_a = serialize_table(&descriptors);
        let table_b = serialize_table(&native_descriptors(&exact_catalogue()).unwrap());
        assert_eq!(table_a, table_b);
        let seal = exact_seal(Sha256::digest(&table_a).into());
        let contract = serialize_contract(&seal, &table_a);
        let hash: [u8; 32] = Sha256::digest(&contract).into();
        assert_eq!(
            emit_rust(&seal, hash, &specs, &descriptors),
            emit_rust(&seal, hash, &specs, &descriptors)
        );
        assert_eq!(emit_verilog(&contract, hash), emit_verilog(&contract, hash));
    }

    #[test]
    fn corruption_cannot_pass_the_native_seal() {
        validate_native_seal(u64::from(PINNED_NATIVE_IMAGE_BYTES), PINNED_NATIVE_IMAGE_SHA256)
            .unwrap();
        let mut corrupted = PINNED_NATIVE_IMAGE_SHA256;
        corrupted[7] ^= 1;
        assert!(validate_native_seal(u64::from(PINNED_NATIVE_IMAGE_BYTES), corrupted).is_err());
        assert!(validate_native_seal(
            u64::from(PINNED_NATIVE_IMAGE_BYTES) - 1,
            PINNED_NATIVE_IMAGE_SHA256
        )
        .is_err());
    }
}
