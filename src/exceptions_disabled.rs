// ARMTODO: Non-x86 builds currently fence off the x86 IDT/exception path
// entirely. A real ARM bring-up will need platform glue here for exception
// vectors, fault reporting, and per-CPU exception installation.

#[derive(Copy, Clone, Debug)]
pub struct Frame {
    pub rbp: usize,
    pub rip: usize,
}

pub(crate) fn init() {}

pub(crate) fn load_this_cpu() {}

pub fn collect_backtrace(_max_frames: usize) -> heapless::Vec<Frame, 64> {
    heapless::Vec::new()
}

pub fn print_backtrace(_max_frames: usize) {}