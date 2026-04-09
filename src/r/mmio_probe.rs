use core::arch::x86_64::_rdtsc;
use core::ptr;

use embassy_time::{Duration as EmbassyDuration, Timer};
use x86_64::instructions::interrupts;

const TARGET_PHYS_ADDR: u64 = 0xE06A0A50;
const PROBE_DELAY_SECS: u64 = 10;
const PAGE_SIZE: usize = 0x1000;
const ENTRY_SIZE: usize = 0x10;
const TABLE_START_PHYS: u64 = 0xE06A09E0;
const TABLE_END_EXCL_PHYS: u64 = 0xE06A0AD0;
const TABLE_MAX_ENTRIES: usize = 32;
const TABLE_PAGE_SCAN_RADIUS: i64 = 4;
const TABLE_PAGE_MIN_NONZERO: usize = 6;
const TABLE_PAGE_MIN_RUN: usize = 4;
const VENDOR_TOGGLE_LOW: u32 = 0x8400_0200;
const VENDOR_TOGGLE_HIGH: u32 = 0x8400_0201;
const WAVE_LED_COUNT: usize = 32;
const WAVE_STEPS: [u8; 9] = [0x00, 0x08, 0x20, 0x60, 0xFF, 0x60, 0x20, 0x08, 0x00];
const WAVE_SWEEPS: usize = 6;
const WAVE_FRAME_INTERVAL_MS: u64 = 100;
const WS2812_BIT_TOTAL_NS: u64 = 1_250;
const WS2812_T0H_NS: u64 = 400;
const WS2812_T1H_NS: u64 = 800;
const WS2812_RESET_NS: u64 = 300_000;

#[derive(Clone, Copy)]
struct PadEntry {
    phys: u64,
    dw0: u32,
    dw1: u32,
    dw2: u32,
    dw3: u32,
}

#[derive(Clone, Copy)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

fn read_u32(base: *mut u8, offset: usize) -> u32 {
    unsafe { ptr::read_volatile(base.add(offset) as *const u32) }
}

fn read_entry(base: *mut u8, offset: usize, phys: u64) -> PadEntry {
    PadEntry {
        phys,
        dw0: read_u32(base, offset),
        dw1: read_u32(base, offset + 4),
        dw2: read_u32(base, offset + 8),
        dw3: read_u32(base, offset + 12),
    }
}

fn classify_dw0(dw0: u32) -> &'static str {
    match dw0 {
        0x0000_0000 => "zero/dead",
        VENDOR_TOGGLE_LOW | VENDOR_TOGGLE_HIGH => "vendor-bitbang-gpio",
        0x4288_0100 | 0x4400_0700 => "native-function-like",
        _ if (dw0 & 0x0000_0001) != 0 => "gpio-like",
        _ if (dw0 & 0x0000_00FF) == 0 && (dw0 & 0x4000_0000) != 0 => "native-function-like",
        _ if (dw0 & 0xC000_0000) == 0xC000_0000 => "locked-ish",
        _ => "other-config",
    }
}

fn matches_vendor_toggle_surface(dw0: u32) -> bool {
    (dw0 & !1) == VENDOR_TOGGLE_LOW
}

fn write_u32(base: *mut u8, offset: usize, value: u32) {
    unsafe { ptr::write_volatile(base.add(offset) as *mut u32, value) }
}

fn cycles_from_ns(tsc_hz: u64, ns: u64) -> u64 {
    (((tsc_hz as u128) * (ns as u128)) / 1_000_000_000u128) as u64
}

fn low_ns_from_high(high_ns: u64) -> u64 {
    WS2812_BIT_TOTAL_NS.saturating_sub(high_ns).max(1)
}

fn busy_wait_cycles(cycles: u64) {
    if cycles == 0 {
        return;
    }
    let start = unsafe { _rdtsc() };
    loop {
        let now = unsafe { _rdtsc() };
        if now.wrapping_sub(start) >= cycles {
            break;
        }
        core::hint::spin_loop();
    }
}

fn send_ws2812_bit(mmio: *mut u8, low_value: u32, high_value: u32, tsc_hz: u64, bit_is_one: bool) {
    let high_cycles = if bit_is_one {
        cycles_from_ns(tsc_hz, WS2812_T1H_NS)
    } else {
        cycles_from_ns(tsc_hz, WS2812_T0H_NS)
    };
    let low_cycles = if bit_is_one {
        cycles_from_ns(tsc_hz, low_ns_from_high(WS2812_T1H_NS))
    } else {
        cycles_from_ns(tsc_hz, low_ns_from_high(WS2812_T0H_NS))
    };

    write_u32(mmio, 0, high_value);
    busy_wait_cycles(high_cycles.max(1));
    write_u32(mmio, 0, low_value);
    busy_wait_cycles(low_cycles.max(1));
}

