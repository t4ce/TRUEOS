#![allow(dead_code)]

#[inline]
pub fn client_count() -> u32 {
    0
}

#[inline]
pub fn has_clients() -> bool {
    false
}

#[inline]
pub fn cached_texture_count() -> u32 {
    0
}

#[inline]
pub fn cached_texture_bytes() -> u32 {
    0
}

pub fn publish_begin_frame(_seq: u32, _flags: u32, _clear_rgb: u32) {}

pub fn publish_end_frame(
    _seq: u32,
    _flags: u32,
    _rgb_draws: u32,
    _tex_draws: u32,
    _draw_bytes: u32,
) {
}

pub fn publish_set_blend(
    _frame_seq: u32,
    _enabled: u32,
    _src_rgb: u32,
    _dst_rgb: u32,
    _src_alpha: u32,
    _dst_alpha: u32,
) {
}

pub fn publish_set_sampler(
    _frame_seq: u32,
    _wrap_s: u32,
    _wrap_t: u32,
    _min_filter: u32,
    _mag_filter: u32,
) {
}

pub fn publish_set_scissor(_frame_seq: u32, _x: u32, _y: u32, _width: u32, _height: u32) {}

pub fn publish_clear_scissor(_frame_seq: u32) {}

pub fn publish_set_render_target(_frame_seq: u32, _tex_id: u32) {}

pub fn publish_clear_render_target(_frame_seq: u32) {}

pub fn publish_clear_rect(_frame_seq: u32, _rgb: u32, _x: u32, _y: u32, _width: u32, _height: u32) {
}

pub fn publish_clear_color_rgba(_frame_seq: u32, _r: u32, _g: u32, _b: u32, _a: u32) {}

pub fn publish_texture_rgba(
    _tex_id: u32,
    _width: u32,
    _height: u32,
    _flags: u32,
    _region: Option<(u32, u32, u32, u32)>,
    _rgba: &[u8],
) {
}

pub fn publish_texture_png(_tex_id: u32, _flags: u32, _data: &[u8]) {}

pub fn publish_texture_jpeg(_tex_id: u32, _flags: u32, _data: &[u8]) {}

pub fn publish_texture_svg(_tex_id: u32, _flags: u32, _data: &[u8]) {}

pub fn publish_draw_rgb_triangles(_frame_seq: u32, _vcount: u32, _vertices: &[u8]) {}

pub fn publish_draw_tex_triangles(
    _frame_seq: u32,
    _tex_id: u32,
    _vcount: u32,
    _sampler_flags: u32,
    _sample_kind: u32,
    _vertices: &[u8],
) {
}

pub fn publish_shader_create(
    _shader_id: u32,
    _stage: u32,
    _format: u32,
    _flags: u32,
    _source: &[u8],
) {
}

pub fn publish_pipeline_create(
    _pipeline_id: u32,
    _stride: u32,
    _pos_offset: u32,
    _color_offset: u32,
    _color_format: u32,
    _texcoord_offset: u32,
    _texcoord_format: u32,
    _vs_shader_id: u32,
    _fs_shader_id: u32,
) {
}

pub fn publish_draw_pipeline_triangles(
    _frame_seq: u32,
    _pipeline_id: u32,
    _vcount: u32,
    _vertices: &[u8],
) {
}
