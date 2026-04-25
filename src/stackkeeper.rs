use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub const TOKIO_LANE_SCRATCH_BYTES: usize = 16 * 1024;

const LANE_TAG_MAGIC: u32 = 0x304B_5453; // "STK0", little endian in memory.
const NO_CPU_SLOT: u32 = u32::MAX;
const TOKIO_LANE0_ID: u32 = 0;
const TRUEOS_KERNEL_VM_ID: u8 = 0;
const LANE_DOMAIN_HOST: u8 = 0;
const LANE_ROLE_TOKIO_BLOCKING: u8 = 1;

#[repr(align(64))]
#[allow(dead_code)]
struct LaneScratch([u8; TOKIO_LANE_SCRATCH_BYTES]);

static mut TOKIO_LANE0_SCRATCH: LaneScratch = LaneScratch([0; TOKIO_LANE_SCRATCH_BYTES]);

static TOKIO_LANE0_LEASED: AtomicBool = AtomicBool::new(false);
static TOKIO_LANE0_ACTIVE: AtomicBool = AtomicBool::new(false);
static TOKIO_LANE0_GENERATION: AtomicU32 = AtomicU32::new(1);
static TOKIO_LANE0_OWNER_CPU_SLOT: AtomicU32 = AtomicU32::new(NO_CPU_SLOT);
static TOKIO_LANE0_OWNER_CORE_KIND: AtomicU32 =
    AtomicU32::new(crate::workers::CORE_KIND_UNKNOWN as u32);
static TOKIO_LANE0_OWNER_VM_ID: AtomicU32 = AtomicU32::new(TRUEOS_KERNEL_VM_ID as u32);
static TOKIO_LANE0_OWNER_DOMAIN: AtomicU32 = AtomicU32::new(LANE_DOMAIN_HOST as u32);
static TOKIO_LANE0_OWNER_ROLE: AtomicU32 = AtomicU32::new(LANE_ROLE_TOKIO_BLOCKING as u32);
static TOKIO_LANE0_ENTER_DEPTH: AtomicU32 = AtomicU32::new(0);
static LOGGED_TOKIO_LANE0: AtomicBool = AtomicBool::new(false);
static LOGGED_TOKIO_LANE_BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug)]
pub struct LaneTag {
    pub magic: u32,
    pub domain: u8,
    pub role: u8,
    pub vm_id: u8,
    pub core_kind: u8,
    pub lane_id: u32,
    pub cpu_slot: u32,
    pub generation: u32,
    pub scratch_base: usize,
    pub scratch_len: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct TokioLaneLease {
    tag: LaneTag,
}

pub struct TokioLaneGuard {
    lease: TokioLaneLease,
    armed: bool,
}

fn lane0_scratch_base() -> usize {
    addr_of_mut!(TOKIO_LANE0_SCRATCH) as usize
}

pub fn try_acquire_tokio_lane(
    cpu_slot: u32,
    core_kind: u8,
    purpose: &'static str,
) -> Option<TokioLaneLease> {
    if TOKIO_LANE0_LEASED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        if !LOGGED_TOKIO_LANE_BUSY.swap(true, Ordering::AcqRel) {
            crate::log!("stackkeeper: single Tokio lane busy; deferred {}\n", purpose);
        }
        return None;
    }

    let generation = TOKIO_LANE0_GENERATION
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    let scratch_base = lane0_scratch_base();

    TOKIO_LANE0_OWNER_CPU_SLOT.store(cpu_slot, Ordering::Release);
    TOKIO_LANE0_OWNER_CORE_KIND.store(core_kind as u32, Ordering::Release);
    TOKIO_LANE0_OWNER_VM_ID.store(TRUEOS_KERNEL_VM_ID as u32, Ordering::Release);
    TOKIO_LANE0_OWNER_DOMAIN.store(LANE_DOMAIN_HOST as u32, Ordering::Release);
    TOKIO_LANE0_OWNER_ROLE.store(LANE_ROLE_TOKIO_BLOCKING as u32, Ordering::Release);

    if !LOGGED_TOKIO_LANE0.swap(true, Ordering::AcqRel) {
        crate::log!(
            "stackkeeper: reserved tagged Tokio lane0 for {} vm={} domain={} role={} cpu_slot={} scratch={:#x}+{}\n",
            purpose,
            TRUEOS_KERNEL_VM_ID,
            LANE_DOMAIN_HOST,
            LANE_ROLE_TOKIO_BLOCKING,
            cpu_slot,
            scratch_base,
            TOKIO_LANE_SCRATCH_BYTES
        );
    }

