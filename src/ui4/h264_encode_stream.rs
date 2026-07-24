//! Resident UI4 scanout-to-Intel-H.264-to-UDP service.
//!
//! After a boot-time hardware proof, each subscriber receives a fresh,
//! deadline-paced ten-second session. Logical D01 scanout is composed in
//! memory, converted to NV12, encoded by Gen12 VDEnc/MFX, and handed directly
//! to the bounded TME1 transport. No filesystem or software-codec stage
//! participates.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};

use embassy_time::{Duration, Timer};

const PROBE_START_DELAY_MS: u64 = 15_000;
const VCS0_PROBE_RETRY_MS: u64 = 50;
const ENCODE_WIDTH: usize = 512;
const ENCODE_HEIGHT: usize = 512;
const ENCODE_LUMA_BYTES: usize = ENCODE_WIDTH * ENCODE_HEIGHT;
const ENCODE_NV12_BYTES: usize = ENCODE_LUMA_BYTES * 3 / 2;
const CADENCE_TOLERANCE_PERCENT: u64 = 5;
// A foreground decode stream owns a session-level VCS0 reservation. Keep the
// boot proof retryable until that stream drains instead of permanently parking
// the encoder after an arbitrary startup window.
const VCS0_PROBE_WAIT_LOG_INTERVAL: usize = 600;

static STATE: AtomicU8 = AtomicU8::new(H264EncodeStreamState::Waiting as u8);
static ENCODE_US: AtomicU64 = AtomicU64::new(0);
static SOURCE_BYTES: AtomicUsize = AtomicUsize::new(0);
static ENCODED_BYTES: AtomicUsize = AtomicUsize::new(0);

