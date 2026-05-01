//! Portable thread/carrier vocabulary.

pub type ThreadId = u64;
pub type CarrierId = ThreadId;

pub const THREAD_ID_INVALID: ThreadId = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Sleeping,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadFlags(pub u32);

impl ThreadFlags {
    pub const NONE: Self = Self(0);
    pub const KERNEL: Self = Self(1 << 0);
    pub const MAIN: Self = Self(1 << 1);
    pub const DETACHED: Self = Self(1 << 2);

    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) != 0
    }

    pub const fn union(self, flag: Self) -> Self {
        Self(self.0 | flag.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierPurpose {
    TokioBlocking,
    RayonWorker,
    VmRunner,
    KernelCpu,
    Service,
    Probe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierDuration {
    Transient,
    Persistent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadSpec {
    pub purpose: CarrierPurpose,
    pub duration: CarrierDuration,
    pub stack_bytes: usize,
    pub flags: ThreadFlags,
}

impl ThreadSpec {
    pub const fn kernel_carrier(
        purpose: CarrierPurpose,
        duration: CarrierDuration,
        stack_bytes: usize,
    ) -> Self {
        Self {
            purpose,
            duration,
            stack_bytes,
            flags: ThreadFlags::KERNEL,
        }
    }
}