    Some(TokioLaneLease {
        tag: LaneTag {
            magic: LANE_TAG_MAGIC,
            domain: LANE_DOMAIN_HOST,
            role: LANE_ROLE_TOKIO_BLOCKING,
            vm_id: TRUEOS_KERNEL_VM_ID,
            core_kind,
            lane_id: TOKIO_LANE0_ID,
            cpu_slot,
            generation,
            scratch_base,
            scratch_len: TOKIO_LANE_SCRATCH_BYTES,
        },
    })
}

#[allow(dead_code)]
pub fn active_tokio_lane0_tag() -> LaneTag {
    LaneTag {
        magic: LANE_TAG_MAGIC,
        domain: TOKIO_LANE0_OWNER_DOMAIN.load(Ordering::Acquire) as u8,
        role: TOKIO_LANE0_OWNER_ROLE.load(Ordering::Acquire) as u8,
        vm_id: TOKIO_LANE0_OWNER_VM_ID.load(Ordering::Acquire) as u8,
        core_kind: TOKIO_LANE0_OWNER_CORE_KIND.load(Ordering::Acquire) as u8,
        lane_id: TOKIO_LANE0_ID,
        cpu_slot: TOKIO_LANE0_OWNER_CPU_SLOT.load(Ordering::Acquire),
        generation: TOKIO_LANE0_GENERATION.load(Ordering::Acquire),
        scratch_base: lane0_scratch_base(),
        scratch_len: TOKIO_LANE_SCRATCH_BYTES,
    }
}

pub fn enter_tokio_lane(lease: TokioLaneLease, _purpose: &'static str) -> TokioLaneGuard {
    // TRUEOS isolation lives at VM/app/process scope. This lane scratchpad is
    // not a trust boundary; it is where Tokio's per-worker bookkeeping belongs:
    // runtime enter state, scheduler-worker identity, task-local runtime state,
    // errno-like fields, and later the architectural TLS base. For now this is
    // the single proven lane, so the active marker is intentionally global.
    let depth = TOKIO_LANE0_ENTER_DEPTH.fetch_add(1, Ordering::AcqRel);
    if depth == 0 {
        TOKIO_LANE0_ACTIVE.store(true, Ordering::Release);
    }

    TokioLaneGuard { lease, armed: true }
}

pub fn release_tokio_lane(lease: TokioLaneLease) -> bool {
    if lease.tag.magic != LANE_TAG_MAGIC || lease.tag.lane_id != TOKIO_LANE0_ID {
        return false;
    }

    let current_generation = TOKIO_LANE0_GENERATION.load(Ordering::Acquire);
    if current_generation != lease.tag.generation {
        return false;
    }

    TOKIO_LANE0_OWNER_CPU_SLOT.store(NO_CPU_SLOT, Ordering::Release);
    TOKIO_LANE0_OWNER_CORE_KIND.store(crate::workers::CORE_KIND_UNKNOWN as u32, Ordering::Release);
    TOKIO_LANE0_OWNER_VM_ID.store(TRUEOS_KERNEL_VM_ID as u32, Ordering::Release);
    TOKIO_LANE0_OWNER_DOMAIN.store(LANE_DOMAIN_HOST as u32, Ordering::Release);
    TOKIO_LANE0_OWNER_ROLE.store(LANE_ROLE_TOKIO_BLOCKING as u32, Ordering::Release);
    TOKIO_LANE0_LEASED.store(false, Ordering::Release);
    LOGGED_TOKIO_LANE_BUSY.store(false, Ordering::Release);
    true
}

impl TokioLaneLease {
    pub fn tag(self) -> LaneTag {
        self.tag
    }
}

impl Drop for TokioLaneGuard {
    fn drop(&mut self) {
        if !self.armed || self.lease.tag.lane_id != TOKIO_LANE0_ID {
            return;
        }

        let prev = TOKIO_LANE0_ENTER_DEPTH.fetch_sub(1, Ordering::AcqRel);
        if prev <= 1 {
            TOKIO_LANE0_ACTIVE.store(false, Ordering::Release);
        }
    }
}
