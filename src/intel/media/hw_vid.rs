use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Timer};

const H264_BOOT_PROBE_ENABLED: bool = true;
const H264_BOOT_PROBE_PLAYBACK_ENABLED: bool = true;
const H264_BOOT_PROBE_PLAYBACK_MAX_FRAMES: usize = 200;
const H264_BOOT_PROBE_PLAYBACK_FRAME_MS: u64 = 33;
const H264_BOOT_PROBE_STREAM_LOAD_TIMEOUT_MS: u64 = 20_000;
const H264_BOOT_PROBE_STREAM_LOAD_POLL_MS: u64 = 250;
const H264_BOOT_PROBE_TIMEOUT_MS: u64 = 5_000;
const H264_BOOT_PROBE_DELAY_MS: u64 = 2_000;
const H264_BOOT_PROBE_STREAM_PATH: &str = "x31_head_movie.annexb.h264";

pub(crate) const fn probe_enabled() -> bool {
    H264_BOOT_PROBE_ENABLED
}

#[embassy_executor::task]
pub(crate) async fn hw_vid_probe_task() {
    if !H264_BOOT_PROBE_ENABLED {
        crate::log!("intel/hw_vid: probe disabled reason=h264-boot-probe-disabled\n");
        return;
    }
    if !crate::intel::has_media_decode_engine() {
        crate::log!("intel/hw_vid: probe skipped reason=no-media-decode-engine\n");
        return;
    }

    Timer::after(EmbassyDuration::from_millis(H264_BOOT_PROBE_DELAY_MS)).await;
    if H264_BOOT_PROBE_PLAYBACK_ENABLED {
        if let Some(stream) = h264_load_playback_stream().await {
            h264_i_p_playback_probe(stream.as_slice()).await;
        } else {
            crate::log!(
                "intel/hw_vid: h264-playback-probe skipped reason=stream-file-unavailable path={} action=require-trueosfs-file\n",
                H264_BOOT_PROBE_STREAM_PATH
            );
        }
        return;
    }

    crate::log!(
        "intel/hw_vid: probe skipped reason=playback-disabled-and-no-embedded-first-frame path={}\n",
        H264_BOOT_PROBE_STREAM_PATH
    );
}

#[derive(Copy, Clone, Debug)]
struct H264BootNal {
    start: usize,
    payload_start: usize,
    end: usize,
    nal_type: u8,
}

async fn h264_load_playback_stream() -> Option<Vec<u8>> {
    let mut waited_ms = 0u64;
    let mut attempts = 0usize;
    let mut last_reason = "not-tried";

    loop {
        attempts += 1;
        if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
            match crate::r::fs::trueosfs::file_out_async(disk, H264_BOOT_PROBE_STREAM_PATH).await {
                Ok(Some(bytes)) => {
                    crate::log!(
                        "intel/hw_vid: h264-playback-probe stream-load accepted=1 path={} bytes={} source=trueosfs-root attempts={} waited_ms={}\n",
                        H264_BOOT_PROBE_STREAM_PATH,
                        bytes.len(),
                        attempts,
                        waited_ms
                    );
                    return Some(bytes);
                }
                Ok(None) => last_reason = "file-missing",
                Err(err) => {
                    crate::log!(
                        "intel/hw_vid: h264-playback-probe stream-load retry path={} attempt={} waited_ms={} err={:?}\n",
                        H264_BOOT_PROBE_STREAM_PATH,
                        attempts,
                        waited_ms,
                        err
                    );
                    last_reason = "read-error";
                }
            }
        } else {
            last_reason = "no-trueosfs-root";
        }

        if waited_ms >= H264_BOOT_PROBE_STREAM_LOAD_TIMEOUT_MS {
            crate::log!(
                "intel/hw_vid: h264-playback-probe stream-load accepted=0 path={} reason={} attempts={} waited_ms={} timeout_ms={}\n",
                H264_BOOT_PROBE_STREAM_PATH,
                last_reason,
                attempts,
                waited_ms,
                H264_BOOT_PROBE_STREAM_LOAD_TIMEOUT_MS
            );
            return None;
        }

        Timer::after(EmbassyDuration::from_millis(H264_BOOT_PROBE_STREAM_LOAD_POLL_MS)).await;
        waited_ms = waited_ms.saturating_add(H264_BOOT_PROBE_STREAM_LOAD_POLL_MS);
    }
}

