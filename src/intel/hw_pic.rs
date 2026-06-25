extern crate alloc;

use alloc::{collections::VecDeque, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use spin::Mutex;

const HW_PIC_PENDING_CAP: usize = 16;
const HW_PIC_OUTPUT_CAP: usize = 32;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);
static SERVICE_STARTED: AtomicBool = AtomicBool::new(false);
static PENDING: Mutex<VecDeque<HwPicJob>> = Mutex::new(VecDeque::new());
static OUTPUTS: Mutex<VecDeque<HwPicOutput>> = Mutex::new(VecDeque::new());
static WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
static OUTPUT_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();

macro_rules! hw_pic_info {
    ($($arg:tt)*) => {
        crate::log_info!(target: "media"; $($arg)*);
    };
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum HwPicCodec {
    Jpeg,
    H264,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum HwPicStatus {
    Ready,
    Streamed,
    Failed,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum HwPicPixelFormat {
    Imc3,
    Unknown,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct HwPicOutput {
    pub id: u32,
    pub codec: HwPicCodec,
    pub status: HwPicStatus,
    pub format: HwPicPixelFormat,
    pub width: u32,
    pub height: u32,
    pub pitch_bytes: usize,
    pub byte_len: usize,
    pub gpu_addr: u64,
    pub phys_addr: u64,
    pub virt_addr: usize,
    pub error_code: i32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct HwPicQueueSnapshot {
    pub pending: usize,
    pub outputs: usize,
    pub service_started: bool,
}

struct HwPicJob {
    id: u32,
    codec: HwPicCodec,
    encoded: Vec<u8>,
}

pub(crate) fn submit_encoded(codec: HwPicCodec, encoded: &[u8]) -> Result<u32, i32> {
    if encoded.is_empty() {
        return Err(-3);
    }

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed).max(1);
    let mut pending = PENDING.lock();
    if pending.len() >= HW_PIC_PENDING_CAP {
        return Err(-11);
    }
    pending.push_back(HwPicJob {
        id,
        codec,
        encoded: encoded.to_vec(),
    });
    drop(pending);

    WAIT.notify_one();
    Ok(id)
}

pub(crate) fn submit_jpeg(encoded: &[u8]) -> Result<u32, i32> {
    submit_encoded(HwPicCodec::Jpeg, encoded)
}

pub(crate) fn submit_h264(encoded: &[u8]) -> Result<u32, i32> {
    submit_encoded(HwPicCodec::H264, encoded)
}

pub(crate) fn output_for_id(id: u32) -> Option<HwPicOutput> {
    let mut outputs = OUTPUTS.lock();
    let pos = outputs.iter().position(|output| output.id == id)?;
    outputs.remove(pos)
}

pub(crate) async fn wait_output_for_id(id: u32, timeout_ms: u64) -> Option<HwPicOutput> {
    loop {
        if let Some(output) = output_for_id(id) {
            return Some(output);
        }
        if timeout_ms == 0 {
            OUTPUT_WAIT.wait_for_event().await;
        } else if !OUTPUT_WAIT.wait_for_event_timeout(timeout_ms).await {
            return None;
        }
    }
}

pub(crate) fn snapshot() -> HwPicQueueSnapshot {
    HwPicQueueSnapshot {
        pending: PENDING.lock().len(),
        outputs: OUTPUTS.lock().len(),
        service_started: SERVICE_STARTED.load(Ordering::Acquire),
    }
}

fn take_job() -> Option<HwPicJob> {
    PENDING.lock().pop_front()
}

fn push_output(output: HwPicOutput) {
    let mut outputs = OUTPUTS.lock();
    while outputs.len() >= HW_PIC_OUTPUT_CAP {
        outputs.pop_front();
    }
    outputs.push_back(output);
    drop(outputs);
    OUTPUT_WAIT.notify_all();
}

#[embassy_executor::task]
pub(crate) async fn hw_pic_service() {
    if SERVICE_STARTED.swap(true, Ordering::AcqRel) {
        hw_pic_info!("intel/hw_pic: duplicate service task entered; parking\n");
        loop {
            embassy_time::Timer::after_secs(3600).await;
        }
    }
    hw_pic_info!("intel/hw_pic: service started backend=media-vdbox\n");
    hw_pic_service_inner().await;
}

async fn hw_pic_service_inner() {
    loop {
        let Some(job) = take_job() else {
            WAIT.wait_for_event().await;
            continue;
        };
        let output = process_job(job);
        hw_pic_info!(
            "intel/hw_pic: output id={} codec={:?} status={:?} fmt={:?} size={}x{} pitch=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} virt=0x{:X} err={}\n",
            output.id,
            output.codec,
            output.status,
            output.format,
            output.width,
            output.height,
            output.pitch_bytes,
            output.byte_len,
            output.gpu_addr,
            output.phys_addr,
            output.virt_addr,
            output.error_code
        );
        push_output(output);
        embassy_time::Timer::after_millis(1).await;
    }
}

fn process_job(job: HwPicJob) -> HwPicOutput {
    match job.codec {
        HwPicCodec::Jpeg => process_jpeg_job(job),
        HwPicCodec::H264 => process_h264_probe_job(job),
    }
}

fn log_stage(id: u32, stage: &str, accepted: bool, detail: &str, code: i32) {
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage={} accepted={} code={} detail={}\n",
        id,
        stage,
        accepted as u8,
        code,
        detail
    );
}

fn failed_output(job: &HwPicJob, code: i32) -> HwPicOutput {
    HwPicOutput {
        id: job.id,
        codec: job.codec,
        status: HwPicStatus::Failed,
        format: HwPicPixelFormat::Unknown,
        width: 0,
        height: 0,
        pitch_bytes: 0,
        byte_len: job.encoded.len(),
        gpu_addr: 0,
        phys_addr: 0,
        virt_addr: 0,
        error_code: code,
    }
}

fn count_annexb_nals(encoded: &[u8]) -> (usize, u32) {
    let mut count = 0usize;
    let mut type_mask = 0u32;
    let mut idx = 0usize;
    while idx + 4 <= encoded.len() {
        let start_len = if encoded[idx..].starts_with(&[0, 0, 1]) {
            3usize
        } else if encoded[idx..].starts_with(&[0, 0, 0, 1]) {
            4usize
        } else {
            idx += 1;
            continue;
        };
        let nal_off = idx + start_len;
        if let Some(&header) = encoded.get(nal_off) {
            let nal_type = header & 0x1F;
            if nal_type < 32 {
                type_mask |= 1u32 << nal_type;
            }
            count += 1;
        }
        idx = nal_off.saturating_add(1);
    }
    (count, type_mask)
}

fn process_h264_probe_job(job: HwPicJob) -> HwPicOutput {
    log_stage(job.id, "job-start", true, "codec=h264", 0);
    let Some(dev) = super::claimed_device() else {
        log_stage(job.id, "device", false, "claimed_device=none", -2);
        return failed_output(&job, -2);
    };
    log_stage(job.id, "device", true, "claimed_device=ok", 0);

    let (engine, windows) = super::xelp_media2_ngin_hw_pic::default_decode_engine_and_window();
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=route accepted=1 codec=h264 engine={} ring_gpu=0x{:X} ctx_gpu=0x{:X} batch_gpu=0x{:X} bitstream_gpu=0x{:X} output_gpu=0x{:X} result_gpu=0x{:X}\n",
        job.id,
        engine.name,
        windows.ring_gpu_addr,
        windows.context_gpu_addr,
        windows.batch_gpu_addr,
        windows.bitstream_gpu_addr,
        windows.output_surface_gpu_addr,
        windows.result_gpu_addr
    );

    let Some(backing) = super::xelp_media2_ngin_hw_pic::ensure_decode_backing(dev, windows) else {
        log_stage(job.id, "backing", false, "alloc-or-map-failed", -5);
        return failed_output(&job, -5);
    };
    if job.encoded.len() > backing.bitstream_bytes {
        log_stage(job.id, "input", false, "encoded-larger-than-bitstream", -12);
        return failed_output(&job, -12);
    }

    let (nal_count, nal_type_mask) = count_annexb_nals(job.encoded.as_slice());
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=input accepted=1 codec=h264 encoded=0x{:X} bitstream_capacity=0x{:X} annexb_nals={} nal_type_mask=0x{:08X}\n",
        job.id,
        job.encoded.len(),
        backing.bitstream_bytes,
        nal_count,
        nal_type_mask
    );

    let Some(proof) = super::xelp_media2_ngin_hw_pic::stream_encoded_to_bitstream(
        dev,
        engine,
        windows,
        backing,
        job.encoded.as_slice(),
    ) else {
        log_stage(job.id, "stream", false, "copy-to-bitstream-failed", -6);
        return failed_output(&job, -6);
    };

    hw_pic_info!(
        "intel/hw_pic: h264 stream-probe id={} engine={} bytes=0x{:X}/0x{:X} annexb_nals={} nal_type_mask=0x{:08X} bitstream_gpu=0x{:X} bitstream_phys=0x{:X} bitstream_virt=0x{:X} sig=0x{:08X} fw_engine_ack_reg=0x{:X} fw_engine_ack=0x{:08X} fw_engine_awake={} fw_global_ack=0x{:08X} fw_awake={}\n",
        job.id,
        proof.engine_name,
        proof.bytes_written,
        proof.capacity,
        nal_count,
        nal_type_mask,
        proof.bitstream_gpu_addr,
        proof.bitstream_phys,
        proof.bitstream_virt,
        proof.signature,
        proof.forcewake_engine_ack_reg,
        proof.forcewake_engine_ack,
        proof.forcewake_engine_awake,
        proof.forcewake_global_ack,
        proof.forcewake_awake_count
    );

    log_stage(job.id, "submit", true, "enter-media-h264-smoke-batch", 0);
    let Some(smoke) = super::xelp_media2_ngin_hw_pic::submit_h264_smoke_batch(
        dev,
        engine,
        windows,
        backing,
        proof.bytes_written,
        job.id,
    ) else {
        log_stage(job.id, "submit", false, "media-h264-smoke-batch-failed", -32);
        return failed_output(&job, -32);
    };

    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=h264-submit accepted={} engine={} retired={} polls={} coded={}x{} pitch=0x{:X} surface_bytes=0x{:X} nals={} nal_mask=0x{:08X} slices={} batch_bytes=0x{:X} ring_bytes=0x{:X}\n",
        job.id,
        smoke.retired as u8,
        smoke.engine_name,
        smoke.retired as u8,
        smoke.poll_iters,
        smoke.coded_width,
        smoke.coded_height,
        smoke.output_surface_pitch,
        smoke.output_surface_bytes,
        smoke.nal_count,
        smoke.nal_type_mask,
        smoke.slice_count,
        smoke.batch_tail_bytes,
        smoke.ring_tail_bytes
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=h264-state accepted=1 sps={} pps={} frame_num={} slice_type={} mb={}x{} slice=0x{:X}+0x{:X} first_mb={}.{} pipe_mode=0x{:08X} surface=0x{:08X}/0x{:08X} img=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} slice_dw=0x{:08X}/0x{:08X}/0x{:08X} bsd=0x{:08X}/0x{:08X}/0x{:08X}\n",
        job.id,
        smoke.sps_id,
        smoke.pps_id,
        smoke.frame_num,
        smoke.slice_type,
        smoke.pic_width_mbs,
        smoke.frame_height_mbs,
        smoke.first_slice_offset,
        smoke.first_slice_len,
        smoke.first_mb_byte_offset,
        smoke.first_mb_bit_offset,
        smoke.pipe_mode_dw1,
        smoke.surface_dw2,
        smoke.surface_dw3,
        smoke.avc_img_dw1,
        smoke.avc_img_dw2,
        smoke.avc_img_dw4,
        smoke.avc_img_dw13,
        smoke.avc_img_dw14,
        smoke.avc_slice_dw1,
        smoke.avc_slice_dw3,
        smoke.avc_slice_dw4,
        smoke.avc_bsd_dw1,
        smoke.avc_bsd_dw2,
        smoke.avc_bsd_dw4
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=h264-markers accepted={} kickoff=0x{:08X}/0x{:08X} presubmit=0x{:08X}/0x{:08X} postsubmit=0x{:08X}/0x{:08X} complete=0x{:08X}/0x{:08X} out_sig=0x{:08X} out_nonzero={}\n",
        job.id,
        smoke.retired as u8,
        smoke.kickoff_value,
        smoke.kickoff_marker,
        smoke.presubmit_value,
        smoke.presubmit_marker,
        smoke.postsubmit_value,
        smoke.postsubmit_marker,
        smoke.complete_value,
        smoke.complete_marker,
        smoke.output_surface_signature,
        smoke.output_surface_nonzero_samples
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=h264-engine-regs accepted=1 el=0x{:08X}:0x{:08X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X}:0x{:08X} acthd_region={} acthd_off=0x{:X} acthd_dw=0x{:08X} bbaddr=0x{:08X}:0x{:08X} dma_fadd=0x{:08X}:0x{:08X} bbstate=0x{:08X} esr=0x{:08X} instps=0x{:08X} psmi=0x{:08X} nopid=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} fault=0x{:08X}/0x{:08X}\n",
        job.id,
        smoke.execlist_status_lo,
        smoke.execlist_status_hi,
        smoke.ring_head,
        smoke.ring_tail,
        smoke.ring_acthd_hi,
        smoke.ring_acthd,
        smoke.acthd_region,
        smoke.acthd_offset_bytes,
        smoke.acthd_dword,
        smoke.bbaddr_hi,
        smoke.bbaddr_lo,
        smoke.dma_fadd_hi,
        smoke.dma_fadd_lo,
        smoke.bbstate,
        smoke.esr,
        smoke.instps,
        smoke.psmi_ctl,
        smoke.nopid,
        smoke.ipeir,
        smoke.ipehr,
        smoke.fault_gen8,
        smoke.fault_gen12
    );

    if !smoke.retired {
        log_stage(job.id, "submit", false, "media-h264-smoke-batch-not-retired", -33);
    }
    HwPicOutput {
        id: job.id,
        codec: job.codec,
        status: if smoke.retired {
            HwPicStatus::Ready
        } else {
            HwPicStatus::Streamed
        },
        format: if smoke.retired {
            HwPicPixelFormat::Imc3
        } else {
            HwPicPixelFormat::Unknown
        },
        width: smoke.coded_width,
        height: smoke.coded_height,
        pitch_bytes: smoke.output_surface_pitch,
        byte_len: smoke.output_surface_bytes,
        gpu_addr: windows.output_surface_gpu_addr,
        phys_addr: backing.output_surface_phys,
        virt_addr: backing.output_surface_virt as usize,
        error_code: if smoke.retired { 0 } else { -33 },
    }
}

