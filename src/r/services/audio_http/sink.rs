//! One bounded PCM producer at a time and the latest completed clip.
use alloc::{sync::Arc, vec::Vec};

/// Two minutes of stereo signed 16-bit little-endian PCM at 48 kHz.
pub(super) const MAX_CLIP_BYTES: usize = 48_000 * 4 * 120;
pub(super) const MAX_APPEND_BYTES: usize = 64 * 1024;
const PREBUFFER_BYTES: usize = 48_000 * 4 * 120 / 1000;
const PRODUCER_IDLE_MS: u64 = 5_000;

pub(super) fn validate_size(bytes: usize) -> Result<(), i32> {
    if bytes == 0 || bytes % 4 != 0 {
        Err(-22)
    } else if bytes > MAX_CLIP_BYTES {
        Err(-27)
    } else {
        Ok(())
    }
}

pub(super) struct Sink {
    clip: Option<Clip>,
    offset: usize,
    next_id: u64,
    stream: Option<Stream>,
    finished_pending_start: Option<u64>,
}

#[derive(Clone)]
pub(super) struct Clip {
    pub id: u64,
    pub pcm: Arc<Vec<u8>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Owner {
    Host,
    Vm { id: u8, generation: u64 },
}

struct Stream {
    owner: Owner,
    id: u64,
    pcm: Vec<u8>,
    offset: usize,
    started_ms: u64,
    last_append_ms: u64,
    buffering: bool,
    underruns: u32,
}

#[derive(Default)]
pub(super) struct RenderReport {
    pub(super) first: Option<(u64, u64)>,
    pub(super) underrun: Option<(u64, u32)>,
}

impl Sink {
    pub(super) const fn new() -> Self {
        Self {
            clip: None,
            offset: 0,
            next_id: 1,
            stream: None,
            finished_pending_start: None,
        }
    }

    /// Copy a completed clip once. Nothing is retained from competing sources,
    /// and the caller can release its allocation as soon as this returns.
    pub(super) fn submit(&mut self, bytes: &[u8]) -> Result<(), i32> {
        validate_size(bytes.len())?;
        if self.busy() {
            return Ok(());
        }
        let mut clip = Vec::new();
        clip.try_reserve_exact(bytes.len()).map_err(|_| -12)?;
        clip.extend_from_slice(bytes);
        let id = self.allocate_id()?;
        self.clip = Some(Clip { id, pcm: Arc::new(clip) });
        self.offset = 0;
        Ok(())
    }

    pub(super) fn busy(&self) -> bool {
        self.stream.is_some()
            || self.clip.as_ref().is_some_and(|clip| self.offset < clip.pcm.len())
    }

    fn allocate_id(&mut self) -> Result<u64, i32> {
        let id = self.next_id;
        if id > i64::MAX as u64 {
            return Err(-75);
        }
        self.next_id += 1;
        Ok(id)
    }

    pub(super) fn begin(&mut self, owner: Owner, now_ms: u64) -> Result<u64, i32> {
        if self.busy() {
            return Err(-16);
        }
        let id = self.allocate_id()?;
        self.stream = Some(Stream {
            owner,
            id,
            pcm: Vec::new(),
            offset: 0,
            started_ms: now_ms,
            last_append_ms: now_ms,
            buffering: true,
            underruns: 0,
        });
        Ok(id)
    }

    fn owned_stream(&mut self, owner: Owner, id: u64) -> Result<&mut Stream, i32> {
        let stream = self.stream.as_mut().filter(|s| s.id == id).ok_or(-2)?;
        if stream.owner != owner {
            return Err(-13);
        }
        Ok(stream)
    }

    pub(super) fn append(
        &mut self,
        owner: Owner,
        id: u64,
        bytes: &[u8],
        now_ms: u64,
    ) -> Result<(), i32> {
        validate_append_size(bytes.len())?;
        let stream = self.owned_stream(owner, id)?;
        let total = stream.pcm.len() + bytes.len();
        validate_size(total)?;
        if total > stream.pcm.capacity() {
            let capacity = stream.pcm.capacity().saturating_mul(2).max(total).min(MAX_CLIP_BYTES);
            stream.pcm.try_reserve_exact(capacity - stream.pcm.len()).map_err(|_| -12)?;
        }
        stream.pcm.extend_from_slice(bytes);
        stream.last_append_ms = now_ms;
        Ok(())
    }