async fn h264_i_p_playback_probe(stream: &[u8]) {
    let mut offset = 0usize;
    let mut nal_count = 0usize;
    let mut idr_seen = 0usize;
    let mut p_seen = 0usize;
    let mut submitted = 0usize;
    let mut skipped_missing_headers = 0usize;
    let mut last_sps = None;
    let mut last_pps = None;

    crate::log!(
        "intel/hw_vid: h264-playback-probe start bytes={} max_frames={} frame_ms={} subset=idr-plus-p source=trueosfs-root path={}\n",
        stream.len(),
        H264_BOOT_PROBE_PLAYBACK_MAX_FRAMES,
        H264_BOOT_PROBE_PLAYBACK_FRAME_MS,
        H264_BOOT_PROBE_STREAM_PATH
    );

    while let Some((nal, next_offset)) = h264_next_annexb_nal(stream, offset) {
        offset = next_offset;
        nal_count += 1;
        match nal.nal_type {
            7 => last_sps = Some(nal),
            8 => last_pps = Some(nal),
            1 | 5 => {
                if nal.nal_type == 5 {
                    idr_seen += 1;
                } else {
                    p_seen += 1;
                }
                let (Some(sps), Some(pps)) = (last_sps, last_pps) else {
                    skipped_missing_headers += 1;
                    continue;
                };
                let mut frame =
                    Vec::with_capacity(h264_nal_len(sps) + h264_nal_len(pps) + h264_nal_len(nal));
                h264_push_nal(&mut frame, stream, sps);
                h264_push_nal(&mut frame, stream, pps);
                h264_push_nal(&mut frame, stream, nal);

                submitted += 1;
                let _ = h264_submit_wait_present_probe_frame(submitted, idr_seen, &frame).await;
                Timer::after(EmbassyDuration::from_millis(H264_BOOT_PROBE_PLAYBACK_FRAME_MS)).await;

                if submitted >= H264_BOOT_PROBE_PLAYBACK_MAX_FRAMES {
                    break;
                }
            }
            _ => {}
        }
    }

    crate::log!(
        "intel/hw_vid: h264-playback-probe done nals={} idr_seen={} p_seen={} submitted={} missing_headers={} stopped_at=0x{:X} reason={}\n",
        nal_count,
        idr_seen,
        p_seen,
        submitted,
        skipped_missing_headers,
        offset,
        if submitted >= H264_BOOT_PROBE_PLAYBACK_MAX_FRAMES {
            "max-frames"
        } else {
            "eos"
        }
    );
}

async fn h264_submit_wait_present_probe_frame(
    playback_frame: usize,
    stream_idr_index: usize,
    encoded: &[u8],
) -> bool {
    let before = crate::intel::hw_pic_snapshot();
    crate::log!(
        "intel/hw_vid: h264-probe submit playback_frame={} stream_idr={} bytes={} pending={} outputs={} service_started={}\n",
        playback_frame,
        stream_idr_index,
        encoded.len(),
        before.pending,
        before.outputs,
        before.service_started as u8
    );

    let id = match crate::intel::hw_pic_submit_h264(encoded) {
        Ok(id) => id,
        Err(err) => {
            crate::log!(
                "intel/hw_vid: h264-probe submit-failed playback_frame={} stream_idr={} err={}\n",
                playback_frame,
                stream_idr_index,
                err
            );
            return false;
        }
    };

    let Some(output) =
        crate::intel::hw_pic_wait_output_for_id(id, H264_BOOT_PROBE_TIMEOUT_MS).await
    else {
        let after = crate::intel::hw_pic_snapshot();
        crate::log!(
            "intel/hw_vid: h264-probe timeout playback_frame={} stream_idr={} id={} pending={} outputs={} service_started={}\n",
            playback_frame,
            stream_idr_index,
            id,
            after.pending,
            after.outputs,
            after.service_started as u8
        );
        return false;
    };

    let stored = h264_present_probe_output(&output);

    crate::log!(
        "intel/hw_vid: h264-probe output playback_frame={} stream_idr={} id={} codec={:?} status={:?} fmt={:?} decoded={}x{} visible={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} stored={} present=ytile-nv12-diagnostic err={}\n",
        playback_frame,
        stream_idr_index,
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
        stored as u8,
        output.error_code
    );
    stored
}

fn h264_present_probe_output(output: &super::hw_pic::HwPicOutput) -> bool {
    if matches!(
        output.status,
        super::hw_pic::HwPicStatus::Ready | super::hw_pic::HwPicStatus::Streamed
    ) && output.format == super::hw_pic::HwPicPixelFormat::Nv12
        && output.width != 0
        && output.height != 0
        && output.pitch_bytes != 0
        && output.byte_len != 0
        && output.virt_addr != 0
    {
        let src =
            unsafe { core::slice::from_raw_parts(output.virt_addr as *const u8, output.byte_len) };
        crate::intel::display::present_ytile_nv12_surface_center(
            src,
            output.width,
            output.height,
            0,
            0,
            output.visible_width,
            output.visible_height,
            output.pitch_bytes,
            output.uv_offset,
        )
    } else {
        false
    }
}

fn h264_nal_len(nal: H264BootNal) -> usize {
    nal.end.saturating_sub(nal.start)
}

fn h264_push_nal(dst: &mut Vec<u8>, bytes: &[u8], nal: H264BootNal) {
    if nal.start <= nal.end && nal.end <= bytes.len() {
        dst.extend_from_slice(&bytes[nal.start..nal.end]);
    }
}

fn h264_next_annexb_nal(bytes: &[u8], offset: usize) -> Option<(H264BootNal, usize)> {
    let mut cursor = offset;
    loop {
        let (start, start_code_len) = h264_find_start_code(bytes, cursor)?;
        let payload_start = start + start_code_len;
        let end = h264_find_start_code(bytes, payload_start)
            .map(|(next, _)| next)
            .unwrap_or(bytes.len());
        cursor = end;
        if payload_start < end && payload_start < bytes.len() {
            return Some((
                H264BootNal {
                    start,
                    payload_start,
                    end,
                    nal_type: bytes[payload_start] & 0x1f,
                },
                cursor,
            ));
        }
        if cursor >= bytes.len() {
            return None;
        }
    }
}

fn h264_find_start_code(bytes: &[u8], offset: usize) -> Option<(usize, usize)> {
    let mut i = offset.min(bytes.len());
    while i + 3 <= bytes.len() {
        if bytes[i..].starts_with(&[0, 0, 1]) {
            return Some((i, 3));
        }
        if i + 4 <= bytes.len() && bytes[i..].starts_with(&[0, 0, 0, 1]) {
            return Some((i, 4));
        }
        i += 1;
    }
    None
}
