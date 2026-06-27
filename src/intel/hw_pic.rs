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
static AVC_DPB: Mutex<AvcDpbState> = Mutex::new(AvcDpbState::new());

const AVC_DPB_RETAINED_REFS: usize = 3;

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
    Nv12,
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
    pub visible_width: u32,
    pub visible_height: u32,
    pub pitch_bytes: usize,
    pub uv_offset: usize,
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

#[derive(Copy, Clone, Debug)]
struct AvcDpbEntry {
    slot: usize,
    frame_store_id: u8,
    frame_num: u16,
    top_field_order_cnt: i32,
    bottom_field_order_cnt: i32,
}

#[derive(Copy, Clone, Debug)]
struct AvcDpbState {
    entries: [Option<AvcDpbEntry>; AVC_DPB_RETAINED_REFS],
}

impl AvcDpbState {
    const fn new() -> Self {
        Self {
            entries: [None; AVC_DPB_RETAINED_REFS],
        }
    }

    fn reset(&mut self) {
        self.entries = [None; AVC_DPB_RETAINED_REFS];
    }

    fn live_count(&self) -> usize {
        let mut count = 0usize;
        for entry in self.entries {
            if entry.is_some() {
                count += 1;
            }
        }
        count
    }

    fn contains_slot(&self, slot: usize) -> bool {
        self.entries
            .iter()
            .flatten()
            .any(|entry| entry.slot == slot)
    }

    fn newest_refs(&self) -> [Option<AvcDpbEntry>; AVC_DPB_RETAINED_REFS] {
        let mut refs = self.entries;
        let mut i = 0usize;
        while i < refs.len() {
            let mut j = i + 1;
            while j < refs.len() {
                if avc_dpb_entry_newer(refs[j], refs[i]) {
                    refs.swap(i, j);
                }
                j += 1;
            }
            i += 1;
        }
        refs
    }

    fn insert_decoded_ref(&mut self, entry: AvcDpbEntry) {
        for existing in &mut self.entries {
            if existing
                .map(|old| old.frame_store_id == entry.frame_store_id)
                .unwrap_or(false)
            {
                *existing = Some(entry);
                return;
            }
        }
        if let Some(empty) = self.entries.iter_mut().find(|slot| slot.is_none()) {
            *empty = Some(entry);
            return;
        }
        let mut oldest_idx = 0usize;
        for idx in 1..self.entries.len() {
            if avc_dpb_entry_older(self.entries[idx], self.entries[oldest_idx]) {
                oldest_idx = idx;
            }
        }
        self.entries[oldest_idx] = Some(entry);
    }
}

fn avc_dpb_entry_newer(lhs: Option<AvcDpbEntry>, rhs: Option<AvcDpbEntry>) -> bool {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => lhs.frame_num > rhs.frame_num,
        (Some(_), None) => true,
        _ => false,
    }
}

fn avc_dpb_entry_older(lhs: Option<AvcDpbEntry>, rhs: Option<AvcDpbEntry>) -> bool {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => lhs.frame_num < rhs.frame_num,
        (Some(_), None) => false,
        (None, Some(_)) => true,
        (None, None) => false,
    }
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
            "intel/hw_pic: output id={} codec={:?} status={:?} fmt={:?} size={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} virt=0x{:X} err={}\n",
            output.id,
            output.codec,
            output.status,
            output.format,
            output.width,
            output.height,
            output.visible_width,
            output.visible_height,
            output.pitch_bytes,
            output.uv_offset,
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
        HwPicCodec::H264 => process_h264_job(job),
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
        visible_width: 0,
        visible_height: 0,
        pitch_bytes: 0,
        uv_offset: 0,
        byte_len: job.encoded.len(),
        gpu_addr: 0,
        phys_addr: 0,
        virt_addr: 0,
        error_code: code,
    }
}

fn align_up_usize(value: usize, align: usize) -> usize {
    if align == 0 {
        return value;
    }
    (value + align - 1) & !(align - 1)
}

#[derive(Copy, Clone, Debug)]
struct AvcDpbProbeLayout {
    slot_bytes: usize,
    slot_count: usize,
    current_slot: usize,
    reference_slots: usize,
    current_gpu_addr: u64,
    first_reference_gpu_addr: u64,
    capacity_bytes: usize,
}

fn avc_dpb_probe_layout(
    output_gpu_addr: u64,
    output_capacity_bytes: usize,
    surface_bytes: usize,
) -> Option<AvcDpbProbeLayout> {
    if surface_bytes == 0 || surface_bytes > output_capacity_bytes {
        return None;
    }
    let slot_bytes = align_up_usize(
        surface_bytes,
        crate::intel::xelp_media_avc_decode_recipe::MFX_GENERAL_STATE_ALIGNMENT as usize,
    );
    if slot_bytes == 0 || slot_bytes > output_capacity_bytes {
        return None;
    }
    let slot_count = output_capacity_bytes / slot_bytes;
    if slot_count == 0 {
        return None;
    }
    let reference_slots = slot_count.saturating_sub(1).min(15);
    Some(AvcDpbProbeLayout {
        slot_bytes,
        slot_count,
        current_slot: 0,
        reference_slots,
        current_gpu_addr: output_gpu_addr,
        first_reference_gpu_addr: output_gpu_addr.saturating_add(slot_bytes as u64),
        capacity_bytes: output_capacity_bytes,
    })
}