#[derive(Default)]
struct LiveEncodeStats {
    frames: usize,
    source_width: u32,
    source_height: u32,
    first_source_fnv1a32: u32,
    last_source_fnv1a32: u32,
    source_changes: usize,
    capture_convert_us: u64,
    capture_convert_max_us: u64,
    encode_us: u64,
    encode_max_us: u64,
    coded_bytes: usize,
    coded_max_bytes: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum H264EncodeStreamState {
    Waiting = 0,
    Encoding = 1,
    Streaming = 2,
    Verified = 3,
    Failed = 4,
}

impl H264EncodeStreamState {
    fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Encoding,
            2 => Self::Streaming,
            3 => Self::Verified,
            4 => Self::Failed,
            _ => Self::Waiting,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct H264EncodeStreamSnapshot {
    pub(crate) state: H264EncodeStreamState,
    pub(crate) encode_us: u64,
    pub(crate) source_bytes: usize,
    pub(crate) encoded_bytes: usize,
}

pub(crate) fn h264_encode_stream_snapshot() -> H264EncodeStreamSnapshot {
    H264EncodeStreamSnapshot {
        state: H264EncodeStreamState::from_raw(STATE.load(Ordering::Acquire)),
        encode_us: ENCODE_US.load(Ordering::Acquire),
        source_bytes: SOURCE_BYTES.load(Ordering::Acquire),
        encoded_bytes: ENCODED_BYTES.load(Ordering::Acquire),
    }
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn ui4_h264_encode_stream_task() {
    let worker = crate::cpu::CpuProfile::current();
    let worker_slot = worker.map(|profile| profile.slot()).unwrap_or(u32::MAX);
    let worker_kind = worker
        .map(|profile| profile.core_kind_name())
        .unwrap_or("unknown");
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: service online carrier=background-worker worker_slot={} worker_kind={} performance_preferred=1 background_slot_min=2 feature=trueos_h264_encode_stream boot_proof=procedural-nv12-hardware-only live_source=ui4-logical-scanout-d01 encode_size={}x{} target_fps=10 backend=gen12-vdenc-mfx output=udp-only live_high_water_cap=1 filesystem_writes=0 software_fallback=0 embedded_probe_asset_bytes=0 udp_protocol=tme1 udp_port={} start_delay_ms={}\n",
        worker_slot,
        worker_kind,
        ENCODE_WIDTH,
        ENCODE_HEIGHT,
        crate::allports::services::MEDIA_ENCODE_UDP_PORT,
        PROBE_START_DELAY_MS,
    );

    Timer::after(Duration::from_millis(PROBE_START_DELAY_MS)).await;

    let mut vcs0_probe = crate::intel::run_media_guc_vcs0_probe_once();
    let mut vcs0_probe_attempts = 1usize;
    while vcs0_probe.state == crate::intel::media::guc_probe::GucVcs0ProbeState::Deferred {
        if vcs0_probe_attempts % VCS0_PROBE_WAIT_LOG_INTERVAL == 0 {
            crate::log_info!(target: "intel/media-encode";
                "intel/media-encode: guc-vcs0-probe waiting state=deferred failure={} attempts={} retry_ms={} action=retry software_fallback=0\n",
                vcs0_probe.failure.name(),
                vcs0_probe_attempts,
                VCS0_PROBE_RETRY_MS,
            );
        }
        Timer::after(Duration::from_millis(VCS0_PROBE_RETRY_MS)).await;
        vcs0_probe = crate::intel::run_media_guc_vcs0_probe_once();
        vcs0_probe_attempts += 1;
    }
    if vcs0_probe.state == crate::intel::media::guc_probe::GucVcs0ProbeState::Passed {
        crate::log_info!(target: "intel/media-encode";
            "intel/media-encode: guc-vcs0-probe accepted=1 engine=vcs0 class=1 instance_mask=0x1 submission_owner=guc serial={} forcewake={} backing={} batch={} context={} registered={} submitted={} retired={} context_destroyed={} hwlrca=0x{:08X}:0x{:08X} markers=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} poll_iters={} elapsed_us={} attempts={} codec_packets=0 encode_claim=0\n",
            vcs0_probe.serial,
            vcs0_probe.forcewake as u8,
            vcs0_probe.backing_ready as u8,
            vcs0_probe.batch_ready as u8,
            vcs0_probe.context_ready as u8,
            vcs0_probe.registered as u8,
            vcs0_probe.submitted as u8,
            vcs0_probe.retired as u8,
            vcs0_probe.context_destroyed as u8,
            vcs0_probe.hwlrca_hi,
            vcs0_probe.hwlrca_lo,
            vcs0_probe.kickoff,
            vcs0_probe.presubmit,
            vcs0_probe.postsubmit,
            vcs0_probe.complete,
            vcs0_probe.poll_iters,
            vcs0_probe.elapsed_us,
            vcs0_probe_attempts,
        );
    } else {
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: guc-vcs0-probe accepted=0 state={:?} failure={} forcewake={} backing={} batch={} context={} registered={} submitted={} retired={} context_destroyed={} serial={} markers=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} poll_iters={} elapsed_us={} attempts={} software_fallback=0 action=park encode_claim=0\n",
            vcs0_probe.state,
            vcs0_probe.failure.name(),
            vcs0_probe.forcewake as u8,
            vcs0_probe.backing_ready as u8,
            vcs0_probe.batch_ready as u8,
            vcs0_probe.context_ready as u8,
            vcs0_probe.registered as u8,
            vcs0_probe.submitted as u8,
            vcs0_probe.retired as u8,
            vcs0_probe.context_destroyed as u8,
            vcs0_probe.serial,
            vcs0_probe.kickoff,
            vcs0_probe.presubmit,
            vcs0_probe.postsubmit,
            vcs0_probe.complete,
            vcs0_probe.poll_iters,
            vcs0_probe.elapsed_us,
            vcs0_probe_attempts,
        );
    }

