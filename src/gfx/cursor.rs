#[inline]
pub fn cursor_overlay_tick() -> i32 {
    crate::r::io::cabi::kernel_cursor_overlay_tick()
}