    pub(super) fn finish(&mut self, owner: Owner, id: u64) -> Result<(), i32> {
        self.owned_stream(owner, id)?;
        let stream = self.stream.take().expect("owner checked above");
        if !stream.pcm.is_empty() {
            self.offset = stream.offset;
            self.finished_pending_start = (stream.offset == 0).then_some(stream.started_ms);
            self.clip = Some(Clip { id, pcm: Arc::new(stream.pcm) });
        }
        Ok(())
    }

    pub(super) fn abort(&mut self, owner: Owner, id: u64) -> Result<(), i32> {
        self.owned_stream(owner, id)?;
        self.stream = None;
        Ok(())
    }

    pub(super) fn release(&mut self, owner: Owner) {
        if self.stream.as_ref().is_some_and(|stream| stream.owner == owner) {
            self.stream = None;
        }
    }

    pub(super) fn expire(&mut self, now_ms: u64) -> Option<u64> {
        let stream = self.stream.as_ref()?;
        if now_ms.saturating_sub(stream.last_append_ms) < PRODUCER_IDLE_MS {
            return None;
        }
        self.stream.take().map(|stream| stream.id)
    }

    /// Retain only the latest accepted clip, including its complete prefix.
    /// HTTP downloads own an Arc so a later accepted clip cannot truncate them.
    pub(super) fn snapshot(&self) -> Option<Clip> { self.clip.clone() }

