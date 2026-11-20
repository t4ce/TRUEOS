//! Two independent PCM slots and the latest completed clip.
use alloc::{sync::Arc, vec::Vec};

/// Two minutes of stereo signed 16-bit little-endian PCM at 48 kHz.
pub(super) const MAX_CLIP_BYTES: usize = 48_000 * 4 * 120;
pub(super) const MAX_APPEND_BYTES: usize = 64 * 1024;
pub(super) const MAX_PRODUCERS: usize = 2;
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
    latest_clip: Option<Clip>,
    next_id: u64,
    slots: [Option<Source>; MAX_PRODUCERS],
    local_enabled: bool,
    http_rendering: bool,
    local: [Option<LocalSource>; MAX_PRODUCERS],
}

#[derive(Clone)]
pub(super) struct Clip {
    pub(super) id: u64,
    pub(super) pcm: Arc<Vec<u8>>,
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

enum Source {
    Live(Stream),
    Clip {
        clip: Clip,
        offset: usize,
        pending_start: Option<u64>,
    },
}

enum LocalSource {
    Live {
        id: u64,
        offset: usize,
        buffering: bool,
    },
    Clip {
        clip: Clip,
        offset: usize,
    },
}

impl LocalSource {
    fn id(&self) -> u64 {
        match self {
            Self::Live { id, .. } => *id,
            Self::Clip { clip, .. } => clip.id,
        }
    }
}

#[derive(Default)]
pub(super) struct RenderReport {
    pub(super) bytes: usize,
    pub(super) first: Option<(u64, u64)>,
    pub(super) underrun: Option<(u64, u32)>,
}

impl Sink {
    pub(super) const fn new() -> Self {
        Self {
            latest_clip: None,
            next_id: 1,
            slots: [None, None],
            local_enabled: false,
            http_rendering: false,
            local: [None, None],
        }
    }

    /// Preserve the complete-clip ABI's successful discard when both slots are occupied.
    pub(super) fn submit(&mut self, bytes: &[u8]) -> Result<(), i32> {
        validate_size(bytes.len())?;
        let Some(slot) = self.free_slot() else {
            return Ok(());
        };
        let mut pcm = Vec::new();
        pcm.try_reserve_exact(bytes.len()).map_err(|_| -12)?;
        pcm.extend_from_slice(bytes);
        let id = self.allocate_id()?;
        let clip = Clip {
            id,
            pcm: Arc::new(pcm),
        };
        self.add_local(LocalSource::Clip {
            clip: clip.clone(),
            offset: 0,
        });
        self.queue_clip(slot, clip, 0, None);
        Ok(())
    }

    fn queue_clip(&mut self, slot: usize, clip: Clip, offset: usize, pending_start: Option<u64>) {
        self.latest_clip = Some(clip.clone());
        if offset < clip.pcm.len() {
            self.slots[slot] = Some(Source::Clip {
                clip,
                offset,
                pending_start,
            });
        }
    }

    fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(Option::is_none)
    }

