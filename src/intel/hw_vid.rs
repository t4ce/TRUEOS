use core::sync::atomic::{AtomicBool, Ordering};

static PROBE_RAN: AtomicBool = AtomicBool::new(false);

#[embassy_executor::task]
pub(crate) async fn hw_vid_probe_task() {
    if PROBE_RAN.swap(true, Ordering::AcqRel) {
        crate::log!("intel/hw_vid: duplicate probe task entered; parking\n");
        loop {
            embassy_time::Timer::after_secs(3600).await;
        }
    }

    crate::log!("intel/hw_vid: first-frame probe begin source=embedded-h264-mp4\n");
    let first_frame = super::medbak::xelp_media2_ngin::run_media2_first_frame_async().await;
    match first_frame {
        Some(frame) => crate::log!(
            "intel/hw_vid: first-frame probe done ready={} submit_completed={} present_ready={} frame={}x{} bitstream=0x{:X} nals={} idr={} output_bytes=0x{:X} sig=0x{:08X} nonzero={}\n",
            frame.ready as u8,
            frame.submit_completed as u8,
            frame.present_ready as u8,
            frame.frame_width,
            frame.frame_height,
            frame.bitstream_bytes,
            frame.sample_nal_count,
            frame.has_idr as u8,
            frame.output_surface_bytes,
            frame.output_surface_signature,
            frame.output_surface_nonzero_samples,
        ),
        None => crate::log!("intel/hw_vid: first-frame probe done ready=0 reason=no-frame\n"),
    }
}
