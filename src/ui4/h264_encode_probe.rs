//! Deferred embedded 512x512x30 H.264 boot-artifact probe.
//!
//! Intel encode readiness is audited before the workload runs. The current
//! VDBOX path is decode-only, so the artifact is produced by the existing
//! no_std I_PCM encoder and is always labelled as a software fallback.

use core::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};

use embassy_time::{Duration, Timer};

const PROBE_START_DELAY_MS: u64 = 15_000;
const VCS0_PROBE_RETRY_MS: u64 = 50;
const VCS0_PROBE_RETRY_LIMIT: usize = 20;
const ROOT_RETRY_MS: u64 = 1_000;

static STATE: AtomicU8 = AtomicU8::new(H264EncodeProbeState::Waiting as u8);
static ENCODE_US: AtomicU64 = AtomicU64::new(0);
static SOURCE_BYTES: AtomicUsize = AtomicUsize::new(0);
static ENCODED_BYTES: AtomicUsize = AtomicUsize::new(0);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum H264EncodeProbeState {
    Waiting = 0,
    Encoding = 1,
    Writing = 2,
    Verified = 3,
    Failed = 4,
}

impl H264EncodeProbeState {
    fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Encoding,
            2 => Self::Writing,
            3 => Self::Verified,
            4 => Self::Failed,
            _ => Self::Waiting,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct H264EncodeProbeSnapshot {
    pub(crate) state: H264EncodeProbeState,
    pub(crate) encode_us: u64,
    pub(crate) source_bytes: usize,
    pub(crate) encoded_bytes: usize,
}

pub(crate) fn h264_encode_probe_snapshot() -> H264EncodeProbeSnapshot {
    H264EncodeProbeSnapshot {
        state: H264EncodeProbeState::from_raw(STATE.load(Ordering::Acquire)),
        encode_us: ENCODE_US.load(Ordering::Acquire),
        source_bytes: SOURCE_BYTES.load(Ordering::Acquire),
        encoded_bytes: ENCODED_BYTES.load(Ordering::Acquire),
    }
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn ui4_h264_encode_probe_task() {
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: service online carrier=background-worker feature=trueos_h264_encode_probe source=embedded-raw-i420 visible={}x{} frames={} asset_bytes={} output=trueosfs-root/video_encode_<timestamp>.h264 udp_protocol=tme1 udp_port={} stream_ring_seconds={} start_delay_ms={}\n",
        trueos_h264_encode_probe::SEQUENCE_WIDTH,
        trueos_h264_encode_probe::SEQUENCE_HEIGHT,
        trueos_h264_encode_probe::SEQUENCE_FRAME_COUNT,
        trueos_h264_encode_probe::SEQUENCE_ASSET_BYTES,
        crate::allports::services::MEDIA_ENCODE_PROBE_UDP_PORT,
        crate::allcaps::media_encode::STREAM_RING_SECONDS,
        PROBE_START_DELAY_MS,
    );

    Timer::after(Duration::from_millis(PROBE_START_DELAY_MS)).await;

    let mut vcs0_probe = crate::intel::run_media_guc_vcs0_probe_once();
    let mut vcs0_probe_attempts = 1usize;
    while vcs0_probe.state == crate::intel::media::guc_probe::GucVcs0ProbeState::Deferred
        && vcs0_probe_attempts < VCS0_PROBE_RETRY_LIMIT
    {
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
            "intel/media-encode: guc-vcs0-probe accepted=0 state={:?} failure={} forcewake={} backing={} batch={} context={} registered={} submitted={} retired={} context_destroyed={} serial={} markers=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} poll_iters={} elapsed_us={} attempts={} fallback=software encode_claim=0\n",
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
            && avc_probe_attempts < VCS0_PROBE_RETRY_LIMIT
        {
            Timer::after(Duration::from_millis(VCS0_PROBE_RETRY_MS)).await;
            avc_probe = crate::intel::run_media_avc_encode_probe_once();
            avc_probe_attempts += 1;
        }
        if avc_probe.state == crate::intel::media::avc_encode_probe::AvcEncodeProbeState::Passed {
            crate::log_info!(target: "intel/media-encode";
                "intel/media-encode: avc-idr-probe accepted=1 engine=vcs0 submission_owner=guc terminal_fence=mi-flush-post-sync source_frame=0 source_layout=nv12-linear visible=512x512 pitch=512 i420_bytes={} nv12_bytes={} i420_fnv1a32=0x{:08X} nv12_fnv1a32=0x{:08X} backing={} surface_uploaded={} batch={} batch_bytes={} codec_packets={} bitstream_buffer_bound={} registered={} submitted={} retired={} context_destroyed={} serial={} hwlrca=0x{:08X}:0x{:08X} markers=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} poll_iters={} elapsed_us={} attempts={} coded_output_validated=0 hardware_encode=0 next=coded-output-validation\n",
                avc_probe.source_i420_bytes,
                avc_probe.source_nv12_bytes,
                avc_probe.source_i420_fnv1a32,
                avc_probe.source_nv12_fnv1a32,
                avc_probe.backing_ready as u8,
                avc_probe.surface_uploaded as u8,
                avc_probe.batch_ready as u8,
                avc_probe.batch_bytes,
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
            );
        } else {
            crate::log_error!(target: "intel/media-encode";
                "intel/media-encode: avc-idr-probe accepted=0 state={:?} failure={} terminal_fence=mi-flush-post-sync forcewake={} backing={} surface_uploaded={} batch={} batch_bytes={} codec_packets={} bitstream_buffer_bound={} registered={} submitted={} retired={} context_destroyed={} serial={} markers=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} poll_iters={} elapsed_us={} attempts={} coded_output_validated=0 hardware_encode=0 fallback=software\n",
                avc_probe.state,
                avc_probe.failure.name(),
                avc_probe.forcewake as u8,
                avc_probe.backing_ready as u8,
                avc_probe.surface_uploaded as u8,
                avc_probe.batch_ready as u8,
                avc_probe.batch_bytes,
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
            "intel/media-encode: avc-idr-probe accepted=0 state=deferred failure=guc-vcs0-transport-probe-unavailable surface_uploaded=0 codec_packets=0 submitted=0 retired=0 coded_output_validated=0 hardware_encode=0 fallback=software\n",
        );
    }

    let readiness = crate::intel::media_encode_readiness();
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: hardware-readiness ready={} device_claimed={} vdbox_discovered={} guc_transport_ready={} guc_media_context_wired={} guc_media_transport_probe_passed={} avc_encode_commands_wired={} avc_encode_probe_passed={} coded_bitstream_output_wired={} decode_transport=execlists probe_transport=guc-vcs0 action=software-fallback\n",
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

    let disk = loop {
        match crate::r::fs::trueosfs::primary_root_handle() {
            Some(disk) if disk.info().is_read_only() => {
                STATE.store(H264EncodeProbeState::Failed as u8, Ordering::Release);
                crate::log_error!(target: "intel/media-encode";
                    "intel/media-encode: rejected stage=trueosfs reason=primary-root-read-only\n",
                );
                park().await;
            }
            Some(disk) => break disk,
            None => {
                crate::log_info!(target: "intel/media-encode";
                    "intel/media-encode: waiting stage=trueosfs reason=primary-root-unavailable retry_ms={}\n",
                    ROOT_RETRY_MS,
                );
                Timer::after(Duration::from_millis(ROOT_RETRY_MS)).await;
            }
        }
    };

    STATE.store(H264EncodeProbeState::Encoding as u8, Ordering::Release);
    let started_ns = crate::chronos::monotonic_nanos();
    let mut encoder = match trueos_h264_encode_probe::DiagnosticSequenceEncoder::new() {
        Ok(encoder) => encoder,
        Err(error) => {
            fail_encode(error, started_ns).await;
        }
    };

    loop {
        match encoder.encode_next() {
            Ok(true) => {
                let frames = encoder.encoded_frames();
                if frames.is_multiple_of(10)
                    || frames == trueos_h264_encode_probe::SEQUENCE_FRAME_COUNT
                {
                    crate::log_info!(target: "intel/media-encode";
                        "intel/media-encode: progress backend=software-less-avc-ipcm frames={}/{}\n",
                        frames,
                        trueos_h264_encode_probe::SEQUENCE_FRAME_COUNT,
                    );
                }
                // Keep the deferred diagnostic cooperative with other worker
                // services even though the codec itself is synchronous.
                Timer::after(Duration::from_millis(1)).await;
            }
            Ok(false) => break,
            Err(error) => {
                fail_encode(error, started_ns).await;
            }
        }
    }

    let proof = match encoder.finish() {
        Ok(proof) => proof,
        Err(error) => {
            fail_encode(error, started_ns).await;
        }
    };
    let encode_us = crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000;
    let metrics = proof.metrics;
    ENCODE_US.store(encode_us, Ordering::Release);
    SOURCE_BYTES.store(metrics.source_bytes, Ordering::Release);
    ENCODED_BYTES.store(metrics.encoded_bytes, Ordering::Release);

    let timestamp = crate::chronos::best_effort_unix_time_seconds();
    let stream_session_id = timestamp
        .map(|timestamp| timestamp as u32)
        .unwrap_or_else(|| (crate::time::uptime_seconds() as u32) ^ (vcs0_probe.serial as u32));
    let path = match timestamp {
        Some(timestamp) => alloc::format!("video_encode_{}.h264", timestamp),
        None => alloc::format!("video_encode_uptime-{}.h264", crate::time::uptime_seconds()),
    };
    STATE.store(H264EncodeProbeState::Writing as u8, Ordering::Release);
    let write_started_ns = crate::chronos::monotonic_nanos();
    match crate::r::fs::trueosfs::file_write_all_async(
        disk,
        path.as_str(),
        proof.annex_b.as_slice(),
    )
    .await
    {
        Ok(true) => {
            let write_us =
                crate::chronos::monotonic_nanos().saturating_sub(write_started_ns) / 1_000;
            STATE.store(H264EncodeProbeState::Verified as u8, Ordering::Release);
            crate::log_info!(target: "intel/media-encode";
                "intel/media-encode: proof accepted=1 backend=software-less-avc-ipcm hardware_encode=0 codec=h264 profile=baseline frames={} all_idr=1 visible={}x{} coded={}x{} macroblocks_per_frame={} source_bytes={} annexb_bytes={} sps_bytes={} pps_bytes={} frame_bytes_min={} frame_bytes_max={} encode_us={} write_us={} source_fnv1a32=0x{:08X} encoded_fnv1a32=0x{:08X} path=trueosfs:/{}\n",
                metrics.frames,
                metrics.visible_width,
                metrics.visible_height,
                metrics.coded_width,
                metrics.coded_height,
                metrics.macroblocks_per_frame,
                metrics.source_bytes,
                metrics.encoded_bytes,
                metrics.sps_bytes,
                metrics.pps_bytes,
                metrics.frame_bytes_min,
                metrics.frame_bytes_max,
                encode_us,
                write_us,
                metrics.source_fnv1a32,
                metrics.encoded_fnv1a32,
                path,
            );
        }
        Ok(false) => {
            STATE.store(H264EncodeProbeState::Failed as u8, Ordering::Release);
            crate::log_error!(target: "intel/media-encode";
                "intel/media-encode: proof accepted=0 stage=trueosfs-write reason=no-space-or-placement path=trueosfs:/{} bytes={}\n",
                path,
                metrics.encoded_bytes,
            );
        }
        Err(error) => {
            STATE.store(H264EncodeProbeState::Failed as u8, Ordering::Release);
            crate::log_error!(target: "intel/media-encode";
                "intel/media-encode: proof accepted=0 stage=trueosfs-write reason=io-error path=trueosfs:/{} bytes={} error={:?}\n",
                path,
                metrics.encoded_bytes,
                error,
            );
        }
    }

    let udp_report =
        super::h264_encode_udp::broadcast_annex_b(proof.annex_b.as_slice(), stream_session_id)
            .await;
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: udp-stream complete protocol=tme1 version=1 session={} queued_units={} sent_units={} sent_datagrams={} sent_payload_bytes={} dropped_units={} dropped_bytes={} high_water_units={} high_water_bytes={} submit_retries={} adapter_send_errors={} network_waits={} subscriber_wait_polls={} peer={}.{}.{}.{}:{} bounded_seconds={} surflive_payload=0\n",
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
        udp_report.adapter_send_errors,
        udp_report.network_waits,
        udp_report.subscriber_wait_polls,
        udp_report.peer_addr[0],
        udp_report.peer_addr[1],
        udp_report.peer_addr[2],
        udp_report.peer_addr[3],
        udp_report.peer_port,
        crate::allcaps::media_encode::STREAM_RING_SECONDS,
    );

    let snapshot = h264_encode_probe_snapshot();
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: snapshot state={:?} encode_us={} source_bytes={} encoded_bytes={} service=parked\n",
        snapshot.state,
        snapshot.encode_us,
        snapshot.source_bytes,
        snapshot.encoded_bytes,
    );
    park().await;
}

async fn fail_encode(error: trueos_h264_encode_probe::ProbeError, started_ns: u64) -> ! {
    let encode_us = crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000;
    ENCODE_US.store(encode_us, Ordering::Release);
    STATE.store(H264EncodeProbeState::Failed as u8, Ordering::Release);
    crate::log_error!(target: "intel/media-encode";
        "intel/media-encode: proof accepted=0 stage=encode backend=software-less-avc-ipcm error={:?} code={} encode_us={} filesystem_writes=0\n",
        error,
        error.code(),
        encode_us,
    );
    park().await;
}

async fn park() -> ! {
    loop {
        Timer::after(Duration::from_secs(3_600)).await;
    }
}
