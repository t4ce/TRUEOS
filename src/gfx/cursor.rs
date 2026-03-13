#[inline]
pub fn cursor_overlay_tick() -> i32 {
    crate::surface::io::cabi::kernel_cursor_overlay_tick()
}
