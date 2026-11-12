//! Bounded, transactional access-unit upload used by live video ingress.
use alloc::{collections::VecDeque, vec::Vec};

pub(super) const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_UNITS: usize = 8;
pub(super) const INVALID: i32 = -3;
pub(super) const BUSY: i32 = -4;

#[derive(Default)]
pub(super) struct AccessUnits {
    queue: VecDeque<Vec<u8>>,
    queued_bytes: usize,
    upload: Option<(usize, Vec<u8>)>,
    first: bool,
}
impl AccessUnits {
    pub(super) fn new() -> Self {
        Self {
            first: true,
            ..Self::default()
        }
    }
    pub(super) fn begin(&mut self, bytes: usize) -> i32 {
        if bytes == 0 || bytes > MAX_BYTES || self.upload.is_some() {
            return INVALID;
        }
        if self.queue.len() >= MAX_UNITS || self.queued_bytes + bytes > MAX_BYTES {
            return BUSY;
        }
        let mut upload = Vec::new();
        if upload.try_reserve_exact(bytes).is_err() {
            return BUSY;
        }
        self.upload = Some((bytes, upload));
        0
    }
    pub(super) fn write(&mut self, offset: usize, bytes: &[u8]) -> i32 {
        let Some((total, upload)) = &mut self.upload else {
            return INVALID;
        };
        if bytes.is_empty() || offset != upload.len() || bytes.len() > *total - upload.len() {
            return INVALID;
        }
        upload.extend_from_slice(bytes);
        0
    }
    pub(super) fn commit(&mut self) -> i32 {
        let Some((total, bytes)) = self.upload.as_ref() else {
            return INVALID;
        };
        if *total != bytes.len() || !valid_access_unit(bytes, self.first) {
            return INVALID;
        }
        let (_, bytes) = self.upload.take().unwrap();
        self.queued_bytes += bytes.len();
        self.queue.push_back(bytes);
        self.first = false;
        0
    }
    pub(super) fn abort(&mut self) {
        self.upload = None;
    }
    pub(super) fn pop(&mut self) -> Option<Vec<u8>> {
        let unit = self.queue.pop_front()?;
        self.queued_bytes -= unit.len();
        Some(unit)
    }
    pub(super) fn clear(&mut self) {
        self.queue.clear();
        self.queued_bytes = 0;
        self.upload = None;
    }
}

// Every transaction contains exactly one Annex-B picture. The first picture
// supplies its parameter sets and an IDR; the client reopens after packet loss.
fn valid_access_unit(bytes: &[u8], first: bool) -> bool {
    let mut sps = false;
    let mut pps = false;
    let mut idr = false;
    let mut picture = false;
    let mut found = false;
    for index in 0..bytes.len().saturating_sub(3) {
        if bytes[index..].starts_with(&[0, 0, 1]) {
            if !found && index > 1 {
                return false;
            }
            found = true;
            match bytes[index + 3] & 31 {
                1 => picture = true,
                5 => {
                    picture = true;
                    idr = true;
                }
                7 => sps = true,
                8 => pps = true,
                _ => {}
            }
        }
    }
    found && picture && (!first || (sps && pps && idr))
}

#[cfg(test)]
mod tests {
    use super::*;
    const IDR: &[u8] = &[0, 0, 0, 1, 0x67, 1, 0, 0, 1, 0x68, 1, 0, 0, 1, 0x65, 1];
    const P: &[u8] = &[0, 0, 1, 0x41, 1];
    fn push(queue: &mut AccessUnits, bytes: &[u8]) -> i32 {
        assert_eq!(queue.begin(bytes.len()), 0);
        assert_eq!(queue.write(0, bytes), 0);
        queue.commit()
    }
    #[test]
    fn rejects_partial_or_out_of_order_uploads_without_publishing() {
        let mut queue = AccessUnits::new();
        assert_eq!(queue.begin(IDR.len()), 0);
        assert_eq!(queue.write(1, IDR), INVALID);
        assert_eq!(queue.write(0, &IDR[..4]), 0);
        assert_eq!(queue.commit(), INVALID);
        assert!(queue.pop().is_none());
        assert_eq!(queue.write(4, &IDR[4..]), 0);
        assert_eq!(queue.commit(), 0);
        assert_eq!(queue.pop().unwrap(), IDR);
    }
    #[test]
    fn first_picture_requires_headers_and_idr() {
        let mut queue = AccessUnits::new();
        assert_eq!(push(&mut queue, P), INVALID);
        queue.abort();
        assert_eq!(push(&mut queue, IDR), 0);
        assert_eq!(push(&mut queue, P), 0);
    }
    #[test]
    fn queue_count_is_bounded_and_recovers_after_consumption() {
        let mut queue = AccessUnits::new();
        assert_eq!(push(&mut queue, IDR), 0);
        for _ in 1..MAX_UNITS {
            assert_eq!(push(&mut queue, P), 0);
        }
        assert_eq!(queue.begin(P.len()), BUSY);
        assert!(queue.pop().is_some());
        assert_eq!(push(&mut queue, P), 0);
    }
    #[test]
    fn aggregate_bytes_include_pending_upload() {
        let mut queue = AccessUnits::new();
        assert_eq!(push(&mut queue, IDR), 0);
        assert_eq!(queue.begin(MAX_BYTES), BUSY);
        assert_eq!(queue.begin(MAX_BYTES - IDR.len()), 0);
        queue.abort();
        assert_eq!(queue.begin(0), INVALID);
        assert_eq!(queue.begin(MAX_BYTES + 1), INVALID);
    }
}
