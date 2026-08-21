//! Small, best-effort recent RAM-use history for the shell.

use alloc::string::String;
use core::fmt::Write;

use trueos_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

pub const HISTORY_LEN: usize = 40;
pub const SAMPLE_MS: u64 = 1_000;

const PMM_DOMAIN: usize = 0;
const HOST_DOMAIN: usize = 1;
const VM_DOMAIN_BASE: usize = 2;
const DOMAIN_COUNT: usize = VM_DOMAIN_BASE + crate::allcaps::hv::VM_ID_LIMIT;
const _: () = assert!(DOMAIN_COUNT <= 128);

struct UsageHistory {
    used_bytes: [[u64; DOMAIN_COUNT]; HISTORY_LEN],
    active_domains: [u128; HISTORY_LEN],
    next: usize,
    len: usize,
    sample_count: u64,
}

impl UsageHistory {
    const fn new() -> Self {
        Self {
            used_bytes: [[0; DOMAIN_COUNT]; HISTORY_LEN],
            active_domains: [0; HISTORY_LEN],
            next: 0,
            len: 0,
            sample_count: 0,
        }
    }

    fn push(&mut self, used_bytes: [u64; DOMAIN_COUNT], active_domains: u128) {
        self.used_bytes[self.next] = used_bytes;
        self.active_domains[self.next] = active_domains;
        self.next = (self.next + 1) % HISTORY_LEN;
        self.len = self.len.saturating_add(1).min(HISTORY_LEN);
        self.sample_count = self.sample_count.saturating_add(1);
    }
}

static HISTORY: Mutex<UsageHistory> = Mutex::new(UsageHistory::new());

#[inline]
pub fn use_percent(used: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    used.saturating_mul(100)
        .saturating_add(total / 2)
        .checked_div(total)
        .unwrap_or(0)
        .min(100) as u8
}

#[inline]
fn activate(active: &mut u128, domain: usize) {
    if domain < 128 {
        *active |= 1u128 << domain;
    }
}

pub fn sample_once() {
    let mut used_bytes = [0u64; DOMAIN_COUNT];
    let mut active_domains = 0u128;

    if let Some(stats) = crate::phys::pmm_stats() {
        let used = stats.total_bytes.saturating_sub(stats.free_bytes);
        used_bytes[PMM_DOMAIN] = used;
        activate(&mut active_domains, PMM_DOMAIN);
    }

    let host = crate::allocators::heap_stats();
    if host.initialized && host.usable_total != 0 {
        let used = host.usable_total.saturating_sub(host.free_bytes) as u64;
        used_bytes[HOST_DOMAIN] = used;
        activate(&mut active_domains, HOST_DOMAIN);
    }

    for vm_id in 0..crate::allcaps::hv::VM_ID_LIMIT {
        let Some(stats) = crate::allocators::hv_guest_heap_stats_if_configured(vm_id as u8) else {
            continue;
        };
        if stats.usable_total == 0 {
            continue;
        }
        let used = stats.usable_total.saturating_sub(stats.free_bytes) as u64;
        let domain = VM_DOMAIN_BASE + vm_id;
        used_bytes[domain] = used;
        activate(&mut active_domains, domain);
    }

    HISTORY.lock().push(used_bytes, active_domains);
}

pub fn sample_count() -> u64 {
    HISTORY.lock().sample_count
}

fn spark_char(level: usize) -> char {
    const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    LEVELS[level.min(LEVELS.len() - 1)]
}

fn history_text_for_domain(domain: usize) -> String {
    let history = HISTORY.lock();
    let mut out = String::with_capacity(history.len);
    let oldest = (history.next + HISTORY_LEN - history.len) % HISTORY_LEN;
    let mut minimum = u64::MAX;
    let mut maximum = 0u64;
    for offset in 0..history.len {
        let idx = (oldest + offset) % HISTORY_LEN;
        if history.active_domains[idx] & (1u128 << domain) != 0 {
            minimum = minimum.min(history.used_bytes[idx][domain]);
            maximum = maximum.max(history.used_bytes[idx][domain]);
        }
    }
    let range = maximum.saturating_sub(minimum);
    for offset in 0..history.len {
        let idx = (oldest + offset) % HISTORY_LEN;
        if history.active_domains[idx] & (1u128 << domain) == 0 {
            out.push('·');
        } else {
            let level = if range == 0 {
                0
            } else {
                history.used_bytes[idx][domain]
                    .saturating_sub(minimum)
                    .saturating_mul(7)
                    / range
            };
            out.push(spark_char(level as usize));
        }
    }
    out
}

pub fn pmm_history_text() -> String {
    history_text_for_domain(PMM_DOMAIN)
}

pub fn host_history_text() -> String {
    history_text_for_domain(HOST_DOMAIN)
}

pub fn vm_history_text(vm_id: u8) -> String {
    let vm_id = vm_id as usize;
    if vm_id >= crate::allcaps::hv::VM_ID_LIMIT {
        return String::new();
    }
    history_text_for_domain(VM_DOMAIN_BASE + vm_id)
}

pub fn bar_text(percent: u8, width: usize) -> String {
    let filled = usize::from(percent)
        .saturating_mul(width)
        .saturating_add(50)
        / 100;
    let mut out = String::with_capacity(width);
    for _ in 0..filled.min(width) {
        out.push('█');
    }
    for _ in filled.min(width)..width {
        out.push('░');
    }
    out
}

pub fn chart_text(percent: u8, history: &str) -> String {
    let mut out = String::new();
    let _ = write!(out, "{percent:>3}% {} {history}", bar_text(percent, 10));
    out
}

#[trueos_executor::task]
pub async fn history_sampler_task() {
    loop {
        sample_once();
        Timer::after(EmbassyDuration::from_millis(SAMPLE_MS)).await;
    }
}
