use heapless::Vec;

/// Simple frame-pointer-based stack frame capture.
#[derive(Copy, Clone, Debug)]
pub struct Frame {
    pub rbp: usize,
    pub rip: usize,
}

const MAX_FRAMES: usize = 64;

/// Collect up to `max_frames` frames using the canonical x86_64 RBP chain.
/// Stops on null/zero RIP, non-forward RBP, or misaligned RBP to avoid loops.
pub fn collect(max_frames: usize) -> Vec<Frame, MAX_FRAMES> {
    let limit = core::cmp::min(max_frames, MAX_FRAMES);

    let mut rbp: *const usize;
    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) rbp, options(nomem, nostack, preserves_flags));
    }

    let mut frames = Vec::<Frame, MAX_FRAMES>::new();
    while frames.len() < limit {
        if rbp.is_null() {
            break;
        }

        // Each frame: [saved_rbp, return_rip]. Bail if unreadable/corrupt.
        let saved_rbp = unsafe { core::ptr::read(rbp) } as usize;
        let ret_addr = unsafe { core::ptr::read(rbp.add(1)) } as usize;

        if ret_addr == 0 {
            break;
        }

        let _ = frames.push(Frame { rbp: rbp as usize, rip: ret_addr });

        // Basic sanity: enforce forward progress and 16-byte alignment of caller frame.
        if saved_rbp <= rbp as usize {
            break;
        }
        if (saved_rbp & 0xF) != 0 {
            break;
        }

        rbp = saved_rbp as *const usize;
    }

    frames
}

/// Print a stack trace to debugcon and VGA log.
pub fn print(max_frames: usize) {
    let frames = collect(max_frames);
    crate::debugconf!("stack trace ({} frames)\n", frames.len());
    for (idx, frame) in frames.iter().enumerate() {
        crate::debugconf!("  #{:<2} rbp=0x{:016X} rip=0x{:016X}\n", idx, frame.rbp, frame.rip);
    }
}