fn process_jpeg_job(job: HwPicJob) -> HwPicOutput {
    log_stage(job.id, "job-start", true, "codec=jpeg", 0);
    let Some(dev) = super::claimed_device() else {
        log_stage(job.id, "device", false, "claimed_device=none", -2);
        return failed_output(&job, -2);
    };
    log_stage(job.id, "device", true, "claimed_device=ok", 0);

    let (engine, windows) = super::xelp_media2_ngin_hw_pic::default_decode_engine_and_window();
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=route accepted=1 engine={} ring_gpu=0x{:X} ctx_gpu=0x{:X} batch_gpu=0x{:X} bitstream_gpu=0x{:X} output_gpu=0x{:X} result_gpu=0x{:X}\n",
        job.id,
        engine.name,
        windows.ring_gpu_addr,
        windows.context_gpu_addr,
        windows.batch_gpu_addr,
        windows.bitstream_gpu_addr,
        windows.output_surface_gpu_addr,
        windows.result_gpu_addr
    );

    let Some(backing) = super::xelp_media2_ngin_hw_pic::ensure_decode_backing(dev, windows) else {
        log_stage(job.id, "backing", false, "alloc-or-map-failed", -5);
        return failed_output(&job, -5);
    };
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=backing accepted=1 ring=0x{:X}/0x{:X} ctx=0x{:X}/0x{:X} batch=0x{:X}/0x{:X} bitstream=0x{:X}/0x{:X} output=0x{:X}/0x{:X} result=0x{:X}/0x{:X}\n",
        job.id,
        backing.ring_phys,
        backing.ring_bytes,
        backing.context_phys,
        backing.context_bytes,
        backing.batch_phys,
        backing.batch_bytes,
        backing.bitstream_phys,
        backing.bitstream_bytes,
        backing.output_surface_phys,
        backing.output_surface_bytes,
        backing.result_phys,
        backing.result_bytes
    );

    if job.encoded.len() > backing.bitstream_bytes {
        log_stage(job.id, "input", false, "encoded-larger-than-bitstream", -12);
        return failed_output(&job, -12);
    }
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=input accepted=1 encoded=0x{:X} bitstream_capacity=0x{:X}\n",
        job.id,
        job.encoded.len(),
        backing.bitstream_bytes
    );

    let Some(proof) = super::xelp_media2_ngin_hw_pic::stream_encoded_to_bitstream(
        dev,
        engine,
        windows,
        backing,
        job.encoded.as_slice(),
    ) else {
        log_stage(job.id, "stream", false, "copy-to-bitstream-failed", -6);
        return failed_output(&job, -6);
    };

    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=stream accepted=1 engine={} bytes=0x{:X} capacity=0x{:X} sig=0x{:08X} bitstream_gpu=0x{:X}\n",
        job.id,
        proof.engine_name,
        proof.bytes_written,
        proof.capacity,
        proof.signature,
        proof.bitstream_gpu_addr
    );
    hw_pic_info!(
        "intel/hw_pic: jpeg encoded-stream id={} engine={} bytes=0x{:X}/0x{:X} bitstream_gpu=0x{:X} bitstream_phys=0x{:X} bitstream_virt=0x{:X} sig=0x{:08X} fw_engine_ack_reg=0x{:X} fw_engine_ack=0x{:08X} fw_engine_awake={} fw_global_ack=0x{:08X} fw_awake={}\n",
        job.id,
        proof.engine_name,
        proof.bytes_written,
        proof.capacity,
        proof.bitstream_gpu_addr,
        proof.bitstream_phys,
        proof.bitstream_virt,
        proof.signature,
        proof.forcewake_engine_ack_reg,
        proof.forcewake_engine_ack,
        proof.forcewake_engine_awake,
        proof.forcewake_global_ack,
        proof.forcewake_awake_count
    );

    log_stage(job.id, "submit", true, "enter-media-jpeg-smoke-batch", 0);
    let Some(smoke) = super::xelp_media2_ngin_hw_pic::submit_jpeg_smoke_batch(
        dev,
        engine,
        windows,
        backing,
        proof.bytes_written,
        job.id,
    ) else {
        log_stage(job.id, "submit", false, "media-jpeg-smoke-batch-failed", -7);
        return failed_output(&job, -7);
    };

    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=submit accepted=1 engine={} retired={} polls={} coded={}x{} pitch=0x{:X} surface_bytes=0x{:X} batch_bytes=0x{:X} ring_bytes=0x{:X}\n",
        job.id,
        smoke.engine_name,
        smoke.retired as u8,
        smoke.poll_iters,
        smoke.coded_width,
        smoke.coded_height,
        smoke.output_surface_pitch,
        smoke.output_surface_bytes,
        smoke.batch_tail_bytes,
        smoke.ring_tail_bytes
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=jpeg-state accepted=1 input={} layout={} output={} components={} interleaved={} dri={} mcu_count={} scan=0x{:X}+0x{:X} bsd_dw4=0x{:08X} pipe_mode=0x{:08X} surface_dw=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} pic_dw=0x{:08X}/0x{:08X} stage_flags=0x{:08X}\n",
        job.id,
        smoke.jpeg_input_format,
        crate::ui3::img::jpeg_layout::JpegSampling::from_mfx_input_format(smoke.jpeg_input_format)
            .as_str(),
        smoke.jpeg_output_format,
        smoke.jpeg_scan_component_count,
        smoke.jpeg_interleaved as u8,
        smoke.jpeg_restart_interval,
        smoke.jpeg_mcu_count,
        smoke.jpeg_scan_data_offset,
        smoke.jpeg_scan_data_length,
        smoke.jpeg_bsd_dw4,
        smoke.pipe_mode_dw1,
        smoke.surface_dw2,
        smoke.surface_dw3,
        smoke.surface_dw4,
        smoke.surface_dw5,
        smoke.jpeg_pic_dw1,
        smoke.jpeg_pic_dw2,
        smoke.stage_flags_value
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=markers accepted={} kickoff=0x{:08X}/0x{:08X} presubmit=0x{:08X}/0x{:08X} postsubmit=0x{:08X}/0x{:08X} complete=0x{:08X}/0x{:08X}\n",
        job.id,
        smoke.retired as u8,
        smoke.kickoff_value,
        smoke.kickoff_marker,
        smoke.presubmit_value,
        smoke.presubmit_marker,
        smoke.postsubmit_value,
        smoke.postsubmit_marker,
        smoke.complete_value,
        smoke.complete_marker
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=engine-regs accepted=1 el=0x{:08X}:0x{:08X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X}:0x{:08X} bbaddr=0x{:08X}:0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} fault=0x{:08X}/0x{:08X}\n",
        job.id,
        smoke.execlist_status_lo,
        smoke.execlist_status_hi,
        smoke.ring_head,
        smoke.ring_tail,
        smoke.ring_acthd_hi,
        smoke.ring_acthd,
        smoke.bbaddr_hi,
        smoke.bbaddr_lo,
        smoke.ipeir,
        smoke.ipehr,
        smoke.fault_gen8,
        smoke.fault_gen12
    );
    hw_pic_info!(
        "intel/hw_pic: jpeg smoke-submit id={} engine={} retired={} polls={} batch_gpu=0x{:X} result_gpu=0x{:X} bitstream_gpu=0x{:X} output_gpu=0x{:X} bytes=0x{:X} coded={}x{} jpeg_in={} jpeg_out={} scan_components={} interleaved={} dri={} mcu_count={} surface=0x{:08X}/0x{:08X} jpeg_pic=0x{:08X}/0x{:08X} batch_bytes=0x{:X} ring_bytes=0x{:X} kickoff=0x{:08X}/0x{:08X} presubmit=0x{:08X}/0x{:08X} postsubmit=0x{:08X}/0x{:08X} complete=0x{:08X}/0x{:08X} stage_flags=0x{:08X} el=0x{:08X}:0x{:08X} start=0x{:08X} ctl=0x{:08X} hws=0x{:08X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} acthd64=0x{:016X} acthd_region={} acthd_off=0x{:X} acthd_dword=0x{:08X} bbaddr64=0x{:016X} dma_fadd64=0x{:016X} bbstate=0x{:08X} esr=0x{:08X} instps=0x{:08X} psmi_ctl=0x{:08X} nopid=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} fault8=0x{:08X} fault12=0x{:08X} fault8_tlb=0x{:08X}/0x{:08X} fault12_tlb=0x{:08X}/0x{:08X} bitstream_dword0=0x{:08X}\n",
        job.id,
        smoke.engine_name,
        smoke.retired as u8,
        smoke.poll_iters,
        smoke.batch_gpu_addr,
        smoke.result_gpu_addr,
        smoke.bitstream_gpu_addr,
        smoke.output_surface_gpu_addr,
        smoke.bitstream_bytes,
        smoke.coded_width,
        smoke.coded_height,
        smoke.jpeg_input_format,
        smoke.jpeg_output_format,
        smoke.jpeg_scan_component_count,
        smoke.jpeg_interleaved as u8,
        smoke.jpeg_restart_interval,
        smoke.jpeg_mcu_count,
        smoke.surface_dw2,
        smoke.surface_dw3,
        smoke.jpeg_pic_dw1,
        smoke.jpeg_pic_dw2,
        smoke.batch_tail_bytes,
        smoke.ring_tail_bytes,
        smoke.kickoff_value,
        smoke.kickoff_marker,
        smoke.presubmit_value,
        smoke.presubmit_marker,
        smoke.postsubmit_value,
        smoke.postsubmit_marker,
        smoke.complete_value,
        smoke.complete_marker,
        smoke.stage_flags_value,
        smoke.execlist_status_lo,
        smoke.execlist_status_hi,
        smoke.ring_start,
        smoke.ring_ctl,
        smoke.ring_hws_pga,
        smoke.ring_head,
        smoke.ring_tail,
        smoke.ring_acthd,
        ((smoke.ring_acthd_hi as u64) << 32) | smoke.ring_acthd as u64,
        smoke.acthd_region,
        smoke.acthd_offset_bytes,
        smoke.acthd_dword,
        ((smoke.bbaddr_hi as u64) << 32) | smoke.bbaddr_lo as u64,
        ((smoke.dma_fadd_hi as u64) << 32) | smoke.dma_fadd_lo as u64,
        smoke.bbstate,
        smoke.esr,
        smoke.instps,
        smoke.psmi_ctl,
        smoke.nopid,
        smoke.ipeir,
        smoke.ipehr,
        smoke.fault_gen8,
        smoke.fault_gen12,
        smoke.fault_tlb_data0_gen8,
        smoke.fault_tlb_data1_gen8,
        smoke.fault_tlb_data0_gen12,
        smoke.fault_tlb_data1_gen12,
        smoke.bitstream_dword0,
    );

    let retired = smoke.retired;
    let output_ready = retired;
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=classify accepted={} retired={} detail={} status={:?} err={}\n",
        job.id,
        output_ready as u8,
        retired as u8,
        smoke.output_surface_detail as u8,
        if output_ready {
            HwPicStatus::Ready
        } else {
            HwPicStatus::Failed
        },
        if output_ready { 0 } else { -13 }
    );
    HwPicOutput {
        id: job.id,
        codec: job.codec,
        status: if output_ready {
            HwPicStatus::Ready
        } else {
            HwPicStatus::Failed
        },
        format: if retired {
            HwPicPixelFormat::Imc3
        } else {
            HwPicPixelFormat::Unknown
        },
        width: if retired { smoke.coded_width } else { 0 },
        height: if retired { smoke.coded_height } else { 0 },
        pitch_bytes: if retired {
            smoke.output_surface_pitch
        } else {
            0
        },
        byte_len: if retired {
            smoke.output_surface_bytes
        } else {
            job.encoded.len()
        },
        gpu_addr: windows.output_surface_gpu_addr,
        phys_addr: backing.output_surface_phys,
        virt_addr: backing.output_surface_virt as usize,
        error_code: if output_ready { 0 } else { -13 },
    }
}
