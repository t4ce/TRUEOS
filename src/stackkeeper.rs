use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub const TOKIO_LANE_COUNT: usize = crate::allcaps::stackkeeper::TOKIO_LANE_COUNT;
pub const TOKIO_LANE_SCRATCH_BYTES: usize = crate::allcaps::stackkeeper::TOKIO_LANE_SCRATCH_BYTES;
const TOKIO_TLS_CPU_TRACK_COUNT: usize = crate::allcaps::stackkeeper::TOKIO_TLS_CPU_TRACK_COUNT;

const LANE_TAG_MAGIC: u32 = 0x304B_5453; // "STK0", little endian in memory.
const NO_CPU_SLOT: u32 = u32::MAX;
const TRUEOS_KERNEL_VM_ID: u8 = 0;
const LANE_DOMAIN_HOST: u8 = 0;
const LANE_DOMAIN_GUEST: u8 = 1;
const LANE_ROLE_TOKIO_BLOCKING: u8 = 1;
const TOKIO_WORKER_RECORD_MAGIC: u32 = 0x524B_5754; // "TWKR", little endian in memory.
const TOKIO_WORKER_RECORD_VERSION: u32 = 1;
const VM_HULL_RECORD_MAGIC: u32 = 0x4C48_4D56; // "VMHL", little endian in memory.
const VM_HULL_RECORD_VERSION: u32 = 1;
pub const VM_HULL_RECORD_ROLE_VM_HULL: u8 = 1;
const WLS_HOST_WORKER_BASE: usize = 0;
const WLS_BLUEPRINT_RUNTIME_BASE: usize = WLS_HOST_WORKER_BASE + TOKIO_LANE_COUNT;
const WLS_BLUEPRINT_WORKER_BASE: usize =
    WLS_BLUEPRINT_RUNTIME_BASE + crate::allcaps::hv::VM_ID_LIMIT;
const WLS_BLUEPRINT_THREAD_SLOTS_PER_VM: usize = 64;
const WLS_BLUEPRINT_THREAD_BASE: usize =
    WLS_BLUEPRINT_WORKER_BASE + crate::allcaps::hv::VM_ID_LIMIT * TOKIO_LANE_COUNT;
const WLS_HOST_FALLBACK_BASE: usize =
    WLS_BLUEPRINT_THREAD_BASE + crate::allcaps::hv::VM_ID_LIMIT * WLS_BLUEPRINT_THREAD_SLOTS_PER_VM;