fn send_ws2812_byte(mmio: *mut u8, low_value: u32, high_value: u32, tsc_hz: u64, value: u8) {
    for shift in (0..8).rev() {
        let bit_is_one = ((value >> shift) & 1) != 0;
        send_ws2812_bit(mmio, low_value, high_value, tsc_hz, bit_is_one);
    }
}

fn send_ws2812_grb(
    mmio: *mut u8,
    low_value: u32,
    high_value: u32,
    tsc_hz: u64,
    g: u8,
    r: u8,
    b: u8,
) {
    send_ws2812_byte(mmio, low_value, high_value, tsc_hz, g);
    send_ws2812_byte(mmio, low_value, high_value, tsc_hz, r);
    send_ws2812_byte(mmio, low_value, high_value, tsc_hz, b);
}

fn build_wave_frame(frame_idx: usize) -> [Rgb; WAVE_LED_COUNT] {
    let mut pixels = [Rgb { r: 0, g: 0, b: 0 }; WAVE_LED_COUNT];
    let span = WAVE_LED_COUNT;
    if span == 0 {
        return pixels;
    }

    let center = frame_idx % span;
    let half = WAVE_STEPS.len() / 2;
    for (step_idx, level) in WAVE_STEPS.iter().copied().enumerate() {
        let pos = (center + span + step_idx - half) % span;
        pixels[pos] = Rgb {
            r: level / 6,
            g: level / 8,
            b: level,
        };
    }
    pixels
}

fn send_wave_frame(mmio: *mut u8, low_value: u32, high_value: u32, tsc_hz: u64, frame_idx: usize) {
    let reset_cycles = cycles_from_ns(tsc_hz, WS2812_RESET_NS).max(1);
    let pixels = build_wave_frame(frame_idx);

    interrupts::without_interrupts(|| {
        write_u32(mmio, 0, low_value);
        busy_wait_cycles(reset_cycles);

        for pixel in pixels {
            send_ws2812_grb(mmio, low_value, high_value, tsc_hz, pixel.g, pixel.r, pixel.b);
        }

        write_u32(mmio, 0, low_value);
        busy_wait_cycles(reset_cycles);
    });
}

async fn run_wave_sender() {
    let Ok(mapped) = crate::pci::mmio::map_mmio_region_exact(TARGET_PHYS_ADDR, 4) else {
        crate::log!("boot-probe:mmio: wave map failed target=0x{:016X}\n", TARGET_PHYS_ADDR);
        return;
    };

    let live_dw0 = read_u32(mapped.as_ptr(), 0);
    let low_value = live_dw0 & !1;
    let high_value = low_value | 1;
    let tsc_hz = crate::r::time::tsc_hz();
    let total_frames = WAVE_LED_COUNT.saturating_mul(WAVE_SWEEPS);

    crate::log!(
        "boot-probe:mmio: wave start target=0x{:016X} led_count={} frames={} interval_ms={} bit_total_ns={} t0h_ns={} t1h_ns={} latch_low_us={} low=0x{:08X} high=0x{:08X} live=0x{:08X} vendor_surface_match={} tsc_hz={}\n",
        TARGET_PHYS_ADDR,
        WAVE_LED_COUNT,
        total_frames,
        WAVE_FRAME_INTERVAL_MS,
        WS2812_BIT_TOTAL_NS,
        WS2812_T0H_NS,
        WS2812_T1H_NS,
        WS2812_RESET_NS / 1000,
        low_value,
        high_value,
        live_dw0,
        matches_vendor_toggle_surface(live_dw0),
        tsc_hz
    );

    for frame_idx in 0..total_frames {
        send_wave_frame(mapped.as_ptr(), low_value, high_value, tsc_hz, frame_idx);
        Timer::after(EmbassyDuration::from_millis(WAVE_FRAME_INTERVAL_MS)).await;
    }

    crate::log!(
        "boot-probe:mmio: wave done target=0x{:016X} led_count={} frames={} order=GRB pattern=moving-wave\n",
        TARGET_PHYS_ADDR,
        WAVE_LED_COUNT,
        total_frames
    );
}

