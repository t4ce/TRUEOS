use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub const TOKIO_LANE_COUNT: usize = crate::allcaps::stackkeeper::TOKIO_LANE_COUNT;
pub const TOKIO_LANE_SCRATCH_BYTES: usize = crate::allcaps::stackkeeper::TOKIO_LANE_SCRATCH_BYTES;
const TOKIO_TLS_CPU_TRACK_COUNT: usize = crate::allcaps::stackkeeper::TOKIO_TLS_CPU_TRACK_COUNT;

const LANE_TAG_MAGIC: u32 = 0x304B_5453; // "STK0", little endian in memory.
const NO_CPU_SLOT: u32 = u32::MAX;
const TRUEOS_KERNEL_VM_ID: u8 = 0;
const LANE_DOMAIN_HOST: u8 = 0;
const LANE_ROLE_TOKIO_BLOCKING: u8 = 1;

#[repr(align(64))]
#[allow(dead_code)]
struct LaneScratch([u8; TOKIO_LANE_SCRATCH_BYTES]);

static mut TOKIO_LANE_SCRATCHES: [LaneScratch; TOKIO_LANE_COUNT] =
    [const { LaneScratch([0; TOKIO_LANE_SCRATCH_BYTES]) }; TOKIO_LANE_COUNT];

static TOKIO_LANE_LEASED: [AtomicBool; TOKIO_LANE_COUNT] =
    [const { AtomicBool::new(false) }; TOKIO_LANE_COUNT];
static TOKIO_LANE_ACTIVE: [AtomicBool; TOKIO_LANE_COUNT] =
    [const { AtomicBool::new(false) }; TOKIO_LANE_COUNT];
static TOKIO_LANE_GENERATION: [AtomicU32; TOKIO_LANE_COUNT] =
    [const { AtomicU32::new(1) }; TOKIO_LANE_COUNT];
static TOKIO_LANE_OWNER_CPU_SLOT: [AtomicU32; TOKIO_LANE_COUNT] =
    [const { AtomicU32::new(NO_CPU_SLOT) }; TOKIO_LANE_COUNT];
static TOKIO_LANE_OWNER_CORE_KIND: [AtomicU32; TOKIO_LANE_COUNT] =
    [const { AtomicU32::new(crate::workers::CORE_KIND_UNKNOWN as u32) }; TOKIO_LANE_COUNT];
static TOKIO_LANE_OWNER_VM_ID: [AtomicU32; TOKIO_LANE_COUNT] =
    [const { AtomicU32::new(TRUEOS_KERNEL_VM_ID as u32) }; TOKIO_LANE_COUNT];
static TOKIO_LANE_OWNER_DOMAIN: [AtomicU32; TOKIO_LANE_COUNT] =
    [const { AtomicU32::new(LANE_DOMAIN_HOST as u32) }; TOKIO_LANE_COUNT];
static TOKIO_LANE_OWNER_ROLE: [AtomicU32; TOKIO_LANE_COUNT] =
    [const { AtomicU32::new(LANE_ROLE_TOKIO_BLOCKING as u32) }; TOKIO_LANE_COUNT];
static TOKIO_LANE_ENTER_DEPTH: [AtomicU32; TOKIO_LANE_COUNT] =
    [const { AtomicU32::new(0) }; TOKIO_LANE_COUNT];
static LOGGED_TOKIO_LANE: [AtomicBool; TOKIO_LANE_COUNT] =
    [const { AtomicBool::new(false) }; TOKIO_LANE_COUNT];
static TOKIO_CURRENT_LANE_BY_CPU: [AtomicU32; TOKIO_TLS_CPU_TRACK_COUNT] =
    [const { AtomicU32::new(u32::MAX) }; TOKIO_TLS_CPU_TRACK_COUNT];
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

fn lane_scratch_base(lane_id: usize) -> usize {
    unsafe { (addr_of_mut!(TOKIO_LANE_SCRATCHES) as *mut LaneScratch).add(lane_id) as usize }
}

