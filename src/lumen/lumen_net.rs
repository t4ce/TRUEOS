extern crate alloc;

// Lumen network matvec adapter.
//
// The important trick here is that remote TRUEOS rigs cannot use our local
// model pointers. Each machine loads the same safetensors model into different
// virtual addresses, so pointer-derived work descriptors are only meaningful
// inside one kernel. During model load we therefore build a small matrix
// manifest: every contiguous BF16 2D weight receives a stable `matrix_id`
// derived from tensor name + shape + dtype. Runtime matvec calls still arrive
// from Lumen as raw pointers, but those pointers are only used locally to look
// up the manifest entry. The network protocol will carry `matrix_id`, row
// range, shape, and the live input vector `x`; the peer resolves `matrix_id`
// against its own manifest and computes against its own resident weights.
//
// This keeps ownership simple: weights stay resident and read-only on each
// rig, activation vectors/results cross the wire, and hard row-splitting must
// stay disabled until TCP result completion or shadow-compare is wired.
// Current shadow mode sends owned descriptor + x-vector chunk frames so peer
// routing and payload reassembly can be proven without changing generation
// math.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use spin::Mutex;

const MIN_REMOTE_ROWS: usize = 64;
const FRAME_MAGIC: u32 = 0x4C4E_4554; // LNET
pub(crate) const PROTOCOL_VERSION: u16 = 1;
pub(crate) const CAP_BF16_MATVEC_ROWS: u32 = 1 << 0;
pub(crate) const CAP_MODEL_RESIDENT_MATRIX_ID: u32 = 1 << 1;
pub(crate) const CAP_ROW_RANGE_OUTPUT_F32: u32 = 1 << 2;

const OP_BF16_MATVEC_ROWS: u16 = 1;

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);
static MATRIX_EPOCH: AtomicU64 = AtomicU64::new(0);
static SHADOW_SUBMITTED: AtomicU64 = AtomicU64::new(0);
static LOGGED_DISABLED: AtomicBool = AtomicBool::new(false);
static LOGGED_SHADOW_DISABLED: AtomicBool = AtomicBool::new(false);
static LOGGED_SHADOW_ENQUEUE: AtomicBool = AtomicBool::new(false);
static LOGGED_SHADOW_DROPPED: AtomicBool = AtomicBool::new(false);
static LOGGED_MISSING_MATRIX: AtomicBool = AtomicBool::new(false);
static LOGGED_ENQUEUE: AtomicBool = AtomicBool::new(false);
static REMOTE_ROUTE_AVAILABLE: AtomicBool = AtomicBool::new(false);
static MATRIX_MANIFEST: Mutex<Vec<LumenMatrixManifestEntry>> = Mutex::new(Vec::new());
static SHADOW_BF16_MATVEC_FRAMES: Mutex<VecDeque<Vec<u8>>> = Mutex::new(VecDeque::new());
static PENDING_BF16_MATVECS: Mutex<VecDeque<RemoteBf16MatvecJob>> = Mutex::new(VecDeque::new());
static RESULT_BF16_MATVECS: Mutex<Vec<RemoteBf16MatvecResultReassembly>> = Mutex::new(Vec::new());

