// Kernel-owned ABI for the Intel entropy/compression research lane.
//
// No codec is admitted here yet. These records freeze the ownership and
// completion contract first so a later IGC artifact can be hash/ABI admitted
// through the existing direct-RCS machinery without inventing a second memory
// or scheduling model.

pub(crate) const ENTROPY_STREAM_MAGIC: u32 = u32::from_le_bytes(*b"TEN1");
pub(crate) const ENTROPY_STREAM_ABI_VERSION: u32 = 1;
pub(crate) const ENTROPY_STREAM_STATE_COUNT: u32 = 32;
pub(crate) const ENTROPY_STREAM_DEFAULT_CHUNK_BYTES: u32 = 256 * 1024;
pub(crate) const ENTROPY_STREAM_MAX_CHUNKS_PER_BATCH: u32 = 4096;

pub(crate) const ENTROPY_MODEL_RAW: u32 = 0;
pub(crate) const ENTROPY_MODEL_ENUMERATIVE_BITPLANE: u32 = 1;
pub(crate) const ENTROPY_MODEL_CTW_BINARY: u32 = 2;
pub(crate) const ENTROPY_MODEL_RANS32: u32 = 3;
pub(crate) const ENTROPY_MODEL_CTW_RANS32: u32 = 4;
pub(crate) const ENTROPY_MODEL_BITS_BACK_RANS: u32 = 5;

pub(crate) const ENTROPY_CHUNK_FLAG_FINAL: u32 = 1 << 0;
pub(crate) const ENTROPY_CHUNK_FLAG_INDEPENDENT: u32 = 1 << 1;
pub(crate) const ENTROPY_CHUNK_FLAG_EXACT_ENUMERATIVE_CLASS: u32 = 1 << 2;
pub(crate) const ENTROPY_CHUNK_FLAG_MODEL_EMBEDDED: u32 = 1 << 3;

pub(crate) const ENTROPY_BATCH_FLAG_PING_A_TO_B: u32 = 1 << 0;
pub(crate) const ENTROPY_BATCH_FLAG_PING_B_TO_A: u32 = 1 << 1;
pub(crate) const ENTROPY_BATCH_FLAG_REQUEST_GT_RP0_WINDOW: u32 = 1 << 2;
pub(crate) const ENTROPY_BATCH_FLAG_VERIFY_ROUND_TRIP: u32 = 1 << 3;

/// One cache line per independently decodable unit.
///
/// `src_gpu` and `dst_gpu` are already mapped in the caller PPGTT before GuC
/// submission. The GPU owns `dst_len`, `checksum`, and implementation-private
/// model storage only while the batch generation is RUNNING. The CPU/UAS
/// side may consume the output only after `EntropyStreamCompletion.generation`
/// reaches the submitted generation with `error_code == 0`.
#[repr(C, align(64))]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct EntropyStreamChunk {
    pub(crate) src_gpu: u64,
    pub(crate) dst_gpu: u64,
    pub(crate) src_len: u32,
    pub(crate) dst_capacity: u32,
    pub(crate) dst_len: u32,
    pub(crate) model: u32,
    pub(crate) flags: u32,
    pub(crate) generation: u32,
    pub(crate) model_offset: u32,
    pub(crate) model_bytes: u32,
    pub(crate) checksum: u32,
    pub(crate) reserved: [u32; 3],
}

impl EntropyStreamChunk {
    pub(crate) const fn input_range_valid(self) -> bool {
        self.src_gpu != 0 && self.src_len != 0
    }

    pub(crate) const fn output_range_valid(self) -> bool {
        self.dst_gpu != 0 && self.dst_capacity != 0
    }

    pub(crate) const fn admitted_model(self) -> bool {
        self.model <= ENTROPY_MODEL_BITS_BACK_RANS
    }
}

/// One cache line describing a ping/pong generation.
#[repr(C, align(64))]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct EntropyStreamBatch {
    pub(crate) magic: u32,
    pub(crate) abi_version: u32,
    pub(crate) generation: u32,
    pub(crate) chunk_count: u32,
    pub(crate) chunk_bytes: u32,
    pub(crate) state_count: u32,
    pub(crate) default_model: u32,
    pub(crate) flags: u32,
    pub(crate) chunks_gpu: u64,
    pub(crate) completion_gpu: u64,
    pub(crate) arena_a_gpu: u64,
    pub(crate) arena_b_gpu: u64,
}

impl EntropyStreamBatch {
    pub(crate) const fn new(
        generation: u32,
        chunk_count: u32,
        chunks_gpu: u64,
        completion_gpu: u64,
        arena_a_gpu: u64,
        arena_b_gpu: u64,
    ) -> Self {
        Self {
            magic: ENTROPY_STREAM_MAGIC,
            abi_version: ENTROPY_STREAM_ABI_VERSION,
            generation,
            chunk_count,
            chunk_bytes: ENTROPY_STREAM_DEFAULT_CHUNK_BYTES,
            state_count: ENTROPY_STREAM_STATE_COUNT,
            default_model: ENTROPY_MODEL_CTW_RANS32,
            flags: ENTROPY_BATCH_FLAG_VERIFY_ROUND_TRIP,
            chunks_gpu,
            completion_gpu,
            arena_a_gpu,
            arena_b_gpu,
        }
    }

    pub(crate) const fn header_valid(self) -> bool {
        self.magic == ENTROPY_STREAM_MAGIC
            && self.abi_version == ENTROPY_STREAM_ABI_VERSION
            && self.chunk_count != 0
            && self.chunk_count <= ENTROPY_STREAM_MAX_CHUNKS_PER_BATCH
            && self.chunk_bytes != 0
            && self.state_count == ENTROPY_STREAM_STATE_COUNT
            && self.chunks_gpu != 0
            && self.completion_gpu != 0
            && self.arena_a_gpu != 0
            && self.arena_b_gpu != 0
    }
}

/// GPU-to-CPU release record. The final walker/epilogue must make all output
/// stores globally visible before publishing `generation` with release-like
/// ordering. The exact PIPE_CONTROL/post-sync sequence belongs in the future
/// RCS encoder and is intentionally not guessed in this ABI-only change.
#[repr(C, align(64))]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct EntropyStreamCompletion {
    pub(crate) generation: u32,
    pub(crate) completed_chunks: u32,
    pub(crate) error_code: u32,
    pub(crate) flags: u32,
    pub(crate) input_bytes: u64,
    pub(crate) output_bytes: u64,
    pub(crate) gpu_ticks: u64,
    pub(crate) reserved: [u64; 3],
}

impl EntropyStreamCompletion {
    pub(crate) const fn retired(self, expected_generation: u32, chunk_count: u32) -> bool {
        self.generation == expected_generation
            && self.completed_chunks == chunk_count
            && self.error_code == 0
    }
}