const NO_BLUEPRINT_THREAD_ID: u32 = 0;

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
static LOGGED_BLUEPRINT_RUNTIME_WLS_SLOT: [AtomicBool; 32] = [const { AtomicBool::new(false) }; 32];
static LOGGED_BLUEPRINT_WORKER_WLS_SLOT: [AtomicBool; 32] = [const { AtomicBool::new(false) }; 32];
static LOGGED_BLUEPRINT_THREAD_WLS_SLOT: [AtomicBool; 32] = [const { AtomicBool::new(false) }; 32];
static LOGGED_HOST_WLS_FALLBACK: AtomicBool = AtomicBool::new(false);
static CURRENT_BLUEPRINT_THREAD_ID_BY_CPU: [AtomicU32; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { AtomicU32::new(NO_BLUEPRINT_THREAD_ID) }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];
static VM_HULL_FS_BASE: [AtomicU64; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { AtomicU64::new(0) }; crate::allcaps::hv::VM_ID_LIMIT];

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

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TokioWorkerRecord {
    pub magic: u32,
    pub version: u32,
    pub lane_id: u32,
    pub tls_slot: u32,
    pub scratch_base: usize,
    pub scratch_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VmHullSnapshot {
    pub role: u8,
    pub lane_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VmHullRecord {
    pub magic: u32,
    pub version: u32,
    pub vm_id: u8,
    pub role: u8,
    pub lane_id: u32,
    pub fs_base: u64,
}

impl VmHullRecord {
    pub fn vtid(self) -> u32 {
        self.lane_id
    }
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

#[inline]
fn wls_host_worker_slot(worker_id: usize) -> u32 {
    WLS_HOST_WORKER_BASE.saturating_add(worker_id.min(TOKIO_LANE_COUNT - 1)) as u32
}

#[inline]
fn wls_blueprint_runtime_slot(vm_id: u8) -> u32 {
    WLS_BLUEPRINT_RUNTIME_BASE.saturating_add(vm_id as usize) as u32
}

#[inline]
fn wls_blueprint_worker_slot(vm_id: u8, worker_id: usize) -> u32 {
    WLS_BLUEPRINT_WORKER_BASE
        .saturating_add((vm_id as usize).saturating_mul(TOKIO_LANE_COUNT))
        .saturating_add(worker_id.min(TOKIO_LANE_COUNT - 1)) as u32
}

#[inline]
fn wls_blueprint_thread_slot(vm_id: u8, thread_id: u32) -> u32 {
    let thread_index = thread_id
        .saturating_sub(1)
        .min((WLS_BLUEPRINT_THREAD_SLOTS_PER_VM - 1) as u32) as usize;
    WLS_BLUEPRINT_THREAD_BASE
        .saturating_add((vm_id as usize).saturating_mul(WLS_BLUEPRINT_THREAD_SLOTS_PER_VM))
        .saturating_add(thread_index) as u32
}

#[inline]
fn wls_host_fallback_slot(cpu_slot: u32) -> u32 {
    if cpu_slot == NO_CPU_SLOT {
        WLS_HOST_FALLBACK_BASE.saturating_add(TOKIO_TLS_CPU_TRACK_COUNT) as u32
    } else {
        WLS_HOST_FALLBACK_BASE.saturating_add(cpu_slot as usize) as u32
    }
}

#[inline]
fn current_blueprint_worker_id() -> Option<usize> {
    current_tokio_worker_id().or_else(|| {
        let cpu_slot = cpu_slot_now();
        if cpu_slot == NO_CPU_SLOT {
            None
        } else {
            Some((cpu_slot as usize) % TOKIO_LANE_COUNT)
        }
    })
}

pub fn current_blueprint_thread_id() -> Option<u32> {
    let cpu_slot = cpu_slot_now();
    if cpu_slot == NO_CPU_SLOT {
        return None;
    }
    let thread_id = CURRENT_BLUEPRINT_THREAD_ID_BY_CPU
        .get(cpu_slot as usize)?
        .load(Ordering::Acquire);
    if thread_id == NO_BLUEPRINT_THREAD_ID {
        None
    } else {
        Some(thread_id)
    }
}

pub fn with_current_blueprint_thread_id<R>(thread_id: usize, f: impl FnOnce() -> R) -> R {
    let cpu_slot = cpu_slot_now();
    if cpu_slot == NO_CPU_SLOT {
        return f();
    }
    let Some(slot) = CURRENT_BLUEPRINT_THREAD_ID_BY_CPU.get(cpu_slot as usize) else {
        return f();
    };
    let thread_id = thread_id.min(u32::MAX as usize) as u32;
    let previous = slot.swap(thread_id.max(1), Ordering::AcqRel);
    let result = f();
    slot.store(previous, Ordering::Release);
    result
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_wls_current_slot() -> u32 {
    if let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() {
        let slot = wls_blueprint_runtime_slot(vm_id);
        let vm_index = vm_id as usize;
        if vm_index < LOGGED_BLUEPRINT_RUNTIME_WLS_SLOT.len()
            && !LOGGED_BLUEPRINT_RUNTIME_WLS_SLOT[vm_index].swap(true, Ordering::AcqRel)
        {
            crate::log!(
                "stackkeeper: blueprint runtime wls vm={} worker=main slot={}\n",
                vm_id,
                slot
            );
        }
        return slot;
    }

    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        if let Some(thread_id) = current_blueprint_thread_id() {
            let slot = wls_blueprint_thread_slot(vm_id, thread_id);
            let vm_index = vm_id as usize;
            if vm_index < LOGGED_BLUEPRINT_THREAD_WLS_SLOT.len()
                && !LOGGED_BLUEPRINT_THREAD_WLS_SLOT[vm_index].swap(true, Ordering::AcqRel)
            {
                crate::log!(
                    "stackkeeper: blueprint thread wls vm={} thread={} slot={}\n",
                    vm_id,
                    thread_id,
                    slot
                );
            }
            return slot;
        }
        let worker_id = current_blueprint_worker_id().unwrap_or(0);
        let slot = wls_blueprint_worker_slot(vm_id, worker_id);
        let vm_index = vm_id as usize;
        if vm_index < LOGGED_BLUEPRINT_WORKER_WLS_SLOT.len()
            && !LOGGED_BLUEPRINT_WORKER_WLS_SLOT[vm_index].swap(true, Ordering::AcqRel)
        {
            crate::log!(
                "stackkeeper: blueprint worker wls vm={} worker={} slot={}\n",
                vm_id,
                worker_id,
                slot
            );
        }
        return slot;
    }

    if let Some(worker_id) = current_tokio_worker_id() {
        return wls_host_worker_slot(worker_id);
    }

    let cpu_slot = cpu_slot_now();
    let slot = wls_host_fallback_slot(cpu_slot);

    if !LOGGED_HOST_WLS_FALLBACK.swap(true, Ordering::AcqRel) {
        crate::log!(
            "stackkeeper: host wls fallback source=cpu cpu_slot={} slot={}\n",
            cpu_slot,
            slot
        );
    }

    slot
}

pub fn tokio_blocking_backing_enabled() -> bool {
    true
}

pub fn current_tokio_worker_tls_slot() -> Option<u32> {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return None;
    }

    let cpu_slot = cpu_slot_now();
    if cpu_slot == NO_CPU_SLOT {
        return None;
    }

    let cpu_slot = cpu_slot as usize;
    if cpu_slot >= TOKIO_TLS_CPU_TRACK_COUNT {
        return None;
    }

    let lane_id = TOKIO_CURRENT_LANE_BY_CPU[cpu_slot].load(Ordering::Acquire);
    if (lane_id as usize) < TOKIO_LANE_COUNT
        && TOKIO_LANE_ACTIVE[lane_id as usize].load(Ordering::Acquire)
    {
        Some(lane_id)
    } else {
        None
    }
}

pub fn current_tokio_worker_id() -> Option<usize> {
    current_tokio_worker_tls_slot().map(|lane_id| lane_id as usize)
}

pub fn tokio_worker_record_for_lane(lane_id: usize) -> TokioWorkerRecord {
    let lane_id = lane_id.min(TOKIO_LANE_COUNT - 1);
    TokioWorkerRecord {
        magic: TOKIO_WORKER_RECORD_MAGIC,
        version: TOKIO_WORKER_RECORD_VERSION,
        lane_id: lane_id as u32,
        tls_slot: lane_id as u32,
        scratch_base: lane_scratch_base(lane_id),
        scratch_len: TOKIO_LANE_SCRATCH_BYTES,
    }
}

pub fn current_vm_hull_snapshot() -> Option<VmHullSnapshot> {
    let vm_id = crate::hv::current_hull_guest_context_vm_id()?;
    Some(VmHullSnapshot {
        role: VM_HULL_RECORD_ROLE_VM_HULL,
        lane_id: vm_id as u32,
    })
}

pub fn vm_hull_record(vm_id: u8) -> VmHullRecord {
    VmHullRecord {
        magic: VM_HULL_RECORD_MAGIC,
        version: VM_HULL_RECORD_VERSION,
        vm_id,
        role: VM_HULL_RECORD_ROLE_VM_HULL,
        lane_id: vm_id as u32,
        fs_base: vm_hull_fs_base(vm_id),
    }
}

pub fn vm_hull_fs_base(vm_id: u8) -> u64 {
    VM_HULL_FS_BASE
        .get(vm_id as usize)
        .map(|slot| slot.load(Ordering::Acquire))
        .unwrap_or(0)
}

pub fn set_vm_hull_fs_base(vm_id: u8, fs_base: u64) -> bool {
    let Some(slot) = VM_HULL_FS_BASE.get(vm_id as usize) else {
        return false;
    };
    slot.store(fs_base, Ordering::Release);
    true
}

pub fn try_acquire_tokio_lane(
    cpu_slot: u32,
    core_kind: u8,
    purpose: &'static str,
) -> Option<TokioLaneLease> {
    try_acquire_tokio_lane_for_domain(
        cpu_slot,
        core_kind,
        TRUEOS_KERNEL_VM_ID,
        LANE_DOMAIN_HOST,
        purpose,
    )
}

pub fn try_acquire_tokio_lane_for_vm(
    cpu_slot: u32,
    core_kind: u8,
    vm_id: u8,
    purpose: &'static str,
) -> Option<TokioLaneLease> {
    try_acquire_tokio_lane_for_domain(cpu_slot, core_kind, vm_id, LANE_DOMAIN_GUEST, purpose)
}

fn try_acquire_tokio_lane_for_domain(
    cpu_slot: u32,
    core_kind: u8,
    vm_id: u8,
    domain: u8,
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
        TOKIO_LANE_OWNER_VM_ID[lane_id].store(vm_id as u32, Ordering::Release);
        TOKIO_LANE_OWNER_DOMAIN[lane_id].store(domain as u32, Ordering::Release);
        TOKIO_LANE_OWNER_ROLE[lane_id].store(LANE_ROLE_TOKIO_BLOCKING as u32, Ordering::Release);

        if !LOGGED_TOKIO_LANE[lane_id].swap(true, Ordering::AcqRel) {
            crate::log_info!(target: "service";
                "stackkeeper: reserved tagged Tokio lane{} for {} vm={} domain={} role={} cpu_slot={} scratch={:#x}+{}\n",
                lane_id,
                purpose,
                vm_id,
                domain,
                LANE_ROLE_TOKIO_BLOCKING,
                cpu_slot,
                scratch_base,
                TOKIO_LANE_SCRATCH_BYTES
            );
        }

        return Some(TokioLaneLease {
            tag: LaneTag {
                magic: LANE_TAG_MAGIC,
                domain,
                role: LANE_ROLE_TOKIO_BLOCKING,
                vm_id,
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

    pub fn tokio_worker_record(self) -> TokioWorkerRecord {
        tokio_worker_record_for_lane(self.tag.lane_id as usize)
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