    if vcs0_probe.state == crate::intel::media::guc_probe::GucVcs0ProbeState::Passed {
        let mut avc_probe = crate::intel::run_media_avc_encode_probe_once();
        let mut avc_probe_attempts = 1usize;
        while avc_probe.state
            == crate::intel::media::avc_encode_probe::AvcEncodeProbeState::Deferred
        {
            if avc_probe_attempts % VCS0_PROBE_WAIT_LOG_INTERVAL == 0 {
                crate::log_info!(target: "intel/media-encode";
                    "intel/media-encode: avc-idr-probe waiting state=deferred failure={} attempts={} retry_ms={} action=retry software_fallback=0\n",
                    avc_probe.failure.name(),
                    avc_probe_attempts,
                    VCS0_PROBE_RETRY_MS,
                );
            }
            Timer::after(Duration::from_millis(VCS0_PROBE_RETRY_MS)).await;
            avc_probe = crate::intel::run_media_avc_encode_probe_once();
            avc_probe_attempts += 1;
        }
        if avc_probe.state == crate::intel::media::avc_encode_probe::AvcEncodeProbeState::Passed {
            crate::log_info!(target: "intel/media-encode";
                "intel/media-encode: avc-idr-probe accepted=1 engine=vcs0 codec_mode=avc-encode submission_owner=guc batch_level=second-level-return terminal_fence=primary-batch-return-marker source=procedural-nv12 source_layout=nv12-linear visible=512x512 pitch=512 source_bytes={} source_fnv1a32=0x{:08X} embedded_probe_asset_bytes=0 backing={} surface_uploaded={} batch={} batch_bytes={} primary_batch_bytes={} ring_bytes={} codec_packets={} bitstream_buffer_bound={} registered={} submitted={} retired={} context_destroyed={} serial={} hwlrca=0x{:08X}:0x{:08X} markers=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} poll_iters={} elapsed_us={} attempts={} coded_output_validated={} frame_bytes_no_excluded_headers={} excluded_header_bytes={} coded_bytes={} coded_fnv1a32=0x{:08X} nal_flags=0b{:04b} mfx_error=0x{:08X} image_status=0x{:08X} slice_bytes={} slices={} bitstream_head={:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X} hardware_encode=1\n",
                avc_probe.source_nv12_bytes,
                avc_probe.source_nv12_fnv1a32,
                avc_probe.backing_ready as u8,
                avc_probe.surface_uploaded as u8,
                avc_probe.batch_ready as u8,
                avc_probe.batch_bytes,
                avc_probe.primary_batch_bytes,
                avc_probe.ring_bytes,
                avc_probe.codec_packets,
                avc_probe.bitstream_buffer_bound as u8,
                avc_probe.registered as u8,
                avc_probe.submitted as u8,
                avc_probe.retired as u8,
                avc_probe.context_destroyed as u8,
                avc_probe.serial,
                avc_probe.hwlrca_hi,
                avc_probe.hwlrca_lo,
                avc_probe.kickoff,
                avc_probe.codec_begin,
                avc_probe.codec_end,
                avc_probe.complete,
                avc_probe.poll_iters,
                avc_probe.elapsed_us,
                avc_probe_attempts,
                avc_probe.coded_output_validated as u8,
                avc_probe.mfc_bitstream_bytecount_frame,
                avc_probe.excluded_header_bytes,
                avc_probe.coded_bytes,
                avc_probe.coded_fnv1a32,
                avc_probe.coded_nal_flags,
                avc_probe.mfx_error,
                avc_probe.mfc_image_status_control,
                avc_probe.mfc_bitstream_bytecount_slice,
                avc_probe.mfc_avc_num_slices,
                avc_probe.bitstream_head[0],
                avc_probe.bitstream_head[1],
                avc_probe.bitstream_head[2],
                avc_probe.bitstream_head[3],
                avc_probe.bitstream_head[4],
                avc_probe.bitstream_head[5],
                avc_probe.bitstream_head[6],
                avc_probe.bitstream_head[7],
            );
        } else {
            crate::log_error!(target: "intel/media-encode";
                "intel/media-encode: avc-idr-probe accepted=0 state={:?} failure={} codec_mode=avc-encode submission_owner=guc batch_level=second-level-return terminal_fence=primary-batch-return-marker forcewake={} backing={} surface_uploaded={} batch={} batch_bytes={} primary_batch_bytes={} ring_bytes={} codec_packets={} bitstream_buffer_bound={} registered={} submitted={} retired={} context_destroyed={} serial={} markers=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} poll_iters={} elapsed_us={} attempts={} coded_output_validated={} frame_bytes_no_excluded_headers={} excluded_header_bytes={} coded_bytes={} coded_fnv1a32=0x{:08X} nal_flags=0b{:04b} mfx_error=0x{:08X} image_status=0x{:08X} slice_bytes={} slices={} bitstream_head={:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X} bitstream_tail={:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X} hardware_encode=0 software_fallback=0 action=park\n",
                avc_probe.state,
                avc_probe.failure.name(),
                avc_probe.forcewake as u8,
                avc_probe.backing_ready as u8,
                avc_probe.surface_uploaded as u8,
                avc_probe.batch_ready as u8,
                avc_probe.batch_bytes,
                avc_probe.primary_batch_bytes,
                avc_probe.ring_bytes,
                avc_probe.codec_packets,
                avc_probe.bitstream_buffer_bound as u8,
                avc_probe.registered as u8,
                avc_probe.submitted as u8,
                avc_probe.retired as u8,
                avc_probe.context_destroyed as u8,
                avc_probe.serial,
                avc_probe.kickoff,
                avc_probe.codec_begin,
                avc_probe.codec_end,
                avc_probe.complete,
                avc_probe.poll_iters,
                avc_probe.elapsed_us,
                avc_probe_attempts,
                avc_probe.coded_output_validated as u8,
                avc_probe.mfc_bitstream_bytecount_frame,
                avc_probe.excluded_header_bytes,
                avc_probe.coded_bytes,
                avc_probe.coded_fnv1a32,
                avc_probe.coded_nal_flags,
                avc_probe.mfx_error,
                avc_probe.mfc_image_status_control,
                avc_probe.mfc_bitstream_bytecount_slice,
                avc_probe.mfc_avc_num_slices,
                avc_probe.bitstream_head[0],
                avc_probe.bitstream_head[1],
                avc_probe.bitstream_head[2],
                avc_probe.bitstream_head[3],
                avc_probe.bitstream_head[4],
                avc_probe.bitstream_head[5],
                avc_probe.bitstream_head[6],
                avc_probe.bitstream_head[7],
                avc_probe.bitstream_head[8],
                avc_probe.bitstream_head[9],
                avc_probe.bitstream_head[10],
                avc_probe.bitstream_head[11],
                avc_probe.bitstream_head[12],
                avc_probe.bitstream_head[13],
                avc_probe.bitstream_head[14],
                avc_probe.bitstream_head[15],
                avc_probe.bitstream_head[16],
                avc_probe.bitstream_head[17],
                avc_probe.bitstream_head[18],
                avc_probe.bitstream_head[19],
            );
            let diag = avc_probe.timeout_diagnostics;
            if diag.valid {
                crate::log_error!(target: "intel/media-encode";
                    "intel/media-encode: avc-idr-timeout engine=vcs0 ring=start:0x{:08X},ctl:0x{:08X},head:0x{:08X},tail:0x{:08X} acthd=0x{:08X}:0x{:08X} acthd_region={} acthd_off=0x{:X} acthd_dword=0x{:08X} bbaddr=0x{:08X}:0x{:08X} dma_fadd=0x{:08X}:0x{:08X} bbstate=0x{:08X} instruction=instdone:0x{:08X},instps:0x{:08X},ipeir:0x{:08X},ipehr:0x{:08X} esr=0x{:08X} psmi_ctl=0x{:08X} nopid=0x{:08X} fault8=0x{:08X} fault12=0x{:08X} fault8_tlb=0x{:08X}/0x{:08X} fault12_tlb=0x{:08X}/0x{:08X}\n",
                    diag.ring_start,
                    diag.ring_ctl,
                    diag.ring_head,
                    diag.ring_tail,
                    diag.ring_acthd_hi,
                    diag.ring_acthd_lo,
                    diag.acthd_region,
                    diag.acthd_offset_bytes,
                    diag.acthd_dword,
                    diag.bbaddr_hi,
                    diag.bbaddr_lo,
                    diag.dma_fadd_hi,
                    diag.dma_fadd_lo,
                    diag.bbstate,
                    diag.instdone,
                    diag.instps,
                    diag.ipeir,
                    diag.ipehr,
                    diag.esr,
                    diag.psmi_ctl,
                    diag.nopid,
                    diag.fault_gen8,
                    diag.fault_gen12,
                    diag.fault_tlb_data0_gen8,
                    diag.fault_tlb_data1_gen8,
                    diag.fault_tlb_data0_gen12,
                    diag.fault_tlb_data1_gen12,
                );
                crate::log_error!(target: "intel/media-encode";
                    "intel/media-encode: avc-idr-timeout codec=mfx_error:0x{:08X},frame_crc:0x{:08X},mb_count:0x{:08X} mfc=frame_bytes:0x{:08X},frame_se_bits:0x{:08X},slice_bytes:0x{:08X},image_mask:0x{:08X},image_control:0x{:08X},qp_count:0x{:08X},slices:0x{:08X} bitstream_head={:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X} mfx_stats_head={:08X}/{:08X}/{:08X}/{:08X} vdenc_stats_head={:08X}/{:08X}/{:08X}/{:08X} slice_size_head={:08X}/{:08X}/{:08X}/{:08X}\n",
                    diag.mfx_error,
                    diag.mfx_frame_crc,
                    diag.mfx_mb_count,
                    diag.mfc_bitstream_bytecount_frame,
                    diag.mfc_bitstream_se_bitcount_frame,
                    diag.mfc_bitstream_bytecount_slice,
                    diag.mfc_image_status_mask,
                    diag.mfc_image_status_control,
                    diag.mfc_qp_status_count,
                    diag.mfc_avc_num_slices,
                    diag.bitstream_head[0],
                    diag.bitstream_head[1],
                    diag.bitstream_head[2],
                    diag.bitstream_head[3],
                    diag.bitstream_head[4],
                    diag.bitstream_head[5],
                    diag.bitstream_head[6],
                    diag.bitstream_head[7],
                    diag.mfx_stats_head[0],
                    diag.mfx_stats_head[1],
                    diag.mfx_stats_head[2],
                    diag.mfx_stats_head[3],
                    diag.vdenc_stats_head[0],
                    diag.vdenc_stats_head[1],
                    diag.vdenc_stats_head[2],
                    diag.vdenc_stats_head[3],
                    diag.slice_size_head[0],
                    diag.slice_size_head[1],
                    diag.slice_size_head[2],
                    diag.slice_size_head[3],
                );
            }
        }
    } else {
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: avc-idr-probe accepted=0 state=deferred failure=guc-vcs0-transport-probe-unavailable surface_uploaded=0 codec_packets=0 submitted=0 retired=0 coded_output_validated=0 hardware_encode=0 software_fallback=0 action=park\n",
        );
    }

    let readiness = crate::intel::media_encode_readiness();
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: hardware-readiness ready={} device_claimed={} vdbox_discovered={} guc_transport_ready={} guc_media_context_wired={} guc_media_transport_probe_passed={} avc_encode_commands_wired={} avc_encode_probe_passed={} coded_bitstream_output_wired={} decode_transport=execlists encode_transport=guc-vcs0 filesystem_writes=0 software_fallback=0\n",
        readiness.ready as u8,
        readiness.device_claimed as u8,
        readiness.vdbox_discovered as u8,
        readiness.guc_transport_ready as u8,
        readiness.guc_media_context_wired as u8,
        readiness.guc_media_transport_probe_passed as u8,
        readiness.avc_encode_commands_wired as u8,
        readiness.avc_encode_probe_passed as u8,
        readiness.coded_bitstream_output_wired as u8,
    );

    if !readiness.ready {
        STATE.store(H264EncodeStreamState::Failed as u8, Ordering::Release);
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: stream rejected stage=hardware-readiness reason=required-gate-failed filesystem_writes=0 software_fallback=0\n",
        );
        park().await;
    }

    let avc_probe = crate::intel::media::avc_encode_probe::snapshot();
    STATE.store(H264EncodeStreamState::Encoding as u8, Ordering::Release);
    let Some(probe_annex_b) = crate::intel::media::avc_encode_probe::take_coded_access_unit()
    else {
        STATE.store(H264EncodeStreamState::Failed as u8, Ordering::Release);
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: stream rejected stage=coded-au-handoff reason=validated-access-unit-unavailable filesystem_writes=0 software_fallback=0\n",
        );
        park().await;
    };
    ENCODE_US.store(avc_probe.elapsed_us, Ordering::Release);
    SOURCE_BYTES.store(avc_probe.source_nv12_bytes, Ordering::Release);
    ENCODED_BYTES.store(probe_annex_b.len(), Ordering::Release);
    drop(probe_annex_b);

    let timestamp = crate::chronos::best_effort_unix_time_seconds();
    let mut stream_session_id = timestamp
        .map(|timestamp| timestamp as u32)
        .unwrap_or_else(|| (crate::time::uptime_seconds() as u32) ^ (vcs0_probe.serial as u32));
    let mut udp_transport = super::h264_encode_udp::MediaUdpTransport::open().await;
    loop {
        STATE.store(H264EncodeStreamState::Streaming as u8, Ordering::Release);
        let mut stats = LiveEncodeStats::default();
        let udp_report = super::h264_encode_udp::stream_generated_annex_b(
            &mut udp_transport,
            stream_session_id,
            crate::allcaps::media_encode::VALIDATION_SESSION_ACCESS_UNITS,
            crate::allcaps::media_encode::REALTIME_HZ,
            |sequence| capture_and_encode_scanout(sequence, &mut stats),
        )
        .await;
        let expected_units = crate::allcaps::media_encode::VALIDATION_SESSION_ACCESS_UNITS;
        let interval_millifps = if udp_report.sent_access_units > 1 && udp_report.elapsed_us != 0 {
            (udp_report.sent_access_units.saturating_sub(1) as u64).saturating_mul(1_000_000_000)
                / udp_report.elapsed_us
        } else {
            0
        };
        let target_millifps = crate::allcaps::media_encode::REALTIME_HZ as u64 * 1_000;
        let cadence_tolerance = target_millifps.saturating_mul(CADENCE_TOLERANCE_PERCENT) / 100;
        let accepted = udp_report.queued_access_units == expected_units
            && udp_report.sent_access_units == expected_units
            && udp_report.dropped_access_units == 0
            && udp_report.submit_retries == 0
            && udp_report.adapter_backpressure_events == 0
            && udp_report.adapter_send_errors == 0
            && udp_report.late_access_units == 0
            && interval_millifps >= target_millifps.saturating_sub(cadence_tolerance)
            && interval_millifps <= target_millifps.saturating_add(cadence_tolerance)
            && stats.frames == expected_units;
        STATE.store(
            if accepted {
                H264EncodeStreamState::Verified
            } else {
                H264EncodeStreamState::Failed
            } as u8,
            Ordering::Release,
        );
        crate::log_info!(target: "intel/media-encode";
            "intel/media-encode: udp-live complete accepted={} source=ui4-logical-scanout-d01 source_size={}x{} encode_size={}x{} scale=aspect-fit-letterbox format=nv12 target_fps={} measured_millifps={} backend=gen12-vdenc-mfx hardware_encode=1 all_idr=1 protocol=tme1 version=1 session={} queued_units={} sent_units={} sent_datagrams={} sent_payload_bytes={} dropped_units={} dropped_bytes={} high_water_units={} high_water_bytes={} submit_retries={} adapter_backpressure_events={} adapter_send_errors={} network_waits={} subscriber_wait_polls={} late_units={} max_late_us={} elapsed_us={} source_first_fnv1a32=0x{:08X} source_last_fnv1a32=0x{:08X} source_changes={} capture_convert_avg_us={} capture_convert_max_us={} encode_avg_us={} encode_max_us={} coded_avg_bytes={} coded_max_bytes={} peer={}.{}.{}.{}:{} bounded_seconds={} executor=dedicated-background-worker worker_slot={} worker_kind={} filesystem_writes=0 software_fallback=0 surflive_payload=0\n",
            accepted as u8,
            stats.source_width,
            stats.source_height,
            ENCODE_WIDTH,
            ENCODE_HEIGHT,
            crate::allcaps::media_encode::REALTIME_HZ,
            interval_millifps,
            udp_report.session_id,
            udp_report.queued_access_units,
            udp_report.sent_access_units,
            udp_report.sent_datagrams,
            udp_report.sent_payload_bytes,
            udp_report.dropped_access_units,
            udp_report.dropped_bytes,
            udp_report.high_water_access_units,
            udp_report.high_water_bytes,
            udp_report.submit_retries,
            udp_report.adapter_backpressure_events,
            udp_report.adapter_send_errors,
            udp_report.network_waits,
            udp_report.subscriber_wait_polls,
            udp_report.late_access_units,
            udp_report.max_late_us,
            udp_report.elapsed_us,
            stats.first_source_fnv1a32,
            stats.last_source_fnv1a32,
            stats.source_changes,
            average_u64(stats.capture_convert_us, stats.frames),
            stats.capture_convert_max_us,
            average_u64(stats.encode_us, stats.frames),
            stats.encode_max_us,
            average_usize(stats.coded_bytes, stats.frames),
            stats.coded_max_bytes,
            udp_report.peer_addr[0],
            udp_report.peer_addr[1],
            udp_report.peer_addr[2],
            udp_report.peer_addr[3],
            udp_report.peer_port,
            crate::allcaps::media_encode::VALIDATION_SESSION_SECONDS,
            worker_slot,
            worker_kind,
        );

        if !accepted {
            let snapshot = h264_encode_stream_snapshot();
            crate::log_error!(target: "intel/media-encode";
                "intel/media-encode: live session rejected state={:?} encode_us={} source_bytes={} encoded_bytes={} reason=session-validation-failed action=return-to-subscriber-wait filesystem_writes=0 software_fallback=0\n",
                snapshot.state,
                snapshot.encode_us,
                snapshot.source_bytes,
                snapshot.encoded_bytes,
            );
            Timer::after(Duration::from_millis(250)).await;
        }
        stream_session_id = stream_session_id.wrapping_add(1);
    }
}

