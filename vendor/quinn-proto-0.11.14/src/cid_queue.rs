use std::ops::Range;

use crate::{ConnectionId, ResetToken, frame::NewConnectionId};

/// DataType stored in CidQueue buffer
type CidData = (ConnectionId, Option<ResetToken>);

/// Sliding window of active Connection IDs
///
/// May contain gaps due to packet loss or reordering
#[derive(Debug)]
pub(crate) struct CidQueue {
    /// Ring buffer indexed by `self.cursor`
    buffer: [Option<CidData>; Self::LEN],
    /// Index at which circular buffer addressing is based
    cursor: usize,
    /// Sequence number of `self.buffer[cursor]`
    ///
    /// The sequence number of the active CID; must be the smallest among CIDs in `buffer`.
    offset: u64,
}

impl CidQueue {
    pub(crate) fn new(cid: ConnectionId) -> Self {
        let mut buffer = [None; Self::LEN];
        buffer[0] = Some((cid, None));
        Self {
            buffer,
            cursor: 0,
            offset: 0,
        }
    }

    /// Handle a `NEW_CONNECTION_ID` frame
    ///
    /// Returns a non-empty range of retired sequence numbers and the reset token of the new active
    /// CID iff any CIDs were retired.
    pub(crate) fn insert(
        &mut self,
        cid: NewConnectionId,
    ) -> Result<Option<(Range<u64>, ResetToken)>, InsertError> {
        // Position of new CID wrt. the current active CID
        let index = match cid.sequence.checked_sub(self.offset) {
            None => return Err(InsertError::Retired),
            Some(x) => x,
        };

        let retired_count = cid.retire_prior_to.saturating_sub(self.offset);
        if index >= Self::LEN as u64 + retired_count {
            return Err(InsertError::ExceedsLimit);
        }

        // Discard retired CIDs, if any
        for i in 0..(retired_count.min(Self::LEN as u64) as usize) {
            self.buffer[(self.cursor + i) % Self::LEN] = None;
        }

        // Record the new CID
        let index = ((self.cursor as u64 + index) % Self::LEN as u64) as usize;
        self.buffer[index] = Some((cid.id, Some(cid.reset_token)));

        if retired_count == 0 {
            return Ok(None);
        }

        // The active CID was retired. Find the first known CID with sequence number of at least
        // retire_prior_to, and inform the caller that all prior CIDs have been retired, and of
        // the new CID's reset token.
        self.cursor = ((self.cursor as u64 + retired_count) % Self::LEN as u64) as usize;
        let (i, (_, token)) = self
            .iter()
            .next()
            .expect("it is impossible to retire a CID without supplying a new one");
        self.cursor = (self.cursor + i) % Self::LEN;
        let orig_offset = self.offset;
        self.offset = cid.retire_prior_to + i as u64;
        // We don't immediately retire CIDs in the range (orig_offset +
        // Self::LEN)..self.offset. These are CIDs that we haven't yet received from a
        // NEW_CONNECTION_ID frame, since having previously received them would violate the
        // connection ID limit we specified based on Self::LEN. If we do receive a such a frame
        // in the future, e.g. due to reordering, we'll retire it then. This ensures we can't be
        // made to buffer an arbitrarily large number of RETIRE_CONNECTION_ID frames.
        Ok(Some((
            orig_offset..self.offset.min(orig_offset + Self::LEN as u64),
            token.expect("non-initial CID missing reset token"),
        )))
    }

    /// Switch to next active CID if possible, return
    /// 1) the corresponding ResetToken and 2) a non-empty range preceding it to retire
    pub(crate) fn next(&mut self) -> Option<(ResetToken, Range<u64>)> {
        let (i, cid_data) = self.iter().nth(1)?;
        self.buffer[self.cursor] = None;

        let orig_offset = self.offset;
        self.offset += i as u64;
        self.cursor = (self.cursor + i) % Self::LEN;
        Some((cid_data.1.unwrap(), orig_offset..self.offset))
    }

    /// Iterate CIDs in CidQueue that are not `None`, including the active CID
    fn iter(&self) -> impl Iterator<Item = (usize, CidData)> + '_ {
        (0..Self::LEN).filter_map(move |step| {
            let index = (self.cursor + step) % Self::LEN;
            self.buffer[index].map(|cid_data| (step, cid_data))
        })
    }

    /// Replace the initial CID
    pub(crate) fn update_initial_cid(&mut self, cid: ConnectionId) {
        debug_assert_eq!(self.offset, 0);
        self.buffer[self.cursor] = Some((cid, None));
    }

    /// Return active remote CID itself
    pub(crate) fn active(&self) -> ConnectionId {
        self.buffer[self.cursor].unwrap().0
    }

    /// Return the sequence number of active remote CID
    pub(crate) fn active_seq(&self) -> u64 {
        self.offset
    }

    pub(crate) const LEN: usize = 5;
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum InsertError {
    /// CID was already retired
    Retired,
    /// Sequence number violates the leading edge of the window
    ExceedsLimit,
}
