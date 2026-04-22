extern crate alloc;

use super::xelp_media_h264src::parse_h264_source_summary;
use super::xelp_media_mp4::{
    parse_pps, parse_sample_vcl_info, parse_sps, visible_h264_frame_dims, write_annex_b_for_sample,
};
use super::xelp_media_ngin;
use super::xelp_media_source;

#[derive(Copy, Clone, Debug)]
pub(crate) struct Media2FirstFrameState {
    pub ready: bool,
    pub submit_completed: bool,
    pub present_ready: bool,
    pub frame_width: u16,
    pub frame_height: u16,
    pub output_surface_pitch: usize,
    pub output_surface_bytes: usize,
    pub output_surface_signature: u32,
    pub output_surface_nonzero_samples: usize,
    pub bitstream_bytes: usize,
    pub sample_nal_count: usize,
    pub has_idr: bool,
}

pub(crate) async fn run_media2_first_frame_async() -> Option<Media2FirstFrameState> {
    let dev = super::claimed_device()?;
    let (engine, windows) = xelp_media_ngin::default_decode_engine_and_window();
    let source = xelp_media_source::fetch_media_source_async().await?;
    let summary = parse_h264_source_summary(&source).await.ok()?;

    let sps = parse_sps(&summary.avcc().sps)?;
    let pps = parse_pps(&summary.avcc().pps, &sps)?;
    let (visible_width, visible_height) = visible_h264_frame_dims(&sps);
    let frame_width = u16::try_from(visible_width)
        .ok()
        .filter(|value| *value != 0)
        .unwrap_or_else(|| summary.width());
    let frame_height = u16::try_from(visible_height)
        .ok()
        .filter(|value| *value != 0)
        .unwrap_or_else(|| summary.height());

    let backing = xelp_media_ngin::ensure_decode_backing(dev, windows)?;
    let annex_b = {
        let bitstream = unsafe {
            core::slice::from_raw_parts_mut(backing.bitstream_virt, backing.bitstream_bytes)
        };
        write_annex_b_for_sample(summary.first_sample(), summary.avcc(), bitstream).ok()?
    };
    let vcl_info = parse_sample_vcl_info(
        summary.first_sample(),
        summary.avcc().nal_length_size,
        &sps,
        &pps,
    );

    let frame = xelp_media_ngin::decode_and_present_frame(
        dev,
        engine,
        windows,
        backing,
        frame_width,
        frame_height,
        &annex_b,
        vcl_info,
        &sps,
        &pps,
        0,
        0,
    )?;

    Some(Media2FirstFrameState {
        ready: frame.ready,
        submit_completed: frame.submit_completed,
        present_ready: frame.present_ready,
        frame_width: frame.frame_width,
        frame_height: frame.frame_height,
        output_surface_pitch: frame.output_surface_pitch,
        output_surface_bytes: frame.output_surface_bytes,
        output_surface_signature: frame.output_surface_signature,
        output_surface_nonzero_samples: frame.output_surface_nonzero_samples,
        bitstream_bytes: frame.bitstream_bytes,
        sample_nal_count: frame.sample_nal_count,
        has_idr: frame.has_idr,
    })
}