fn log_pad_entry(entry: PadEntry, target_pad_id: Option<u32>) {
    let target_mark = if entry.phys == TARGET_PHYS_ADDR {
        " target-phys"
    } else if Some(entry.dw1) == target_pad_id {
        " target-pad-id"
    } else {
        ""
    };
    crate::log!(
        "boot-probe:mmio: entry phys=0x{:016X} dw0=0x{:08X} dw1=0x{:08X} dw2=0x{:08X} dw3=0x{:08X} class={}{}\n",
        entry.phys,
        entry.dw0,
        entry.dw1,
        entry.dw2,
        entry.dw3,
        classify_dw0(entry.dw0),
        target_mark
    );
}

fn log_dw0_clusters(entries: &[PadEntry], count: usize) {
    let mut values = [0u32; TABLE_MAX_ENTRIES];
    let mut counts = [0usize; TABLE_MAX_ENTRIES];
    let mut used = 0usize;

    for entry in entries.iter().take(count) {
        let mut slot = None;
        for idx in 0..used {
            if values[idx] == entry.dw0 {
                slot = Some(idx);
                break;
            }
        }
        let idx = match slot {
            Some(idx) => idx,
            None => {
                values[used] = entry.dw0;
                counts[used] = 0;
                let idx = used;
                used += 1;
                idx
            }
        };
        counts[idx] = counts[idx].saturating_add(1);
    }

    crate::log!("boot-probe:mmio: dw0 clusters begin\n");
    for idx in 0..used {
        crate::log!(
            "boot-probe:mmio: dw0 cluster value=0x{:08X} count={} class={}\n",
            values[idx],
            counts[idx],
            classify_dw0(values[idx])
        );
    }
    crate::log!("boot-probe:mmio: dw0 clusters end\n");
}

fn parse_pad_table() -> Option<(u32, [PadEntry; TABLE_MAX_ENTRIES], usize)> {
    let page_phys = TABLE_START_PHYS & !((PAGE_SIZE as u64) - 1);
    let mapped = crate::pci::mmio::map_mmio_region_exact(page_phys, PAGE_SIZE).ok()?;
    let table_len = (TABLE_END_EXCL_PHYS - TABLE_START_PHYS) as usize;
    let table_off = (TABLE_START_PHYS - page_phys) as usize;
    let target_off = (TARGET_PHYS_ADDR - page_phys) as usize;
    let target_entry_off = target_off & !(ENTRY_SIZE - 1);

    let zero = PadEntry {
        phys: 0,
        dw0: 0,
        dw1: 0,
        dw2: 0,
        dw3: 0,
    };
    let mut entries = [zero; TABLE_MAX_ENTRIES];
    let mut count = 0usize;
    let mut target_pad_id = None;

    crate::log!(
        "boot-probe:mmio: parsing pad-table table=0x{:016X}..0x{:016X} target=0x{:016X}\n",
        TABLE_START_PHYS,
        TABLE_END_EXCL_PHYS.saturating_sub(ENTRY_SIZE as u64),
        TARGET_PHYS_ADDR
    );

    for rel in (0..table_len).step_by(ENTRY_SIZE) {
        let phys = TABLE_START_PHYS + rel as u64;
        let entry = read_entry(mapped.as_ptr(), table_off + rel, phys);
        if rel == target_entry_off.saturating_sub(table_off) {
            target_pad_id = Some(entry.dw1);
        }
        if count < TABLE_MAX_ENTRIES {
            entries[count] = entry;
            count += 1;
        }
    }

    for entry in entries.iter().take(count) {
        log_pad_entry(*entry, target_pad_id);
    }
    log_dw0_clusters(&entries, count);

    if let Some(target_pad_id) = target_pad_id {
        let target_dw0 = read_u32(mapped.as_ptr(), target_off);
        crate::log!(
            "boot-probe:mmio: target entry confirmed phys=0x{:016X} pad-id=0x{:X} class={} vendor_low=0x{:08X} vendor_high=0x{:08X} live_dw0=0x{:08X} bit0_is_data_edge=true note=dll-flips-only-bit0-with-software-timed-waits\n",
            TARGET_PHYS_ADDR,
            target_pad_id,
            classify_dw0(target_dw0),
            VENDOR_TOGGLE_LOW,
            VENDOR_TOGGLE_HIGH,
            target_dw0
        );
        crate::log!(
            "boot-probe:mmio: target entry vendor-surface-match={} preserved_config_bits=0x{:08X}\n",
            matches_vendor_toggle_surface(target_dw0),
            target_dw0 & !1
        );
        for entry in entries.iter().take(count) {
            if entry.dw1 == target_pad_id {
                continue;
            }
            if classify_dw0(entry.dw0) == "native-function-like" {
                crate::log!(
                    "boot-probe:mmio: native-like neighbor candidate phys=0x{:016X} pad-id=0x{:X} dw0=0x{:08X}\n",
                    entry.phys,
                    entry.dw1,
                    entry.dw0
                );
            }
        }
        Some((target_pad_id, entries, count))
    } else {
        crate::log!("boot-probe:mmio: target entry candidate not found inside structured table\n");
        None
    }
}