fn avc_prepare_reference_state(
    plan: &mut crate::intel::xelp_media_avc_decode_recipe::AvcLongFormatIdrPlan,
    layout: AvcDpbProbeLayout,
    output_gpu_addr: u64,
) -> Result<
    (
        usize,
        crate::intel::xelp_media_avc_decode_recipe::AvcReferenceState,
        [crate::intel::xelp_media_avc_decode_recipe::AvcGpuResourceRange; 16],
        usize,
    ),
    i32,
> {
    use crate::intel::xelp_media_avc_decode_recipe::{
        AvcGpuResourceRange, AvcReferenceFrameBinding, AvcReferenceState, AvcSliceClass,
    };

    if layout.slot_count < 4 {
        return Err(-29);
    }

    let mut dpb = AVC_DPB.lock();
    if plan.slice.class == AvcSliceClass::I {
        dpb.reset();
    }

    let active_l0 = if plan.slice.class == AvcSliceClass::P {
        usize::from(plan.slice.num_ref_idx_l0_active_minus1) + 1
    } else {
        0
    };
    let newest_refs = dpb.newest_refs();
    let live_count = dpb.live_count();
    if active_l0 > live_count {
        return Err(-30);
    }

    let current_slot = if plan.slice.class == AvcSliceClass::I {
        0
    } else {
        let mut free = None;
        let mut slot = 0usize;
        while slot < layout.slot_count.min(16) {
            if !dpb.contains_slot(slot) {
                free = Some(slot);
                break;
            }
            slot += 1;
        }
        free.ok_or(-31)?
    };

    let dummy = AvcGpuResourceRange {
        gpu_addr: output_gpu_addr.saturating_add((current_slot * layout.slot_bytes) as u64),
        bytes: plan.resources.dest_surface.byte_len,
    };
    let mut reference_surfaces = [dummy; 16];
    let mut refs = [None; 16];
    for entry in dpb.entries.iter().flatten() {
        let frame_store_id = entry.frame_store_id as usize;
        if frame_store_id < 16 {
            let surface_gpu_addr =
                output_gpu_addr.saturating_add((entry.slot * layout.slot_bytes) as u64);
            reference_surfaces[frame_store_id] = AvcGpuResourceRange {
                gpu_addr: surface_gpu_addr,
                bytes: plan.resources.dest_surface.byte_len,
            };
            refs[frame_store_id] = Some(AvcReferenceFrameBinding {
                frame_store_id: entry.frame_store_id,
                frame_num: entry.frame_num,
                top_field_order_cnt: entry.top_field_order_cnt,
                bottom_field_order_cnt: entry.bottom_field_order_cnt,
                surface_gpu_addr,
                dmv_gpu_addr: 0,
            });
        }
    }

    let mut l0 = [0u8; 16];
    let mut idx = 0usize;
    while idx < active_l0 {
        let Some(entry) = newest_refs[idx] else {
            return Err(-32);
        };
        l0[idx] = entry.frame_store_id;
        idx += 1;
    }

    plan.resources.reference_surface_count = live_count;
    Ok((
        current_slot,
        AvcReferenceState {
            refs,
            ref_count: live_count,
            l0,
            l0_count: active_l0,
        },
        reference_surfaces,
        live_count,
    ))
}

fn avc_commit_decoded_reference(
    plan: crate::intel::xelp_media_avc_decode_recipe::AvcLongFormatIdrPlan,
    current_slot: usize,
) {
    if !plan.picture.reference_pic {
        return;
    }
    AVC_DPB.lock().insert_decoded_ref(AvcDpbEntry {
        slot: current_slot,
        frame_store_id: current_slot as u8,
        frame_num: plan.picture.frame_num,
        top_field_order_cnt: plan.picture.top_field_order_cnt,
        bottom_field_order_cnt: plan.picture.bottom_field_order_cnt,
    });
}

