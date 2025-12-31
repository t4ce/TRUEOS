use core::fmt::{self, Write};

#[inline(always)]
pub(crate) fn debugcon_write_str_raw(s: &str) {
    for &b in s.as_bytes() {
        unsafe { crate::portio::outb(0xE9, b) };
    }
}

#[inline(always)]
pub(crate) fn debugcon_write_byte_raw(b: u8) {
    unsafe { crate::portio::outb(0xE9, b) };
}

#[inline(always)]
pub(crate) fn debugcon_write_str(s: &str) {
    for &b in s.as_bytes() {
        debugcon_write_byte_raw(b);
        let _ = crate::truelog::try_write_byte(b);
    }
}

#[inline(always)]
pub(crate) fn debugcon_write_byte(b: u8) {
    debugcon_write_byte_raw(b);
    let _ = crate::truelog::try_write_byte(b);
}

pub(crate) struct DebugCon;

impl Write for DebugCon {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        debugcon_write_str(s);
        Ok(())
    }
}

#[macro_export]
macro_rules! debugconf {
    ($($tt:tt)*) => {{
        let _ = core::fmt::write(&mut $crate::debugcon::DebugCon, format_args!($($tt)*));
        let white = 0x00_FF_FF_FF;
        let (_, bg, shadow) = $crate::vga::current_colors()
            .unwrap_or((white, 0, $crate::vga::DEFAULT_SHADOW_COLOR));
        let _ = $crate::vga::log_fmt(format_args!($($tt)*), white, bg, shadow);
    }};
}
