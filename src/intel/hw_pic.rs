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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum HwPicCodec {
    Jpeg,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum HwPicStatus {
    Pending,
    Ready,
    Failed,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum HwPicPixelFormat {
    Nv12,
    Rgba8,
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

pub(crate) fn take_output() -> Option<HwPicOutput> {
    OUTPUTS.lock().pop_front()
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
        crate::log!("intel/hw_pic: duplicate service task entered; parking\n");
        loop {
            embassy_time::Timer::after_secs(3600).await;
        }
    }
    crate::log!("intel/hw_pic: service started backend=media-vdbox\n");
    hw_pic_service_inner().await;
}

async fn hw_pic_service_inner() {
    loop {
        let Some(job) = take_job() else {
            WAIT.wait_for_event().await;
            continue;
        };
        let output = process_job(job);
        crate::log!(
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
    }
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

fn process_jpeg_job(job: HwPicJob) -> HwPicOutput {
    let Some(dev) = super::claimed_device() else {
        return failed_output(&job, -2);
    };
    let (engine, windows) = super::xelp_media2_ngin_hw_pic::default_decode_engine_and_window();
    let Some(backing) = super::xelp_media2_ngin_hw_pic::ensure_decode_backing(dev, windows) else {
        return failed_output(&job, -5);
    };
    if job.encoded.len() > backing.bitstream_bytes {
        return failed_output(&job, -12);
    }

    let Some(proof) = super::xelp_media2_ngin_hw_pic::stream_encoded_to_bitstream(
        dev,
        engine,
        windows,
        backing,
        job.encoded.as_slice(),
    ) else {
        return failed_output(&job, -6);
    };

    crate::log!(
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

    let Some(smoke) = super::xelp_media2_ngin_hw_pic::submit_jpeg_smoke_batch(
        dev,
        engine,
        windows,
        backing,
        proof.bytes_written,
        job.id,
    ) else {
        return failed_output(&job, -7);
    };

    crate::log!(
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
    let output_ready = retired && smoke.output_surface_detail;
    HwPicOutput {
        id: job.id,
        codec: job.codec,
        status: if output_ready {
            HwPicStatus::Ready
        } else {
            HwPicStatus::Failed
        },
        format: if retired {
            HwPicPixelFormat::Nv12
        } else {
            HwPicPixelFormat::Unknown
        },
        width: if retired { smoke.coded_width } else { 0 },
        height: if retired { smoke.coded_height } else { 0 },
        pitch_bytes: if retired { smoke.output_surface_pitch } else { 0 },
        byte_len: if retired {
            smoke.output_surface_bytes
        } else {
            job.encoded.len()
        },
        gpu_addr: windows.output_surface_gpu_addr,
        phys_addr: backing.output_surface_phys,
        virt_addr: backing.output_surface_virt as usize,
        error_code: if output_ready {
            0
        } else if retired {
            -14
        } else {
            -13
        },
    }
}
