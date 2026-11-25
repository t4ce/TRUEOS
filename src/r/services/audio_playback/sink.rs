//! Bounded playback producers, output assignments, and per-output PCM cursors.
use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

/// Two minutes of stereo signed 16-bit little-endian PCM at 48 kHz.
pub(crate) const MAX_CLIP_BYTES: usize = 48_000 * 4 * 120;
pub(crate) const MAX_APPEND_BYTES: usize = 64 * 1024;
pub(crate) const MAX_PRODUCERS: usize = 2;
const PCM_FRAME_BYTES: usize = 4;
const VIDEO_PCM_CAPACITY: usize = 48_000 * PCM_FRAME_BYTES / 2;
const PREBUFFER_BYTES: usize = 48_000 * PCM_FRAME_BYTES * 120 / 1000;
const ANALOG_MONITOR_PREBUFFER_BYTES: usize = 48_000 * PCM_FRAME_BYTES * 32 / 1000;
const PRODUCER_IDLE_MS: u64 = 5_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputBucket {
    Http,
    Analog,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProducerOwner {
    Host,
    Vm { id: u8, generation: u64 },
    Video(VideoAudioOwner),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VideoAudioOwner {
    pub slot: usize,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RouteSource {
    pub id: u64,
    pub owner: ProducerOwner,
    pub http: bool,
    pub analog: bool,
    pub active: bool,
}

#[derive(Clone)]
pub(crate) struct Clip {
    pub id: u64,
    pub pcm: Arc<Vec<u8>>,
    http_allowed: Arc<AtomicBool>,
}

impl Clip {
    fn new(id: u64, pcm: Vec<u8>, http: bool) -> Self {
        Self {
            id,
            pcm: Arc::new(pcm),
            http_allowed: Arc::new(AtomicBool::new(http)),
        }
    }

    pub(crate) fn http_forward_allowed(&self) -> bool {
        self.http_allowed.load(Ordering::Acquire)
    }
}

pub(crate) struct Sink {
    latest_clip: Option<RetainedClip>,
    next_id: u64,
    slots: [Option<Source>; MAX_PRODUCERS],
    preferences: [Option<VmPreference>; crate::hv::TRUEOS_VM_ID_LIMIT],
    local_enabled: bool,
    http_rendering: bool,
    pub(super) http_enabled: bool,
    pub(super) analog_connected: bool,
}

#[derive(Clone, Copy)]
struct VmPreference {
    generation: u64,
    routes: [bool; 2],
}

struct RetainedClip {
    clip: Clip,
    owner: ProducerOwner,
    routes: [bool; 2],
}

struct Source {
    id: u64,
    owner: ProducerOwner,
    routes: [bool; 2],
    pcm: Pcm,
    cursors: [Cursor; 2],
}

enum Pcm {
    Live {
        samples: Vec<u8>,
        last_append_ms: u64,
        analog_monitor: bool,
    },
    Video(VideoQueue),
    Complete(Clip),
}

struct VideoQueue {
    pcm: Vec<u8>,
    first_byte: usize,
    started: bool,
    paused: bool,
    ended: bool,
}

impl VideoQueue {
    fn new() -> Result<Self, i32> {
        let mut pcm = Vec::new();
        pcm.try_reserve_exact(VIDEO_PCM_CAPACITY).map_err(|_| -12)?;
        Ok(Self {
            pcm,
            first_byte: 0,
            started: false,
            paused: false,
            ended: false,
        })
    }

    fn end(&self) -> usize {
        self.first_byte + self.pcm.len()
    }

    fn write(&mut self, bytes: &[u8], consumed: usize) -> Result<usize, i32> {
        if self.ended {
            return Err(-32);
        }

        // If both clocks played silence through an underrun, accept the late
        // samples without retaining them. New audio stays on the movie timeline.
        let late = consumed.saturating_sub(self.end()).min(bytes.len());
        let current = &bytes[late..];
        if late != 0 || current.len() > VIDEO_PCM_CAPACITY - self.pcm.len() {
            let reclaim = consumed.saturating_sub(self.first_byte).min(self.pcm.len());
            self.pcm.drain(..reclaim);
            self.first_byte += reclaim + late;
        }
        let count = current.len().min(VIDEO_PCM_CAPACITY - self.pcm.len());
        self.pcm.extend_from_slice(&current[..count]);
        Ok(late + count)
    }

    fn read(&self, cursor: &mut Cursor, bytes: usize) -> &[u8] {
        if !self.started || self.paused {
            return &[];
        }
        let start = cursor.offset.min(self.end());
        let count = bytes.min(self.end() - start);
        // Preserve media time when the demuxer is late. A short slice fills
        // the rest of this output block with silence.
        if !self.ended {
            cursor.offset = cursor.offset.saturating_add(bytes);
        } else {
            cursor.offset += count;
        }
        &self.pcm[start - self.first_byte..start - self.first_byte + count]
    }
}

#[derive(Clone, Copy)]
struct Cursor {
    offset: usize,
    first_pending: Option<u64>,
    buffering: bool,
    underruns: u32,
}

impl Cursor {
    const fn new(started_ms: Option<u64>) -> Self {
        Self {
            offset: 0,
            first_pending: started_ms,
            buffering: true,
            underruns: 0,
        }
    }
}

#[derive(Default)]
pub(crate) struct RenderReport {
    pub bytes: usize,
    pub first: Option<(u64, u64)>,
    pub underrun: Option<(u64, u32)>,
}

impl RouteSource {
    fn new(id: u64, owner: ProducerOwner, [http, analog]: [bool; 2], active: bool) -> Self {
        Self {
            id,
            owner,
            http,
            analog,
            active,
        }
    }
}

impl Source {
    fn read(&mut self, output: OutputBucket, bytes: usize) -> (&[u8], RenderReport) {
        let mut report = RenderReport::default();
        let cursor = &mut self.cursors[output as usize];
        let (pcm, prebuffer) = match &self.pcm {
            Pcm::Live {
                samples,
                analog_monitor,
                ..
            } => {
                let bytes = if *analog_monitor && output == OutputBucket::Analog {
                    ANALOG_MONITOR_PREBUFFER_BYTES
                } else {
                    PREBUFFER_BYTES
                };
                (samples.as_slice(), Some(bytes))
            }
            Pcm::Video(queue) => {
                let pcm = queue.read(cursor, bytes);
                return if self.routes[output as usize] {
                    report.bytes = pcm.len();
                    (pcm, report)
                } else {
                    (&[], report)
                };
            }
            Pcm::Complete(clip) => (clip.pcm.as_slice(), None),
        };
        let available = pcm.len() - cursor.offset;
        if cursor.buffering && prebuffer.is_some_and(|needed| available < needed) {
            return (&[], report);
        }

        let count = bytes.min(available);
        let start = cursor.offset;
        cursor.offset += count;
        if count != 0 {
            report.first = cursor.first_pending.take().map(|ms| (self.id, ms));
        }
        if prebuffer.is_some() {
            cursor.buffering = count < bytes;
            if cursor.buffering {
                cursor.underruns = cursor.underruns.saturating_add(1);
                report.underrun = Some((self.id, cursor.underruns));
            }
        }
        // A deselected output still follows playback so selecting it again
        // resumes the stream at its current position.
        if !self.routes[output as usize] {
            return (&[], RenderReport::default());
        }
        report.bytes = count;
        (&pcm[start..cursor.offset], report)
    }

    fn min_offset(&self) -> usize {
        self.cursors[0].offset.min(self.cursors[1].offset)
    }

    fn drained(&self) -> bool {
        match &self.pcm {
            Pcm::Live { .. } => false,
            Pcm::Video(queue) => queue.ended && self.min_offset() >= queue.end(),
            Pcm::Complete(clip) => self
                .cursors
                .iter()
                .all(|cursor| cursor.offset == clip.pcm.len()),
        }
    }
}

impl Sink {
    pub(crate) const fn new() -> Self {
        Self {
            latest_clip: None,
            next_id: 1,
            slots: [None, None],
            preferences: [None; crate::hv::TRUEOS_VM_ID_LIMIT],
            local_enabled: false,
            http_rendering: false,
            http_enabled: true,
            analog_connected: false,
        }
    }

    fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(Option::is_none)
    }

    fn allocate_id(&mut self) -> Result<u64, i32> {
        let id = self.next_id;
        if id > i64::MAX as u64 {
            return Err(-75);
        }
        self.next_id += 1;
        Ok(id)
    }

    fn routes_for(&self, owner: ProducerOwner) -> [bool; 2] {
        if let ProducerOwner::Vm { id, generation } = owner
            && let Some(Some(preference)) = self.preferences.get(usize::from(id))
            && preference.generation == generation
        {
            return preference.routes;
        }
        [true; 2]
    }

    pub(crate) fn route_sources(&self) -> [Option<RouteSource>; MAX_PRODUCERS] {
        let mut sources = self.slots.each_ref().map(|slot| {
            slot.as_ref()
                .map(|source| RouteSource::new(source.id, source.owner, source.routes, true))
        });
        if let Some(retained) = &self.latest_clip
            && !sources
                .iter()
                .flatten()
                .any(|source| source.id == retained.clip.id)
            && let Some(row) = sources.iter_mut().find(|slot| slot.is_none())
        {
            *row = Some(RouteSource::new(
                retained.clip.id,
                retained.owner,
                retained.routes,
                false,
            ));
        }
        sources
    }

    pub(crate) fn toggle_route(
        &mut self,
        id: u64,
        bucket: OutputBucket,
        run_generation: impl FnOnce(u8) -> Option<u64>,
    ) -> bool {
        let retained = self
            .latest_clip
            .as_mut()
            .filter(|retained| retained.clip.id == id);
        let (owner, routes) = if let Some(source) = self
            .slots
            .iter_mut()
            .flatten()
            .find(|source| source.id == id)
        {
            toggle_bucket(&mut source.routes, bucket);
            if let Pcm::Complete(clip) = &source.pcm {
                clip.http_allowed.store(
                    source.routes[OutputBucket::Http as usize],
                    Ordering::Release,
                );
            }
            if let Some(retained) = retained {
                retained.routes = source.routes;
            }
            (source.owner, source.routes)
        } else if let Some(retained) = retained {
            toggle_bucket(&mut retained.routes, bucket);
            retained.clip.http_allowed.store(
                retained.routes[OutputBucket::Http as usize],
                Ordering::Release,
            );
            (retained.owner, retained.routes)
        } else {
            return false;
        };
        // Keep the setting for subsequent short utterances from this VM run.
        // Existing streams keep their own assignments.
        if let ProducerOwner::Vm { id, generation } = owner
            && run_generation(id) == Some(generation)
            && let Some(preference) = self.preferences.get_mut(usize::from(id))
        {
            let mut defaults = preference
                .filter(|current| current.generation == generation)
                .map_or([true; 2], |current| current.routes);
            defaults[bucket as usize] = routes[bucket as usize];
            *preference = Some(VmPreference {
                generation,
                routes: defaults,
            });
        }
        true
    }

    /// Preserve the complete-clip ABI's successful discard when both slots are occupied.
    pub(crate) fn submit(&mut self, owner: ProducerOwner, bytes: &[u8]) -> Result<(), i32> {
        validate_size(bytes.len())?;
        let Some(slot) = self.free_slot() else {
            return Ok(());
        };
        let mut pcm = Vec::new();
        pcm.try_reserve_exact(bytes.len()).map_err(|_| -12)?;
        pcm.extend_from_slice(bytes);
        let id = self.allocate_id()?;
        let routes = self.routes_for(owner);
        let clip = Clip::new(id, pcm, routes[OutputBucket::Http as usize]);
        self.latest_clip = Some(RetainedClip {
            clip: clip.clone(),
            owner,
            routes,
        });
        self.slots[slot] = Some(Source {
            id,
            owner,
            routes,
            pcm: Pcm::Complete(clip),
            cursors: [Cursor::new(None); 2],
        });
        Ok(())
    }

    pub(crate) fn begin(&mut self, owner: ProducerOwner, now_ms: u64) -> Result<u64, i32> {
        self.begin_live(owner, now_ms, false)
    }

    pub(crate) fn begin_analog_monitor(
        &mut self,
        owner: ProducerOwner,
        now_ms: u64,
    ) -> Result<u64, i32> {
        self.begin_live(owner, now_ms, true)
    }

    fn begin_live(&mut self, owner: ProducerOwner, now_ms: u64, analog_monitor: bool) -> Result<u64, i32> {
        let slot = self.free_slot().ok_or(-16)?;
        let id = self.allocate_id()?;
        self.slots[slot] = Some(Source {
            id,
            owner,
            routes: self.routes_for(owner),
            pcm: Pcm::Live {
                samples: Vec::new(),
                last_append_ms: now_ms,
                analog_monitor,
            },
            cursors: [Cursor::new(Some(now_ms)); 2],
        });
        Ok(id)
    }

    fn video_slot(&self, owner: VideoAudioOwner) -> Option<usize> {
        self.slots.iter().position(|slot| {
            slot.as_ref().is_some_and(|source| {
                source.owner == ProducerOwner::Video(owner) && matches!(source.pcm, Pcm::Video(_))
            })
        })
    }

    pub(crate) fn begin_video(&mut self, owner: VideoAudioOwner) -> Result<u64, i32> {
        if self.video_slot(owner).is_some() {
            return Err(-16);
        }
        let slot = self.free_slot().ok_or(-16)?;
        let queue = VideoQueue::new()?;
        let id = self.allocate_id()?;
        let owner = ProducerOwner::Video(owner);
        self.slots[slot] = Some(Source {
            id,
            owner,
            routes: self.routes_for(owner),
            pcm: Pcm::Video(queue),
            cursors: [Cursor::new(None); 2],
        });
        Ok(id)
    }

    pub(crate) fn write_video(
        &mut self,
        owner: VideoAudioOwner,
        id: u64,
        bytes: &[u8],
    ) -> Result<usize, i32> {
        if bytes.len() % PCM_FRAME_BYTES != 0 {
            return Err(-22);
        }
        let slot = self.video_slot(owner).ok_or(-2)?;
        let source = self.slots[slot]
            .as_mut()
            .filter(|source| source.id == id)
            .ok_or(-2)?;
        let consumed = source.min_offset();
        let Pcm::Video(queue) = &mut source.pcm else {
            unreachable!("owned video source");
        };
        queue.write(bytes, consumed)
    }

    pub(crate) fn start_video(&mut self, owner: VideoAudioOwner) {
        if let Some(slot) = self.video_slot(owner)
            && let Some(Source {
                pcm: Pcm::Video(queue),
                ..
            }) = &mut self.slots[slot]
        {
            queue.started = true;
        }
    }

    pub(crate) fn pause_video(&mut self, owner: VideoAudioOwner, paused: bool) {
        if let Some(slot) = self.video_slot(owner)
            && let Some(Source {
                pcm: Pcm::Video(queue),
                ..
            }) = &mut self.slots[slot]
        {
            queue.paused = paused;
        }
    }

    pub(crate) fn finish_video(&mut self, owner: VideoAudioOwner, id: u64) {
        if let Some(slot) = self.video_slot(owner)
            && let Some(Source {
                id: current,
                pcm: Pcm::Video(queue),
                ..
            }) = &mut self.slots[slot]
            && *current == id
        {
            queue.ended = true;
            self.release_drained();
        }
    }

    pub(crate) fn stop_video(&mut self, owner: VideoAudioOwner, id: Option<u64>) {
        if let Some(slot) = self.video_slot(owner)
            && self.slots[slot]
                .as_ref()
                .is_some_and(|source| id.is_none_or(|id| id == source.id))
        {
            self.slots[slot] = None;
        }
    }

    pub(crate) fn video_queued_frames(&self, owner: VideoAudioOwner, id: u64) -> usize {
        let Some(source) = self
            .video_slot(owner)
            .and_then(|slot| self.slots[slot].as_ref())
            .filter(|source| source.id == id)
        else {
            return 0;
        };
        let Pcm::Video(queue) = &source.pcm else {
            unreachable!("owned video source");
        };
        queue.end().saturating_sub(source.min_offset()) / PCM_FRAME_BYTES
    }

    fn owned_slot(&self, owner: ProducerOwner, id: u64) -> Result<usize, i32> {
        for (index, source) in self.slots.iter().enumerate() {
            if let Some(source) = source
                && source.id == id
                && matches!(source.pcm, Pcm::Live { .. })
            {
                return if source.owner == owner {
                    Ok(index)
                } else {
                    Err(-13)
                };
            }
        }
        Err(-2)
    }

    pub(crate) fn append(
        &mut self,
        owner: ProducerOwner,
        id: u64,
        bytes: &[u8],
        now_ms: u64,
    ) -> Result<(), i32> {
        validate_append_size(bytes.len())?;
        let slot = self.owned_slot(owner, id)?;
        let Some(Source {
            pcm: Pcm::Live {
                samples,
                last_append_ms,
                ..
            },
            ..
        }) = &mut self.slots[slot]
        else {
            unreachable!("owned live stream");
        };
        let total = samples.len() + bytes.len();
        validate_size(total)?;
        if total > samples.capacity() {
            let capacity = samples
                .capacity()
                .saturating_mul(2)
                .max(total)
                .min(MAX_CLIP_BYTES);
            samples
                .try_reserve_exact(capacity - samples.len())
                .map_err(|_| -12)?;
        }
        samples.extend_from_slice(bytes);
        *last_append_ms = now_ms;
        Ok(())
    }

    pub(crate) fn finish(&mut self, owner: ProducerOwner, id: u64) -> Result<(), i32> {
        let slot = self.owned_slot(owner, id)?;
        let Some(mut source) = self.slots[slot].take() else {
            unreachable!("owned live stream");
        };
        let Pcm::Live { samples, .. } = source.pcm else {
            unreachable!("owned live stream");
        };
        if !samples.is_empty() {
            let clip = Clip::new(id, samples, source.routes[OutputBucket::Http as usize]);
            self.latest_clip = Some(RetainedClip {
                clip: clip.clone(),
                owner,
                routes: source.routes,
            });
            source.pcm = Pcm::Complete(clip);
            if !source.drained() {
                self.slots[slot] = Some(source);
            }
        }
        Ok(())
    }

    pub(crate) fn abort(&mut self, owner: ProducerOwner, id: u64) -> Result<(), i32> {
        let slot = self.owned_slot(owner, id)?;
        self.slots[slot] = None;
        Ok(())
    }

    pub(crate) fn release(&mut self, owner: ProducerOwner) {
        for slot in &mut self.slots {
            if matches!(slot, Some(source) if source.owner == owner && matches!(source.pcm, Pcm::Live { .. }))
            {
                *slot = None;
            }
        }
        if let ProducerOwner::Vm { id, generation } = owner
            && let Some(preference) = self.preferences.get_mut(usize::from(id))
            && preference.is_some_and(|preference| preference.generation == generation)
        {
            *preference = None;
        }
    }

    pub(crate) fn expire(&mut self, now_ms: u64) -> [Option<u64>; MAX_PRODUCERS] {
        self.slots.each_mut().map(|slot| {
            if let Some(Source {
                id,
                pcm: Pcm::Live { last_append_ms, .. },
                ..
            }) = slot
                && now_ms.saturating_sub(*last_append_ms) >= PRODUCER_IDLE_MS
            {
                let expired = *id;
                *slot = None;
                Some(expired)
            } else {
                None
            }
        })
    }

    pub(crate) fn active_ids(&self) -> [Option<u64>; MAX_PRODUCERS] {
        self.slots
            .each_ref()
            .map(|slot| slot.as_ref().map(|source| source.id))
    }

    pub(crate) fn owns_live_pcm(&self, owner: ProducerOwner, id: u64) -> bool {
        self.slots.iter().flatten().any(|source| {
            source.owner == owner && source.id == id
                && matches!(&source.pcm, Pcm::Live { samples, .. } if !samples.is_empty())
        })
    }

    pub(crate) fn http_active_ids(&self) -> [Option<u64>; MAX_PRODUCERS] {
        self.slots.each_ref().map(|slot| {
            let source = slot.as_ref()?;
            if !source.routes[OutputBucket::Http as usize] {
                return None;
            }
            let offset = source.cursors[OutputBucket::Http as usize].offset;
            let silent = match &source.pcm {
                Pcm::Live { .. } => false,
                Pcm::Complete(clip) => offset == clip.pcm.len(),
                Pcm::Video(queue) => {
                    !queue.started || queue.paused || (queue.ended && offset >= queue.end())
                }
            };
            if silent {
                return None;
            }
            Some(source.id)
        })
    }

    /// A finite download keeps its clip alive across replacement.
    pub(crate) fn snapshot(&self) -> Option<Clip> {
        self.latest_clip
            .as_ref()
            .map(|retained| retained.clip.clone())
    }

    pub(crate) fn set_http_rendering(&mut self, rendering: bool) {
        let rendering = rendering && self.http_enabled;
        if self.http_rendering != rendering {
            self.http_rendering = rendering;
            if !rendering && self.local_enabled {
                self.follow_clock(OutputBucket::Analog, OutputBucket::Http);
            }
        }
    }

    pub(crate) fn set_local_playback(&mut self, enabled: bool) {
        if !enabled {
            self.analog_connected = false;
        }
        if self.local_enabled != enabled {
            self.local_enabled = enabled;
            if !enabled && self.http_rendering {
                self.follow_clock(OutputBucket::Http, OutputBucket::Analog);
            }
        }
    }

    pub(crate) fn http_rendering(&self) -> bool {
        self.http_rendering
    }

    fn follow_clock(&mut self, from: OutputBucket, to: OutputBucket) {
        for source in self.slots.iter_mut().flatten() {
            let mut cursor = source.cursors[from as usize];
            if to == OutputBucket::Http
                && matches!(source.pcm, Pcm::Live { analog_monitor: true, .. })
            {
                // Rejoining HTTP keeps the playback position and starts
                // collecting the larger network lead at that position.
                cursor.buffering = true;
            }
            source.cursors[to as usize] = cursor;
        }
        self.release_drained();
    }

    fn release_drained(&mut self) {
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(Source::drained) {
                *slot = None;
            }
        }
    }

    pub(crate) fn render_sources(
        &mut self,
        a: &mut [u8],
        b: &mut [u8],
    ) -> [RenderReport; MAX_PRODUCERS] {
        let [first, second] = &mut self.slots;
        let reports = [copy_http_source(first, a), copy_http_source(second, b)];
        if !self.local_enabled {
            self.follow_clock(OutputBucket::Http, OutputBucket::Analog);
        } else {
            self.release_drained();
        }
        reports
    }

    pub(crate) fn render_local(&mut self, out: &mut [[i16; 2]], now_ms: u64) {
        let _ = self.expire(now_ms);
        let max_bytes = core::mem::size_of_val(out);
        let [first, second] = &mut self.slots;
        let (a, _) = read_source(first, OutputBucket::Analog, max_bytes);
        let (b, _) = read_source(second, OutputBucket::Analog, max_bytes);
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
        if !self.http_rendering {
            self.follow_clock(OutputBucket::Analog, OutputBucket::Http);
        } else {
            self.release_drained();
        }
    }

    pub(crate) fn advance_headless(&mut self, bytes: usize, now_ms: u64) {
        if self.http_rendering || self.local_enabled {
            return;
        }
        let _ = self.expire(now_ms);
        for source in self.slots.iter_mut().flatten() {
            let _ = source.read(OutputBucket::Http, bytes);
        }
        self.follow_clock(OutputBucket::Http, OutputBucket::Analog);
    }

    #[cfg(test)]
    pub(crate) fn render(&mut self, out: &mut [u8]) -> [RenderReport; MAX_PRODUCERS] {
        let mut a = alloc::vec![0; out.len()];
        let mut b = alloc::vec![0; out.len()];
        let reports = self.render_sources(&mut a, &mut b);
        mix_center(out, &a[..reports[0].bytes], &b[..reports[1].bytes]);
        reports
    }

    #[cfg(test)]
    pub(crate) fn active_count(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    #[cfg(test)]
    pub(crate) fn busy(&self) -> bool {
        self.active_count() != 0
    }
}

fn toggle_bucket(routes: &mut [bool; 2], bucket: OutputBucket) {
    routes[bucket as usize] = !routes[bucket as usize];
}

fn read_source(
    source: &mut Option<Source>,
    output: OutputBucket,
    bytes: usize,
) -> (&[u8], RenderReport) {
    match source {
        Some(source) => source.read(output, bytes),
        None => (&[], RenderReport::default()),
    }
}

fn copy_http_source(source: &mut Option<Source>, out: &mut [u8]) -> RenderReport {
    let (samples, report) = read_source(source, OutputBucket::Http, out.len());
    out[..report.bytes].copy_from_slice(samples);
    out[report.bytes..].fill(0);
    report
}

fn sample(pcm: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes([pcm[offset], pcm[offset + 1]])
}

fn center_sample(a: &[u8], b: &[u8], offset: usize) -> i16 {
    ((i32::from(sample(a, offset)) + i32::from(sample(b, offset))) / 2) as i16
}

/// Stereo playback is centered during overlap and at unity with one producer.
pub(crate) fn mix_center(out: &mut [u8], a: &[u8], b: &[u8]) {
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

pub(crate) fn validate_size(bytes: usize) -> Result<(), i32> {
    if bytes == 0 || bytes % 4 != 0 {
        Err(-22)
    } else if bytes > MAX_CLIP_BYTES {
        Err(-27)
    } else {
        Ok(())
    }
}

pub(crate) fn validate_append_size(bytes: usize) -> Result<(), i32> {
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
    fn video_starts_after_publication_pauses_and_drains_without_retaining_a_clip() {
        let mut sink = Sink::new();
        sink.set_local_playback(true);
        sink.set_http_rendering(true);
        let owner = VideoAudioOwner {
            slot: 0,
            generation: 1,
        };
        let id = sink.begin_video(owner).unwrap();
        assert_eq!(
            sink.route_sources()[0],
            Some(RouteSource::new(
                id,
                ProducerOwner::Video(owner),
                [true; 2],
                true
            ))
        );
        assert_eq!(
            sink.write_video(owner, id, &[8, 0, 9, 0, 10, 0, 11, 0]),
            Ok(8)
        );

        let mut http = [0; 4];
        let mut local = [[0; 2]; 1];
        sink.render(&mut http);
        sink.render_local(&mut local, 0);
        assert_eq!(http, [0; 4]);
        assert_eq!(local, [[0; 2]]);
        assert_eq!(sink.http_active_ids(), [None, None]);
        assert_eq!(sink.video_queued_frames(owner, id), 2);

        sink.pause_video(owner, true);
        sink.start_video(owner);
        sink.render(&mut http);
        sink.render_local(&mut local, 0);
        assert_eq!(http, [0; 4]);
        assert_eq!(local, [[0; 2]]);
        assert_eq!(sink.video_queued_frames(owner, id), 2);

        sink.pause_video(owner, false);
        assert_eq!(sink.http_active_ids(), [Some(id), None]);
        sink.render(&mut http);
        sink.render_local(&mut local, 0);
        assert_eq!(http, [8, 0, 9, 0]);
        assert_eq!(local, [[8, 9]]);
        assert_eq!(sink.video_queued_frames(owner, id), 1);

        sink.toggle_route(id, OutputBucket::Http, |_| None);
        sink.finish_video(owner, id);
        sink.render(&mut http);
        assert_eq!(http, [0; 4]);
        assert_eq!(sink.video_queued_frames(owner, id), 1);
        sink.render_local(&mut local, 0);
        assert_eq!(local, [[10, 11]]);
        assert_eq!(sink.video_queued_frames(owner, id), 0);
        assert_eq!(sink.active_ids(), [None, None]);
        assert!(sink.snapshot().is_none());
    }

    #[test]
    fn video_queue_reclaims_consumed_pcm_and_drops_only_late_frames_after_underrun() {
        let mut sink = Sink::new();
        let owner = VideoAudioOwner {
            slot: 0,
            generation: 1,
        };
        let id = sink.begin_video(owner).unwrap();
        let pcm = [8, 0, 9, 0].repeat(VIDEO_PCM_CAPACITY / 4);
        assert_eq!(sink.write_video(owner, id, &pcm), Ok(VIDEO_PCM_CAPACITY));
        assert_eq!(sink.write_video(owner, id, &[1; 4]), Ok(0));
        assert_eq!(sink.write_video(owner, id, &[1; 3]), Err(-22));
        sink.start_video(owner);

        let mut block = [0; 8];
        sink.render(&mut block);
        assert_eq!(block, [8, 0, 9, 0, 8, 0, 9, 0]);
        assert_eq!(sink.write_video(owner, id, &[1; 12]), Ok(8));
        let mut rest = alloc::vec![0; VIDEO_PCM_CAPACITY + 8];
        sink.render(&mut rest);
        assert_eq!(
            &rest[..VIDEO_PCM_CAPACITY - 8],
            &pcm[..VIDEO_PCM_CAPACITY - 8]
        );
        assert_eq!(
            &rest[VIDEO_PCM_CAPACITY - 8..],
            &[1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(sink.video_queued_frames(owner, id), 0);

        assert_eq!(
            sink.write_video(owner, id, &[1, 0, 2, 0, 3, 0, 4, 0, 5, 0, 6, 0]),
            Ok(12)
        );
        assert_eq!(sink.video_queued_frames(owner, id), 1);
        sink.render(&mut block);
        assert_eq!(block, [5, 0, 6, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn video_mixes_with_ordinary_audio_and_generation_fences_replacement() {
        let mut sink = Sink::new();
        let old = VideoAudioOwner {
            slot: 0,
            generation: 1,
        };
        let new = VideoAudioOwner {
            slot: 0,
            generation: 2,
        };
        let id = sink.begin_video(old).unwrap();
        assert_eq!(sink.begin_video(old), Err(-16));
        assert_eq!(sink.write_video(old, id, &[40, 0, 60, 0]), Ok(4));
        sink.start_video(old);
        sink.submit(ProducerOwner::Host, &[100, 0, 120, 0]).unwrap();
        let clip = sink.snapshot().unwrap();
        let mut block = [0; 4];
        sink.render(&mut block);
        assert_eq!(block, [70, 0, 90, 0]);
        sink.stop_video(old, Some(id));

        let next = sink.begin_video(new).unwrap();
        assert_eq!(sink.write_video(old, id, &[1; 4]), Err(-2));
        sink.start_video(old);
        sink.stop_video(old, None);
        assert_eq!(sink.write_video(new, next, &[8, 0, 9, 0]), Ok(4));
        sink.render(&mut block);
        assert_eq!(block, [0; 4]);
        sink.start_video(new);
        sink.render(&mut block);
        assert_eq!(block, [8, 0, 9, 0]);
        assert_eq!(sink.snapshot().unwrap().id, clip.id);
    }

    #[test]
    fn source_ids_follow_live_and_finished_audio_until_its_own_drain() {
        let mut sink = Sink::new();
        let owner = ProducerOwner::Host;
        let speech = sink.begin(owner, 0).unwrap();
        sink.append(owner, speech, &[1; 4], 0).unwrap();
        sink.submit(ProducerOwner::Host, &[2; 8]).unwrap();
        let other = sink.snapshot().unwrap().id;
        assert_eq!(sink.active_ids(), [Some(speech), Some(other)]);
        sink.finish(owner, speech).unwrap();
        assert_eq!(sink.active_ids(), [Some(speech), Some(other)]);
        sink.render(&mut [0; 4]);
        assert_eq!(sink.active_ids(), [None, Some(other)]);
        sink.render(&mut [0; 4]);
        assert_eq!(sink.active_ids(), [None, None]);
    }

    #[test]
    fn two_clips_play_and_a_third_is_discarded() {
        let mut sink = Sink::new();
        sink.submit(ProducerOwner::Host, &[100, 0, 200, 0, 8, 0, 9, 0])
            .unwrap();
        sink.submit(ProducerOwner::Host, &[200, 0, 100, 0]).unwrap();
        let id = sink.snapshot().unwrap().id;
        assert_eq!(sink.submit(ProducerOwner::Host, &[42; 4]), Ok(()));
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
        sink.submit(ProducerOwner::Host, &input).unwrap();
        input.fill(99);
        drop(input);
        let mut block = [0; 8];
        sink.render(&mut block);
        assert_eq!(block, [1, 2, 3, 4, 0, 0, 0, 0]);
        assert!(!sink.busy());
        sink.submit(ProducerOwner::Host, &[5, 6, 7, 8]).unwrap();
        sink.render(&mut block);
        assert_eq!(&block[..4], &[5, 6, 7, 8]);
    }

    #[test]
    fn completed_utterance_longer_than_the_old_half_second_buffer_is_lossless() {
        let mut sink = Sink::new();
        let bytes: Vec<u8> = (0..48_000 * 4 * 3).map(|n| (n % 251) as u8).collect();
        sink.submit(ProducerOwner::Host, &bytes).unwrap();
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
        assert_eq!(sink.submit(ProducerOwner::Host, &[]), Err(-22));
        assert_eq!(sink.submit(ProducerOwner::Host, &[1, 2]), Err(-22));
        assert_eq!(validate_size(MAX_CLIP_BYTES), Ok(()));
        assert_eq!(validate_size(MAX_CLIP_BYTES + 4), Err(-27));
        assert!(sink.snapshot().is_none());
    }

    #[test]
    fn streams_after_prebuffer_and_keeps_all_pcm_for_final_snapshot() {
        let mut sink = Sink::new();
        let id = sink.begin(ProducerOwner::Host, 10).unwrap();
        let initial = alloc::vec![17; PREBUFFER_BYTES - 4];
        sink.append(ProducerOwner::Host, id, &initial, 20).unwrap();
        let mut block = [1; 8];
        assert!(sink.render(&mut block)[0].first.is_none());
        assert_eq!(block, [0; 8]);
        assert!(sink.snapshot().is_none());
        sink.append(ProducerOwner::Host, id, &[23; 4], 30).unwrap();
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
        sink.append(ProducerOwner::Host, id, &[29; 4], 40).unwrap();
        sink.finish(ProducerOwner::Host, id).unwrap();
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
    fn analog_monitor_only_shortens_jack_start_and_rebuffer() {
        let mut sink = Sink::new();
        sink.set_local_playback(true);
        sink.set_http_rendering(true);
        let owner = ProducerOwner::Host;
        let monitor = sink.begin_analog_monitor(owner, 10).unwrap();
        let standard = sink.begin(owner, 10).unwrap();
        let frame = [5, 0, 9, 0];
        let below_threshold = frame.repeat(ANALOG_MONITOR_PREBUFFER_BYTES / PCM_FRAME_BYTES - 1);
        for id in [monitor, standard] {
            sink.append(owner, id, &below_threshold, 20).unwrap();
        }
        let mut jack = [[0; 2]; 1];
        let mut first = [0; PCM_FRAME_BYTES];
        let mut second = [0; PCM_FRAME_BYTES];
        sink.render_local(&mut jack, 20);
        assert_eq!(jack, [[0; 2]]);

        for id in [monitor, standard] {
            sink.append(owner, id, &frame, 30).unwrap();
        }
        assert_eq!(
            sink.render_sources(&mut first, &mut second).map(|r| r.bytes),
            [0; 2]
        );

        // The monitor is audible on Jack, while a legacy stream with the
        // same number of queued samples is still buffering.
        let mut drain = alloc::vec![[0; 2]; ANALOG_MONITOR_PREBUFFER_BYTES / PCM_FRAME_BYTES + 1];
        sink.render_local(&mut drain, 30);
        assert_eq!(drain[0], [5, 9]);
        assert_eq!(drain[drain.len() - 2], [5, 9]);
        assert_eq!(drain[drain.len() - 1], [0, 0]);
        sink.append(owner, monitor, &below_threshold, 40).unwrap();
        sink.render_local(&mut jack, 40);
        assert_eq!(jack, [[0; 2]]);
        sink.append(owner, monitor, &frame, 50).unwrap();
        sink.render_local(&mut jack, 50);
        assert_eq!(jack, [[5, 9]]);

        let to_http_threshold =
            (PREBUFFER_BYTES - 2 * ANALOG_MONITOR_PREBUFFER_BYTES) / PCM_FRAME_BYTES;
        sink.append(owner, monitor, &frame.repeat(to_http_threshold), 60)
            .unwrap();
        assert_eq!(
            sink.render_sources(&mut first, &mut second).map(|r| r.bytes),
            [PCM_FRAME_BYTES, 0]
        );
        assert_eq!(first, frame);
    }

    #[test]
    fn http_keeps_standard_prebuffer_after_following_an_analog_monitor() {
        let mut sink = Sink::new();
        sink.set_local_playback(true);
        let owner = ProducerOwner::Host;
        let id = sink.begin_analog_monitor(owner, 0).unwrap();
        let frame = [11, 0, 13, 0];
        let monitor_frames = ANALOG_MONITOR_PREBUFFER_BYTES / PCM_FRAME_BYTES;
        let http_frames = PREBUFFER_BYTES / PCM_FRAME_BYTES;
        let mut initial = frame.repeat(monitor_frames);
        initial[0] = 4;
        sink.append(owner, id, &initial, 10).unwrap();
        let mut jack = [[0; 2]; 1];
        sink.render_local(&mut jack, 10);
        assert_eq!(jack, [[4, 13]]);

        sink.set_http_rendering(true);
        let mut first = [0; PCM_FRAME_BYTES];
        let mut second = [0; PCM_FRAME_BYTES];
        assert_eq!(sink.render_sources(&mut first, &mut second)[0].bytes, 0);
        sink.append(owner, id, &frame.repeat(http_frames - monitor_frames), 20)
            .unwrap();
        assert_eq!(sink.render_sources(&mut first, &mut second)[0].bytes, 0);
        sink.append(owner, id, &frame, 30).unwrap();
        assert_eq!(
            sink.render_sources(&mut first, &mut second)[0].bytes,
            PCM_FRAME_BYTES
        );
        assert_eq!(first, frame, "the HTTP clock kept the previous analog position");
    }

    #[test]
    fn two_live_producers_prebuffer_independently_and_keep_their_slots() {
        let mut sink = Sink::new();
        let vm = ProducerOwner::Vm {
            id: 2,
            generation: 1,
        };
        let a = sink.begin(ProducerOwner::Host, 10).unwrap();
        let b = sink.begin(vm, 11).unwrap();
        let mut left = [0; 8];
        let mut right = [0; 8];
        sink.append(vm, b, &alloc::vec![7; PREBUFFER_BYTES], 12)
            .unwrap();

        let reports = sink.render_sources(&mut left, &mut right);
        assert_eq!((reports[0].bytes, reports[1].bytes), (0, 8));
        assert_eq!((left, right), ([0; 8], [7; 8]));

        sink.append(ProducerOwner::Host, a, &alloc::vec![4; PREBUFFER_BYTES], 13)
            .unwrap();
        let reports = sink.render_sources(&mut left, &mut right);
        assert_eq!((reports[0].bytes, reports[1].bytes), (8, 8));
        assert_eq!((left, right), ([4; 8], [7; 8]));

        sink.abort(ProducerOwner::Host, a).unwrap();
        let reports = sink.render_sources(&mut left, &mut right);
        assert_eq!((reports[0].bytes, reports[1].bytes), (0, 8));
        assert_eq!((left, right), ([0; 8], [7; 8]));
    }

    #[test]
    fn ownership_abort_and_expiry_preserve_the_previous_completed_clip() {
        let mut sink = Sink::new();
        sink.submit(ProducerOwner::Host, &[3; 4]).unwrap();
        let previous = sink.snapshot().unwrap().id;
        sink.render(&mut [0; 4]);
        let owner = ProducerOwner::Vm {
            id: 1,
            generation: 10,
        };
        let reused = ProducerOwner::Vm {
            id: 1,
            generation: 11,
        };
        let id = sink.begin(owner, 10).unwrap();
        let other = sink.begin(reused, 20).unwrap();
        assert_eq!(sink.begin(ProducerOwner::Host, 20), Err(-16));
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
        sink.set_http_rendering(true);
        let a = sink.begin(ProducerOwner::Host, 0).unwrap();
        let b = sink.begin(ProducerOwner::Host, 0).unwrap();
        let mut a_pcm = [16, 0, 48, 0].repeat(PREBUFFER_BYTES / 4);
        let mut b_pcm = [48, 0, 16, 0].repeat(PREBUFFER_BYTES / 4);
        a_pcm[..4].copy_from_slice(&[32, 0, 64, 0]);
        b_pcm[..4].copy_from_slice(&[64, 0, 32, 0]);
        sink.append(ProducerOwner::Host, a, &a_pcm, 0).unwrap();
        sink.append(ProducerOwner::Host, b, &b_pcm, 0).unwrap();

        let mut local = [[0; 2]; 1];
        sink.render_local(&mut local, 0);
        assert_eq!(local, [[48, 48]]);
        sink.render_local(&mut local, 0);
        assert_eq!(local, [[32, 32]]);

        let mut http = [0; 4];
        sink.render(&mut http);
        assert_eq!(http, [48, 0, 48, 0]);
        sink.finish(ProducerOwner::Host, a).unwrap();
        sink.abort(ProducerOwner::Host, b).unwrap();
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
            sink.submit(ProducerOwner::Host, &[value, 0, value, 0])
                .unwrap();
            sink.render_local(&mut out, 0);
            assert_eq!(out, [[i16::from(value); 2], [0; 2]]);
            assert!(!sink.busy());
            assert_eq!(*sink.snapshot().unwrap().pcm, [value, 0, value, 0]);
        }
    }

    #[test]
    fn each_output_receives_only_its_assigned_producers() {
        let mut sink = Sink::new();
        sink.set_local_playback(true);
        sink.set_http_rendering(true);
        sink.submit(ProducerOwner::Host, &[1, 0, 2, 0, 3, 0, 4, 0])
            .unwrap();
        let first = sink.snapshot().unwrap().id;
        sink.submit(ProducerOwner::Host, &[5, 0, 6, 0, 7, 0, 8, 0])
            .unwrap();
        let second = sink.snapshot().unwrap().id;
        assert!(sink.toggle_route(first, OutputBucket::Analog, |_| None));
        assert!(sink.toggle_route(second, OutputBucket::Http, |_| None));
        assert_eq!(sink.http_active_ids(), [Some(first), None]);
        assert!(!sink.snapshot().unwrap().http_forward_allowed());

        let mut http = [0; 4];
        let mut local = [[0; 2]; 1];
        sink.render(&mut http);
        sink.render_local(&mut local, 0);
        assert_eq!(http, [1, 0, 2, 0]);
        assert_eq!(local, [[5, 6]]);

        // Changing the routes resumes at the current position on both clocks.
        assert!(sink.toggle_route(first, OutputBucket::Analog, |_| None));
        assert!(sink.toggle_route(first, OutputBucket::Http, |_| None));
        assert!(sink.toggle_route(second, OutputBucket::Http, |_| None));
        assert!(sink.toggle_route(second, OutputBucket::Analog, |_| None));
        sink.render(&mut http);
        sink.render_local(&mut local, 0);
        assert_eq!(http, [7, 0, 8, 0]);
        assert_eq!(local, [[3, 4]]);
        assert_eq!(sink.active_ids(), [None, None]);
        assert!(!sink.toggle_route(first, OutputBucket::Http, |_| None));
    }

    #[test]
    fn complete_clip_route_updates_existing_downloads_without_affecting_other_clips() {
        let mut sink = Sink::new();
        sink.submit(ProducerOwner::Host, &[1; 4]).unwrap();
        let older = sink.snapshot().unwrap();
        sink.submit(ProducerOwner::Host, &[2; 4]).unwrap();
        let current = sink.snapshot().unwrap();
        assert!(older.http_forward_allowed());
        assert!(current.http_forward_allowed());
        assert!(sink.toggle_route(current.id, OutputBucket::Http, |_| None));
        assert!(!current.http_forward_allowed());
        assert!(older.http_forward_allowed());
        assert!(sink.toggle_route(current.id, OutputBucket::Http, |_| None));
        assert!(current.http_forward_allowed());
    }

    #[test]
    fn vm_route_defaults_follow_its_generation_without_changing_concurrent_streams() {
        let mut sink = Sink::new();
        let owner = ProducerOwner::Vm {
            id: 1,
            generation: 9,
        };
        let first = sink.begin(owner, 0).unwrap();
        let second = sink.begin(owner, 0).unwrap();
        assert!(sink.toggle_route(first, OutputBucket::Http, |_| Some(9)));
        assert!(sink.toggle_route(second, OutputBucket::Analog, |_| Some(9)));
        assert!(!sink.route_sources()[0].unwrap().http);
        assert!(sink.route_sources()[0].unwrap().analog);
        assert!(sink.route_sources()[1].unwrap().http);
        assert!(!sink.route_sources()[1].unwrap().analog);

        sink.abort(owner, first).unwrap();
        sink.abort(owner, second).unwrap();
        sink.submit(owner, &[2; 4]).unwrap();
        assert!(!sink.route_sources()[0].unwrap().http);
        assert!(!sink.route_sources()[0].unwrap().analog);

        let next = ProducerOwner::Vm {
            id: 1,
            generation: 10,
        };
        sink.release(owner);
        sink.begin(next, 0).unwrap();
        assert!(sink.route_sources()[1].unwrap().http);
        // A queued clip from the old run can still be routed on its own.
        let clip = sink.snapshot().unwrap();
        assert!(sink.toggle_route(clip.id, OutputBucket::Analog, |_| Some(10)));
        assert!(sink.route_sources()[0].unwrap().analog);
        assert!(sink.routes_for(next).into_iter().all(|route| route));
    }

    #[test]
    fn most_recent_completed_clip_remains_configurable_after_playback() {
        let mut sink = Sink::new();
        let owner = ProducerOwner::Vm {
            id: 1,
            generation: 9,
        };
        sink.submit(owner, &[1; 4]).unwrap();
        let clip = sink.snapshot().unwrap();
        sink.render(&mut [0; 4]);
        assert_eq!(sink.active_ids(), [None, None]);
        let route = sink.route_sources()[0].unwrap();
        assert_eq!(route.owner, owner);
        assert_eq!(route.id, clip.id);
        assert!(!route.active);
        assert!(sink.toggle_route(route.id, OutputBucket::Http, |_| Some(9)));
        assert!(!clip.http_forward_allowed());

        let next = sink.begin(owner, 10).unwrap();
        let route = sink.route_sources()[0].unwrap();
        assert_eq!(route.id, next);
        assert!(route.active);
        assert!(!route.http);
        assert!(route.analog);
    }

    #[test]
    fn fallback_clock_releases_unrouted_clips_and_stops_when_hda_takes_over() {
        let mut sink = Sink::new();
        sink.submit(ProducerOwner::Host, &[1, 0, 2, 0, 3, 0, 4, 0])
            .unwrap();
        let clip = sink.snapshot().unwrap();
        assert!(sink.toggle_route(clip.id, OutputBucket::Http, |_| None));
        sink.advance_headless(4, 0);
        assert_eq!(sink.active_ids(), [Some(clip.id), None]);

        sink.set_local_playback(true);
        sink.advance_headless(4, 1);
        let mut local = [[0; 2]; 1];
        sink.render_local(&mut local, 1);
        assert_eq!(local, [[3, 4]]);
        assert_eq!(sink.active_ids(), [None, None]);

        sink.set_local_playback(false);
        sink.submit(ProducerOwner::Host, &[5, 0, 6, 0]).unwrap();
        let next = sink.snapshot().unwrap();
        assert!(sink.toggle_route(next.id, OutputBucket::Http, |_| None));
        assert!(sink.toggle_route(next.id, OutputBucket::Analog, |_| None));
        sink.advance_headless(4, 2);
        assert!(!sink.busy());
    }
}