fn avc_scratch_bindings(
    plan: crate::intel::xelp_media_avc_decode_recipe::AvcLongFormatIdrPlan,
    dest_gpu_addr: u64,
    dest_bytes: usize,
    missing_reference_gpu_addr: u64,
    missing_reference_bytes: usize,
    reference_surfaces: [crate::intel::xelp_media_avc_decode_recipe::AvcGpuResourceRange; 16],
    bitstream_gpu_addr: u64,
    bitstream_bytes: usize,
    scratch_gpu_addr: u64,
    scratch_bytes: usize,
) -> crate::intel::xelp_media_avc_decode_recipe::AvcPacketResourceBindings {
    use crate::intel::xelp_media_avc_decode_recipe::{
        AvcGpuResourceRange, MFX_GENERAL_STATE_ALIGNMENT,
    };

    let align = MFX_GENERAL_STATE_ALIGNMENT as usize;
    let mut next = scratch_gpu_addr;
    let scratch_end = scratch_gpu_addr.saturating_add(scratch_bytes as u64);
    let intra = AvcGpuResourceRange {
        gpu_addr: next,
        bytes: plan.resources.rowstore.intra,
    };
    next += align_up_usize(intra.bytes, align) as u64;
    let deblocking_filter = AvcGpuResourceRange {
        gpu_addr: next,
        bytes: plan.resources.rowstore.deblocking_filter,
    };
    next += align_up_usize(deblocking_filter.bytes, align) as u64;
    let bsd_mpc = AvcGpuResourceRange {
        gpu_addr: next,
        bytes: plan.resources.rowstore.bsd_mpc,
    };
    next += align_up_usize(bsd_mpc.bytes, align) as u64;
    let mpr = AvcGpuResourceRange {
        gpu_addr: next,
        bytes: plan.resources.rowstore.mpr,
    };
    next += align_up_usize(mpr.bytes, align) as u64;
    let dmv_write = AvcGpuResourceRange {
        gpu_addr: next,
        bytes: plan.resources.dmv_write_buffer_bytes,
    };
    next += align_up_usize(dmv_write.bytes, align) as u64;
    let dmv_reference = AvcGpuResourceRange {
        gpu_addr: next,
        bytes: plan.resources.dmv_reference_buffer_bytes,
    };
    let required_end = dmv_reference
        .gpu_addr
        .saturating_add(dmv_reference.bytes as u64);
    let (dmv_write, dmv_reference) = if required_end <= scratch_end {
        (dmv_write, dmv_reference)
    } else {
        let invalid = AvcGpuResourceRange {
            gpu_addr: scratch_gpu_addr,
            bytes: 0,
        };
        (invalid, invalid)
    };

    crate::intel::xelp_media_avc_decode_recipe::AvcPacketResourceBindings {
        dest_surface: AvcGpuResourceRange {
            gpu_addr: dest_gpu_addr,
            bytes: dest_bytes,
        },
        missing_reference_surface: AvcGpuResourceRange {
            gpu_addr: missing_reference_gpu_addr,
            bytes: missing_reference_bytes,
        },
        reference_surfaces,
        bitstream: AvcGpuResourceRange {
            gpu_addr: bitstream_gpu_addr,
            bytes: bitstream_bytes,
        },
        intra_rowstore: intra,
        deblocking_filter_rowstore: deblocking_filter,
        bsd_mpc_rowstore: bsd_mpc,
        mpr_rowstore: mpr,
        dmv_write_buffer: dmv_write,
        dmv_reference_buffer: dmv_reference,
    }
}