    /// Drain one playback block. Silence fills idle and underrun gaps.
    pub(super) fn render(&mut self, out: &mut [u8]) -> RenderReport {
        let mut report = RenderReport::default();
        assert_eq!(out.len() % 4, 0);
        out.fill(0);
        if let Some(stream) = &mut self.stream {
            let available = stream.pcm.len() - stream.offset;
            if stream.buffering && available < PREBUFFER_BYTES {
                return report;
            }
            stream.buffering = false;
            let count = out.len().min(available);
            if count != 0 && stream.offset == 0 {
                report.first = Some((stream.id, stream.started_ms));
            }
            out[..count].copy_from_slice(&stream.pcm[stream.offset..stream.offset + count]);
            stream.offset += count;
            if count < out.len() {
                stream.buffering = true;
                stream.underruns = stream.underruns.saturating_add(1);
                report.underrun = Some((stream.id, stream.underruns));
            }
            return report;
        }
        let Some(clip) = &self.clip else { return report; };
        let count = out.len().min(clip.pcm.len().saturating_sub(self.offset));
        if count != 0 && let Some(started_ms) = self.finished_pending_start.take() {
            report.first = Some((clip.id, started_ms));
        }
        out[..count].copy_from_slice(&clip.pcm[self.offset..self.offset + count]);
        self.offset += count;
        report
    }
}

pub(super) fn validate_append_size(bytes: usize) -> Result<(), i32> {
    if bytes > MAX_APPEND_BYTES {
        Err(-27)
    } else {
        validate_size(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_clip_wins_and_competitors_succeed_without_being_queued() {
        let mut sink = Sink::new();
        sink.submit(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!(sink.submit(&[9, 10, 11, 12]), Ok(()));
        let mut block = [0; 4];
        sink.render(&mut block);
        assert_eq!(block, [1, 2, 3, 4]);
        sink.render(&mut block);
        assert_eq!(block, [5, 6, 7, 8]);
        sink.render(&mut block);
        assert_eq!(block, [0; 4]);
    }

    #[test]
    fn last_sample_releases_ownership_and_caller_memory_is_not_borrowed() {
        let mut sink = Sink::new();
        let mut input = alloc::vec![1, 2, 3, 4];
        sink.submit(&input).unwrap();
        input.fill(99);
        drop(input);
        let mut block = [0; 8];
        sink.render(&mut block);
        assert_eq!(block, [1, 2, 3, 4, 0, 0, 0, 0]);
        assert!(!sink.busy());
        sink.submit(&[5, 6, 7, 8]).unwrap();
        sink.render(&mut block);
        assert_eq!(&block[..4], &[5, 6, 7, 8]);
    }

    #[test]
    fn completed_utterance_longer_than_the_old_half_second_buffer_is_lossless() {
        let mut sink = Sink::new();
        let bytes: Vec<u8> = (0..48_000 * 4 * 3).map(|n| (n % 251) as u8).collect();
        sink.submit(&bytes).unwrap();
        let mut actual = Vec::new();
        for _ in 0..300 {
            let mut block = [0; 1920];
            sink.render(&mut block);
            actual.extend_from_slice(&block);
        }
        assert_eq!(actual, bytes);
        assert!(!sink.busy());
    }

    #[test]
    fn invalid_or_oversized_clips_never_claim_the_sink() {
        let mut sink = Sink::new();
        assert_eq!(sink.submit(&[]), Err(-22));
        assert_eq!(sink.submit(&[1, 2]), Err(-22));
        assert_eq!(validate_size(MAX_CLIP_BYTES), Ok(()));
        assert_eq!(validate_size(MAX_CLIP_BYTES + 4), Err(-27));
        assert!(sink.snapshot().is_none());
    }

    #[test]
    fn streams_after_prebuffer_and_keeps_all_pcm_for_final_snapshot() {
        let mut sink = Sink::new();
        let id = sink.begin(Owner::Host, 10).unwrap();
        let initial = alloc::vec![17; PREBUFFER_BYTES - 4];
        sink.append(Owner::Host, id, &initial, 20).unwrap();
        let mut block = [1; 8];
        assert!(sink.render(&mut block).first.is_none());
        assert_eq!(block, [0; 8]);
        assert!(sink.snapshot().is_none());
        sink.append(Owner::Host, id, &[23; 4], 30).unwrap();
        assert_eq!(sink.render(&mut block).first, Some((id, 10)));
        assert_eq!(block, [17; 8]);
        assert!(sink.snapshot().is_none());
        let mut last = alloc::vec![1; PREBUFFER_BYTES - block.len() + 4];
        assert_eq!(sink.render(&mut last).underrun, Some((id, 1)));
        assert_eq!(&last[last.len() - 8..], &[23, 23, 23, 23, 0, 0, 0, 0]);
        assert!(sink.busy());
        assert!(sink.render(&mut block).underrun.is_none());

        // Finishing a short final piece releases the prebuffer. It is heard
        // once, and the temporary silence is not saved in the retained clip.
        sink.append(Owner::Host, id, &[29; 4], 40).unwrap();
        sink.finish(Owner::Host, id).unwrap();
        assert_eq!(sink.snapshot().unwrap().id, id);
        assert!(sink.render(&mut block).first.is_none());
        assert_eq!(block, [29, 29, 29, 29, 0, 0, 0, 0]);
        assert!(!sink.busy());
        let clip = sink.snapshot().unwrap();
        assert_eq!(&clip.pcm[..initial.len()], &initial);
        assert_eq!(&clip.pcm[initial.len()..], &[23, 23, 23, 23, 29, 29, 29, 29]);
    }

    #[test]
    fn ownership_abort_and_expiry_preserve_the_previous_completed_clip() {
        let mut sink = Sink::new();
        sink.submit(&[3; 4]).unwrap();
        let previous = sink.snapshot().unwrap().id;
        sink.render(&mut [0; 4]);
        let owner = Owner::Vm { id: 1, generation: 10 };
        let reused = Owner::Vm { id: 1, generation: 11 };
        let id = sink.begin(owner, 10).unwrap();
        assert_eq!(sink.begin(reused, 20), Err(-16));
        assert_eq!(sink.append(reused, id, &[5; 4], 30), Err(-13));
        assert_eq!(sink.finish(reused, id), Err(-13));
        assert_eq!(sink.abort(reused, id), Err(-13));
        sink.release(reused);
        assert_eq!(sink.append(owner, id, &[5; 4], 40), Ok(()));
        assert_eq!(sink.expire(40 + PRODUCER_IDLE_MS - 1), None);
        assert_eq!(sink.expire(40 + PRODUCER_IDLE_MS), Some(id));
        assert_eq!(sink.finish(owner, id), Err(-2));
        let id = sink.begin(reused, 50).unwrap();
        sink.release(reused);
        assert_eq!(sink.abort(reused, id), Err(-2));
        assert_eq!(sink.snapshot().unwrap().id, previous);
        assert!(!sink.busy());
    }
}