#[derive(Copy, Clone, Debug)]
pub(crate) struct LumenNetBackendTelemetry {
    pub(crate) protocol_version: u16,
    pub(crate) caps: u32,
    pub(crate) capacity_lanes: u32,
    pub(crate) local_workers: u32,
    pub(crate) pending_bf16_matvecs: u32,
    pub(crate) min_remote_rows: u32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ShadowBf16MatvecProof {
    pub(crate) rows: usize,
    pub(crate) checksum: u64,
    pub(crate) first: f32,
    pub(crate) last: f32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct RemoteBf16MatvecTicket {
    pub(crate) job_id: u64,
    pub(crate) row_start: usize,
    pub(crate) row_end: usize,
}

#[derive(Copy, Clone, Debug)]
struct RemoteBf16MatvecJob {
    job_id: u64,
    matrix_id: u64,
    row_start: usize,
    row_end: usize,
    n_rows: usize,
    k_dim: usize,
    x_ptr: usize,
    x_len: usize,
    w_rowmajor_bf16_ptr: usize,
    w_rowmajor_bf16_len: usize,
    out_ptr: usize,
    out_len: usize,
}

#[derive(Debug)]
struct RemoteBf16MatvecResultReassembly {
    job_id: u64,
    total_bytes: usize,
    received_bytes: usize,
    chunks: usize,
    data: Vec<u8>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct RemoteBf16MatvecResultUpdate {
    pub(crate) received_bytes: usize,
    pub(crate) total_bytes: usize,
    pub(crate) chunks: usize,
    pub(crate) complete: bool,
    pub(crate) copied_rows: usize,
    pub(crate) checksum: u64,
}

#[derive(Copy, Clone, Debug)]
struct LumenMatrixManifestEntry {
    matrix_id: u64,
    name_hash: u64,
    name_len: u32,
    dtype_code: u16,
    rows: u32,
    k_dim: u32,
    data_ptr: usize,
    byte_len: usize,
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct LumenNetFrameHeader {
    magic: u32,
    version: u16,
    opcode: u16,
    job_id: u64,
    matrix_id: u64,
    row_start: u64,
    row_count: u64,
    n_rows: u64,
    k_dim: u64,
    x_bytes: u64,
    output_bytes: u64,
}

pub(crate) fn begin_matrix_manifest_load() {
    MATRIX_MANIFEST.lock().clear();
    let epoch = MATRIX_EPOCH
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    crate::log!("lumen-net: matrix manifest reset epoch={} source=model-load\n", epoch);
}

pub(crate) fn register_loaded_matrix(
    name: &str,
    dtype: &str,
    shape: &[usize],
    data_ptr: usize,
    byte_len: usize,
) {
    if dtype != "BF16" || shape.len() != 2 || data_ptr == 0 || byte_len == 0 {
        return;
    }
    let rows = shape[0];
    let k_dim = shape[1];
    let Some(expected_len) = rows
        .checked_mul(k_dim)
        .and_then(|values| values.checked_mul(2))
    else {
        return;
    };
    if expected_len != byte_len || rows > u32::MAX as usize || k_dim > u32::MAX as usize {
        return;
    }

    let name_hash = stable_name_hash(name);
    let matrix_id = stable_matrix_id(name_hash, rows, k_dim, dtype_code(dtype));
    let entry = LumenMatrixManifestEntry {
        matrix_id,
        name_hash,
        name_len: name.len().min(u32::MAX as usize) as u32,
        dtype_code: dtype_code(dtype),
        rows: rows as u32,
        k_dim: k_dim as u32,
        data_ptr,
        byte_len,
    };

    let mut manifest = MATRIX_MANIFEST.lock();
    if let Some(existing) = manifest.iter_mut().find(|item| item.matrix_id == matrix_id) {
        *existing = entry;
    } else {
        manifest.push(entry);
    }

    if manifest.len() <= 4 || manifest.len() % 16 == 0 {
        crate::log!(
            "lumen-net: matrix manifest register count={} matrix=0x{:016X} name_hash=0x{:016X} name_len={} dtype={} rows={} k_dim={} bytes={}\n",
            manifest.len(),
            matrix_id,
            entry.name_hash,
            entry.name_len,
            entry.dtype_code,
            entry.rows,
            entry.k_dim,
            entry.byte_len
        );
    }
}

pub(crate) fn enqueue_remote_bf16_matvec_suffix(
    x: &[f32],
    w_rowmajor_bf16: &[u8],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
    chunk_rows: usize,
) -> Option<RemoteBf16MatvecTicket> {
    if !route_bf16_matvec_to_net_backend() {
        enqueue_shadow_bf16_matvec_suffix(x, w_rowmajor_bf16, n_rows, k_dim, out, chunk_rows);
        if !LOGGED_DISABLED.swap(true, Ordering::AcqRel) {
            crate::log!(
                "lumen-net: remote bf16 matvec adapter present route_enabled=0 action=local-burn-baby-only\n"
            );
        }
        return None;
    }

    if n_rows < MIN_REMOTE_ROWS || chunk_rows == 0 || x.len() < k_dim || out.len() < n_rows {
        return None;
    }

    let Some(expected_w_len) = n_rows
        .checked_mul(k_dim)
        .and_then(|values| values.checked_mul(2))
    else {
        return None;
    };
    if w_rowmajor_bf16.len() < expected_w_len {
        return None;
    }

    let Some(matrix) =
        resolve_bf16_matrix(w_rowmajor_bf16.as_ptr() as usize, expected_w_len, n_rows, k_dim)
    else {
        if !LOGGED_MISSING_MATRIX.swap(true, Ordering::AcqRel) {
            crate::log!(
                "lumen-net: remote bf16 matvec skipped reason=no-stable-matrix-id rows={} k_dim={} bytes={} action=local-burn-baby-only\n",
                n_rows,
                k_dim,
                expected_w_len
            );
        }
        return None;
    };

    let half = n_rows / 2;
    let row_start = half
        .div_ceil(chunk_rows)
        .saturating_mul(chunk_rows)
        .min(n_rows);
    if row_start >= n_rows {
        return None;
    }

    let job_id = NEXT_JOB_ID.fetch_add(1, Ordering::AcqRel);
    let job = RemoteBf16MatvecJob {
        job_id,
        matrix_id: matrix.matrix_id,
        row_start,
        row_end: n_rows,
        n_rows,
        k_dim,
        x_ptr: x.as_ptr() as usize,
        x_len: k_dim,
        w_rowmajor_bf16_ptr: w_rowmajor_bf16.as_ptr() as usize,
        w_rowmajor_bf16_len: expected_w_len,
        out_ptr: out.as_mut_ptr() as usize,
        out_len: n_rows,
    };

    let header = encode_bf16_matvec_header(&job);
    let host_cookie = host_descriptor_cookie(&job);
    let x_bytes = k_dim.saturating_mul(core::mem::size_of::<f32>());
    let x_chunk_bytes = shadow_x_chunk_bytes();
    let x_chunks = x_bytes.div_ceil(x_chunk_bytes);
    let needed_frames = x_chunks.saturating_add(1);

    {
        let mut queue = SHADOW_BF16_MATVEC_FRAMES.lock();
        let frame_cap = crate::allcaps::lumen::NET_BF16_MATVEC_SHADOW_FRAME_QUEUE_CAP;
        if queue.len().saturating_add(needed_frames) > frame_cap {
            if !LOGGED_SHADOW_DROPPED.swap(true, Ordering::AcqRel) {
                crate::log!(
                    "lumen-net: bf16 matvec remote drop reason=frame-queue-full cap={} need={} pending_frames={}\n",
                    frame_cap,
                    needed_frames,
                    queue.len(),
                );
            }
            return None;
        }
        push_bf16_matvec_frames(&mut queue, &job, x_chunk_bytes);
    }

    PENDING_BF16_MATVECS.lock().push_back(job);
    if !LOGGED_ENQUEUE.swap(true, Ordering::AcqRel) {
        crate::log!(
            "lumen-net: bf16 matvec remote enqueue job={} rows={}..{} n_rows={} k_dim={} matrix=0x{:016X} host=0x{:016X} frame_magic=0x{:08X} opcode={} x_bytes={} frames={} completion=tcp-result\n",
            job.job_id,
            job.row_start,
            job.row_end,
            job.n_rows,
            job.k_dim,
            job.matrix_id,
            host_cookie,
            header.magic,
            header.opcode,
            x_bytes,
            needed_frames
        );
    }

    Some(RemoteBf16MatvecTicket {
        job_id,
        row_start,
        row_end: n_rows,
    })
}

fn enqueue_shadow_bf16_matvec_suffix(
    x: &[f32],
    w_rowmajor_bf16: &[u8],
    n_rows: usize,
    k_dim: usize,
    out: &mut [f32],
    chunk_rows: usize,
) {
    if !shadow_bf16_matvec_to_net_backend() {
        if !LOGGED_SHADOW_DISABLED.swap(true, Ordering::AcqRel) {
            crate::log!("lumen-net: shadow bf16 matvec route_enabled=0 action=no-shadow-frames\n");
        }
        return;
    }

    if n_rows < MIN_REMOTE_ROWS || chunk_rows == 0 || x.len() < k_dim || out.len() < n_rows {
        return;
    }
    let Some(expected_w_len) = n_rows
        .checked_mul(k_dim)
        .and_then(|values| values.checked_mul(2))
    else {
        return;
    };
    if w_rowmajor_bf16.len() < expected_w_len {
        return;
    }
    let Some(matrix) =
        resolve_bf16_matrix(w_rowmajor_bf16.as_ptr() as usize, expected_w_len, n_rows, k_dim)
    else {
        if !LOGGED_MISSING_MATRIX.swap(true, Ordering::AcqRel) {
            crate::log!(
                "lumen-net: shadow bf16 matvec skipped reason=no-stable-matrix-id rows={} k_dim={} bytes={}\n",
                n_rows,
                k_dim,
                expected_w_len
            );
        }
        return;
    };

    let submitted = SHADOW_SUBMITTED.fetch_add(1, Ordering::AcqRel);
    if submitted >= crate::allcaps::lumen::NET_BF16_MATVEC_SHADOW_MAX_JOBS_PER_BOOT {
        return;
    }

    let half = n_rows / 2;
    let row_start = half
        .div_ceil(chunk_rows)
        .saturating_mul(chunk_rows)
        .min(n_rows);
    if row_start >= n_rows {
        return;
    }

    let job_id = NEXT_JOB_ID.fetch_add(1, Ordering::AcqRel);
    let job = RemoteBf16MatvecJob {
        job_id,
        matrix_id: matrix.matrix_id,
        row_start,
        row_end: n_rows,
        n_rows,
        k_dim,
        x_ptr: x.as_ptr() as usize,
        x_len: k_dim,
        w_rowmajor_bf16_ptr: w_rowmajor_bf16.as_ptr() as usize,
        w_rowmajor_bf16_len: expected_w_len,
        out_ptr: out.as_mut_ptr() as usize,
        out_len: n_rows,
    };

    let x_bytes = k_dim.saturating_mul(core::mem::size_of::<f32>());
    let x_chunk_bytes = shadow_x_chunk_bytes();
    let x_chunks = x_bytes.div_ceil(x_chunk_bytes);
    let needed_frames = x_chunks.saturating_add(1);

    let mut queue = SHADOW_BF16_MATVEC_FRAMES.lock();
    let frame_cap = crate::allcaps::lumen::NET_BF16_MATVEC_SHADOW_FRAME_QUEUE_CAP;
    if queue.len().saturating_add(needed_frames) > frame_cap {
        if !LOGGED_SHADOW_DROPPED.swap(true, Ordering::AcqRel) {
            crate::log!(
                "lumen-net: shadow bf16 matvec drop reason=frame-queue-full cap={} need={} pending_frames={} submitted={}\n",
                frame_cap,
                needed_frames,
                queue.len(),
                submitted.saturating_add(1),
            );
        }
        return;
    }

    push_bf16_matvec_frames(&mut queue, &job, x_chunk_bytes);

    if !LOGGED_SHADOW_ENQUEUE.swap(true, Ordering::AcqRel) {
        crate::log!(
            "lumen-net: shadow bf16 matvec enqueue job={} matrix=0x{:016X} rows={}..{} n_rows={} k_dim={} x_bytes={} x_chunks={} frames={} note=descriptor-and-x-chunks-local-compute-full-width\n",
            job.job_id,
            job.matrix_id,
            job.row_start,
            job.row_end,
            job.n_rows,
            job.k_dim,
            x_bytes,
            x_chunks,
            needed_frames,
        );
    }
}

pub(crate) fn pending_remote_bf16_matvecs() -> usize {
    PENDING_BF16_MATVECS.lock().len()
}

pub(crate) fn pending_shadow_bf16_matvecs() -> usize {
    SHADOW_BF16_MATVEC_FRAMES.lock().len()
}

pub(crate) fn take_shadow_bf16_matvec_frame() -> Option<Vec<u8>> {
    SHADOW_BF16_MATVEC_FRAMES.lock().pop_front()
}

pub(crate) fn record_remote_bf16_matvec_result_chunk(
    job_id: u64,
    offset: usize,
    total_bytes: usize,
    chunk: &[u8],
) -> Option<RemoteBf16MatvecResultUpdate> {
    if chunk.is_empty()
        || total_bytes == 0
        || offset.saturating_add(chunk.len()) > total_bytes
        || total_bytes % core::mem::size_of::<f32>() != 0
    {
        return None;
    }

    let mut results = RESULT_BF16_MATVECS.lock();
    let index = if let Some(index) = results.iter().position(|entry| entry.job_id == job_id) {
        index
    } else {
        let mut data = Vec::new();
        data.resize(total_bytes, 0);
        results.push(RemoteBf16MatvecResultReassembly {
            job_id,
            total_bytes,
            received_bytes: 0,
            chunks: 0,
            data,
        });
        results.len().saturating_sub(1)
    };

    let entry = &mut results[index];
    if entry.total_bytes != total_bytes || entry.data.len() != total_bytes {
        return None;
    }
    entry.data[offset..offset.saturating_add(chunk.len())].copy_from_slice(chunk);
    entry.received_bytes = entry
        .received_bytes
        .saturating_add(chunk.len())
        .min(total_bytes);
    entry.chunks = entry.chunks.saturating_add(1);
    let complete = entry.received_bytes >= entry.total_bytes;
    let checksum = if complete {
        fnv1a64(entry.data.as_slice())
    } else {
        0
    };
    let received_bytes = if complete {
        total_bytes
    } else {
        entry.received_bytes
    };
    let chunks = entry.chunks;
    let mut copied_rows = 0usize;

    if complete {
        let mut pending = PENDING_BF16_MATVECS.lock();
        if let Some(pending_index) = pending.iter().position(|job| job.job_id == job_id) {
            let job = pending[pending_index];
            let expected_bytes = job
                .row_end
                .saturating_sub(job.row_start)
                .saturating_mul(core::mem::size_of::<f32>());
            if expected_bytes == entry.total_bytes && job.out_len >= job.row_end {
                let out = unsafe {
                    core::slice::from_raw_parts_mut(job.out_ptr as *mut f32, job.out_len)
                };
                for (idx, bytes) in entry.data.chunks_exact(4).enumerate() {
                    out[job.row_start + idx] =
                        f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    copied_rows = copied_rows.saturating_add(1);
                }
                pending.remove(pending_index);
            }
        }
        results.remove(index);
    }

    Some(RemoteBf16MatvecResultUpdate {
        received_bytes,
        total_bytes,
        chunks,
        complete,
        copied_rows,
        checksum,
    })
}

pub(crate) fn remote_bf16_matvec_pending(job_id: u64) -> bool {
    PENDING_BF16_MATVECS
        .lock()
        .iter()
        .any(|job| job.job_id == job_id)
}

pub(crate) fn cancel_remote_bf16_matvec(job_id: u64) -> bool {
    RESULT_BF16_MATVECS
        .lock()
        .retain(|entry| entry.job_id != job_id);
    let mut pending = PENDING_BF16_MATVECS.lock();
    if let Some(index) = pending.iter().position(|job| job.job_id == job_id) {
        pending.remove(index);
        true
    } else {
        false
    }
}

pub(crate) fn wait_remote_bf16_matvec(ticket: RemoteBf16MatvecTicket) -> bool {
    let start = embassy_time_driver::now();
    let timeout_ticks = crate::allcaps::lumen::NET_BF16_MATVEC_RESULT_WAIT_TIMEOUT_MS
        .saturating_mul(embassy_time_driver::TICK_HZ.max(1))
        / 1000;
    loop {
        if !remote_bf16_matvec_pending(ticket.job_id) {
            return true;
        }
        crate::time::poll();
        crate::smp::poll();
        if timeout_ticks != 0 && embassy_time_driver::now().saturating_sub(start) > timeout_ticks {
            return false;
        }
        core::hint::spin_loop();
    }
}

fn push_bf16_matvec_frames(
    queue: &mut VecDeque<Vec<u8>>,
    job: &RemoteBf16MatvecJob,
    x_chunk_bytes: usize,
) {
    let x_bytes = job.x_len.saturating_mul(core::mem::size_of::<f32>());
    queue.push_back(encode_shadow_descriptor_frame(job));
    for (chunk_index, offset) in (0..x_bytes).step_by(x_chunk_bytes.max(1)).enumerate() {
        let end = offset.saturating_add(x_chunk_bytes).min(x_bytes);
        // `x` is the live activation slice for this matvec call; encode the
        // bytes into owned queue frames before returning to inference.
        let chunk = unsafe {
            core::slice::from_raw_parts(
                (job.x_ptr as *const u8).add(offset),
                end.saturating_sub(offset),
            )
        };
        queue.push_back(encode_shadow_x_chunk_frame(
            job.job_id,
            chunk_index,
            offset,
            x_bytes,
            chunk,
        ));
    }
}

pub(crate) fn compute_shadow_bf16_matvec_proof(
    matrix_id: u64,
    row_start: usize,
    row_count: usize,
    n_rows: usize,
    k_dim: usize,
    x_bytes: &[u8],
    max_rows: usize,
) -> Option<ShadowBf16MatvecProof> {
    compute_shadow_bf16_matvec_result_bytes(
        matrix_id, row_start, row_count, n_rows, k_dim, x_bytes, max_rows,
    )
    .map(|(proof, _result)| proof)
}

pub(crate) fn compute_shadow_bf16_matvec_result_bytes(
    matrix_id: u64,
    row_start: usize,
    row_count: usize,
    n_rows: usize,
    k_dim: usize,
    x_bytes: &[u8],
    max_proof_rows: usize,
) -> Option<(ShadowBf16MatvecProof, Vec<u8>)> {
    if !crate::allcaps::lumen::NET_BF16_MATVEC_SHADOW_COMPUTE_PROOF {
        return None;
    }
    if row_count == 0 || n_rows == 0 || k_dim == 0 || max_proof_rows == 0 {
        return None;
    }
    if row_start >= n_rows || x_bytes.len() != k_dim.saturating_mul(core::mem::size_of::<f32>()) {
        return None;
    }
    let row_end = row_start.saturating_add(row_count).min(n_rows);
    if row_end <= row_start {
        return None;
    }

    let matrix = resolve_bf16_matrix_by_id(matrix_id, n_rows, k_dim)?;
    let all_rows = row_end.saturating_sub(row_start);
    let proof_rows = all_rows.min(max_proof_rows);
    let mut x = Vec::with_capacity(k_dim);
    for chunk in x_bytes.chunks_exact(4) {
        x.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    if x.len() != k_dim {
        return None;
    }

    let w_len = n_rows.checked_mul(k_dim)?.checked_mul(2)?;
    if matrix.byte_len < w_len {
        return None;
    }
    let weights = unsafe { core::slice::from_raw_parts(matrix.data_ptr as *const u8, w_len) };
    let mut checksum = 0xCBF2_9CE4_8422_2325u64;
    let mut first = 0.0f32;
    let mut last = 0.0f32;
    let mut result = Vec::with_capacity(all_rows.saturating_mul(core::mem::size_of::<f32>()));
    for (result_idx, row) in (row_start..row_end).enumerate() {
        let base = row.checked_mul(k_dim)?.checked_mul(2)?;
        let row_weights = weights.get(base..base.saturating_add(k_dim.saturating_mul(2)))?;
        let mut acc = 0.0f32;
        for idx in 0..k_dim {
            let off = idx.saturating_mul(2);
            let bits = u16::from_le_bytes([row_weights[off], row_weights[off + 1]]);
            acc += x[idx] * bf16_to_f32(bits);
        }
        if result_idx == 0 {
            first = acc;
        }
        last = acc;
        let acc_bytes = acc.to_le_bytes();
        result.extend_from_slice(&acc_bytes);
        if result_idx < proof_rows {
            for byte in acc_bytes {
                checksum ^= byte as u64;
                checksum = checksum.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
    }

    Some((
        ShadowBf16MatvecProof {
            rows: proof_rows,
            checksum,
            first,
            last,
        },
        result,
    ))
}

fn encode_shadow_descriptor_frame(job: &RemoteBf16MatvecJob) -> Vec<u8> {
    let header = encode_bf16_matvec_header(job);
    let text = alloc::format!(
        "C0DEC0DE LUMEN_MATVEC_SHADOW v=1 job={} matrix=0x{:016X} rows={}..{} n_rows={} k_dim={} row_count={} x_bytes={} out_bytes={} frame_magic=0x{:08X} opcode={} note=descriptor-before-x-chunks\n",
        job.job_id,
        job.matrix_id,
        job.row_start,
        job.row_end,
        job.n_rows,
        job.k_dim,
        job.row_end.saturating_sub(job.row_start),
        header.x_bytes,
        header.output_bytes,
        header.magic,
        header.opcode
    );
    text.into_bytes()
}

fn encode_shadow_x_chunk_frame(
    job_id: u64,
    chunk_index: usize,
    offset: usize,
    total_bytes: usize,
    chunk: &[u8],
) -> Vec<u8> {
    let mut out = alloc::format!(
        "C0DEC0DE LUMEN_MATVEC_XCHUNK v=1 job={} chunk={} offset={} bytes={} total={} hex=",
        job_id,
        chunk_index,
        offset,
        chunk.len(),
        total_bytes,
    )
    .into_bytes();
    append_hex(&mut out, chunk);
    out.push(b'\n');
    out
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

pub(crate) fn route_bf16_matvec_to_net_backend() -> bool {
    crate::allcaps::lumen::ROUTE_BF16_MATVEC_TO_NET_BACKEND
        && REMOTE_ROUTE_AVAILABLE.load(Ordering::Acquire)
}

pub(crate) fn shadow_bf16_matvec_to_net_backend() -> bool {
    crate::allcaps::lumen::SHADOW_BF16_MATVEC_TO_NET_BACKEND
}

pub(crate) fn set_remote_bf16_route_available(available: bool) {
    REMOTE_ROUTE_AVAILABLE.store(available, Ordering::Release);
}

pub(crate) fn backend_telemetry(capacity_lanes: u32) -> LumenNetBackendTelemetry {
    LumenNetBackendTelemetry {
        protocol_version: PROTOCOL_VERSION,
        caps: CAP_BF16_MATVEC_ROWS | CAP_MODEL_RESIDENT_MATRIX_ID | CAP_ROW_RANGE_OUTPUT_F32,
        capacity_lanes,
        local_workers: crate::lumen::burn_baby::online_worker_count().min(u32::MAX as usize) as u32,
        pending_bf16_matvecs: pending_remote_bf16_matvecs()
            .saturating_add(pending_shadow_bf16_matvecs())
            .min(u32::MAX as usize) as u32,
        min_remote_rows: MIN_REMOTE_ROWS.min(u32::MAX as usize) as u32,
    }
}

fn encode_bf16_matvec_header(job: &RemoteBf16MatvecJob) -> LumenNetFrameHeader {
    LumenNetFrameHeader {
        magic: FRAME_MAGIC,
        version: PROTOCOL_VERSION,
        opcode: OP_BF16_MATVEC_ROWS,
        job_id: job.job_id,
        matrix_id: job.matrix_id,
        row_start: job.row_start as u64,
        row_count: job.row_end.saturating_sub(job.row_start) as u64,
        n_rows: job.n_rows as u64,
        k_dim: job.k_dim as u64,
        x_bytes: job.x_len.saturating_mul(core::mem::size_of::<f32>()) as u64,
        output_bytes: job
            .row_end
            .saturating_sub(job.row_start)
            .saturating_mul(core::mem::size_of::<f32>()) as u64,
    }
}

fn shadow_x_chunk_bytes() -> usize {
    crate::allcaps::lumen::NET_BF16_MATVEC_SHADOW_X_CHUNK_BYTES.max(1)
}

fn append_hex(out: &mut Vec<u8>, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize]);
        out.push(HEX[(byte & 0x0F) as usize]);
    }
}

fn resolve_bf16_matrix(
    data_ptr: usize,
    byte_len: usize,
    rows: usize,
    k_dim: usize,
) -> Option<LumenMatrixManifestEntry> {
    let manifest = MATRIX_MANIFEST.lock();
    manifest.iter().copied().find(|entry| {
        entry.data_ptr == data_ptr
            && entry.byte_len == byte_len
            && entry.rows as usize == rows
            && entry.k_dim as usize == k_dim
    })
}

fn resolve_bf16_matrix_by_id(
    matrix_id: u64,
    rows: usize,
    k_dim: usize,
) -> Option<LumenMatrixManifestEntry> {
    let byte_len = rows.checked_mul(k_dim)?.checked_mul(2)?;
    let manifest = MATRIX_MANIFEST.lock();
    manifest.iter().copied().find(|entry| {
        entry.matrix_id == matrix_id
            && entry.byte_len == byte_len
            && entry.rows as usize == rows
            && entry.k_dim as usize == k_dim
    })
}

fn bf16_to_f32(bits: u16) -> f32 {
    f32::from_bits((bits as u32) << 16)
}

fn stable_name_hash(name: &str) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for byte in name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

fn dtype_code(dtype: &str) -> u16 {
    match dtype {
        "BF16" => 1,
        "F16" => 2,
        "F32" => 3,
        "I8" => 4,
        _ => 0,
    }
}

fn stable_matrix_id(name_hash: u64, rows: usize, k_dim: usize, dtype_code: u16) -> u64 {
    let mut value = 0x4C55_4D45_4E4D_4154u64; // LUMENMAT
    value ^= name_hash;
    value = value.rotate_left(17) ^ rows as u64;
    value = value.rotate_left(17) ^ k_dim as u64;
    value = value.rotate_left(7) ^ dtype_code as u64;
    value
}

fn host_descriptor_cookie(job: &RemoteBf16MatvecJob) -> u64 {
    let mut value = job.x_ptr as u64;
    value ^= (job.x_len as u64).rotate_left(7);
    value ^= (job.w_rowmajor_bf16_ptr as u64).rotate_left(17);
    value ^= (job.w_rowmajor_bf16_len as u64).rotate_left(23);
    value ^= (job.out_ptr as u64).rotate_left(31);
    value ^= (job.out_len as u64).rotate_left(43);
    value
}
