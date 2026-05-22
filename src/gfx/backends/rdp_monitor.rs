//! RDP monitor backend facade.
//!
//! This keeps the remote-monitor integration at the gfx backend boundary while
//! leaving the TCP service in `r::rdp`.  CABI/UI2 still supplies the semantic
//! texture ids and frame boundaries; this module is the backend-side adapter
//! that publishes those operations to the port-100 monitor service.

pub struct RdpMonitorBackend;

impl RdpMonitorBackend {
    #[inline]
    pub fn begin_frame(seq: u32, flags: u32, clear_rgb: u32) {
        crate::r::rdp::publish_begin_frame(seq, flags, clear_rgb);
    }

    #[inline]
    pub fn end_frame(seq: u32, flags: u32, rgb_draws: u32, tex_draws: u32, draw_bytes: u32) {
        crate::r::rdp::publish_end_frame(seq, flags, rgb_draws, tex_draws, draw_bytes);
    }

    #[inline]
    pub fn set_blend(
        frame_seq: u32,
        enabled: u32,
        src_rgb: u32,
        dst_rgb: u32,
        src_alpha: u32,
        dst_alpha: u32,
    ) {
        crate::r::rdp::publish_set_blend(
            frame_seq, enabled, src_rgb, dst_rgb, src_alpha, dst_alpha,
        );
    }

    #[inline]
    pub fn set_sampler(
        frame_seq: u32,
        wrap_s: u32,
        wrap_t: u32,
        min_filter: u32,
        mag_filter: u32,
    ) {
        crate::r::rdp::publish_set_sampler(frame_seq, wrap_s, wrap_t, min_filter, mag_filter);
    }

    #[inline]
    pub fn set_scissor(frame_seq: u32, x: u32, y: u32, width: u32, height: u32) {
        crate::r::rdp::publish_set_scissor(frame_seq, x, y, width, height);
    }

    #[inline]
    pub fn clear_scissor(frame_seq: u32) {
        crate::r::rdp::publish_clear_scissor(frame_seq);
    }

    #[inline]
    pub fn set_render_target(frame_seq: u32, tex_id: u32) {
        crate::r::rdp::publish_set_render_target(frame_seq, tex_id);
    }

    #[inline]
    pub fn clear_render_target(frame_seq: u32) {
        crate::r::rdp::publish_clear_render_target(frame_seq);
    }

    #[inline]
    pub fn clear_rect(frame_seq: u32, rgb: u32, x: u32, y: u32, width: u32, height: u32) {
        crate::r::rdp::publish_clear_rect(frame_seq, rgb, x, y, width, height);
    }

    #[inline]
    pub fn texture_rgba(
        tex_id: u32,
        width: u32,
        height: u32,
        flags: u32,
        region: Option<(u32, u32, u32, u32)>,
        rgba: &[u8],
    ) {
        crate::r::rdp::publish_texture_rgba(tex_id, width, height, flags, region, rgba);
    }

    #[inline]
    pub fn texture_png(tex_id: u32, flags: u32, data: &[u8]) {
        crate::r::rdp::publish_texture_png(tex_id, flags, data);
    }

    #[inline]
    pub fn texture_jpeg(tex_id: u32, flags: u32, data: &[u8]) {
        crate::r::rdp::publish_texture_jpeg(tex_id, flags, data);
    }

    #[inline]
    pub fn texture_svg(tex_id: u32, flags: u32, data: &[u8]) {
        crate::r::rdp::publish_texture_svg(tex_id, flags, data);
    }

    #[inline]
    pub fn draw_rgb_triangles(frame_seq: u32, vcount: u32, vertices: &[u8]) {
        crate::r::rdp::publish_draw_rgb_triangles(frame_seq, vcount, vertices);
    }

    #[inline]
    pub fn draw_tex_triangles(
        frame_seq: u32,
        tex_id: u32,
        vcount: u32,
        sampler_flags: u32,
        sample_kind: u32,
        vertices: &[u8],
    ) {
        crate::r::rdp::publish_draw_tex_triangles(
            frame_seq,
            tex_id,
            vcount,
            sampler_flags,
            sample_kind,
            vertices,
        );
    }
}
