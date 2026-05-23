use std::{collections::VecDeque, ops::Range};

use bytes::{Buf, Bytes};

use crate::{VarInt, range_set::RangeSet};

/// Buffer of outgoing retransmittable stream data
#[derive(Default, Debug)]
pub(super) struct SendBuffer {
    /// Data queued by the application but not yet acknowledged. May or may not have been sent.
    unacked_segments: VecDeque<Bytes>,
    /// Total size of `unacked_segments`
    unacked_len: usize,
    /// The first offset that hasn't been written by the application, i.e. the offset past the end of `unacked`
    offset: u64,
    /// The first offset that hasn't been sent
    ///
    /// Always lies in (offset - unacked.len())..offset
    unsent: u64,
    /// Acknowledged ranges which couldn't be discarded yet as they don't include the earliest
    /// offset in `unacked`
    // TODO: Recover storage from these by compacting (#700)
    acks: RangeSet,
    /// Previously transmitted ranges deemed lost
    retransmits: RangeSet,
}

impl SendBuffer {
    /// Construct an empty buffer at the initial offset
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Append application data to the end of the stream
    pub(super) fn write(&mut self, data: Bytes) {
        self.unacked_len += data.len();
        self.offset += data.len() as u64;
        self.unacked_segments.push_back(data);
    }

    /// Discard a range of acknowledged stream data
    pub(super) fn ack(&mut self, mut range: Range<u64>) {
        // Clamp the range to data which is still tracked
        let base_offset = self.offset - self.unacked_len as u64;
        range.start = base_offset.max(range.start);
        range.end = base_offset.max(range.end);

        self.acks.insert(range);

        while self.acks.min() == Some(self.offset - self.unacked_len as u64) {
            let prefix = self.acks.pop_min().unwrap();
            let mut to_advance = (prefix.end - prefix.start) as usize;

            self.unacked_len -= to_advance;
            while to_advance > 0 {
                let front = self
                    .unacked_segments
                    .front_mut()
                    .expect("Expected buffered data");

                if front.len() <= to_advance {
                    to_advance -= front.len();
                    self.unacked_segments.pop_front();

                    if self.unacked_segments.len() * 4 < self.unacked_segments.capacity() {
                        self.unacked_segments.shrink_to_fit();
                    }
                } else {
                    front.advance(to_advance);
                    to_advance = 0;
                }
            }
        }
    }

    /// Compute the next range to transmit on this stream and update state to account for that
    /// transmission.
    ///
    /// `max_len` here includes the space which is available to transmit the
    /// offset and length of the data to send. The caller has to guarantee that
    /// there is at least enough space available to write maximum-sized metadata
    /// (8 byte offset + 8 byte length).
    ///
    /// The method returns a tuple:
    /// - The first return value indicates the range of data to send
    /// - The second return value indicates whether the length needs to be encoded
    ///   in the STREAM frames metadata (`true`), or whether it can be omitted
    ///   since the selected range will fill the whole packet.
    pub(super) fn poll_transmit(&mut self, mut max_len: usize) -> (Range<u64>, bool) {
        debug_assert!(max_len >= 8 + 8);
        let mut encode_length = false;

        if let Some(range) = self.retransmits.pop_min() {
            // Retransmit sent data

            // When the offset is known, we know how many bytes are required to encode it.
            // Offset 0 requires no space
            if range.start != 0 {
                max_len -= VarInt::size(unsafe { VarInt::from_u64_unchecked(range.start) });
            }
            if range.end - range.start < max_len as u64 {
                encode_length = true;
                max_len -= 8;
            }

            let end = range.end.min((max_len as u64).saturating_add(range.start));
            if end != range.end {
                self.retransmits.insert(end..range.end);
            }
            return (range.start..end, encode_length);
        }

        // Transmit new data

        // When the offset is known, we know how many bytes are required to encode it.
        // Offset 0 requires no space
        if self.unsent != 0 {
            max_len -= VarInt::size(unsafe { VarInt::from_u64_unchecked(self.unsent) });
        }
        if self.offset - self.unsent < max_len as u64 {
            encode_length = true;
            max_len -= 8;
        }

        let end = self
            .offset
            .min((max_len as u64).saturating_add(self.unsent));
        let result = self.unsent..end;
        self.unsent = end;
        (result, encode_length)
    }

    /// Returns data which is associated with a range
    ///
    /// This function can return a subset of the range, if the data is stored
    /// in noncontiguous fashion in the send buffer. In this case callers
    /// should call the function again with an incremented start offset to
    /// retrieve more data.
    pub(super) fn get(&self, offsets: Range<u64>) -> &[u8] {
        let base_offset = self.offset - self.unacked_len as u64;

        let mut segment_offset = base_offset;
        for segment in self.unacked_segments.iter() {
            if offsets.start >= segment_offset
                && offsets.start < segment_offset + segment.len() as u64
            {
                let start = (offsets.start - segment_offset) as usize;
                let end = (offsets.end - segment_offset) as usize;

                return &segment[start..end.min(segment.len())];
            }
            segment_offset += segment.len() as u64;
        }

        &[]
    }

    /// Queue a range of sent but unacknowledged data to be retransmitted
    pub(super) fn retransmit(&mut self, range: Range<u64>) {
        debug_assert!(range.end <= self.unsent, "unsent data can't be lost");
        self.retransmits.insert(range);
    }

    pub(super) fn retransmit_all_for_0rtt(&mut self) {
        debug_assert_eq!(self.offset, self.unacked_len as u64);
        self.unsent = 0;
    }

    /// First stream offset unwritten by the application, i.e. the offset that the next write will
    /// begin at
    pub(super) fn offset(&self) -> u64 {
        self.offset
    }

    /// Whether all sent data has been acknowledged
    pub(super) fn is_fully_acked(&self) -> bool {
        self.unacked_len == 0
    }

    /// Whether there's data to send
    ///
    /// There may be sent unacknowledged data even when this is false.
    pub(super) fn has_unsent_data(&self) -> bool {
        self.unsent != self.offset || !self.retransmits.is_empty()
    }

    /// Compute the amount of data that hasn't been acknowledged
    pub(super) fn unacked(&self) -> u64 {
        self.unacked_len as u64 - self.acks.iter().map(|x| x.end - x.start).sum::<u64>()
    }
}
