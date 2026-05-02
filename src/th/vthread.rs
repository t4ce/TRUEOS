use core::sync::atomic::{AtomicU32, Ordering};

#[cfg(target_arch = "x86_64")]
use x86_64::registers::model_specific::Msr;

#[cfg(target_arch = "x86_64")]
const IA32_FS_BASE: u32 = 0xC000_0100;

pub const VTHREAD_MAGIC: u64 = 0x4452_4854_4555_5254; // "TRUETHRD", stable tag.
pub const VTHREAD_VERSION: u32 = 1;
pub const VTHREAD_ROLE_TOKIO_BLOCKING: u32 = 1;
pub const VTHREAD_NO_ID: u32 = u32::MAX;

const VTHREAD_RECORD_COUNT: usize = crate::allcaps::stackkeeper::TOKIO_LANE_COUNT;
const TLS_SLOT_COUNT: usize = 64;

#[repr(C, align(64))]
pub struct VThreadRecord {
    magic: u64,
    version: u32,
    vtid: u32,
    role: u32,
    lane_id: u32,
    tls_epoch: AtomicU32,
    tls_slot: u32,
    scratch_base: usize,
}

#[derive(Copy, Clone, Debug)]
pub struct VThreadSnapshot {
    pub record_addr: usize,
    pub vtid: u32,
    pub role: u32,
    pub lane_id: u32,
    pub tls_slot: u32,
    pub fs_base: usize,
    pub cpu_slot: u32,
}

#[derive(Copy, Clone, Debug)]
pub struct VThreadTlsProbe {
    pub snapshot: Option<VThreadSnapshot>,
    pub tls_addr: usize,
    pub before: u32,
    pub after: u32,
}

pub struct VThreadGuard {
    old_fs_base: usize,
    armed: bool,
}

impl VThreadRecord {
    const fn new(lane_id: u32) -> Self {
        Self {
            magic: VTHREAD_MAGIC,
            version: VTHREAD_VERSION,
            vtid: lane_id.saturating_add(1),
            role: VTHREAD_ROLE_TOKIO_BLOCKING,
            lane_id,
            tls_epoch: AtomicU32::new(1),
            tls_slot: lane_id,
            scratch_base: 0,
        }
    }

    #[inline]
    pub fn vtid(&self) -> u32 {
        self.vtid
    }

    #[inline]
    pub fn lane_id(&self) -> u32 {
        self.lane_id
    }

    #[inline]
    pub fn tls_slot(&self) -> u32 {
        self.tls_slot
    }

    #[inline]
    fn valid(&self) -> bool {
        self.magic == VTHREAD_MAGIC
            && self.version == VTHREAD_VERSION
            && (self.tls_slot as usize) < TLS_SLOT_COUNT
    }

    #[inline]
    fn snapshot(&'static self, fs_base: usize) -> VThreadSnapshot {
        VThreadSnapshot {
            record_addr: self as *const Self as usize,
            vtid: self.vtid,
            role: self.role,
            lane_id: self.lane_id,
            tls_slot: self.tls_slot,
            fs_base,
            cpu_slot: crate::percpu::current_slot() as u32,
        }
    }
}

static VTHREAD_RECORDS: [VThreadRecord; VTHREAD_RECORD_COUNT] = [
    VThreadRecord::new(0),
    VThreadRecord::new(1),
    VThreadRecord::new(2),
    VThreadRecord::new(3),
    VThreadRecord::new(4),
    VThreadRecord::new(5),
    VThreadRecord::new(6),
    VThreadRecord::new(7),
    VThreadRecord::new(8),
    VThreadRecord::new(9),
    VThreadRecord::new(10),
    VThreadRecord::new(11),
    VThreadRecord::new(12),
    VThreadRecord::new(13),
    VThreadRecord::new(14),
    VThreadRecord::new(15),
];

static PROBE_TLS_CELLS: [AtomicU32; TLS_SLOT_COUNT] = [const { AtomicU32::new(0) }; TLS_SLOT_COUNT];

#[inline]
pub fn tokio_blocking_backing_enabled() -> bool {
    crate::allcaps::stackkeeper::TOKIO_BLOCKING_VTHREAD_BACKING
}

#[inline]
pub fn record_for_lane(lane_id: usize) -> &'static VThreadRecord {
    &VTHREAD_RECORDS[lane_id.min(VTHREAD_RECORDS.len().saturating_sub(1))]
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn read_fs_base() -> usize {
    unsafe { Msr::new(IA32_FS_BASE).read() as usize }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
fn read_fs_base() -> usize {
    0
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn write_fs_base(value: usize) {
    unsafe { Msr::new(IA32_FS_BASE).write(value as u64) };
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
fn write_fs_base(_value: usize) {}

#[inline]
fn record_from_fs_base(fs_base: usize) -> Option<&'static VThreadRecord> {
    let start = VTHREAD_RECORDS.as_ptr() as usize;
    let end = start.saturating_add(core::mem::size_of_val(&VTHREAD_RECORDS));
    let stride = core::mem::size_of::<VThreadRecord>();

    if fs_base < start || fs_base >= end || stride == 0 {
        return None;
    }

    let offset = fs_base - start;
    if offset % stride != 0 {
        return None;
    }

    let index = offset / stride;
    let record = &VTHREAD_RECORDS[index];
    if record.valid() {
        Some(record)
    } else {
        None
    }
}

#[inline]
pub fn current_record() -> Option<&'static VThreadRecord> {
    record_from_fs_base(read_fs_base())
}

#[inline]
pub fn current_snapshot() -> Option<VThreadSnapshot> {
    let fs_base = read_fs_base();
    record_from_fs_base(fs_base).map(|record| record.snapshot(fs_base))
}

#[inline]
pub fn current_id() -> Option<u32> {
    current_record().map(VThreadRecord::vtid)
}

#[inline]
pub fn current_tls_slot() -> Option<u32> {
    current_record().map(VThreadRecord::tls_slot)
}

#[inline]
pub fn current_fs_base_for_probe() -> usize {
    read_fs_base()
}

pub fn enter(record: &'static VThreadRecord) -> VThreadGuard {
    if !record.valid() {
        return VThreadGuard {
            old_fs_base: 0,
            armed: false,
        };
    }

    let old_fs_base = read_fs_base();
    write_fs_base(record as *const VThreadRecord as usize);
    VThreadGuard {
        old_fs_base,
        armed: true,
    }
}

pub fn probe_tls_touch(label: u32) -> VThreadTlsProbe {
    let snapshot = current_snapshot();
    let slot = snapshot
        .map(|snapshot| snapshot.tls_slot as usize)
        .filter(|slot| *slot < PROBE_TLS_CELLS.len())
        .unwrap_or(0);
    let cell = &PROBE_TLS_CELLS[slot];
    let before = cell.load(Ordering::Acquire);
    cell.store(label, Ordering::Release);
    let after = cell.load(Ordering::Acquire);

    VThreadTlsProbe {
        snapshot,
        tls_addr: cell as *const AtomicU32 as usize,
        before,
        after,
    }
}

impl Drop for VThreadGuard {
    fn drop(&mut self) {
        if self.armed {
            write_fs_base(self.old_fs_base);
        }
    }
}
