#!/usr/bin/env python3
"""Exercise the production logger and UI4 F9 callback without GPU hardware."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item


ROOT = Path(__file__).resolve().parents[2]


def main():
    harness = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
extern crate self as trueos_time;
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
pub struct Instant;
impl Instant {
    pub fn now() -> Self { Self }
    pub fn as_millis(&self) -> u64 { 100 }
}
#[macro_export]
macro_rules! log_info {
    (target: $target:expr; $($args:tt)*) => { let _ = format_args!($($args)*); };
}
mod intel { pub mod gpgpu {
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct GpgpuRect { pub x:i32, pub y:i32, pub width:u32, pub height:u32 }
    impl GpgpuRect {
        pub const fn new(x:i32,y:i32,width:u32,height:u32)->Self {Self{x,y,width,height}}
    }
    pub struct GpgpuSolidRect { pub rect:GpgpuRect, pub color_rgba:u32 }
} }
mod spirit {
    #[derive(Copy, Clone)]
    pub struct SpiritSurfaceLayout {
        pub width:u32, pub height:u32, pub pitch_bytes:u32, pub byte_len:usize,
    }
    #[path = "LOGGER_PATH"] pub mod gpu_logger;
}
mod r { pub mod keyboard {
    pub const KEYBOARD_OUTPUT_KIND_KEY:u32 = 1;
    pub const KEYBOARD_KEY_F9:u16 = 109;
    pub const KEYBOARD_OUTPUT_FLAG_PRESS:u32 = 1;
    pub const KEYBOARD_OUTPUT_FLAG_REPEAT:u32 = 32;
    pub struct TrueosKeyboardOutputEvent { pub kind:u32, pub key_code:u16, pub flags:u32 }
} }
#[derive(Debug, PartialEq, Eq)]
enum GlobalKeyboardDisposition { PassThrough, Consume }
mod ui4 {
    use super::GlobalKeyboardDisposition;
    mod callback {
        CALLBACK
        #[test]
        fn f9_consumes_both_edges_and_ignores_repeats_and_other_keys() {
            use crate::r::keyboard::*;
            use super::GlobalKeyboardDisposition::{Consume, PassThrough};
            let logger = crate::spirit::gpu_logger::status;
            let mut event = TrueosKeyboardOutputEvent {
                kind:KEYBOARD_OUTPUT_KIND_KEY, key_code:KEYBOARD_KEY_F9,
                flags:KEYBOARD_OUTPUT_FLAG_PRESS,
            };
            assert!(!logger().active);
            assert_eq!(capture_spirit_gpu_logger_hotkey(&event), Consume);
            assert!(logger().active);
            event.flags |= KEYBOARD_OUTPUT_FLAG_REPEAT;
            assert_eq!(capture_spirit_gpu_logger_hotkey(&event), Consume);
            assert!(logger().active);
            event.flags = 0;
            assert_eq!(capture_spirit_gpu_logger_hotkey(&event), Consume);
            assert!(logger().active);
            event.key_code = 110;
            event.flags = KEYBOARD_OUTPUT_FLAG_PRESS;
            assert_eq!(capture_spirit_gpu_logger_hotkey(&event), PassThrough);
            assert!(logger().active);
            event.key_code = KEYBOARD_KEY_F9;
            event.kind = 2;
            assert_eq!(capture_spirit_gpu_logger_hotkey(&event), PassThrough);
            event.kind = KEYBOARD_OUTPUT_KIND_KEY;
            assert_eq!(capture_spirit_gpu_logger_hotkey(&event), Consume);
            assert!(!logger().active);
        }
    }
}
'''
    harness = harness.replace("LOGGER_PATH", str(ROOT / "src/spirit/gpu_logger.rs"))
    harness = harness.replace("CALLBACK", item("src/ui4/input_broker.rs", "capture_spirit_gpu_logger_hotkey"))
    with tempfile.TemporaryDirectory(prefix="trueos-spirit-f9-") as directory:
        source = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        source.write_text(harness)
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
