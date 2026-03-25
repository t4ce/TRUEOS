use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub const APMARBLE_SLOT: u32 = 2;
pub const MARBLE_RACE_LANES: usize = 4;

// Per-lane work counters for strict modulo fairness.
static PENDING_UNITS_0: AtomicU32 = AtomicU32::new(0);
static PENDING_UNITS_1: AtomicU32 = AtomicU32::new(0);
static PENDING_UNITS_2: AtomicU32 = AtomicU32::new(0);
static PENDING_UNITS_3: AtomicU32 = AtomicU32::new(0);

static POLL_TICKS: AtomicU64 = AtomicU64::new(0);
static CONSUMED_UNITS_0: AtomicU64 = AtomicU64::new(0);
static CONSUMED_UNITS_1: AtomicU64 = AtomicU64::new(0);
static CONSUMED_UNITS_2: AtomicU64 = AtomicU64::new(0);
static CONSUMED_UNITS_3: AtomicU64 = AtomicU64::new(0);
static NEXT_LANE: AtomicU32 = AtomicU32::new(0);
static BOOT_JOB_DONE: AtomicBool = AtomicBool::new(false);

const PETERSEN_EDGES: [(usize, usize); 15] = [
    (0, 1),
    (1, 2),
    (2, 3),
    (3, 4),
    (4, 0),
    (0, 5),
    (1, 6),
    (2, 7),
    (3, 8),
    (4, 9),
    (5, 7),
    (7, 9),
    (9, 6),
    (6, 8),
    (8, 5),
];

const MARBLE_COLOR_WORDS_100: [&str; 100] = [
    "red", "green", "blue", "purble", "red", "green", "blue", "purble", "red", "green", "blue",
    "purble", "red", "green", "blue", "purble", "red", "green", "blue", "purble", "red", "green",
    "blue", "purble", "red", "green", "blue", "purble", "red", "green", "blue", "purble", "red",
    "green", "blue", "purble", "red", "green", "blue", "purble", "red", "green", "blue", "purble",
    "red", "green", "blue", "purble", "red", "green", "blue", "purble", "red", "green", "blue",
    "purble", "red", "green", "blue", "purble", "red", "green", "blue", "purble", "red", "green",
    "blue", "purble", "red", "green", "blue", "purble", "red", "green", "blue", "purble", "red",
    "green", "blue", "purble", "red", "green", "blue", "purble", "red", "green", "blue", "purble",
    "red", "green", "blue", "purble", "red", "green", "blue", "purble", "red", "green", "blue",
    "purble",
];

#[inline(always)]
fn raw_port_write_bytes(bytes: &[u8]) {
    for &byte in bytes {
        unsafe { crate::portio::outb(0xE9, byte) };
    }
}

#[inline(always)]
fn raw_port_repeat(byte: u8, count: usize) {
    for _ in 0..count {
        unsafe { crate::portio::outb(0xE9, byte) };
    }
}

#[inline(always)]
fn color_word(index: usize) -> &'static str {
    MARBLE_COLOR_WORDS_100[index % MARBLE_COLOR_WORDS_100.len()]
}

#[inline(always)]
fn is_valid_coloring(colors: &[u8; 10]) -> bool {
    PETERSEN_EDGES
        .iter()
        .all(|&(left, right)| colors[left] != colors[right])
}

fn find_petersen_coloring_bruteforce() -> Option<[u8; 10]> {
    let mut colors = [0u8; 10];
    // 3^10 states
    for mut state in 0..59_049u32 {
        for slot in &mut colors {
            *slot = (state % 3) as u8;
            state /= 3;
        }
        if is_valid_coloring(&colors) {
            return Some(colors);
        }
    }
    None
}

fn run_boot_job_once_lowlevel() {
    if BOOT_JOB_DONE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    // Input trace: one O marble per inferred vertex (Petersen has 10 vertices).
    raw_port_repeat(b'O', 10);
    raw_port_write_bytes(b"\n");

    if let Some(colors) = find_petersen_coloring_bruteforce() {
        // Output trace: edge-ordered color assignment.
        for &(left, right) in &PETERSEN_EDGES {
            raw_port_write_bytes(color_word(colors[left] as usize).as_bytes());
            raw_port_write_bytes(b", ");
            raw_port_write_bytes(color_word(colors[right] as usize).as_bytes());
            raw_port_write_bytes(b"; ");
        }
        raw_port_write_bytes(b"\n");
    } else {
        raw_port_write_bytes(b"no-coloring\n");
    }
}

