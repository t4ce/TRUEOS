const EFI_UNSPECIFIED_TIMEZONE: i16 = 0x07ff;
const EFI_TIME_ADJUST_DAYLIGHT: u8 = 0x01;
const EFI_TIME_IN_DAYLIGHT: u8 = 0x02;
const SECONDS_PER_DAY: i64 = 86_400;

/// UEFI `EFI_TIME`, including the explicitly specified padding bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct EfiTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub pad1: u8,
    pub nanosecond: u32,
    pub time_zone: i16,
    pub daylight: u8,
    pub pad2: u8,
}

fn days_in_month(month: u8, year: u16) -> Option<u8> {
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        _ => return None,
    };
    Some(days)
}

fn is_valid(time: &EfiTime) -> bool {
    let Some(month_days) = days_in_month(time.month, time.year) else {
        return false;
    };

    (1900..=9999).contains(&time.year)
        && (1..=month_days).contains(&time.day)
        && time.hour <= 23
        && time.minute <= 59
        && time.second <= 59
        && time.nanosecond <= 999_999_999
        && (time.time_zone == EFI_UNSPECIFIED_TIMEZONE || (-1440..=1440).contains(&time.time_zone))
        && time.daylight & !(EFI_TIME_ADJUST_DAYLIGHT | EFI_TIME_IN_DAYLIGHT) == 0
}

fn julian_day_number(day: u8, month: u8, year: u16) -> i64 {
    let day = i64::from(day);
    let month = i64::from(month);
    let year = i64::from(year);
    let a = (14 - month) / 12;
    let y = year + 4800 - a;
    let m = month + 12 * a - 3;

    day + (153 * m + 2) / 5 + 365 * y + y / 4 - y / 100 + y / 400 - 32_045
}

/// Convert a validated UEFI local calendar value to signed Unix seconds.
///
/// UEFI defines `LocalTime = UTC - TimeZone`, hence the positive timezone
/// adjustment below. An unspecified timezone preserves the firmware calendar
/// as-is, matching the only meaningful fallback available without an offset.
pub(crate) fn unix_seconds(time: &EfiTime) -> Option<i64> {
    if !is_valid(time) {
        return None;
    }

    let epoch_days = julian_day_number(1, 1, 1970);
    let current_days = julian_day_number(time.day, time.month, time.year);
    let mut timestamp = (current_days - epoch_days) * SECONDS_PER_DAY
        + i64::from(time.hour) * 3600
        + i64::from(time.minute) * 60
        + i64::from(time.second);

    if time.time_zone != EFI_UNSPECIFIED_TIMEZONE {
        timestamp += i64::from(time.time_zone) * 60;
    }

    // DateAtBoot and the TrueOS wall-clock ABI carry integral seconds. Keep
    // sub-second firmware time truncated instead of rounding into the future.
    Some(timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> EfiTime {
        EfiTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
            time_zone: EFI_UNSPECIFIED_TIMEZONE,
            ..EfiTime::default()
        }
    }

    #[test]
    fn efi_time_layout_matches_uefi() {
        assert_eq!(core::mem::size_of::<EfiTime>(), 16);
        assert_eq!(core::mem::align_of::<EfiTime>(), 4);
    }

    #[test]
    fn converts_epoch_and_leap_day() {
        assert_eq!(unix_seconds(&utc(1970, 1, 1, 0, 0, 0)), Some(0));
        assert_eq!(unix_seconds(&utc(2000, 2, 29, 0, 0, 0)), Some(951_782_400));
    }

    #[test]
    fn converts_local_time_to_utc() {
        let mut time = utc(1970, 1, 1, 12, 0, 0);
        time.time_zone = 60;
        assert_eq!(unix_seconds(&time), Some(13 * 3600));

        time.time_zone = -300;
        assert_eq!(unix_seconds(&time), Some(7 * 3600));
    }

    #[test]
    fn rejects_invalid_firmware_values() {
        let mut time = utc(2100, 2, 29, 0, 0, 0);
        assert_eq!(unix_seconds(&time), None);

        time = utc(2024, 1, 1, 0, 0, 0);
        time.time_zone = 1441;
        assert_eq!(unix_seconds(&time), None);

        time.time_zone = EFI_UNSPECIFIED_TIMEZONE;
        time.daylight = 0x80;
        assert_eq!(unix_seconds(&time), None);

        time.daylight = 0;
        time.nanosecond = 1_000_000_000;
        assert_eq!(unix_seconds(&time), None);
    }
}