fn score_table_like_page(page_phys: u64) {
    let Some(mapped) = crate::pci::mmio::map_mmio_region_exact(page_phys, PAGE_SIZE).ok() else {
        return;
    };

    let mut nonzero_entries = 0usize;
    let mut zero_tail_entries = 0usize;
    let mut longest_run = 0usize;
    let mut current_run = 0usize;
    let mut first_pad: Option<u32> = None;
    let mut last_pad: Option<u32> = None;
    let mut prev_pad: Option<u32> = None;
    let mut target_pad_hits = 0usize;

    for offset in (0..PAGE_SIZE).step_by(ENTRY_SIZE) {
        let entry = read_entry(mapped.as_ptr(), offset, page_phys + offset as u64);
        if entry.dw0 != 0 {
            nonzero_entries = nonzero_entries.saturating_add(1);
        }
        if entry.dw2 == 0 && entry.dw3 == 0 {
            zero_tail_entries = zero_tail_entries.saturating_add(1);
        }
        if entry.dw1 <= 0x1FF {
            if first_pad.is_none() {
                first_pad = Some(entry.dw1);
            }
            last_pad = Some(entry.dw1);
            if let Some(prev) = prev_pad {
                if entry.dw1 == prev.saturating_add(1) {
                    current_run = current_run.saturating_add(1);
                } else {
                    current_run = 1;
                }
            } else {
                current_run = 1;
            }
            prev_pad = Some(entry.dw1);
            if current_run > longest_run {
                longest_run = current_run;
            }
            if entry.dw1 == 0x63 {
                target_pad_hits = target_pad_hits.saturating_add(1);
            }
        } else {
            current_run = 0;
            prev_pad = None;
        }
    }

    if nonzero_entries < TABLE_PAGE_MIN_NONZERO
        || longest_run < TABLE_PAGE_MIN_RUN
        || zero_tail_entries < TABLE_PAGE_MIN_NONZERO
    {
        return;
    }

    crate::log!(
        "boot-probe:mmio: nearby table-like page phys=0x{:016X} nonzero_entries={} zero_tail_entries={} longest_pad_run={} first_pad={:?} last_pad={:?} target_pad_hits={}\n",
        page_phys,
        nonzero_entries,
        zero_tail_entries,
        longest_run,
        first_pad,
        last_pad,
        target_pad_hits
    );
}

fn scan_nearby_pages() {
    let base_page = TABLE_START_PHYS & !((PAGE_SIZE as u64) - 1);
    crate::log!(
        "boot-probe:mmio: scanning nearby pages around 0x{:016X} for [dw0][pad-id][0][0] pattern\n",
        base_page
    );
    for delta in -TABLE_PAGE_SCAN_RADIUS..=TABLE_PAGE_SCAN_RADIUS {
        let page_phys = base_page.wrapping_add_signed(delta * PAGE_SIZE as i64);
        score_table_like_page(page_phys);
    }
}

#[embassy_executor::task]
pub async fn oneshot_mmio_probe_task() {
    Timer::after(EmbassyDuration::from_secs(PROBE_DELAY_SECS)).await;

    let parsed = parse_pad_table();
    scan_nearby_pages();

    if let Some((target_pad_id, entries, count)) = parsed {
        let mut gpio_like = 0usize;
        let mut vendor_bitbang_like = 0usize;
        let mut native_like = 0usize;
        let mut locked_like = 0usize;
        for entry in entries.iter().take(count) {
            match classify_dw0(entry.dw0) {
                "gpio-like" => gpio_like = gpio_like.saturating_add(1),
                "vendor-bitbang-gpio" => {
                    vendor_bitbang_like = vendor_bitbang_like.saturating_add(1)
                }
                "native-function-like" => native_like = native_like.saturating_add(1),
                "locked-ish" => locked_like = locked_like.saturating_add(1),
                _ => {}
            }
        }
        crate::log!(
            "boot-probe:mmio: summary target_pad_id=0x{:X} gpio_like={} vendor_bitbang_like={} native_like={} locked_like={} note=target-bank-is-pad-table-and-target-register-matches-dll-bitbang-shape\n",
            target_pad_id,
            gpio_like,
            vendor_bitbang_like,
            native_like,
            locked_like
        );
    }

    run_wave_sender().await;

    crate::log!("boot-probe:mmio: structured probe done\n");
}
