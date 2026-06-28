use crate::intel::types::Rgba8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CursorOverlayGlyphSpec {
    pub tex_id: u32,
    pub draw_w_px: u16,
    pub draw_h_px: u16,
    pub src_x: u16,
    pub src_y: u16,
    pub src_w: u16,
    pub src_h: u16,
    pub atlas_w: u16,
    pub atlas_h: u16,
}

pub(crate) fn cursor_overlay_glyph_spec(
    _cursor_id: u32,
    _slot_id: u32,
    _view_h: u32,
) -> Option<CursorOverlayGlyphSpec> {
    None
}

pub(crate) fn cursor_color_rgba8_for_cursor_id(cursor_id: u32) -> Rgba8 {
    const COLORS: [Rgba8; 6] = [
        Rgba8::new(255, 0, 0, 255),
        Rgba8::new(0, 160, 255, 255),
        Rgba8::new(0, 220, 120, 255),
        Rgba8::new(255, 190, 0, 255),
        Rgba8::new(220, 80, 255, 255),
        Rgba8::new(255, 255, 255, 255),
    ];
    COLORS[(cursor_id.saturating_sub(1) as usize) % COLORS.len()]
}
