use alloc::{string::String, vec::Vec};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};

pub type CpuCallFn = fn(u64) -> u64;

pub const STATE_IDLE: u8 = 0;
pub const STATE_PENDING: u8 = 1;
pub const STATE_RUNNING: u8 = 2;
pub const STATE_DONE: u8 = 3;
pub const HLT_HISTORY_LEN: usize = 80;
pub const HLT_SAMPLE_MS: u64 = 50;

const HLT_HISTORY_LOW_BITS: usize = 64;
const HLT_HISTORY_HIGH_BITS: usize = HLT_HISTORY_LEN - HLT_HISTORY_LOW_BITS;
const HLT_HISTORY_HIGH_MASK: u64 = (1u64 << HLT_HISTORY_HIGH_BITS) - 1;

#[repr(C, align(64))]
struct Mailbox {
    online: AtomicU8,
    state: AtomicU8,
    hlt_now: AtomicU8,
    hlt_history_low: AtomicU64,
    hlt_history_high: AtomicU64,
    seq: AtomicU64,
    func: AtomicUsize,
    arg: AtomicU64,
    ret: AtomicU64,
}

impl Mailbox {
    const fn new() -> Self {
        Self {
            online: AtomicU8::new(0),
            state: AtomicU8::new(STATE_IDLE),
            hlt_now: AtomicU8::new(0),
            hlt_history_low: AtomicU64::new(u64::MAX),
            hlt_history_high: AtomicU64::new(HLT_HISTORY_HIGH_MASK),
            seq: AtomicU64::new(0),
            func: AtomicUsize::new(0),
            arg: AtomicU64::new(0),
            ret: AtomicU64::new(0),
        }
    }
}

static MAILBOX_PTR: AtomicPtr<Mailbox> = AtomicPtr::new(null_mut());
static MAILBOX_LEN: AtomicUsize = AtomicUsize::new(0);
static INIT_ONCE: spin::Once<()> = spin::Once::new();
static REQUEST_SEQ: AtomicU64 = AtomicU64::new(1);
static HLT_SAMPLE_COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Copy, Clone, Debug)]
pub struct MailboxRead {
    pub online: bool,
    pub state: u8,
    pub hlt_now: bool,
    pub seq: u64,
    pub ret: u64,
}

#[inline]
pub fn is_init() -> bool {
    !MAILBOX_PTR.load(Ordering::Acquire).is_null() && MAILBOX_LEN.load(Ordering::Acquire) != 0
}

#[inline]
pub fn cpu_count() -> usize {
    MAILBOX_LEN.load(Ordering::Acquire)
}

pub fn init(cpu_count: usize) {
    INIT_ONCE.call_once(|| {
        if cpu_count == 0 {
            return;
        }
        let mut v: Vec<Mailbox> = Vec::with_capacity(cpu_count);
        for _ in 0..cpu_count {
            v.push(Mailbox::new());
        }
        let mut boxed = v.into_boxed_slice();
        let ptr = boxed.as_mut_ptr();
        let len = boxed.len();
        core::mem::forget(boxed);
        MAILBOX_PTR.store(ptr, Ordering::Release);
        MAILBOX_LEN.store(len, Ordering::Release);
    });
}

/// Mark the current CPU as online for mailbox targeting.
///
/// Call this after per-CPU initialization (GS base / `PerCpu`) is established.
pub fn mark_online() {
    if !is_init() {
        return;
    }
    let slot = crate::percpu::this_cpu().cpu_index() as usize;
    let Some(m) = mailbox(slot) else { return };
    m.online.store(1, Ordering::Release);
}

pub fn mark_current_hlt_state(hlt: bool) {
    if !is_init() {
        return;
    }
    let cpu_ptr = crate::percpu::try_this_cpu_ptr();
    if cpu_ptr.is_null() {
        return;
    }
    let slot = unsafe { (*cpu_ptr).cpu_index() as usize };
    let Some(m) = mailbox(slot) else { return };
    m.hlt_now.store(u8::from(hlt), Ordering::Release);
}

#[inline]
fn mailbox(slot: usize) -> Option<&'static Mailbox> {
    let ptr = MAILBOX_PTR.load(Ordering::Acquire);
    let len = MAILBOX_LEN.load(Ordering::Acquire);
    if ptr.is_null() || slot >= len {
        return None;
    }
    Some(unsafe { &*ptr.add(slot) })
}

#[derive(Copy, Clone, Debug)]
pub struct SubmitReport {
    pub seq: u64,
    pub targeted_aps: usize,
    pub submitted_aps: usize,
    pub busy_aps: usize,
}

