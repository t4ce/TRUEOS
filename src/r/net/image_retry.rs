//! Finite Retry-After policy and shared host pacing for image GETs only.
use alloc::{string::String, vec::Vec};

const MIN_RETRY_MS: u64 = 1_000;
const HOST_CAP: usize = 16;
const PACING_WINDOW_MS: u64 = 30_000;

fn http_date_seconds(value: &str) -> Option<u64> {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    use httpdate::time::UNIX_EPOCH;
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    use std::time::UNIX_EPOCH;
    httpdate::parse_http_date(value).ok()?.duration_since(UNIX_EPOCH).ok().map(|time| time.as_secs())
}

/// Invalid/missing headers still get one second of backoff. A valid date with
/// no time reference cannot be honored safely, so do not retry it early.
pub(super) fn retry_after_delay_ms(
    value: Option<&str>,
    response_date: Option<&str>,
    now_unix_seconds: Option<u64>,
) -> Option<u64> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else { return Some(MIN_RETRY_MS); };
    if value.bytes().all(|byte| byte.is_ascii_digit()) {
        let seconds = value.bytes().fold(0u64, |total, digit| total.saturating_mul(10).saturating_add(u64::from(digit - b'0')));
        return Some(seconds.saturating_mul(1_000).max(MIN_RETRY_MS));
    }
    let Some(retry_at) = http_date_seconds(value) else { return Some(MIN_RETRY_MS); };
    let now = now_unix_seconds.or_else(|| response_date.and_then(http_date_seconds))?;
    Some(retry_at.saturating_sub(now).saturating_mul(1_000).max(MIN_RETRY_MS))
}

struct HostBackoff {
    host: String,
    not_before_ms: u64,
    next_slot_ms: u64,
    expires_ms: u64,
}

pub(super) struct HostBackoffTable {
    hosts: Vec<HostBackoff>,
}

impl HostBackoffTable {
    pub(super) const fn new() -> Self { Self { hosts: Vec::new() } }

    pub(super) fn record_rate_limit(&mut self, host: &str, now_ms: u64, delay_ms: u64) {
        self.hosts.retain(|entry| entry.expires_ms > now_ms);
        let next_slot_ms = now_ms.saturating_add(delay_ms.max(MIN_RETRY_MS));
        let expires_ms = next_slot_ms.saturating_add(PACING_WINDOW_MS);
        if let Some(entry) = self.hosts.iter_mut().find(|entry| entry.host == host) {
            entry.not_before_ms = entry.not_before_ms.max(next_slot_ms);
            entry.next_slot_ms = entry.next_slot_ms.max(next_slot_ms);
            entry.expires_ms = entry.expires_ms.max(expires_ms);
            return;
        }
        if self.hosts.len() == HOST_CAP {
            let oldest = self.hosts.iter().enumerate().min_by_key(|(_, entry)| entry.expires_ms).map(|(index, _)| index).unwrap();
            self.hosts.swap_remove(oldest);
        }
        self.hosts.push(HostBackoff { host: String::from(host), not_before_ms: next_slot_ms, next_slot_ms, expires_ms });
    }

    /// Reserve under the shared lock, then wait without it. Failed deadline
    /// admission consumes no slot; a canceled admitted request may leave one.
    pub(super) fn reserve(&mut self, host: &str, now_ms: u64, deadline_ms: u64) -> Option<u64> {
        if now_ms >= deadline_ms { return None; }
        self.hosts.retain(|entry| entry.expires_ms > now_ms);
        let Some(entry) = self.hosts.iter_mut().find(|entry| entry.host == host) else { return Some(now_ms); };
        let slot = entry.next_slot_ms.max(now_ms);
        if slot >= deadline_ms { return None; }
        entry.next_slot_ms = slot.saturating_add(MIN_RETRY_MS);
        Some(slot)
    }

    /// A response from another in-flight worker may extend the server's quiet
    /// period after a request reserved its slot. Recheck before sending it.
    pub(super) fn cooldown_until(&self, host: &str, now_ms: u64) -> u64 {
        self.hosts.iter().find(|entry| entry.host == host && entry.expires_ms > now_ms)
            .map(|entry| entry.not_before_ms.max(now_ms)).unwrap_or(now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_seconds_are_bounded_without_early_retry_on_overflow() {
        for value in [None, Some(""), Some("invalid"), Some("-1"), Some("1.5"), Some("0")] {
            assert_eq!(retry_after_delay_ms(value, None, None), Some(1_000));
        }
        assert_eq!(retry_after_delay_ms(Some(" 2 "), None, None), Some(2_000));
        assert_eq!(retry_after_delay_ms(Some("999999999999999999999999"), None, None), Some(u64::MAX));
    }

    #[test]
    fn date_prefers_current_clock_then_server_date_and_never_uses_no_anchor() {
        let date = "Sun, 06 Nov 1994 08:49:37 GMT";
        let future = "Sun, 06 Nov 1994 08:49:40 GMT";
        let now = http_date_seconds(date).unwrap();
        assert_eq!(retry_after_delay_ms(Some(future), Some(date), Some(now + 1)), Some(2_000));
        assert_eq!(retry_after_delay_ms(Some(future), Some(date), None), Some(3_000));
        assert_eq!(retry_after_delay_ms(Some(future), None, None), None);
        assert_eq!(retry_after_delay_ms(Some(date), None, Some(now + 10)), Some(1_000));
    }

    #[test]
    fn shared_slots_space_requests_and_preserve_deadlines() {
        let mut table = HostBackoffTable::new();
        assert_eq!(table.reserve("one:443", 0, 15_000), Some(0));
        table.record_rate_limit("one:443", 100, 1_000);
        assert_eq!(table.reserve("one:443", 100, 15_000), Some(1_100));
        assert_eq!(table.reserve("one:443", 100, 15_000), Some(2_100));
        assert_eq!(table.reserve("other:443", 100, 15_000), Some(100));
        assert_eq!(table.reserve("one:443", 100, 3_100), None);
        assert_eq!(table.reserve("one:443", 100, 15_000), Some(3_100));
        assert_eq!(table.reserve("one:443", 32_000, 40_000), Some(32_000));
    }

    #[test]
    fn host_table_is_bounded_and_later_limits_do_not_shorten_a_cooldown() {
        let mut table = HostBackoffTable::new();
        table.record_rate_limit("long:443", 0, 10_000);
        table.record_rate_limit("long:443", 100, 1_000);
        assert_eq!(table.reserve("long:443", 100, 15_000), Some(10_000));
        for index in 0..40 { table.record_rate_limit(&alloc::format!("host{index}:443"), index, 1_000); }
        assert_eq!(table.hosts.len(), HOST_CAP);
    }
}