fn capture_and_encode_scanout(sequence: u32, stats: &mut LiveEncodeStats) -> Option<Vec<u8>> {
    let capture_started_ns = crate::chronos::monotonic_nanos();
    let capture = match super::screenshot::capture_stream_scanout_rgba() {
        Ok(capture) => capture,
        Err(error) => {
            crate::log_error!(target: "intel/media-encode";
                "intel/media-encode: live frame rejected sequence={} stage=ui4-scanout-capture error={:?} filesystem_writes=0 software_fallback=0\n",
                sequence,
                error,
            );
            return None;
        }
    };
    let mut nv12 = alloc::vec![0u8; ENCODE_NV12_BYTES];
    if !rgba_to_nv12_letterboxed(
        capture.width,
        capture.height,
        capture.rgba.as_slice(),
        nv12.as_mut_slice(),
    ) {
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: live frame rejected sequence={} stage=rgba-to-nv12 source={}x{} rgba_bytes={} filesystem_writes=0 software_fallback=0\n",
            sequence,
            capture.width,
            capture.height,
            capture.rgba.len(),
        );
        return None;
    }
    let capture_convert_us =
        crate::chronos::monotonic_nanos().saturating_sub(capture_started_ns) / 1_000;
    let source_hash = fnv1a32(nv12.as_slice());

    STATE.store(H264EncodeStreamState::Encoding as u8, Ordering::Release);
    let encode = crate::intel::media::avc_encode_probe::run_nv12_frame(nv12.as_slice());
    STATE.store(H264EncodeStreamState::Streaming as u8, Ordering::Release);
    if encode.state != crate::intel::media::avc_encode_probe::AvcEncodeProbeState::Passed
        || !encode.coded_output_validated
    {
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: live frame rejected sequence={} stage=hardware-encode state={:?} failure={} elapsed_us={} coded_bytes={} mfx_error=0x{:08X} software_fallback=0\n",
            sequence,
            encode.state,
            encode.failure.name(),
            encode.elapsed_us,
            encode.coded_bytes,
            encode.mfx_error,
        );
        return None;
    }
    let Some(annex_b) = crate::intel::media::avc_encode_probe::take_coded_access_unit() else {
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: live frame rejected sequence={} stage=coded-au-handoff reason=validated-access-unit-unavailable software_fallback=0\n",
            sequence,
        );
        return None;
    };

    if stats.frames == 0 {
        stats.first_source_fnv1a32 = source_hash;
    } else if stats.last_source_fnv1a32 != source_hash {
        stats.source_changes = stats.source_changes.saturating_add(1);
    }
    stats.frames = stats.frames.saturating_add(1);
    stats.source_width = capture.width;
    stats.source_height = capture.height;
    stats.last_source_fnv1a32 = source_hash;
    stats.capture_convert_us = stats.capture_convert_us.saturating_add(capture_convert_us);
    stats.capture_convert_max_us = stats.capture_convert_max_us.max(capture_convert_us);
    stats.encode_us = stats.encode_us.saturating_add(encode.elapsed_us);
    stats.encode_max_us = stats.encode_max_us.max(encode.elapsed_us);
    stats.coded_bytes = stats.coded_bytes.saturating_add(annex_b.len());
    stats.coded_max_bytes = stats.coded_max_bytes.max(annex_b.len());
    ENCODE_US.store(encode.elapsed_us, Ordering::Release);
    SOURCE_BYTES.store(nv12.len(), Ordering::Release);
    ENCODED_BYTES.store(annex_b.len(), Ordering::Release);
    Some(annex_b)
}

