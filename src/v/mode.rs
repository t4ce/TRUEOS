use core::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemMode {
    Normal = 0,
    Benchmark = 1,
}

static SYSTEM_MODE: AtomicU8 = AtomicU8::new(SystemMode::Normal as u8);

pub fn set_mode(mode: SystemMode) {
    SYSTEM_MODE.store(mode as u8, Ordering::Release);
}

pub fn mode() -> SystemMode {
    match SYSTEM_MODE.load(Ordering::Acquire) {
        1 => SystemMode::Benchmark,
        _ => SystemMode::Normal,
    }
}

pub fn is_benchmark() -> bool {
    mode() == SystemMode::Benchmark
}

pub fn is_normal() -> bool {
    mode() == SystemMode::Normal
}
