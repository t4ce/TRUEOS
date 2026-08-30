#[cfg(not(test))]
use alloc::{collections::VecDeque, vec::Vec};
#[cfg(test)]
use std::{collections::VecDeque, vec::Vec};

struct TcpTxChunk {
    bytes: Vec<u8>,
    cursor: usize,
}

impl TcpTxChunk {
    #[inline]
    fn remaining(&self) -> &[u8] {
        &self.bytes[self.cursor..]
    }

    #[inline]
    fn advance(&mut self, sent: usize) {
        self.cursor = self.cursor.saturating_add(sent).min(self.bytes.len());
    }

    #[inline]
    fn is_complete(&self) -> bool {
        self.cursor == self.bytes.len()
    }
}

/// TCP payloads retained in submission order until smoltcp accepts them.
///
/// Each queue entry keeps ownership of its original allocation. Partial sends
/// advance only the first chunk's cursor, while `queued_bytes` preserves the
/// byte-accurate backlog metric used by adapter diagnostics.
pub(crate) struct TcpTxQueue {
    chunks: VecDeque<TcpTxChunk>,
    queued_bytes: usize,
}

impl TcpTxQueue {
    pub(crate) fn new() -> Self {
        Self {
            chunks: VecDeque::new(),
            queued_bytes: 0,
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.queued_bytes
    }

    pub(crate) fn push(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        self.queued_bytes = self.queued_bytes.saturating_add(bytes.len());
        self.chunks.push_back(TcpTxChunk { bytes, cursor: 0 });
    }

    #[inline]
    pub(crate) fn front(&self) -> Option<&[u8]> {
        self.chunks.front().map(TcpTxChunk::remaining)
    }

    pub(crate) fn advance(&mut self, sent: usize) {
        let complete = {
            let Some(front) = self.chunks.front_mut() else {
                return;
            };
            debug_assert!(sent <= front.remaining().len());
            front.advance(sent);
            front.is_complete()
        };
        self.queued_bytes = self.queued_bytes.saturating_sub(sent);
        if complete {
            let _ = self.chunks.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TcpTxQueue;

    #[test]
    fn retains_owned_chunks_and_advances_partial_sends() {
        let mut queue = TcpTxQueue::new();
        let first = Vec::from(&b"abc"[..]);
        let first_allocation = first.as_ptr();

        queue.push(first);
        queue.push(Vec::from(&b"defg"[..]));

        assert_eq!(queue.len(), 7);
        assert_eq!(queue.front(), Some(&b"abc"[..]));
        assert_eq!(queue.front().unwrap().as_ptr(), first_allocation);

        queue.advance(2);
        assert_eq!(queue.len(), 5);
        assert_eq!(queue.front(), Some(&b"c"[..]));

        queue.advance(1);
        assert_eq!(queue.len(), 4);
        assert_eq!(queue.front(), Some(&b"defg"[..]));

        queue.advance(4);
        assert_eq!(queue.len(), 0);
        assert!(queue.is_empty());
        assert_eq!(queue.front(), None);
    }

    #[test]
    fn ignores_empty_submissions_without_disturbing_fifo_order() {
        let mut queue = TcpTxQueue::new();
        queue.push(Vec::new());
        queue.push(Vec::from(&b"small"[..]));
        queue.push(Vec::new());
        queue.push(vec![0xA5; 128 * 1024]);

        assert_eq!(queue.len(), 128 * 1024 + 5);
        assert_eq!(queue.front(), Some(&b"small"[..]));

        queue.advance(5);
        assert_eq!(queue.front().map(<[u8]>::len), Some(128 * 1024));
        assert_eq!(queue.len(), 128 * 1024);
    }
}