fn rgba_to_nv12_letterboxed(
    source_width: u32,
    source_height: u32,
    rgba: &[u8],
    nv12: &mut [u8],
) -> bool {
    let source_width_usize = source_width as usize;
    let source_height_usize = source_height as usize;
    if source_width == 0
        || source_height == 0
        || source_width_usize
            .checked_mul(source_height_usize)
            .and_then(|pixels| pixels.checked_mul(4))
            != Some(rgba.len())
        || nv12.len() != ENCODE_NV12_BYTES
    {
        return false;
    }

    nv12[..ENCODE_LUMA_BYTES].fill(16);
    nv12[ENCODE_LUMA_BYTES..].fill(128);
    let (mut active_width, mut active_height) = if u64::from(source_width) * ENCODE_HEIGHT as u64
        >= u64::from(source_height) * ENCODE_WIDTH as u64
    {
        (
            ENCODE_WIDTH,
            (ENCODE_WIDTH as u64 * u64::from(source_height) / u64::from(source_width)) as usize,
        )
    } else {
        (
            (ENCODE_HEIGHT as u64 * u64::from(source_width) / u64::from(source_height)) as usize,
            ENCODE_HEIGHT,
        )
    };
    active_width = (active_width.max(2) & !1).min(ENCODE_WIDTH);
    active_height = (active_height.max(2) & !1).min(ENCODE_HEIGHT);
    let offset_x = ((ENCODE_WIDTH - active_width) / 2) & !1;
    let offset_y = ((ENCODE_HEIGHT - active_height) / 2) & !1;

    for destination_y in 0..active_height {
        let source_y = destination_y * source_height_usize / active_height;
        let luma_row = (offset_y + destination_y) * ENCODE_WIDTH + offset_x;
        for destination_x in 0..active_width {
            let source_x = destination_x * source_width_usize / active_width;
            let (red, green, blue) = composited_rgb(rgba, source_width_usize, source_x, source_y);
            nv12[luma_row + destination_x] = rgb_to_luma(red, green, blue);
        }
    }

    for destination_y in (0..active_height).step_by(2) {
        let uv_row = ENCODE_LUMA_BYTES + ((offset_y + destination_y) / 2) * ENCODE_WIDTH + offset_x;
        for destination_x in (0..active_width).step_by(2) {
            let mut red_sum = 0u32;
            let mut green_sum = 0u32;
            let mut blue_sum = 0u32;
            for y in destination_y..destination_y + 2 {
                let source_y = y * source_height_usize / active_height;
                for x in destination_x..destination_x + 2 {
                    let source_x = x * source_width_usize / active_width;
                    let (red, green, blue) =
                        composited_rgb(rgba, source_width_usize, source_x, source_y);
                    red_sum += u32::from(red);
                    green_sum += u32::from(green);
                    blue_sum += u32::from(blue);
                }
            }
            let (u, v) =
                rgb_to_chroma((red_sum / 4) as u8, (green_sum / 4) as u8, (blue_sum / 4) as u8);
            nv12[uv_row + destination_x] = u;
            nv12[uv_row + destination_x + 1] = v;
        }
    }
    true
}

