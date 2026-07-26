use core::sync::atomic::{AtomicU8, Ordering};

pub const DEFAULT_TIMEZONE_NAME: &str = "UTC";

const TIMEZONE_UNINITIALIZED: u8 = u8::MAX;
const TIMEZONE_UTC: u8 = 0;
const TIMEZONE_EUROPE_BERLIN: u8 = 1;

static CURRENT_TIMEZONE: AtomicU8 = AtomicU8::new(TIMEZONE_UNINITIALIZED);

#[inline]
pub fn current_language_code() -> &'static str {
    trueos_locale::DEFAULT_LANGUAGE_CODE
}

#[inline]
pub fn current_intl_locale_code() -> &'static str {
    current_intl_profile().code
}

#[inline]
pub fn current_intl_profile() -> &'static trueos_locale::IntlLocaleProfile {
    trueos_locale::intl_locale_profile(trueos_locale::DEFAULT_INTL_LOCALE)
}

fn timezone_id(name: &str) -> Option<u8> {
    match name {
        "UTC" | "Etc/UTC" | "GMT" | "Etc/GMT" => Some(TIMEZONE_UTC),
        "Europe/Berlin" | "CET" => Some(TIMEZONE_EUROPE_BERLIN),
        // RFC 4833 DHCP option 100 carries a POSIX TZ string. Accept the
        // canonical CET/CEST form without requiring an IANA database.
        value if value.starts_with("CET-1CEST,") => Some(TIMEZONE_EUROPE_BERLIN),
        _ => None,
    }
}

fn timezone_name(id: u8) -> &'static str {
    match id {
        TIMEZONE_EUROPE_BERLIN => "Europe/Berlin",
        _ => DEFAULT_TIMEZONE_NAME,
    }
}

fn timezone_from_cmdline(cmdline: &str) -> Option<u8> {
    cmdline.split_ascii_whitespace().find_map(|arg| {
        ["timezone=", "tz=", "TRUEOS_TIMEZONE=", "TZ="]
            .iter()
            .find_map(|prefix| arg.strip_prefix(prefix))
            .and_then(timezone_id)
    })
}

pub fn prime_bootloader_timezone() {
    if CURRENT_TIMEZONE.load(Ordering::Acquire) != TIMEZONE_UNINITIALIZED {
        return;
    }

    let timezone = crate::limine::executable_cmdline()
        .and_then(timezone_from_cmdline)
        .unwrap_or(TIMEZONE_UTC);
    let _ = CURRENT_TIMEZONE.compare_exchange(
        TIMEZONE_UNINITIALIZED,
        timezone,
        Ordering::AcqRel,
        Ordering::Acquire,
    );
}

/// Apply an RFC 4833 DHCP timezone value if TRUEOS understands it.
///
/// Option 101 supplies an IANA name; option 100 supplies a POSIX TZ string.
/// Unknown zones are left unchanged rather than silently using a wrong offset.
pub fn set_timezone_from_network(name: &str) -> bool {
    let Some(timezone) = timezone_id(name) else {
        return false;
    };
    CURRENT_TIMEZONE.store(timezone, Ordering::Release);
    true
}

fn current_timezone_id() -> u8 {
    match CURRENT_TIMEZONE.load(Ordering::Acquire) {
        TIMEZONE_UNINITIALIZED => TIMEZONE_UTC,
        timezone => timezone,
    }
}

#[inline]
pub fn current_timezone_name() -> &'static str {
    timezone_name(current_timezone_id())
}

fn civil_year_from_unix_seconds(unix_seconds: u64) -> i32 {
    let z = (unix_seconds / 86_400) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    if mp + if mp < 10 { 3 } else { -9 } <= 2 {
        year += 1;
    }
    year
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i32;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day as i32 - 1;
    let day_of_era = yoe * 365 + yoe / 4 - yoe / 100 + day_of_year;
    era as i64 * 146_097 + day_of_era as i64 - 719_468
}

fn last_sunday(year: i32, month: u32, last_day: u32) -> u32 {
    let days = days_from_civil(year, month, last_day);
    let weekday = (days + 4).rem_euclid(7) as u32;
    last_day - weekday
}

fn berlin_utc_offset_seconds(unix_seconds: u64) -> i32 {
    let year = civil_year_from_unix_seconds(unix_seconds);
    let march_transition =
        days_from_civil(year, 3, last_sunday(year, 3, 31)) as u64 * 86_400 + 3_600;
    let october_transition =
        days_from_civil(year, 10, last_sunday(year, 10, 31)) as u64 * 86_400 + 3_600;

    if unix_seconds >= march_transition && unix_seconds < october_transition {
        2 * 3_600
    } else {
        3_600
    }
}

#[inline]
pub fn utc_offset_seconds(unix_seconds: u64) -> i32 {
    match current_timezone_id() {
        TIMEZONE_EUROPE_BERLIN => berlin_utc_offset_seconds(unix_seconds),
        _ => 0,
    }
}

#[inline]
pub fn local_unix_time_seconds(unix_seconds: u64) -> u64 {
    let offset = utc_offset_seconds(unix_seconds);
    if offset >= 0 {
        unix_seconds.saturating_add(offset as u64)
    } else {
        unix_seconds.saturating_sub(offset.unsigned_abs() as u64)
    }
}

#[inline]
pub fn env_var(key: &str) -> Option<&'static str> {
    match key {
        "LANG" | "LANGUAGE" | "TRUEOS_LANGUAGE" => Some(current_language_code()),
        "LC_ALL" | "LC_COLLATE" | "LC_CTYPE" | "LC_MESSAGES" | "LC_MONETARY" | "LC_NUMERIC"
        | "LC_TIME" | "TRUEOS_LOCALE" => Some(current_intl_locale_code()),
        "TZ" | "TRUEOS_TIMEZONE" => Some(current_timezone_name()),
        _ => None,
    }
}
