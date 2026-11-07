//! Ownership retained across bounded TLS queues. No socket or TLS policy lives here.
extern crate alloc;
use alloc::{collections::VecDeque, vec::Vec};

pub(crate) struct Delivery<T> {
    pending: VecDeque<T>,
}
impl<T> Delivery<T> {
    pub(crate) fn new() -> Self {
        Self {
            pending: VecDeque::new(),
        }
    }
    pub(crate) fn blocked(&self) -> bool {
        !self.pending.is_empty()
    }
    pub(crate) fn clear(&mut self) {
        self.pending.clear();
    }
    pub(crate) fn emit(&mut self, item: T, mut enqueue: impl FnMut(T) -> Result<(), T>) {
        if self.pending.is_empty() {
            if let Err(item) = enqueue(item) {
                self.pending.push_back(item);
            }
        } else {
            self.pending.push_back(item);
        }
    }
    pub(crate) fn flush(&mut self, mut enqueue: impl FnMut(T) -> Result<(), T>) -> bool {
        while let Some(item) = self.pending.pop_front() {
            if let Err(item) = enqueue(item) {
                self.pending.push_front(item);
                return false;
            }
        }
        true
    }
}

pub(crate) struct PendingSend {
    bytes: Vec<u8>,
    offset: usize,
}
impl PendingSend {
    pub(crate) fn new() -> Self {
        Self {
            bytes: Vec::new(),
            offset: 0,
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
    pub(crate) fn stage(&mut self, bytes: Vec<u8>) {
        assert!(self.is_empty());
        self.bytes = bytes;
        self.offset = 0;
    }
    pub(crate) fn flush(
        &mut self,
        chunk: usize,
        mut send: impl FnMut(&[u8]) -> Result<(), ()>,
    ) -> bool {
        assert!(chunk != 0);
        while self.offset < self.bytes.len() {
            let end = self.bytes.len().min(self.offset.saturating_add(chunk));
            if send(&self.bytes[self.offset..end]).is_err() {
                return false;
            }
            self.offset = end;
        }
        self.bytes.clear();
        self.offset = 0;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejected_data_and_terminal_are_retained_in_order() {
        let mut delivery = Delivery::new();
        let mut output = Vec::new();
        for value in [
            b"opened".as_slice(),
            b"connected",
            b"data-A",
            b"data-B",
            b"error",
        ] {
            delivery.emit(value.to_vec(), Err);
        }
        assert!(delivery.blocked());
        for _ in 0..5 {
            let mut accepted = false;
            delivery.flush(|value| {
                if accepted {
                    Err(value)
                } else {
                    accepted = true;
                    output.push(value);
                    Ok(())
                }
            });
        }
        assert!(!delivery.blocked());
        assert_eq!(
            output,
            vec![
                b"opened".to_vec(),
                b"connected".to_vec(),
                b"data-A".to_vec(),
                b"data-B".to_vec(),
                b"error".to_vec()
            ]
        );
    }
    #[test]
    fn partial_ciphertext_submission_never_replays_prefix() {
        let bytes: Vec<_> = (0..100_003).map(|v| (v % 251) as u8).collect();
        let mut pending = PendingSend::new();
        pending.stage(bytes.clone());
        let mut out = Vec::new();
        while !pending.is_empty() {
            let mut budget = 1;
            pending.flush(32768, |part| {
                if budget == 0 {
                    Err(())
                } else {
                    budget -= 1;
                    out.extend_from_slice(part);
                    Ok(())
                }
            });
        }
        assert_eq!(out, bytes);
    }
    #[test]
    fn full_queue_consumes_no_ciphertext() {
        let mut pending = PendingSend::new();
        pending.stage(b"secret".to_vec());
        assert!(!pending.flush(2, |_| Err(())));
        let mut out = Vec::new();
        assert!(pending.flush(2, |part| {
            out.extend_from_slice(part);
            Ok(())
        }));
        assert_eq!(out, b"secret");
    }
}
