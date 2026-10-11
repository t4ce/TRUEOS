//! Fixed IPv4 profile for the laptop's specific Realtek Ethernet adapter.
use core::sync::atomic::{AtomicU8, Ordering};

pub(crate) const MAC: [u8; 6] = [0xf4, 0x6b, 0x8c, 0x90, 0x36, 0x2c];
pub(crate) const ADDRESS: [u8; 4] = [192, 168, 178, 50];
pub(crate) const GATEWAY: [u8; 4] = [192, 168, 178, 1];
pub(crate) const PREFIX_LEN: u8 = 24;
// Pending, configured, failed. The display reads this without taking NIC locks.
static RESULT: AtomicU8 = AtomicU8::new(0);

pub(crate) fn matches(mac: [u8; 6], identity: Option<(u16, u16)>) -> bool {
    mac == MAC && matches!(identity, Some((0x0bda, 0x8153)) | Some((0x10ec, _)))
}

pub(crate) fn record_assignment(assigned: bool) {
    RESULT.store(if assigned { 1 } else { 2 }, Ordering::Release);
}

pub(crate) fn assignment_result() -> Option<bool> {
    match RESULT.load(Ordering::Acquire) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_laptop_realtek_adapter_matches() {
        assert!(matches(MAC, Some((0x0bda, 0x8153))));
        assert!(matches(MAC, Some((0x10ec, 0x8168))));
        assert!(!matches([0, 0xe0, 0x25, 0xcd, 0xe0, 0x3b], Some((0x0bda, 0x8153))));
        assert!(!matches(MAC, Some((0x8086, 0x15f3))));
        assert!(!matches(MAC, None));
    }
    #[test]
    fn assignment_requires_an_explicit_result() {
        assert_eq!(assignment_result(), None);
        record_assignment(true);
        assert_eq!(assignment_result(), Some(true));
        record_assignment(false);
        assert_eq!(assignment_result(), Some(false));
    }
}
