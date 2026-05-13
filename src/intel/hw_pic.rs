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
    let (_engine, windows) = super::xelp_media2_ngin::default_decode_engine_and_window();
    let Some(backing) = super::xelp_media2_ngin::ensure_decode_backing(dev, windows) else {
        return failed_output(&job, -5);
    };
    if job.encoded.len() > backing.bitstream_bytes {
        return failed_output(&job, -12);
    }

    // First landing patch owns the async object model and proven media backing.
    // The JPEG MFX packet builder will replace this placeholder with a submitted
    // VDBOX decode and publish the decoded surface under the same id.
    HwPicOutput {
        id: job.id,
        codec: job.codec,
        status: HwPicStatus::Failed,
        format: HwPicPixelFormat::Unknown,
        width: 0,
        height: 0,
        pitch_bytes: 0,
        byte_len: job.encoded.len(),
        gpu_addr: windows.output_surface_gpu_addr,
        phys_addr: backing.output_surface_phys,
        virt_addr: backing.output_surface_virt as usize,
        error_code: -8,
    }
}