#[inline]
fn pending_for_lane(lane: usize) -> Option<&'static AtomicU32> {
    match lane {
        0 => Some(&PENDING_UNITS_0),
        1 => Some(&PENDING_UNITS_1),
        2 => Some(&PENDING_UNITS_2),
        3 => Some(&PENDING_UNITS_3),
        _ => None,
    }
}

#[inline]
fn consumed_for_lane(lane: usize) -> Option<&'static AtomicU64> {
    match lane {
        0 => Some(&CONSUMED_UNITS_0),
        1 => Some(&CONSUMED_UNITS_1),
        2 => Some(&CONSUMED_UNITS_2),
        3 => Some(&CONSUMED_UNITS_3),
        _ => None,
    }
}

#[inline]
fn try_consume_from_lane(lane: usize) -> bool {
    let Some(pending) = pending_for_lane(lane) else {
        return false;
    };

    let mut current = pending.load(Ordering::Acquire);
    while current != 0 {
        match pending.compare_exchange_weak(
            current,
            current - 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                if let Some(consumed) = consumed_for_lane(lane) {
                    consumed.fetch_add(1, Ordering::Relaxed);
                }
                return true;
            }
            Err(observed) => current = observed,
        }
    }
    false
}

#[inline]
pub fn lane_available(total_cpus: usize) -> bool {
    total_cpus > APMARBLE_SLOT as usize
}

#[inline]
pub fn submit_units(units: u32, total_cpus: usize) -> bool {
    submit_units_on_lane(0, units, total_cpus)
}

#[inline]
pub fn submit_units_on_lane(lane: usize, units: u32, total_cpus: usize) -> bool {
    if units == 0 || !lane_available(total_cpus) {
        return false;
    }

    let Some(pending) = pending_for_lane(lane % MARBLE_RACE_LANES) else {
        return false;
    };
    pending.fetch_add(units, Ordering::AcqRel);
    true
}

#[inline]
pub fn poll_for_current_slot(current_slot: u32, total_cpus: usize) {
    if !lane_available(total_cpus) || current_slot != APMARBLE_SLOT {
        return;
    }

    run_boot_job_once_lowlevel();

    POLL_TICKS.fetch_add(1, Ordering::Relaxed);

    // Strictly fair modulo race: each poll starts from rotating lane index and
    // scans every lane once with no lane priority.
    let start = NEXT_LANE.fetch_add(1, Ordering::Relaxed) as usize % MARBLE_RACE_LANES;
    for step in 0..MARBLE_RACE_LANES {
        let lane = (start + step) % MARBLE_RACE_LANES;
        if try_consume_from_lane(lane) {
            break;
        }
    }
}

#[inline]
pub fn stats() -> (u32, u64, u64) {
    let p0 = PENDING_UNITS_0.load(Ordering::Acquire);
    let p1 = PENDING_UNITS_1.load(Ordering::Acquire);
    let p2 = PENDING_UNITS_2.load(Ordering::Acquire);
    let p3 = PENDING_UNITS_3.load(Ordering::Acquire);

    let c0 = CONSUMED_UNITS_0.load(Ordering::Relaxed);
    let c1 = CONSUMED_UNITS_1.load(Ordering::Relaxed);
    let c2 = CONSUMED_UNITS_2.load(Ordering::Relaxed);
    let c3 = CONSUMED_UNITS_3.load(Ordering::Relaxed);

    (
        p0.saturating_add(p1).saturating_add(p2).saturating_add(p3),
        POLL_TICKS.load(Ordering::Relaxed),
        c0.saturating_add(c1).saturating_add(c2).saturating_add(c3),
    )
}

#[inline]
pub fn lane_stats(lane: usize) -> Option<(u32, u64)> {
    let lane = lane % MARBLE_RACE_LANES;
    let pending = pending_for_lane(lane)?;
    let consumed = consumed_for_lane(lane)?;
    Some((
        pending.load(Ordering::Acquire),
        consumed.load(Ordering::Relaxed),
    ))
}
