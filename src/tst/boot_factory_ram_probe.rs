use core::cmp::min;

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

const FACTORY_RAM_PROBE_PAGE_BYTES: usize = 4096;
const FACTORY_RAM_PROBE_SAMPLE_BYTES: usize = 100;
const FACTORY_RAM_PROBE_DELAY_SECS: u64 = 5;

#[derive(Clone, Debug)]
pub struct FactoryRamProbeSnapshot {
    pub page_phys: u64,
    pub page_bytes: usize,
    pub sample_count: usize,
    pub sample_phys: [u64; FACTORY_RAM_PROBE_SAMPLE_BYTES],
    pub sample_values: [u8; FACTORY_RAM_PROBE_SAMPLE_BYTES],
}

#[derive(Clone, Debug)]
struct FactoryRamProbeState {
    page_phys: u64,
    sample_count: usize,
    sample_phys: [u64; FACTORY_RAM_PROBE_SAMPLE_BYTES],
    sample_values: [u8; FACTORY_RAM_PROBE_SAMPLE_BYTES],
}

static FACTORY_RAM_PROBE_STATE: Mutex<Option<FactoryRamProbeState>> = Mutex::new(None);

pub fn factory_ram_probe_snapshot() -> Option<FactoryRamProbeSnapshot> {
    let guard = FACTORY_RAM_PROBE_STATE.lock();
    let state = guard.as_ref()?;
    Some(FactoryRamProbeSnapshot {
        page_phys: state.page_phys,
        page_bytes: FACTORY_RAM_PROBE_PAGE_BYTES,
        sample_count: state.sample_count,
        sample_phys: state.sample_phys,
        sample_values: state.sample_values,
    })
}

fn next_walk_state(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state
}

fn build_random_sample(page_phys: u64) -> FactoryRamProbeState {
    let page_virt = crate::phys::phys_to_virt(page_phys as usize);
    let page = unsafe {
        core::slice::from_raw_parts(page_virt as *const u8, FACTORY_RAM_PROBE_PAGE_BYTES)
    };

    let mut seed = crate::tyche::rdrand_u64()
        .or_else(|| crate::tyche::rdseed_u64())
        .unwrap_or(page_phys ^ crate::time::uptime_seconds());
    seed ^= page[0] as u64;
    seed ^= (page[FACTORY_RAM_PROBE_PAGE_BYTES / 2] as u64) << 8;
    seed ^= (page[FACTORY_RAM_PROBE_PAGE_BYTES - 1] as u64) << 16;

    let mut used = [false; FACTORY_RAM_PROBE_PAGE_BYTES];
    let mut sample_phys = [0u64; FACTORY_RAM_PROBE_SAMPLE_BYTES];
    let mut sample_values = [0u8; FACTORY_RAM_PROBE_SAMPLE_BYTES];
    let mut count = 0usize;

    while count < FACTORY_RAM_PROBE_SAMPLE_BYTES {
        let off =
            ((next_walk_state(&mut seed) >> 16) as usize) & (FACTORY_RAM_PROBE_PAGE_BYTES - 1);
        if used[off] {
            continue;
        }
        used[off] = true;
        sample_phys[count] = page_phys + off as u64;
        sample_values[count] = page[off];
        count += 1;
    }

    FactoryRamProbeState {
        page_phys,
        sample_count: count,
        sample_phys,
        sample_values,
    }
}

#[embassy_executor::task]
pub async fn boot_factory_ram_probe_task() {
    let Some(page_phys) = crate::phys::alloc_phys_range(
        FACTORY_RAM_PROBE_PAGE_BYTES,
        FACTORY_RAM_PROBE_PAGE_BYTES,
        0x0010_0000,
        None,
    ) else {
        crate::log_warn!(target: "boot"; "factory-ram-probe: no PMM page available\n");
        return;
    };

    let state = build_random_sample(page_phys);
    crate::log_info!(
        target: "boot";
        "factory-ram-probe: reserved phys=0x{:X} sampled={} hold={}s\n",
        state.page_phys,
        state.sample_count,
        FACTORY_RAM_PROBE_DELAY_SECS
    );
    *FACTORY_RAM_PROBE_STATE.lock() = Some(state);

    Timer::after(EmbassyDuration::from_secs(FACTORY_RAM_PROBE_DELAY_SECS)).await;

    let snapshot = {
        let mut guard = FACTORY_RAM_PROBE_STATE.lock();
        let Some(state) = guard.as_mut() else {
            return;
        };
        FactoryRamProbeSnapshot {
            page_phys: state.page_phys,
            page_bytes: FACTORY_RAM_PROBE_PAGE_BYTES,
            sample_count: min(state.sample_count, FACTORY_RAM_PROBE_SAMPLE_BYTES),
            sample_phys: state.sample_phys,
            sample_values: state.sample_values,
        }
    };

    if !crate::phys::free_phys_range(snapshot.page_phys, FACTORY_RAM_PROBE_PAGE_BYTES) {
        crate::log_warn!(
            target: "boot";
            "factory-ram-probe: failed to free phys=0x{:X} size=0x{:X}\n",
            snapshot.page_phys,
            FACTORY_RAM_PROBE_PAGE_BYTES
        );
    }

    *FACTORY_RAM_PROBE_STATE.lock() = None;
    crate::log_info!(
        target: "boot";
        "factory-ram-probe: released phys=0x{:X} sampled={}\n",
        snapshot.page_phys,
        snapshot.sample_count
    );
}