/// Submit a call to all online APs (CPU slots 1..N-1).
///
/// This is intentionally non-blocking: APs execute the call when they next
/// poll their mailbox (see [`poll`]).
///
/// If an AP already has a pending/running request, it will be counted as busy
/// and will not be overwritten.
pub fn submit_to_all_online_aps(func: CpuCallFn, arg: u64) -> SubmitReport {
    let len = cpu_count();
    if len <= 1 {
        return SubmitReport {
            seq: 0,
            targeted_aps: 0,
            submitted_aps: 0,
            busy_aps: 0,
        };
    }

    let seq = REQUEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let func_bits = func as usize;

    let mut targeted_aps: usize = 0;
    let mut submitted_aps: usize = 0;
    let mut busy_aps: usize = 0;

    for slot in 1..len {
        let Some(m) = mailbox(slot) else { continue };

        if m.online.load(Ordering::Acquire) == 0 {
            continue;
        }
        targeted_aps += 1;

        let st = m.state.load(Ordering::Acquire);
        if st == STATE_PENDING || st == STATE_RUNNING {
            busy_aps += 1;
            continue;
        }

        m.ret.store(0, Ordering::Relaxed);
        m.arg.store(arg, Ordering::Relaxed);
        m.func.store(func_bits, Ordering::Release);
        m.seq.store(seq, Ordering::Release);
        m.state.store(STATE_PENDING, Ordering::Release);
        submitted_aps += 1;
    }

    SubmitReport {
        seq,
        targeted_aps,
        submitted_aps,
        busy_aps,
    }
}

/// Poll and execute any pending mailbox call for the current CPU.
///
/// Call this from AP loops (and any other per-CPU idle loop) to enable simple
/// cross-CPU rendezvous without IPIs.
pub fn poll() {
    if !is_init() {
        return;
    }

    let slot = crate::percpu::this_cpu().cpu_index() as usize;
    let Some(m) = mailbox(slot) else { return };

    if m.state.load(Ordering::Acquire) != STATE_PENDING {
        return;
    }

    if m.state
        .compare_exchange(STATE_PENDING, STATE_RUNNING, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let func_bits = m.func.load(Ordering::Acquire);
    let arg = m.arg.load(Ordering::Relaxed);

    let ret = if func_bits == 0 {
        0
    } else {
        let f: CpuCallFn = unsafe { core::mem::transmute(func_bits) };
        f(arg)
    };

    m.ret.store(ret, Ordering::Release);
    m.state.store(STATE_DONE, Ordering::Release);
}

pub fn sample_hlt_history() {
    let len = cpu_count();
    if len == 0 {
        return;
    }

    for slot in 0..len {
        let Some(m) = mailbox(slot) else { continue };
        let active_bit = u64::from(m.hlt_now.load(Ordering::Acquire) == 0);
        let low = m.hlt_history_low.load(Ordering::Acquire);
        let high = m.hlt_history_high.load(Ordering::Acquire);
        m.hlt_history_low
            .store((low << 1) | active_bit, Ordering::Release);
        m.hlt_history_high
            .store(((high << 1) | (low >> 63)) & HLT_HISTORY_HIGH_MASK, Ordering::Release);
    }

    HLT_SAMPLE_COUNT.fetch_add(1, Ordering::AcqRel);
}

pub fn hlt_sample_count() -> u64 {
    HLT_SAMPLE_COUNT.load(Ordering::Acquire)
}

pub fn hlt_history_text(slot: usize) -> Option<String> {
    let m = mailbox(slot)?;
    let low = m.hlt_history_low.load(Ordering::Acquire);
    let high = m.hlt_history_high.load(Ordering::Acquire);
    let mut out = String::with_capacity(HLT_HISTORY_LEN);
    for idx in (0..HLT_HISTORY_HIGH_BITS).rev() {
        let hot = ((high >> idx) & 1) != 0;
        out.push(if hot { '!' } else { '.' });
    }
    for idx in (0..HLT_HISTORY_LOW_BITS).rev() {
        let hot = ((low >> idx) & 1) != 0;
        out.push(if hot { '!' } else { '.' });
    }
    Some(out)
}

#[embassy_executor::task]
pub async fn hlt_history_sampler_task() {
    loop {
        sample_hlt_history();
        Timer::after(EmbassyDuration::from_millis(HLT_SAMPLE_MS)).await;
    }
}

pub fn read(slot: usize) -> Option<MailboxRead> {
    let m = mailbox(slot)?;
    Some(MailboxRead {
        online: m.online.load(Ordering::Acquire) != 0,
        state: m.state.load(Ordering::Acquire),
        hlt_now: m.hlt_now.load(Ordering::Acquire) != 0,
        seq: m.seq.load(Ordering::Acquire),
        ret: m.ret.load(Ordering::Acquire),
    })
}

// Unused for now; kept here as a ready-to-restore helper if AP rendezvous
// waits become part of the SMP bringup flow again.
// /// Wait for all online APs to finish a given request sequence.
// pub fn wait_all_online_aps(seq: u64, spins: usize) -> bool {
//     if seq == 0 {
//         return true;
//     }
//     let len = cpu_count();
//     if len <= 1 {
//         return true;
//     }
//
//     for _ in 0..spins {
//         let mut done = true;
//         for slot in 1..len {
//             let Some(r) = read(slot) else {
//                 done = false;
//                 break;
//             };
//             if !r.online {
//                 continue;
//             }
//             if r.seq != seq || r.state != STATE_DONE {
//                 done = false;
//                 break;
//             }
//         }
//         if done {
//             return true;
//         }
//         wait::spin_step();
//     }
//
//     false
// }
//
// /// Backwards-compat alias.
// pub fn wait_all_aps(seq: u64, spins: usize) -> bool {
//     wait_all_online_aps(seq, spins)
// }