    pub(super) fn active_count(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    pub(super) fn busy(&self) -> bool {
        self.active_count() != 0
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
        let slot = self.free_slot().ok_or(-16)?;
        let id = self.allocate_id()?;
        self.slots[slot] = Some(Source::Live(Stream {
            owner,
            id,
            pcm: Vec::new(),
            offset: 0,
            started_ms: now_ms,
            last_append_ms: now_ms,
            buffering: true,
            underruns: 0,
        }));
        self.add_local(LocalSource::Live {
            id,
            offset: 0,
            buffering: true,
        });
        Ok(id)
    }

    fn owned_slot(&self, owner: Owner, id: u64) -> Result<usize, i32> {
        for (index, slot) in self.slots.iter().enumerate() {
            if let Some(Source::Live(stream)) = slot
                && stream.id == id
            {
                return if stream.owner == owner {
                    Ok(index)
                } else {
                    Err(-13)
                };
            }
        }
        Err(-2)
    }

    pub(super) fn append(
        &mut self,
        owner: Owner,
        id: u64,
        bytes: &[u8],
        now_ms: u64,
    ) -> Result<(), i32> {
        validate_append_size(bytes.len())?;
        let slot = self.owned_slot(owner, id)?;
        let Some(Source::Live(stream)) = &mut self.slots[slot] else {
            unreachable!("owned live stream");
        };
        let total = stream.pcm.len() + bytes.len();
        validate_size(total)?;
        if total > stream.pcm.capacity() {
            let capacity = stream
                .pcm
                .capacity()
                .saturating_mul(2)
                .max(total)
                .min(MAX_CLIP_BYTES);
            stream
                .pcm
                .try_reserve_exact(capacity - stream.pcm.len())
                .map_err(|_| -12)?;
        }
        stream.pcm.extend_from_slice(bytes);
        stream.last_append_ms = now_ms;
        Ok(())
    }

    pub(super) fn finish(&mut self, owner: Owner, id: u64) -> Result<(), i32> {
        let slot = self.owned_slot(owner, id)?;
        let Some(Source::Live(stream)) = self.slots[slot].take() else {
            unreachable!("owned live stream");
        };
        if !stream.pcm.is_empty() {
            let pending_start = (stream.offset == 0).then_some(stream.started_ms);
            let clip = Clip {
                id,
                pcm: Arc::new(stream.pcm),
            };
            if let Some(local) = self.local_slot(id)
                && let Some(LocalSource::Live { offset, .. }) = local.take()
                && offset < clip.pcm.len()
            {
                *local = Some(LocalSource::Clip {
                    clip: clip.clone(),
                    offset,
                });
            }
            self.queue_clip(slot, clip, stream.offset, pending_start);
        } else if let Some(local) = self.local_slot(id) {
            *local = None;
        }
        Ok(())
    }

    pub(super) fn abort(&mut self, owner: Owner, id: u64) -> Result<(), i32> {
        let slot = self.owned_slot(owner, id)?;
        self.slots[slot] = None;
        if let Some(local) = self.local_slot(id) {
            *local = None;
        }
        Ok(())
    }

    pub(super) fn release(&mut self, owner: Owner) {
        for slot in &mut self.slots {
            if matches!(slot, Some(Source::Live(stream)) if stream.owner == owner) {
                *slot = None;
            }
        }
        self.remove_orphaned_local_streams();
    }

    pub(super) fn expire(&mut self, now_ms: u64) -> [Option<u64>; MAX_PRODUCERS] {
        let expired = self.slots.each_mut().map(|slot| {
            if let Some(Source::Live(stream)) = slot
                && now_ms.saturating_sub(stream.last_append_ms) >= PRODUCER_IDLE_MS
            {
                let id = stream.id;
                *slot = None;
                Some(id)
            } else {
                None
            }
        });
        self.remove_orphaned_local_streams();
        expired
    }

    /// HTTP downloads own an Arc, so a later clip cannot truncate them.
    pub(super) fn snapshot(&self) -> Option<Clip> {
        self.latest_clip.clone()
    }

    pub(super) fn render_sources(
        &mut self,
        a: &mut [u8],
        b: &mut [u8],
    ) -> [RenderReport; MAX_PRODUCERS] {
        let [first, second] = &mut self.slots;
        [render_source(first, a), render_source(second, b)]
    }

    pub(super) fn set_http_rendering(&mut self) {
        self.http_rendering = true;
    }

    pub(super) fn set_local_playback(&mut self, enabled: bool) {
        if enabled == self.local_enabled {
            return;
        }
        self.local_enabled = enabled;
        self.local = if enabled {
            self.slots.each_ref().map(|slot| match slot {
                Some(Source::Live(stream)) => Some(LocalSource::Live {
                    id: stream.id,
                    offset: stream.offset,
                    buffering: stream.buffering,
                }),
                Some(Source::Clip { clip, offset, .. }) => Some(LocalSource::Clip {
                    clip: clip.clone(),
                    offset: *offset,
                }),
                None => None,
            })
        } else {
            [None, None]
        };
    }

    fn local_slot(&mut self, id: u64) -> Option<&mut Option<LocalSource>> {
        self.local
            .iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|source| source.id() == id))
    }

    fn add_local(&mut self, source: LocalSource) {
        if self.local_enabled
            && let Some(slot) = self.local.iter_mut().find(|slot| slot.is_none())
        {
            *slot = Some(source);
        }
    }

    fn remove_orphaned_local_streams(&mut self) {
        for slot in &mut self.local {
            if let Some(LocalSource::Live { id, .. }) = slot
                && live_stream(&self.slots, *id).is_none()
            {
                *slot = None;
            }
        }
    }

    pub(super) fn render_local(&mut self, out: &mut [[i16; 2]], now_ms: u64) {
        let local_clock_only = self.local_enabled && !self.http_rendering;
        if local_clock_only {
            let _ = self.expire(now_ms);
        }
        let max_bytes = core::mem::size_of_val(out);
        let [first, second] = &mut self.local;
        let a = local_samples(first, &self.slots, max_bytes);
        let b = local_samples(second, &self.slots, max_bytes);
        for (index, frame) in out.iter_mut().enumerate() {
            let offset = index * 4;
            let a = a.get(offset..offset + 4);
            let b = b.get(offset..offset + 4);
            *frame = match (a, b) {
                (Some(a), Some(b)) => [center_sample(a, b, 0), center_sample(a, b, 2)],
                (Some(pcm), None) | (None, Some(pcm)) => [sample(pcm, 0), sample(pcm, 2)],
                (None, None) => [0; 2],
            };
        }
        // With no NIC playback task, the local clock frees the legacy slots.
        // The network starts from this position when it comes online.
        if local_clock_only {
            self.advance_http_without_nic();
        }
        for slot in &mut self.local {
            if matches!(slot, Some(LocalSource::Clip { clip, offset }) if *offset == clip.pcm.len())
            {
                *slot = None;
            }
        }
    }

    fn advance_http_without_nic(&mut self) {
        for (http, local) in self.slots.iter_mut().zip(&self.local) {
            match (http, local) {
                (
                    Some(Source::Live(stream)),
                    Some(LocalSource::Live {
                        id,
                        offset,
                        buffering,
                    }),
                ) if stream.id == *id => {
                    stream.offset = *offset;
                    stream.buffering = *buffering;
                }
                (
                    Some(Source::Clip {
                        clip,
                        offset,
                        pending_start,
                    }),
                    Some(LocalSource::Clip {
                        clip: local_clip,
                        offset: local,
                    }),
                ) if clip.id == local_clip.id => {
                    *offset = *local;
                    if *offset != 0 {
                        *pending_start = None;
                    }
                }
                _ => {}
            }
        }
        for slot in &mut self.slots {
            if matches!(slot, Some(Source::Clip { clip, offset, .. }) if *offset == clip.pcm.len())
            {
                *slot = None;
            }
        }
    }

    #[cfg(test)]
    pub(super) fn render(&mut self, out: &mut [u8]) -> [RenderReport; MAX_PRODUCERS] {
        let mut a = alloc::vec![0; out.len()];
        let mut b = alloc::vec![0; out.len()];
        let reports = self.render_sources(&mut a, &mut b);
        mix_center(out, &a[..reports[0].bytes], &b[..reports[1].bytes]);
        reports
    }
}

