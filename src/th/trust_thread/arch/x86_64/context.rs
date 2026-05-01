//! x86_64 saved context shape.
//!
//! This is a cleaned extraction of the TrustOS context structure.  The actual
//! `switch_context` assembly is intentionally not copied into the portable core
//! yet; TRUEOS needs to decide where interrupt/preemption and AP-loop dispatch
//! enter this world first.

#[derive(Debug, Clone)]
#[repr(C, align(16))]
pub struct ThreadContext {
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rsp: u64,
    pub rip: u64,
    pub rflags: u64,
    pub cr3: u64,
    pub fxsave_area: [u8; 512],
}

impl ThreadContext {
    pub const fn zeroed() -> Self {
        Self {
            rbx: 0,
            rbp: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rsp: 0,
            rip: 0,
            rflags: 0x202,
            cr3: 0,
            fxsave_area: [0; 512],
        }
    }

    pub fn set_entry(&mut self, entry: u64) {
        self.rip = entry;
    }

    pub fn set_stack(&mut self, stack_pointer: u64) {
        self.rsp = stack_pointer;
    }

    pub fn set_page_table(&mut self, cr3: u64) {
        self.cr3 = cr3;
    }
}

impl Default for ThreadContext {
    fn default() -> Self {
        Self::zeroed()
    }
}
