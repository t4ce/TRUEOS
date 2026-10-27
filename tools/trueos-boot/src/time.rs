//! Firmware RTC conversion, matching the existing boot path's UTC convention.
pub fn unix_seconds(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Option<i64> {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [31, if leap {29} else {28}, 31,30,31,30,31,31,30,31,30,31];
    if year < 1970 || !(1..=12).contains(&month) || day == 0
        || day > month_days[month as usize - 1] || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year / 400;
    let yoe = year - era * 400;
    let month = i64::from(month);
    let doy = (153 * (month + if month > 2 {-3} else {9}) + 2) / 5 + i64::from(day) - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    Some(days * 86400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn epoch_leap_year_and_invalid_dates() {
        assert_eq!(unix_seconds(1970,1,1,0,0,0), Some(0));
        assert_eq!(unix_seconds(2000,1,1,0,0,0), Some(946684800));
        assert_eq!(unix_seconds(2024,2,29,0,0,0), Some(1709164800));
        assert_eq!(unix_seconds(2025,2,29,0,0,0), None);
        assert_eq!(unix_seconds(2100,2,29,0,0,0), None);
    }
}