fn live_stream(slots: &[Option<Source>], id: u64) -> Option<&Stream> {
    slots.iter().flatten().find_map(|source| match source {
        Source::Live(stream) if stream.id == id => Some(stream),
        _ => None,
    })
}

fn local_samples<'a>(
    source: &'a mut Option<LocalSource>,
    slots: &'a [Option<Source>],
    max_bytes: usize,
) -> &'a [u8] {
    let (pcm, offset) = match source {
        Some(LocalSource::Live {
            id,
            offset,
            buffering,
        }) => {
            let Some(stream) = live_stream(slots, *id) else {
                return &[];
            };
            let available = stream.pcm.len() - *offset;
            if *buffering && available < PREBUFFER_BYTES {
                return &[];
            }
            *buffering = available < max_bytes;
            (stream.pcm.as_slice(), offset)
        }
        Some(LocalSource::Clip { clip, offset }) => (clip.pcm.as_slice(), offset),
        None => return &[],
    };
    let start = *offset;
    *offset += max_bytes.min(pcm.len() - start);
    &pcm[start..*offset]
}

fn sample(pcm: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes([pcm[offset], pcm[offset + 1]])
}

fn center_sample(a: &[u8], b: &[u8], offset: usize) -> i16 {
    ((i32::from(sample(a, offset)) + i32::from(sample(b, offset))) / 2) as i16
}

