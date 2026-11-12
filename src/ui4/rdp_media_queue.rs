//! One bounded media queue owned by an authenticated SSH RDP session.
//! Session IDs are host-issued lifetimes, never peer-provided credentials.
extern crate alloc;
use alloc::{collections::VecDeque, vec::Vec};

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_PACKETS: usize = 16 * 1024;
pub(super) const MAX_PACKET_BYTES: usize = 1200;

pub(super) struct Queue {
    owner: Option<u64>,
    bytes: usize,
    packets: VecDeque<Vec<u8>>,
}

impl Queue {
    pub const fn new() -> Self {
        Self { owner: None, bytes: 0, packets: VecDeque::new() }
    }
    pub fn owner(&self) -> Option<u64> { self.owner }
    pub fn subscribe(&mut self, session: u64) -> bool {
        if session == 0 || self.owner.is_some_and(|owner| owner != session) { return false; }
        self.owner = Some(session);
        true
    }
    pub fn unsubscribe(&mut self, session: u64) {
        if self.owner == Some(session) {
            self.clear(session);
            self.owner = None;
        }
    }
    pub fn clear(&mut self, session: u64) {
        if self.owner == Some(session) {
            self.packets.clear();
            self.bytes = 0;
        }
    }
    pub fn push(&mut self, session: u64, packet: Vec<u8>) -> bool {
        if self.owner != Some(session) || packet.is_empty() || packet.len() > MAX_PACKET_BYTES
            || self.packets.len() >= MAX_PACKETS || packet.len() > MAX_BYTES.saturating_sub(self.bytes)
        { return false; }
        self.bytes += packet.len();
        self.packets.push_back(packet);
        true
    }
    pub fn pop(&mut self, session: u64) -> Option<Vec<u8>> {
        if self.owner != Some(session) { return None; }
        let packet = self.packets.pop_front()?;
        self.bytes -= packet.len();
        Some(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test] fn only_the_live_authenticated_owner_can_read_or_write() {
        let mut queue = Queue::new();
        assert!(!queue.subscribe(0));
        assert!(!queue.push(1, vec![1]));
        assert!(queue.subscribe(1));
        assert!(!queue.subscribe(2));
        assert!(!queue.push(2, vec![2]));
        assert!(queue.push(1, vec![3]));
        assert!(queue.pop(2).is_none());
        assert_eq!(queue.pop(1), Some(vec![3]));
    }
    #[test] fn disconnect_retires_all_queued_video_before_next_session() {
        let mut queue = Queue::new();
        queue.subscribe(1); queue.push(1, vec![1]);
        queue.unsubscribe(2);
        assert_eq!(queue.owner(), Some(1));
        queue.unsubscribe(1);
        assert_eq!(queue.owner(), None);
        assert!(queue.subscribe(2));
        assert!(queue.pop(2).is_none());
        assert!(!queue.push(1, vec![1]));
        queue.unsubscribe(1);
        assert_eq!(queue.owner(), Some(2));
    }
    #[test] fn packet_and_total_byte_limits_never_grow_on_failed_admission() {
        let mut queue = Queue::new(); queue.subscribe(1);
        assert!(!queue.push(1, vec![0; MAX_PACKET_BYTES + 1]));
        assert!(!queue.push(1, vec![]));
        let mut count = 0;
        while queue.push(1, vec![0; MAX_PACKET_BYTES]) { count += 1; }
        assert!(queue.bytes <= MAX_BYTES);
        assert!(count <= MAX_PACKETS);
        assert!(count > 4096, "one maximum encoded IDR must fit");
        let bytes = queue.bytes;
        assert!(!queue.push(1, vec![0; MAX_PACKET_BYTES]));
        assert_eq!(queue.bytes, bytes);
        assert!(queue.pop(1).is_some());
        assert!(queue.push(1, vec![0; MAX_PACKET_BYTES]));
    }
    #[test] fn aborted_stream_discards_dependencies_but_keeps_session_owner() {
        let mut queue = Queue::new(); queue.subscribe(1);
        queue.push(1, vec![1]); queue.push(1, vec![2]);
        queue.clear(2);
        assert_eq!(queue.packets.len(), 2);
        queue.clear(1);
        assert_eq!(queue.owner(), Some(1));
        assert!(queue.pop(1).is_none());
        assert!(queue.push(1, vec![3]));
        assert_eq!(queue.pop(1), Some(vec![3]));
    }
}