fn cpu_slot_now() -> u32 {
    let slot = crate::percpu::current_slot();
    if slot > u32::MAX as usize {
        NO_CPU_SLOT
    } else {
        slot as u32
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_tls_current_cpu_slot() -> u32 {
    cpu_slot_now()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_tls_current_slot() -> u32 {
    if crate::t::th::vthread::tokio_blocking_backing_enabled()
        && let Some(slot) = crate::t::th::vthread::current_tls_slot()
    {
        return slot;
    }

    let cpu_slot = cpu_slot_now();

    if cpu_slot != NO_CPU_SLOT {
        let cpu_slot_usize = cpu_slot as usize;
        if cpu_slot_usize < TOKIO_TLS_CPU_TRACK_COUNT {
            let lane_id = TOKIO_CURRENT_LANE_BY_CPU[cpu_slot_usize].load(Ordering::Acquire);
            if (lane_id as usize) < TOKIO_LANE_COUNT
                && TOKIO_LANE_ACTIVE[lane_id as usize].load(Ordering::Acquire)
            {
                return lane_id;
            }
        }
    }

    let fallback = TOKIO_LANE_COUNT as u32;
    if cpu_slot == NO_CPU_SLOT {
        fallback
    } else {
        fallback.saturating_add(cpu_slot)
    }
}

pub fn try_acquire_tokio_lane(
    cpu_slot: u32,
    core_kind: u8,
    purpose: &'static str,
) -> Option<TokioLaneLease> {
    for lane_id in 0..TOKIO_LANE_COUNT {
        if TOKIO_LANE_LEASED[lane_id]
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            continue;
        }

        let generation = TOKIO_LANE_GENERATION[lane_id]
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        let scratch_base = lane_scratch_base(lane_id);

        TOKIO_LANE_OWNER_CPU_SLOT[lane_id].store(cpu_slot, Ordering::Release);
        TOKIO_LANE_OWNER_CORE_KIND[lane_id].store(core_kind as u32, Ordering::Release);
        TOKIO_LANE_OWNER_VM_ID[lane_id].store(TRUEOS_KERNEL_VM_ID as u32, Ordering::Release);
        TOKIO_LANE_OWNER_DOMAIN[lane_id].store(LANE_DOMAIN_HOST as u32, Ordering::Release);
        TOKIO_LANE_OWNER_ROLE[lane_id].store(LANE_ROLE_TOKIO_BLOCKING as u32, Ordering::Release);

        if !LOGGED_TOKIO_LANE[lane_id].swap(true, Ordering::AcqRel) {
            crate::log_info!(target: "service";
                "stackkeeper: reserved tagged Tokio lane{} for {} vm={} domain={} role={} cpu_slot={} scratch={:#x}+{}\n",
                lane_id,
                purpose,
                TRUEOS_KERNEL_VM_ID,
                LANE_DOMAIN_HOST,
                LANE_ROLE_TOKIO_BLOCKING,
                cpu_slot,
                scratch_base,
                TOKIO_LANE_SCRATCH_BYTES
            );
        }

        return Some(TokioLaneLease {
            tag: LaneTag {
                magic: LANE_TAG_MAGIC,
                domain: LANE_DOMAIN_HOST,
                role: LANE_ROLE_TOKIO_BLOCKING,
                vm_id: TRUEOS_KERNEL_VM_ID,
                core_kind,
                lane_id: lane_id as u32,
                cpu_slot,
                generation,
                scratch_base,
                scratch_len: TOKIO_LANE_SCRATCH_BYTES,
            },
        });
    }

    if !LOGGED_TOKIO_LANE_BUSY.swap(true, Ordering::AcqRel) {
        crate::log_warn!(target: "service";
            "stackkeeper: all {} Tokio lanes busy; deferred {}\n",
            TOKIO_LANE_COUNT,
            purpose
        );
    }
    None
}

#[allow(dead_code)]
pub fn active_tokio_lane0_tag() -> LaneTag {
    active_tokio_lane_tag(0)
}

#[allow(dead_code)]
pub fn active_tokio_lane_tag(lane_id: usize) -> LaneTag {
    let lane_id = lane_id.min(TOKIO_LANE_COUNT - 1);
    LaneTag {
        magic: LANE_TAG_MAGIC,
        domain: TOKIO_LANE_OWNER_DOMAIN[lane_id].load(Ordering::Acquire) as u8,
        role: TOKIO_LANE_OWNER_ROLE[lane_id].load(Ordering::Acquire) as u8,
        vm_id: TOKIO_LANE_OWNER_VM_ID[lane_id].load(Ordering::Acquire) as u8,
        core_kind: TOKIO_LANE_OWNER_CORE_KIND[lane_id].load(Ordering::Acquire) as u8,
        lane_id: lane_id as u32,
        cpu_slot: TOKIO_LANE_OWNER_CPU_SLOT[lane_id].load(Ordering::Acquire),
        generation: TOKIO_LANE_GENERATION[lane_id].load(Ordering::Acquire),
        scratch_base: lane_scratch_base(lane_id),
        scratch_len: TOKIO_LANE_SCRATCH_BYTES,
    }
}

pub fn enter_tokio_lane(lease: TokioLaneLease, _purpose: &'static str) -> TokioLaneGuard {
    // TRUEOS isolation lives at VM/app/process scope. These trusted lanes are
    // per-Tokio-worker bookkeeping slots inside one VM: runtime enter state,
    // scheduler-worker identity, task-local runtime state, errno-like fields,
    // and later the architectural TLS base.
    let lane_id = lease.tag.lane_id as usize;
    if lane_id >= TOKIO_LANE_COUNT || lease.tag.magic != LANE_TAG_MAGIC {
        return TokioLaneGuard {
            lease,
            armed: false,
        };
    }

    let depth = TOKIO_LANE_ENTER_DEPTH[lane_id].fetch_add(1, Ordering::AcqRel);
    if depth == 0 {
        TOKIO_LANE_ACTIVE[lane_id].store(true, Ordering::Release);
    }
    let cpu_slot = lease.tag.cpu_slot as usize;
    if cpu_slot < TOKIO_TLS_CPU_TRACK_COUNT {
        TOKIO_CURRENT_LANE_BY_CPU[cpu_slot].store(lane_id as u32, Ordering::Release);
    }

    TokioLaneGuard { lease, armed: true }
}

pub fn release_tokio_lane(lease: TokioLaneLease) -> bool {
    let lane_id = lease.tag.lane_id as usize;
    if lease.tag.magic != LANE_TAG_MAGIC || lane_id >= TOKIO_LANE_COUNT {
        return false;
    }

    let current_generation = TOKIO_LANE_GENERATION[lane_id].load(Ordering::Acquire);
    if current_generation != lease.tag.generation {
        return false;
    }

    TOKIO_LANE_OWNER_CPU_SLOT[lane_id].store(NO_CPU_SLOT, Ordering::Release);
    TOKIO_LANE_OWNER_CORE_KIND[lane_id]
        .store(crate::workers::CORE_KIND_UNKNOWN as u32, Ordering::Release);
    TOKIO_LANE_OWNER_VM_ID[lane_id].store(TRUEOS_KERNEL_VM_ID as u32, Ordering::Release);
    TOKIO_LANE_OWNER_DOMAIN[lane_id].store(LANE_DOMAIN_HOST as u32, Ordering::Release);
    TOKIO_LANE_OWNER_ROLE[lane_id].store(LANE_ROLE_TOKIO_BLOCKING as u32, Ordering::Release);
    TOKIO_LANE_LEASED[lane_id].store(false, Ordering::Release);
    LOGGED_TOKIO_LANE_BUSY.store(false, Ordering::Release);
    true
}

impl TokioLaneLease {
    pub fn tag(self) -> LaneTag {
        self.tag
    }

    pub fn vthread_record(self) -> &'static crate::t::th::vthread::VThreadRecord {
        crate::t::th::vthread::record_for_lane(self.tag.lane_id as usize)
    }
}

impl Drop for TokioLaneGuard {
    fn drop(&mut self) {
        let lane_id = self.lease.tag.lane_id as usize;
        if !self.armed || lane_id >= TOKIO_LANE_COUNT {
            return;
        }

        let prev = TOKIO_LANE_ENTER_DEPTH[lane_id].fetch_sub(1, Ordering::AcqRel);
        if prev <= 1 {
            TOKIO_LANE_ACTIVE[lane_id].store(false, Ordering::Release);
            let cpu_slot = self.lease.tag.cpu_slot as usize;
            if cpu_slot < TOKIO_TLS_CPU_TRACK_COUNT {
                let _ = TOKIO_CURRENT_LANE_BY_CPU[cpu_slot].compare_exchange(
                    lane_id as u32,
                    u32::MAX,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
            }
        }
    }
}