fn composited_rgb(rgba: &[u8], source_width: usize, x: usize, y: usize) -> (u8, u8, u8) {
    let offset = (y * source_width + x) * 4;
    let alpha = u32::from(rgba[offset + 3]);
    (
        ((u32::from(rgba[offset]) * alpha + 127) / 255) as u8,
        ((u32::from(rgba[offset + 1]) * alpha + 127) / 255) as u8,
        ((u32::from(rgba[offset + 2]) * alpha + 127) / 255) as u8,
    )
}

fn rgb_to_luma(red: u8, green: u8, blue: u8) -> u8 {
    let value =
        ((66 * i32::from(red) + 129 * i32::from(green) + 25 * i32::from(blue) + 128) >> 8) + 16;
    value.clamp(16, 235) as u8
}

fn rgb_to_chroma(red: u8, green: u8, blue: u8) -> (u8, u8) {
    let red = i32::from(red);
    let green = i32::from(green);
    let blue = i32::from(blue);
    let u = ((-38 * red - 74 * green + 112 * blue + 128) >> 8) + 128;
    let v = ((112 * red - 94 * green - 18 * blue + 128) >> 8) + 128;
    (u.clamp(16, 240) as u8, v.clamp(16, 240) as u8)
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn average_u64(total: u64, count: usize) -> u64 {
    total / count.max(1) as u64
}

fn average_usize(total: usize, count: usize) -> usize {
    total / count.max(1)
}

async fn park() -> ! {
    loop {
        Timer::after(Duration::from_secs(3_600)).await;
    }
}