fn render_source(slot: &mut Option<Source>, out: &mut [u8]) -> RenderReport {
    let mut report = RenderReport::default();
    out.fill(0);
    match slot {
        Some(Source::Live(stream)) => {
            let available = stream.pcm.len() - stream.offset;
            if stream.buffering && available < PREBUFFER_BYTES {
                return report;
            }
            stream.buffering = false;
            report.bytes = out.len().min(available);
            if report.bytes != 0 && stream.offset == 0 {
                report.first = Some((stream.id, stream.started_ms));
            }
            out[..report.bytes]
                .copy_from_slice(&stream.pcm[stream.offset..stream.offset + report.bytes]);
            stream.offset += report.bytes;
            if report.bytes < out.len() {
                stream.buffering = true;
                stream.underruns = stream.underruns.saturating_add(1);
                report.underrun = Some((stream.id, stream.underruns));
            }
        }
        Some(Source::Clip {
            clip,
            offset,
            pending_start,
        }) => {
            report.bytes = out.len().min(clip.pcm.len() - *offset);
            if report.bytes != 0
                && let Some(started_ms) = pending_start.take()
            {
                report.first = Some((clip.id, started_ms));
            }
            out[..report.bytes].copy_from_slice(&clip.pcm[*offset..*offset + report.bytes]);
            *offset += report.bytes;
            if *offset == clip.pcm.len() {
                *slot = None;
            }
        }
        None => {}
    }
    report
}

