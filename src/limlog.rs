use crate::{debugconf, limine, long_mode_active};

pub fn log_limine_markers() {
    if long_mode_active() {
        debugconf!("64bit");
    }
    match limine::hhdm_offset() {
        Some(off) => debugconf!("LIMINE HHDM OK offset=0x{:X}\n", off),
        None => debugconf!("LIMINE HHDM MISSING\n"),
    }

    let req_ptr = &limine::MEMMAP_REQUEST as *const _ as usize;
    let resp_ptr = limine::MEMMAP_REQUEST
        .get_response()
        .map(|r| r as *const _ as usize)
        .unwrap_or(0);
    if let Some(entries) = limine::memmap_entries() {
        debugconf!(
            "LIMINE MEMMAP OK entries={} req=0x{:X} resp=0x{:X}\n",
            entries.len(),
            req_ptr,
            resp_ptr
        );
    } else {
        debugconf!(
            "LIMINE MEMMAP MISSING req=0x{:X} resp=0x{:X}\n",
            req_ptr,
            resp_ptr
        );
    }

    match limine::boot_timestamp_secs() {
        Some(ts) => {
            let (year, month, day, hour, minute, second) = unix_timestamp_to_ymdhms(ts);
            debugconf!(
                "LIMINE DATE {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC (ts={})\n",
                year,
                month,
                day,
                hour,
                minute,
                second,
                ts
            );
        }
        None => debugconf!("LIMINE DATE MISSING\n"),
    }

    match limine::bootloader_performance() {
        Some(perf) => debugconf!(
            "LIMINE PERF reset={}us init={}us exec={}us\n",
            perf.reset_usec(),
            perf.init_usec(),
            perf.exec_usec()
        ),
        None => debugconf!("LIMINE PERF MISSING\n"),
    }

    let req_ptr = &limine::MEMMAP_REQUEST as *const _ as usize;
    let resp_ptr = limine::MEMMAP_REQUEST
        .get_response()
        .map(|r| r as *const _ as usize)
        .unwrap_or(0);
    if let Some(entries) = limine::memmap_entries() {
        for entry in entries {
            debugconf!(
                "memmap {:016X}-{:016X} len=0x{:X} type={}\n",
                entry.base,
                entry.base + entry.length,
                entry.length,
                limine::memmap_type_name(entry.entry_type)
            );
        }
    }
}

fn unix_timestamp_to_ymdhms(ts: u64) -> (u32, u8, u8, u8, u8, u8) {
    const SECS_PER_MIN: u64 = 60;
    const SECS_PER_HOUR: u64 = 60 * SECS_PER_MIN;
    const SECS_PER_DAY: u64 = 24 * SECS_PER_HOUR;

    let mut days = ts / SECS_PER_DAY;
    let mut rem = ts % SECS_PER_DAY;

    let hour = (rem / SECS_PER_HOUR) as u8;
    rem %= SECS_PER_HOUR;
    let minute = (rem / SECS_PER_MIN) as u8;
    let second = (rem % SECS_PER_MIN) as u8;

    let mut year: u32 = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366u64 } else { 365u64 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_lengths = month_lengths(year);
    let mut month_idx = 0;
    while month_idx < month_lengths.len() {
        let len = month_lengths[month_idx] as u64;
        if days < len {
            let day = (days + 1) as u8;
            return (year, (month_idx + 1) as u8, day, hour, minute, second);
        }
        days -= len;
        month_idx += 1;
    }

    (year, 12, 31, hour, minute, second)
}

fn month_lengths(year: u32) -> [u8; 12] {
    if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    }
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