fn process_h264_job(job: HwPicJob) -> HwPicOutput {
    use crate::intel::xelp_media_avc_decode_recipe::{
        AVC_CMD_OFFSET_AVC_BSD_OBJECT, AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE,
        AVC_CMD_OFFSET_AVC_IMG_STATE, AVC_CMD_OFFSET_AVC_PICID_STATE,
        AVC_CMD_OFFSET_AVC_QM_INTRA_4X4_STATE, AVC_CMD_OFFSET_AVC_REF_IDX_STATE,
        AVC_CMD_OFFSET_AVC_SLICE_STATE, AVC_CMD_OFFSET_BSP_BUF_BASE_ADDR_STATE,
        AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE, AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE,
        AVC_CMD_OFFSET_PIPE_MODE, AVC_CMD_OFFSET_SURFACE_STATE, MFX_AVC_DMV_DEST_BOTTOM,
        MFX_AVC_DMV_DEST_TOP, build_long_format_single_i_or_p_command_stream,
        parse_annexb_single_i_or_p_plan,
    };

    log_stage(job.id, "job-start", true, "codec=h264 stage=avc-single-i-or-p-live", 0);

    let mut plan = match parse_annexb_single_i_or_p_plan(job.encoded.as_slice()) {
        Ok(plan) => plan,
        Err(err) => {
            hw_pic_info!(
                "intel/hw_pic-stage: id={} stage=avc-parse accepted=0 code=-20 err={:?} bytes=0x{:X}\n",
                job.id,
                err,
                job.encoded.len()
            );
            return failed_output(&job, -20);
        }
    };

    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=avc-parse accepted=1 milestone=long-format-single-i-or-p class={:?} frame_num={} poc={}/{} refs_l0={} coded={}x{} visible={}x{} mb={}x{} bitstream=0x{:X} slice=0x{:X}+0x{:X} payload_bit={} first_mb_byte={} first_mb_bit={} qp_delta={} entropy={} transform8x8={}\n",
        job.id,
        plan.slice.class,
        plan.picture.frame_num,
        plan.picture.top_field_order_cnt,
        plan.picture.bottom_field_order_cnt,
        plan.slice.num_ref_idx_l0_active_minus1.saturating_add(1),
        plan.picture.coded_width(),
        plan.picture.coded_height(),
        plan.picture.visible_width(),
        plan.picture.visible_height(),
        plan.picture.pic_width_in_mbs(),
        plan.picture.pic_height_in_mbs(),
        plan.bitstream_bytes,
        plan.slice.slice_data_offset,
        plan.slice.slice_data_size,
        plan.slice.slice_data_bit_offset_from_payload,
        plan.slice.first_mb_byte_offset,
        plan.slice.slice_data_bit_offset,
        plan.slice.slice_qp_delta,
        plan.picture.entropy_coding_mode as u8,
        plan.picture.transform_8x8 as u8
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=avc-resources accepted=1 surface={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X} rowstore intra=0x{:X} deblock=0x{:X} bsd_mpc=0x{:X} mpr=0x{:X} dmv_write=0x{:X} dmv_ref=0x{:X} refs={}\n",
        job.id,
        plan.resources.dest_surface.width,
        plan.resources.dest_surface.height,
        plan.resources.dest_surface.pitch_bytes,
        plan.resources.dest_surface.uv_offset,
        plan.resources.dest_surface.byte_len,
        plan.resources.rowstore.intra,
        plan.resources.rowstore.deblocking_filter,
        plan.resources.rowstore.bsd_mpc,
        plan.resources.rowstore.mpr,
        plan.resources.dmv_write_buffer_bytes,
        plan.resources.dmv_reference_buffer_bytes,
        plan.resources.reference_surface_count
    );

    let Some(dev) = super::claimed_device() else {
        log_stage(job.id, "device", false, "claimed_device=none", -2);
        return failed_output(&job, -2);
    };
    log_stage(job.id, "device", true, "claimed_device=ok", 0);

    let (engine, windows) = super::xelp_media2_ngin_hw_pic::default_decode_engine_and_window();
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=route accepted=1 codec=h264 engine={} bitstream_gpu=0x{:X} output_gpu=0x{:X} result_gpu=0x{:X}\n",
        job.id,
        engine.name,
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
    if plan.resources.dest_surface.byte_len > backing.output_surface_bytes {
        log_stage(job.id, "surface", false, "planned-surface-larger-than-backing", -22);
        return failed_output(&job, -22);
    }
    let Some(dpb_layout) = avc_dpb_probe_layout(
        windows.output_surface_gpu_addr,
        backing.output_surface_bytes,
        plan.resources.dest_surface.byte_len,
    ) else {
        hw_pic_info!(
            "intel/hw_pic-stage: id={} stage=avc-dpb-layout accepted=0 code=-28 surface_bytes=0x{:X} capacity=0x{:X}\n",
            job.id,
            plan.resources.dest_surface.byte_len,
            backing.output_surface_bytes
        );
        return failed_output(&job, -28);
    };
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=avc-dpb-layout accepted=1 current_slot={} ref_slots={} total_slots={} slot_bytes=0x{:X} capacity=0x{:X} current_gpu=0x{:X} first_ref_gpu=0x{:X} policy=current-plus-three-short-refs\n",
        job.id,
        dpb_layout.current_slot,
        dpb_layout.reference_slots,
        dpb_layout.slot_count,
        dpb_layout.slot_bytes,
        dpb_layout.capacity_bytes,
        dpb_layout.current_gpu_addr,
        dpb_layout.first_reference_gpu_addr
    );
    let (current_slot, references, reference_surfaces, live_refs) =
        match avc_prepare_reference_state(&mut plan, dpb_layout, windows.output_surface_gpu_addr) {
            Ok(prepared) => prepared,
            Err(code) => {
                hw_pic_info!(
                    "intel/hw_pic-stage: id={} stage=avc-dpb-prepare accepted=0 code={} class={:?} frame_num={} poc={}/{} active_l0={} live_refs={}\n",
                    job.id,
                    code,
                    plan.slice.class,
                    plan.picture.frame_num,
                    plan.picture.top_field_order_cnt,
                    plan.picture.bottom_field_order_cnt,
                    plan.slice.num_ref_idx_l0_active_minus1.saturating_add(1),
                    AVC_DPB.lock().live_count()
                );
                return failed_output(&job, code);
            }
        };
    let output_slot_offset = current_slot.saturating_mul(dpb_layout.slot_bytes);
    let output_gpu_addr = windows
        .output_surface_gpu_addr
        .saturating_add(output_slot_offset as u64);
    let output_phys_addr = backing
        .output_surface_phys
        .saturating_add(output_slot_offset as u64);
    let output_virt_addr = unsafe { backing.output_surface_virt.add(output_slot_offset) };
    let missing_ref_offset = output_slot_offset;
    let missing_ref_required =
        missing_ref_offset.saturating_add(plan.resources.dest_surface.byte_len);
    if missing_ref_required > backing.output_surface_bytes {
        hw_pic_info!(
            "intel/hw_pic-stage: id={} stage=avc-dummy-ref accepted=0 code=-27 offset=0x{:X} bytes=0x{:X} capacity=0x{:X}\n",
            job.id,
            missing_ref_offset,
            plan.resources.dest_surface.byte_len,
            backing.output_surface_bytes
        );
        return failed_output(&job, -27);
    }
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=avc-dpb-prepare accepted=1 class={:?} frame_num={} poc={}/{} current_slot={} output_offset=0x{:X} output_gpu=0x{:X} live_refs={} active_l0={} l0=[{}, {}, {}] ref_frames={}\n",
        job.id,
        plan.slice.class,
        plan.picture.frame_num,
        plan.picture.top_field_order_cnt,
        plan.picture.bottom_field_order_cnt,
        current_slot,
        output_slot_offset,
        output_gpu_addr,
        live_refs,
        references.l0_count,
        references.l0[0],
        references.l0[1],
        references.l0[2],
        plan.resources.reference_surface_count
    );
    let avc_scratch_required = align_up_usize(
        plan.resources.rowstore.intra,
        crate::intel::xelp_media_avc_decode_recipe::MFX_GENERAL_STATE_ALIGNMENT as usize,
    )
    .saturating_add(align_up_usize(
        plan.resources.rowstore.deblocking_filter,
        crate::intel::xelp_media_avc_decode_recipe::MFX_GENERAL_STATE_ALIGNMENT as usize,
    ))
    .saturating_add(align_up_usize(
        plan.resources.rowstore.bsd_mpc,
        crate::intel::xelp_media_avc_decode_recipe::MFX_GENERAL_STATE_ALIGNMENT as usize,
    ))
    .saturating_add(align_up_usize(
        plan.resources.rowstore.mpr,
        crate::intel::xelp_media_avc_decode_recipe::MFX_GENERAL_STATE_ALIGNMENT as usize,
    ))
    .saturating_add(align_up_usize(
        plan.resources.dmv_write_buffer_bytes,
        crate::intel::xelp_media_avc_decode_recipe::MFX_GENERAL_STATE_ALIGNMENT as usize,
    ))
    .saturating_add(plan.resources.dmv_reference_buffer_bytes);
    if avc_scratch_required > backing.avc_scratch_bytes {
        hw_pic_info!(
            "intel/hw_pic-stage: id={} stage=avc-scratch accepted=0 code=-23 required=0x{:X} capacity=0x{:X}\n",
            job.id,
            avc_scratch_required,
            backing.avc_scratch_bytes
        );
        return failed_output(&job, -23);
    }
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=avc-scratch accepted=1 gpu=0x{:X} phys=0x{:X} virt=0x{:X} required=0x{:X} capacity=0x{:X}\n",
        job.id,
        windows.avc_scratch_gpu_addr,
        backing.avc_scratch_phys,
        backing.avc_scratch_virt as usize,
        avc_scratch_required,
        backing.avc_scratch_bytes
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
        "intel/hw_pic-stage: id={} stage=stream accepted=1 codec=h264 engine={} bytes=0x{:X}/0x{:X} sig=0x{:08X} bitstream_gpu=0x{:X} fw_engine_awake={} fw_global_ack=0x{:08X}\n",
        job.id,
        proof.engine_name,
        proof.bytes_written,
        proof.capacity,
        proof.signature,
        proof.bitstream_gpu_addr,
        proof.forcewake_engine_awake as u8,
        proof.forcewake_global_ack
    );

    let bindings = avc_scratch_bindings(
        plan,
        output_gpu_addr,
        plan.resources.dest_surface.byte_len,
        output_gpu_addr,
        plan.resources.dest_surface.byte_len,
        reference_surfaces,
        windows.bitstream_gpu_addr,
        backing.bitstream_bytes,
        windows.avc_scratch_gpu_addr,
        backing.avc_scratch_bytes,
    );
    let stream = match build_long_format_single_i_or_p_command_stream(plan, bindings, references) {
        Ok(stream) => stream,
        Err(err) => {
            hw_pic_info!(
                "intel/hw_pic-stage: id={} stage=avc-command-stream accepted=0 code=-21 err={:?}\n",
                job.id,
                err
            );
            return failed_output(&job, -21);
        }
    };
    let command_dword = |offset: usize| stream.dwords.get(offset).copied().unwrap_or(0);
    let command_addr = |offset: usize| {
        u64::from(command_dword(offset)) | (u64::from(command_dword(offset + 1)) << 32)
    };

    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=avc-command-stream accepted=1 submit_ready=1 commands={} dwords={} headers pipe=0x{:08X} surface=0x{:08X} pipebuf=0x{:08X} indobj=0x{:08X} bsp=0x{:08X} picid=0x{:08X} img=0x{:08X} qm0=0x{:08X} direct=0x{:08X} refidx=0x{:08X} slice=0x{:08X} bsd=0x{:08X}\n",
        job.id,
        stream.command_count,
        stream.dwords.len(),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_PIPE_MODE)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_SURFACE_STATE)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_BSP_BUF_BASE_ADDR_STATE)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_AVC_PICID_STATE)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_AVC_IMG_STATE)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_AVC_QM_INTRA_4X4_STATE)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_AVC_REF_IDX_STATE)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_AVC_SLICE_STATE)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_AVC_BSD_OBJECT)
            .copied()
            .unwrap_or(0)
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=avc-bitstream-state accepted=1 pipe_pre=0x{:X}/0x{:08X} pipe_post=0x{:X}/0x{:08X} ind_base=0x{:X} ind_attr=0x{:08X} ind_upper=0x{:X} bsd_len=0x{:X} bsd_start=0x{:X} bsd_dw3=0x{:08X} bsd_dw4=0x{:08X} bsd_dw5=0x{:08X} surface_dw2=0x{:08X} surface_dw3=0x{:08X} surface_y=0x{:08X} surface_uv=0x{:08X}\n",
        job.id,
        command_addr(AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 1),
        command_dword(AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 3),
        command_addr(AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 4),
        command_dword(AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 6),
        command_addr(AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + 1),
        command_dword(AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + 3),
        command_addr(AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + 4),
        command_dword(AVC_CMD_OFFSET_AVC_BSD_OBJECT + 1),
        command_dword(AVC_CMD_OFFSET_AVC_BSD_OBJECT + 2),
        command_dword(AVC_CMD_OFFSET_AVC_BSD_OBJECT + 3),
        command_dword(AVC_CMD_OFFSET_AVC_BSD_OBJECT + 4),
        command_dword(AVC_CMD_OFFSET_AVC_BSD_OBJECT + 5),
        command_dword(AVC_CMD_OFFSET_SURFACE_STATE + 2),
        command_dword(AVC_CMD_OFFSET_SURFACE_STATE + 3),
        command_dword(AVC_CMD_OFFSET_SURFACE_STATE + 4),
        command_dword(AVC_CMD_OFFSET_SURFACE_STATE + 5)
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=avc-profile accepted=1 name=green-rollback clear_y={} clear_uv={} surface=tileys64-dw3-0x48003ff9 pipe=both ind=full-bitstream-window probe=tile64+ytile+linear\n",
        job.id,
        super::xelp_media2_ngin::MEDIA_AVC_ROLLBACK_CLEAR_LUMA,
        super::xelp_media2_ngin::MEDIA_AVC_ROLLBACK_CLEAR_CHROMA
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=avc-bindings accepted=1 synthetic_rowstore=0 nv12_clear_y={} nv12_clear_uv={} dest=0x{:X}/0x{:X} missing_ref=0x{:X}/0x{:X} missing_ref_offset=0x{:X} bitstream=0x{:X}/0x{:X} intra=0x{:X}/0x{:X} deblock=0x{:X}/0x{:X} bsd_mpc=0x{:X}/0x{:X} mpr=0x{:X}/0x{:X} dmv_write=0x{:X}/0x{:X} dmv_ref=0x{:X}/0x{:X}\n",
        job.id,
        super::xelp_media2_ngin::MEDIA_AVC_ROLLBACK_CLEAR_LUMA,
        super::xelp_media2_ngin::MEDIA_AVC_ROLLBACK_CLEAR_CHROMA,
        bindings.dest_surface.gpu_addr,
        bindings.dest_surface.bytes,
        bindings.missing_reference_surface.gpu_addr,
        bindings.missing_reference_surface.bytes,
        missing_ref_offset,
        bindings.bitstream.gpu_addr,
        bindings.bitstream.bytes,
        bindings.intra_rowstore.gpu_addr,
        bindings.intra_rowstore.bytes,
        bindings.deblocking_filter_rowstore.gpu_addr,
        bindings.deblocking_filter_rowstore.bytes,
        bindings.bsd_mpc_rowstore.gpu_addr,
        bindings.bsd_mpc_rowstore.bytes,
        bindings.mpr_rowstore.gpu_addr,
        bindings.mpr_rowstore.bytes,
        bindings.dmv_write_buffer.gpu_addr,
        bindings.dmv_write_buffer.bytes,
        bindings.dmv_reference_buffer.gpu_addr,
        bindings.dmv_reference_buffer.bytes
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=avc-ref-state accepted=1 picid_dw1=0x{:08X} picid_list0=0x{:08X} pipe_ref0=0x{:X} pipe_ref15=0x{:X} direct_ref0=0x{:X} direct_ref15=0x{:X} direct_attr=0x{:08X} direct_write=0x{:X} direct_write_attr=0x{:08X} poc_top={} poc_bottom={} refidx_dw1=0x{:08X} refidx_entry0=0x{:08X}\n",
        job.id,
        command_dword(AVC_CMD_OFFSET_AVC_PICID_STATE + 1),
        command_dword(AVC_CMD_OFFSET_AVC_PICID_STATE + 2),
        command_addr(AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 19),
        command_addr(AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 49),
        command_addr(AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 1),
        command_addr(AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 31),
        command_dword(AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 33),
        command_addr(AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 34),
        command_dword(AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 36),
        command_dword(AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 37 + MFX_AVC_DMV_DEST_TOP),
        command_dword(AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 37 + MFX_AVC_DMV_DEST_BOTTOM),
        command_dword(AVC_CMD_OFFSET_AVC_REF_IDX_STATE + 1),
        command_dword(AVC_CMD_OFFSET_AVC_REF_IDX_STATE + 2)
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=avc-img-state accepted=1 dw3=0x{:08X} dw4=0x{:08X} dw13=0x{:08X} dw14=0x{:08X} dw15=0x{:08X} active_l0={} active_l1={} ref_frames={}\n",
        job.id,
        stream
            .dwords
            .get(AVC_CMD_OFFSET_AVC_IMG_STATE + 3)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_AVC_IMG_STATE + 4)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_AVC_IMG_STATE + 13)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_AVC_IMG_STATE + 14)
            .copied()
            .unwrap_or(0),
        stream
            .dwords
            .get(AVC_CMD_OFFSET_AVC_IMG_STATE + 15)
            .copied()
            .unwrap_or(0),
        plan.picture.num_ref_idx_l0_active_minus1.saturating_add(1),
        plan.picture.num_ref_idx_l1_active_minus1.saturating_add(1),
        plan.resources.reference_surface_count
    );

    log_stage(job.id, "submit", true, "enter-media-avc-single-i-or-p-batch", 0);
    let Some(avc) = super::xelp_media2_ngin_hw_pic::submit_avc_single_idr_batch(
        dev,
        engine,
        windows,
        backing,
        stream.dwords.as_slice(),
        proof.bytes_written,
        plan.picture.coded_width(),
        plan.picture.coded_height(),
        plan.resources.dest_surface.pitch_bytes,
        plan.resources.dest_surface.byte_len,
        output_slot_offset,
        missing_ref_offset,
        job.id,
    ) else {
        log_stage(job.id, "submit", false, "media-avc-single-i-or-p-batch-failed", -24);
        return failed_output(&job, -24);
    };

    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=submit accepted=1 codec=h264 engine={} retired={} detail={} polls={} coded={}x{} pitch=0x{:X} surface_bytes=0x{:X} command_dwords={} batch_bytes=0x{:X} ring_bytes=0x{:X}\n",
        job.id,
        avc.engine_name,
        avc.retired as u8,
        avc.output_surface_detail as u8,
        avc.poll_iters,
        avc.coded_width,
        avc.coded_height,
        avc.output_surface_pitch,
        avc.output_surface_bytes,
        avc.command_dwords,
        avc.batch_tail_bytes,
        avc.ring_tail_bytes
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=markers accepted={} kickoff=0x{:08X}/0x{:08X} presubmit=0x{:08X}/0x{:08X} postsubmit=0x{:08X}/0x{:08X} complete=0x{:08X}/0x{:08X}\n",
        job.id,
        avc.retired as u8,
        avc.kickoff_value,
        avc.kickoff_marker,
        avc.presubmit_value,
        avc.presubmit_marker,
        avc.postsubmit_value,
        avc.postsubmit_marker,
        avc.complete_value,
        avc.complete_marker
    );
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=engine-regs accepted=1 el=0x{:08X}:0x{:08X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X}:0x{:08X} acthd_region={} acthd_off=0x{:X} acthd_dword=0x{:08X} bbaddr=0x{:08X}:0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} fault=0x{:08X}/0x{:08X} stage_flags=0x{:08X} sig=0x{:08X} nonzero={}\n",
        job.id,
        avc.execlist_status_lo,
        avc.execlist_status_hi,
        avc.ring_head,
        avc.ring_tail,
        avc.ring_acthd_hi,
        avc.ring_acthd,
        avc.acthd_region,
        avc.acthd_offset_bytes,
        avc.acthd_dword,
        avc.bbaddr_hi,
        avc.bbaddr_lo,
        avc.ipeir,
        avc.ipehr,
        avc.fault_gen8,
        avc.fault_gen12,
        avc.stage_flags_value,
        avc.output_surface_signature,
        avc.output_surface_nonzero_samples
    );
    hw_pic_info!(
        "intel/hw_pic: avc-submit id={} engine={} retired={} detail={} batch_gpu=0x{:X} result_gpu=0x{:X} bitstream_gpu=0x{:X} output_gpu=0x{:X} scratch_gpu=0x{:X} bytes=0x{:X} coded={}x{} pitch=0x{:X} surface_bytes=0x{:X} command_dwords={} batch_bytes=0x{:X} ring_bytes=0x{:X} kickoff=0x{:08X}/0x{:08X} presubmit=0x{:08X}/0x{:08X} postsubmit=0x{:08X}/0x{:08X} complete=0x{:08X}/0x{:08X} el=0x{:08X}:0x{:08X} start=0x{:08X} ctl=0x{:08X} hws=0x{:08X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X}:0x{:08X} acthd_region={} acthd_off=0x{:X} acthd_dword=0x{:08X} bbaddr64=0x{:016X} dma_fadd64=0x{:016X} bbstate=0x{:08X} esr=0x{:08X} instps=0x{:08X} psmi_ctl=0x{:08X} nopid=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} fault8=0x{:08X} fault12=0x{:08X} fault8_tlb=0x{:08X}/0x{:08X} fault12_tlb=0x{:08X}/0x{:08X} bitstream_dword0=0x{:08X} sig=0x{:08X} nonzero={}\n",
        job.id,
        avc.engine_name,
        avc.retired as u8,
        avc.output_surface_detail as u8,
        avc.batch_gpu_addr,
        avc.result_gpu_addr,
        avc.bitstream_gpu_addr,
        avc.output_surface_gpu_addr,
        avc.avc_scratch_gpu_addr,
        avc.bitstream_bytes,
        avc.coded_width,
        avc.coded_height,
        avc.output_surface_pitch,
        avc.output_surface_bytes,
        avc.command_dwords,
        avc.batch_tail_bytes,
        avc.ring_tail_bytes,
        avc.kickoff_value,
        avc.kickoff_marker,
        avc.presubmit_value,
        avc.presubmit_marker,
        avc.postsubmit_value,
        avc.postsubmit_marker,
        avc.complete_value,
        avc.complete_marker,
        avc.execlist_status_lo,
        avc.execlist_status_hi,
        avc.ring_start,
        avc.ring_ctl,
        avc.ring_hws_pga,
        avc.ring_head,
        avc.ring_tail,
        avc.ring_acthd_hi,
        avc.ring_acthd,
        avc.acthd_region,
        avc.acthd_offset_bytes,
        avc.acthd_dword,
        ((avc.bbaddr_hi as u64) << 32) | avc.bbaddr_lo as u64,
        ((avc.dma_fadd_hi as u64) << 32) | avc.dma_fadd_lo as u64,
        avc.bbstate,
        avc.esr,
        avc.instps,
        avc.psmi_ctl,
        avc.nopid,
        avc.ipeir,
        avc.ipehr,
        avc.fault_gen8,
        avc.fault_gen12,
        avc.fault_tlb_data0_gen8,
        avc.fault_tlb_data1_gen8,
        avc.fault_tlb_data0_gen12,
        avc.fault_tlb_data1_gen12,
        avc.bitstream_dword0,
        avc.output_surface_signature,
        avc.output_surface_nonzero_samples
    );

    let output_status = if avc.retired && avc.output_surface_detail {
        HwPicStatus::Ready
    } else if avc.retired {
        HwPicStatus::Streamed
    } else {
        HwPicStatus::Failed
    };
    let output_presentable = avc.retired;
    let output_error = if avc.retired {
        if avc.output_surface_detail { 0 } else { -26 }
    } else {
        -25
    };
    hw_pic_info!(
        "intel/hw_pic-stage: id={} stage=classify accepted={} retired={} detail={} status={:?} err={}\n",
        job.id,
        output_presentable as u8,
        avc.retired as u8,
        avc.output_surface_detail as u8,
        output_status,
        output_error
    );
    if output_status == HwPicStatus::Ready {
        avc_commit_decoded_reference(plan, current_slot);
        hw_pic_info!(
            "intel/hw_pic-stage: id={} stage=avc-dpb-commit accepted=1 class={:?} frame_num={} poc={}/{} slot={} retained_refs={}\n",
            job.id,
            plan.slice.class,
            plan.picture.frame_num,
            plan.picture.top_field_order_cnt,
            plan.picture.bottom_field_order_cnt,
            current_slot,
            AVC_DPB.lock().live_count()
        );
    } else {
        hw_pic_info!(
            "intel/hw_pic-stage: id={} stage=avc-dpb-commit accepted=0 class={:?} frame_num={} slot={} reason=decode-not-ready status={:?}\n",
            job.id,
            plan.slice.class,
            plan.picture.frame_num,
            current_slot,
            output_status
        );
    }

    HwPicOutput {
        id: job.id,
        codec: job.codec,
        status: output_status,
        format: HwPicPixelFormat::Nv12,
        width: if avc.retired { avc.coded_width } else { 0 },
        height: if avc.retired { avc.coded_height } else { 0 },
        visible_width: if avc.retired {
            plan.picture.visible_width()
        } else {
            0
        },
        visible_height: if avc.retired {
            plan.picture.visible_height()
        } else {
            0
        },
        pitch_bytes: if avc.retired {
            avc.output_surface_pitch
        } else {
            0
        },
        uv_offset: if avc.retired {
            plan.resources.dest_surface.uv_offset
        } else {
            0
        },
        byte_len: if avc.retired {
            avc.output_surface_bytes
        } else {
            job.encoded.len()
        },
        gpu_addr: output_gpu_addr,
        phys_addr: output_phys_addr,
        virt_addr: output_virt_addr as usize,
        error_code: output_error,
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
        visible_width: if retired { smoke.coded_width } else { 0 },
        visible_height: if retired { smoke.coded_height } else { 0 },
        pitch_bytes: if retired {
            smoke.output_surface_pitch
        } else {
            0
        },
        uv_offset: 0,
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