/// Existing stereo WAV clients hear the centered mix during overlap and unity solo playback.
pub(super) fn mix_center(out: &mut [u8], a: &[u8], b: &[u8]) {
    let overlap = a.len().min(b.len());
    for ((x, y), dest) in a[..overlap]
        .chunks_exact(2)
        .zip(b[..overlap].chunks_exact(2))
        .zip(out.chunks_exact_mut(2))
    {
        dest.copy_from_slice(&center_sample(x, y, 0).to_le_bytes());
    }
    let longer = if a.len() > b.len() { a } else { b };
    out[overlap..longer.len()].copy_from_slice(&longer[overlap..]);
    out[longer.len()..].fill(0);
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
    fn two_clips_play_and_a_third_is_discarded() {
        let mut sink = Sink::new();
        sink.submit(&[100, 0, 200, 0, 8, 0, 9, 0]).unwrap();
        sink.submit(&[200, 0, 100, 0]).unwrap();
        let id = sink.snapshot().unwrap().id;
        assert_eq!(sink.submit(&[42; 4]), Ok(()));
        assert_eq!(sink.snapshot().unwrap().id, id);
        let mut block = [0; 8];
        sink.render(&mut block);
        assert_eq!(block, [150, 0, 150, 0, 8, 0, 9, 0]);
        sink.render(&mut block);
        assert_eq!(block, [0; 8]);
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
        assert!(sink.render(&mut block)[0].first.is_none());
        assert_eq!(block, [0; 8]);
        assert!(sink.snapshot().is_none());
        sink.append(Owner::Host, id, &[23; 4], 30).unwrap();
        assert_eq!(sink.render(&mut block)[0].first, Some((id, 10)));
        assert_eq!(block, [17; 8]);
        assert!(sink.snapshot().is_none());
        let mut last = alloc::vec![1; PREBUFFER_BYTES - block.len() + 4];
        assert_eq!(sink.render(&mut last)[0].underrun, Some((id, 1)));
        assert_eq!(&last[last.len() - 8..], &[23, 23, 23, 23, 0, 0, 0, 0]);
        assert!(sink.busy());
        assert!(sink.render(&mut block)[0].underrun.is_none());

        // Finishing a short final piece releases the prebuffer. It is heard
        // once, and the temporary silence is not saved in the retained clip.
        sink.append(Owner::Host, id, &[29; 4], 40).unwrap();
        sink.finish(Owner::Host, id).unwrap();
        assert_eq!(sink.snapshot().unwrap().id, id);
        assert!(sink.render(&mut block)[0].first.is_none());
        assert_eq!(block, [29, 29, 29, 29, 0, 0, 0, 0]);
        assert!(!sink.busy());
        let clip = sink.snapshot().unwrap();
        assert_eq!(&clip.pcm[..initial.len()], &initial);
        assert_eq!(
            &clip.pcm[initial.len()..],
            &[23, 23, 23, 23, 29, 29, 29, 29]
        );
    }

    #[test]
    fn two_live_producers_prebuffer_independently_and_keep_their_slots() {
        let mut sink = Sink::new();
        let vm = Owner::Vm {
            id: 2,
            generation: 1,
        };
        let a = sink.begin(Owner::Host, 10).unwrap();
        let b = sink.begin(vm, 11).unwrap();
        let mut left = [0; 8];
        let mut right = [0; 8];
        sink.append(vm, b, &alloc::vec![7; PREBUFFER_BYTES], 12)
            .unwrap();

        let reports = sink.render_sources(&mut left, &mut right);
        assert_eq!((reports[0].bytes, reports[1].bytes), (0, 8));
        assert_eq!((left, right), ([0; 8], [7; 8]));

        sink.append(Owner::Host, a, &alloc::vec![4; PREBUFFER_BYTES], 13)
            .unwrap();
        let reports = sink.render_sources(&mut left, &mut right);
        assert_eq!((reports[0].bytes, reports[1].bytes), (8, 8));
        assert_eq!((left, right), ([4; 8], [7; 8]));

        sink.abort(Owner::Host, a).unwrap();
        let reports = sink.render_sources(&mut left, &mut right);
        assert_eq!((reports[0].bytes, reports[1].bytes), (0, 8));
        assert_eq!((left, right), ([0; 8], [7; 8]));
    }

    #[test]
    fn ownership_abort_and_expiry_preserve_the_previous_completed_clip() {
        let mut sink = Sink::new();
        sink.submit(&[3; 4]).unwrap();
        let previous = sink.snapshot().unwrap().id;
        sink.render(&mut [0; 4]);
        let owner = Owner::Vm {
            id: 1,
            generation: 10,
        };
        let reused = Owner::Vm {
            id: 1,
            generation: 11,
        };
        let id = sink.begin(owner, 10).unwrap();
        let other = sink.begin(reused, 20).unwrap();
        assert_eq!(sink.begin(Owner::Host, 20), Err(-16));
        assert_eq!(sink.append(reused, id, &[5; 4], 30), Err(-13));
        assert_eq!(sink.finish(reused, id), Err(-13));
        assert_eq!(sink.abort(reused, id), Err(-13));
        sink.release(reused);
        assert_eq!(sink.append(reused, other, &[5; 4], 30), Err(-2));
        assert_eq!(sink.append(owner, id, &[5; 4], 40), Ok(()));
        assert_eq!(sink.expire(40 + PRODUCER_IDLE_MS - 1), [None, None]);
        assert_eq!(sink.expire(40 + PRODUCER_IDLE_MS), [Some(id), None]);
        assert_eq!(sink.finish(owner, id), Err(-2));
        let id = sink.begin(reused, 50).unwrap();
        sink.release(reused);
        assert_eq!(sink.abort(reused, id), Err(-2));
        assert_eq!(sink.snapshot().unwrap().id, previous);
        assert!(!sink.busy());
    }

    #[test]
    fn local_and_http_each_consume_the_same_live_and_finished_frames() {
        let mut sink = Sink::new();
        sink.set_local_playback(true);
        sink.set_http_rendering();
        let a = sink.begin(Owner::Host, 0).unwrap();
        let b = sink.begin(Owner::Host, 0).unwrap();
        let mut a_pcm = [16, 0, 48, 0].repeat(PREBUFFER_BYTES / 4);
        let mut b_pcm = [48, 0, 16, 0].repeat(PREBUFFER_BYTES / 4);
        a_pcm[..4].copy_from_slice(&[32, 0, 64, 0]);
        b_pcm[..4].copy_from_slice(&[64, 0, 32, 0]);
        sink.append(Owner::Host, a, &a_pcm, 0).unwrap();
        sink.append(Owner::Host, b, &b_pcm, 0).unwrap();

        let mut local = [[0; 2]; 1];
        sink.render_local(&mut local, 0);
        assert_eq!(local, [[48, 48]]);
        sink.render_local(&mut local, 0);
        assert_eq!(local, [[32, 32]]);

        let mut http = [0; 4];
        sink.render(&mut http);
        assert_eq!(http, [48, 0, 48, 0]);
        sink.finish(Owner::Host, a).unwrap();
        sink.abort(Owner::Host, b).unwrap();
        sink.render_local(&mut local, 0);
        assert_eq!(local, [[16, 48]]);
        sink.render(&mut http);
        assert_eq!(http, [16, 0, 48, 0]);
    }

    #[test]
    fn local_only_playback_frees_slots_and_does_not_drop_retained_clips() {
        let mut sink = Sink::new();
        sink.set_local_playback(true);
        let mut out = [[0; 2]; 2];
        for value in [1, 2, 3] {
            sink.submit(&[value, 0, value, 0]).unwrap();
            sink.render_local(&mut out, 0);
            assert_eq!(out, [[i16::from(value); 2], [0; 2]]);
            assert!(!sink.busy());
            assert_eq!(*sink.snapshot().unwrap().pcm, [value, 0, value, 0]);
        }
    }
}
