//! Shared PCM playback for the Intel HDA service.

use alloc::{collections::VecDeque, sync::Arc, vec::Vec};
use core::{
    future::poll_fn,
    ops::Range,
    task::{Poll, Waker},
};
use spin::Mutex;

pub(crate) type Frame = [i16; 2];
pub(super) const OUTPUT_RATE: u32 = 48_000;
const STREAMS: usize = 8;
const QUEUE_FRAMES: usize = 4096;
const FRACTION_BITS: u32 = 32;
const FRACTION_MASK: u64 = (1 << FRACTION_BITS) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Unavailable,
    NoSlots,
    Allocation,
    Closed,
    InvalidPosition,
    InvalidRate,
    InvalidSpeed,
    InvalidWave,
}

/// Immutable, RAM-backed signed 16-bit PCM shared by independent playheads.
#[derive(Clone)]
pub(crate) struct Clip {
    bytes: Arc<Vec<u8>>,
    data: Range<usize>,
    channels: u16,
    sample_rate: u32,
}

impl Clip {
    /// Uses the provided WAV allocation directly. Unknown RIFF chunks are skipped.
    pub(crate) fn from_wav(bytes: Arc<Vec<u8>>) -> Result<Self, Error> {
        if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err(Error::InvalidWave);
        }
        let riff_end = (le_u32(&bytes[4..8]) as usize)
            .checked_add(8)
            .filter(|&end| (12..=bytes.len()).contains(&end))
            .ok_or(Error::InvalidWave)?;
        let mut format = None;
        let mut data = None;
        let mut offset = 12;
        while offset < riff_end {
            let header = bytes
                .get(offset..riff_end)
                .and_then(|chunk| chunk.get(..8))
                .ok_or(Error::InvalidWave)?;
            let size = le_u32(&header[4..]) as usize;
            let start = offset + 8;
            let end = start
                .checked_add(size)
                .filter(|&end| end <= riff_end)
                .ok_or(Error::InvalidWave)?;
            match &header[..4] {
                b"fmt " if format.is_none() && size >= 16 => {
                    let fmt = &bytes[start..end];
                    let channels = le_u16(&fmt[2..4]);
                    let sample_rate = le_u32(&fmt[4..8]);
                    if le_u16(&fmt[..2]) != 1
                        || !matches!(channels, 1 | 2)
                        || le_u16(&fmt[14..16]) != 16
                        || le_u16(&fmt[12..14]) != channels * 2
                    {
                        return Err(Error::InvalidWave);
                    }
                    validate_rate(sample_rate)?;
                    if le_u32(&fmt[8..12]) != sample_rate * u32::from(channels) * 2 {
                        return Err(Error::InvalidWave);
                    }
                    format = Some((channels, sample_rate));
                }
                b"data" if data.is_none() => data = Some(start..end),
                _ => {}
            }
            offset = end
                .checked_add(size & 1)
                .filter(|&padded| padded <= riff_end)
                .ok_or(Error::InvalidWave)?;
        }
        let (channels, sample_rate) = format.ok_or(Error::InvalidWave)?;
        let data = data.ok_or(Error::InvalidWave)?;
        if data.len() % (usize::from(channels) * 2) != 0 {
            return Err(Error::InvalidWave);
        }
        Ok(Self {
            bytes,
            data,
            channels,
            sample_rate,
        })
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) fn frames(&self) -> u64 {
        (self.data.len() / (usize::from(self.channels) * 2)) as u64
    }

    fn frame(&self, index: u64) -> Frame {
        let offset = self.data.start + index as usize * usize::from(self.channels) * 2;
        let left = le_u16(&self.bytes[offset..offset + 2]) as i16;
        let right = if self.channels == 2 {
            le_u16(&self.bytes[offset + 2..offset + 4]) as i16
        } else {
            left
        };
        [left, right]
    }
}

fn le_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes(bytes.try_into().expect("PCM word"))
}

fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().expect("RIFF field"))
}

fn validate_rate(sample_rate: u32) -> Result<(), Error> {
    if (8_000..=192_000).contains(&sample_rate) {
        Ok(())
    } else {
        Err(Error::InvalidRate)
    }
}

fn step(sample_rate: u32, permille: u32) -> Result<u64, Error> {
    if !(250..=4_000).contains(&permille) {
        return Err(Error::InvalidSpeed);
    }
    let numerator = (u64::from(sample_rate) * u64::from(permille)) << FRACTION_BITS;
    Ok(numerator / (u64::from(OUTPUT_RATE) * 1000))
}

fn interpolate(a: Frame, b: Frame, fraction: u64) -> Frame {
    let [left, right] = [0, 1].map(|channel| {
        let base = i64::from(a[channel]);
        let delta = i64::from(b[channel]) - base;
        (base + ((delta * fraction as i64) >> FRACTION_BITS)) as i16
    });
    [left, right]
}

struct Mixer {
    enabled: bool,
    sequence: u64,
    sources: [Option<Source>; STREAMS],
}

static MIXER: Mutex<Mixer> = Mutex::new(Mixer {
    enabled: false,
    sequence: 0,
    sources: [const { None }; STREAMS],
});

struct Source {
    id: u64,
    sample_rate: u32,
    step: u64,
    paused: bool,
    data: Input,
}

enum Input {
    File {
        clip: Clip,
        cursor: u64,
    },
    Live {
        queue: VecDeque<Frame>,
        fraction: u64,
        finished: bool,
        writer: Option<Waker>,
    },
}

impl Source {
    fn next_frame(&mut self) -> Option<Frame> {
        if self.paused {
            return None;
        }
        match &mut self.data {
            Input::File { clip, cursor } => {
                let index = *cursor >> FRACTION_BITS;
                let last = clip.frames().checked_sub(1)?;
                if index > last {
                    return None;
                }
                let a = clip.frame(index);
                let b = clip.frame((index + 1).min(last));
                let output = interpolate(a, b, *cursor & FRACTION_MASK);
                *cursor = (*cursor + self.step).min(clip.frames() << FRACTION_BITS);
                Some(output)
            }
            Input::Live {
                queue, fraction, ..
            } => {
                let a = *queue.front()?;
                let b = queue.get(1).copied().unwrap_or(a);
                let output = interpolate(a, b, *fraction);
                let next = *fraction + self.step;
                let consumed = ((next >> FRACTION_BITS) as usize).min(queue.len());
                queue.drain(..consumed);
                *fraction = if queue.is_empty() {
                    0
                } else {
                    next & FRACTION_MASK
                };
                Some(output)
            }
        }
    }
}

struct Stream {
    slot: usize,
    id: u64,
}

impl Stream {
    fn open(sample_rate: u32, data: Input) -> Result<Self, Error> {
        let step = step(sample_rate, 1000)?;
        let mut mixer = MIXER.lock();
        if !mixer.enabled {
            return Err(Error::Unavailable);
        }
        let slot = mixer
            .sources
            .iter()
            .position(Option::is_none)
            .ok_or(Error::NoSlots)?;
        mixer.sequence = mixer.sequence.checked_add(1).ok_or(Error::NoSlots)?;
        let id = mixer.sequence;
        mixer.sources[slot] = Some(Source {
            id,
            sample_rate,
            step,
            paused: false,
            data,
        });
        Ok(Self { slot, id })
    }

    fn with_source<T>(&self, f: impl FnOnce(&mut Source) -> Result<T, Error>) -> Result<T, Error> {
        let mut mixer = MIXER.lock();
        let source = mixer.sources[self.slot]
            .as_mut()
            .filter(|source| source.id == self.id);
        f(source.ok_or(Error::Closed)?)
    }

    fn set_paused(&self, paused: bool) -> Result<(), Error> {
        self.with_source(|source| {
            source.paused = paused;
            Ok(())
        })
    }

    fn set_speed(&self, permille: u32) -> Result<(), Error> {
        self.with_source(|source| {
            source.step = step(source.sample_rate, permille)?;
            Ok(())
        })
    }

    fn close(&self) {
        let mut mixer = MIXER.lock();
        let slot = &mut mixer.sources[self.slot];
        if slot.as_ref().is_some_and(|source| source.id == self.id) {
            let source = slot.take();
            drop(mixer);
            drop(source);
        }
    }
}

/// A playhead over a shared WAV. Drop the handle to stop playback.
pub(crate) struct FileStream(Stream);

impl FileStream {
    pub(crate) fn open(clip: Clip) -> Result<Self, Error> {
        Stream::open(clip.sample_rate(), Input::File { clip, cursor: 0 }).map(Self)
    }

    pub(crate) fn seek(&self, frame: u64) -> Result<(), Error> {
        self.0.with_source(|source| {
            let Input::File { clip, cursor } = &mut source.data else {
                unreachable!("file handle owns a file source");
            };
            if frame > clip.frames() {
                return Err(Error::InvalidPosition);
            }
            *cursor = frame << FRACTION_BITS;
            Ok(())
        })
    }

    /// Source frames queued for playback, including frames already handed to DMA.
    pub(crate) fn position(&self) -> Result<u64, Error> {
        self.0.with_source(|source| {
            let Input::File { cursor, .. } = source.data else {
                unreachable!("file handle owns a file source");
            };
            Ok(cursor >> FRACTION_BITS)
        })
    }

    /// 1000 is normal speed. Resampling changes the pitch with the rate.
    pub(crate) fn set_speed(&self, permille: u32) -> Result<(), Error> {
        self.0.set_speed(permille)
    }

    pub(crate) fn set_paused(&self, paused: bool) -> Result<(), Error> {
        self.0.set_paused(paused)
    }
}

impl Drop for FileStream {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// A single writer to a bounded PCM queue. Drop aborts, finish drains.
pub(crate) struct LiveStream(Option<Stream>);

impl LiveStream {
    pub(crate) fn open(sample_rate: u32) -> Result<Self, Error> {
        validate_rate(sample_rate)?;
        let queue = crate::allocators::with_host_alloc_domain_strong(|| {
            let mut queue = VecDeque::new();
            queue
                .try_reserve_exact(QUEUE_FRAMES)
                .map_err(|_| Error::Allocation)?;
            Ok(queue)
        })?;
        Stream::open(
            sample_rate,
            Input::Live {
                queue,
                fraction: 0,
                finished: false,
                writer: None,
            },
        )
        .map(|stream| Self(Some(stream)))
    }

    fn stream(&self) -> &Stream {
        self.0.as_ref().expect("live stream handle")
    }

    /// Writes a prefix of the given stereo frames. Zero means the queue is full.
    pub(crate) fn try_write(&mut self, frames: &[Frame]) -> Result<usize, Error> {
        self.write_frames(frames, None)
    }

    fn write_frames(&mut self, frames: &[Frame], waker: Option<&Waker>) -> Result<usize, Error> {
        self.stream().with_source(|source| {
            let Input::Live { queue, writer, .. } = &mut source.data else {
                unreachable!("live handle owns a live source");
            };
            let count = frames.len().min(QUEUE_FRAMES - queue.len());
            if count == 0 && !frames.is_empty() {
                if let Some(waker) = waker
                    && !writer.as_ref().is_some_and(|old| old.will_wake(waker))
                {
                    *writer = Some(waker.clone());
                }
            } else {
                queue.extend(frames[..count].iter().copied());
                *writer = None;
            }
            Ok(count)
        })
    }

    /// Waits for queue space and writes a prefix. A cancelled wait writes nothing.
    pub(crate) async fn write(&mut self, frames: &[Frame]) -> Result<usize, Error> {
        poll_fn(|cx| match self.write_frames(frames, Some(cx.waker())) {
            Ok(0) if !frames.is_empty() => Poll::Pending,
            result => Poll::Ready(result),
        })
        .await
    }

    /// Drops queued frames. Audio already handed to DMA may still play.
    pub(crate) fn flush(&mut self) -> Result<(), Error> {
        self.stream().with_source(|source| {
            let Input::Live {
                queue,
                fraction,
                writer,
                ..
            } = &mut source.data
            else {
                unreachable!("live handle owns a live source");
            };
            queue.clear();
            *fraction = 0;
            *writer = None;
            Ok(())
        })
    }

    pub(crate) fn set_speed(&self, permille: u32) -> Result<(), Error> {
        self.stream().set_speed(permille)
    }

    pub(crate) fn set_paused(&self, paused: bool) -> Result<(), Error> {
        self.stream().set_paused(paused)
    }

    pub(crate) fn finish(mut self) -> Result<(), Error> {
        let stream = self.0.take().expect("live stream handle");
        stream.with_source(|source| {
            let Input::Live {
                finished, writer, ..
            } = &mut source.data
            else {
                unreachable!("live handle owns a live source");
            };
            *finished = true;
            *writer = None;
            source.paused = false;
            Ok(())
        })
    }
}

impl Drop for LiveStream {
    fn drop(&mut self) {
        if let Some(stream) = self.0.take() {
            stream.close();
        }
    }
}

pub(super) fn set_available(enabled: bool) {
    let mut mixer = MIXER.lock();
    mixer.enabled = enabled;
    if !enabled {
        let sources = mixer.sources.each_mut().map(Option::take);
        drop(mixer);
        for source in sources.into_iter().flatten() {
            if let Input::Live {
                writer: Some(writer),
                ..
            } = source.data
            {
                writer.wake();
            }
        }
    }
}

/// Mix onto the legacy producer frames already in this block. Only the HDA task calls this.
pub(super) fn mix(block: &mut [Frame]) {
    let mut mixer = MIXER.lock();
    for output in block {
        let [mut left, mut right] = output.map(i32::from);
        for source in mixer.sources.iter_mut().flatten() {
            if let Some([l, r]) = source.next_frame() {
                left += i32::from(l);
                right += i32::from(r);
            }
        }
        *output = [left, right].map(|sample| sample.clamp(i16::MIN.into(), i16::MAX.into()) as i16);
    }
    let mut wake: [Option<Waker>; STREAMS] = [const { None }; STREAMS];
    let mut drained: [Option<Source>; STREAMS] = [const { None }; STREAMS];
    for ((slot, wake), drained) in mixer.sources.iter_mut().zip(&mut wake).zip(&mut drained) {
        if let Some(Source {
            data:
                Input::Live {
                    queue,
                    finished,
                    writer,
                    ..
                },
            ..
        }) = slot
        {
            if queue.len() < QUEUE_FRAMES {
                *wake = writer.take();
            }
            if *finished && queue.is_empty() {
                *drained = slot.take();
            }
        }
    }
    drop(mixer);
    drop(drained);
    for writer in wake.into_iter().flatten() {
        writer.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{future::Future, task::Context};
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        task::Wake,
    };

    fn wave() -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        bytes.extend_from_slice(&16u32.to_le_bytes());
        for word in [1u16, 1] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for dword in [48_000u32, 96_000] {
            bytes.extend_from_slice(&dword.to_le_bytes());
        }
        for word in [2u16, 16] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(b"JUNK\x01\0\0\0x\0data\x06\0\0\0");
        for sample in [100i16, 200, 300] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let len = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&len.to_le_bytes());
        bytes
    }

    #[test]
    fn wav_uses_the_original_pcm_and_validates_chunk_bounds() {
        let bytes = Arc::new(wave());
        let clip = Clip::from_wav(bytes.clone()).unwrap();
        assert!(Arc::ptr_eq(&clip.bytes, &bytes));
        assert_eq!(clip.frames(), 3);
        assert_eq!(clip.frame(2), [300; 2]);
        let mut truncated = wave();
        truncated.pop();
        assert!(matches!(
            Clip::from_wav(Arc::new(truncated)),
            Err(Error::InvalidWave)
        ));
    }

    #[derive(Default)]
    struct WakeFlag(AtomicBool);

    impl Wake for WakeFlag {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    fn render<const N: usize>() -> [Frame; N] {
        let mut block = [[0; 2]; N];
        mix(&mut block);
        block
    }

    #[test]
    fn playback_controls_and_live_backpressure_share_a_mixer() {
        set_available(true);
        let clip = Clip::from_wav(Arc::new(wave())).unwrap();
        let a = FileStream::open(clip.clone()).unwrap();
        let b = FileStream::open(clip).unwrap();
        b.seek(2).unwrap();
        let mut live = LiveStream::open(OUTPUT_RATE).unwrap();
        live.try_write(&[[5, -5]]).unwrap();
        assert_eq!(render::<2>(), [[405, 395], [200, 200]]);
        assert_eq!(a.position(), Ok(2));
        assert_eq!(b.position(), Ok(3));
        assert_eq!(b.seek(4), Err(Error::InvalidPosition));
        a.set_paused(true).unwrap();
        assert_eq!(render::<1>(), [[0; 2]]);
        a.seek(0).unwrap();
        a.set_speed(500).unwrap();
        a.set_paused(false).unwrap();
        assert_eq!(render::<3>(), [[100; 2], [150; 2], [200; 2]]);
        drop((a, b));

        let input = [[30_000, -30_000]; QUEUE_FRAMES];
        assert_eq!(live.try_write(&input), Ok(QUEUE_FRAMES));
        assert_eq!(live.try_write(&[[0; 2]]), Ok(0));
        let mut other = LiveStream::open(OUTPUT_RATE).unwrap();
        other.try_write(&[[10_000, -10_000]]).unwrap();
        let flag = Arc::new(WakeFlag::default());
        let waker = Waker::from(flag.clone());
        let mut cx = Context::from_waker(&waker);
        {
            let mut wait = core::pin::pin!(live.write(&[[1; 2]]));
            assert_eq!(wait.as_mut().poll(&mut cx), Poll::Pending);
            assert_eq!(render::<1>(), [[i16::MAX, i16::MIN]]);
            assert!(flag.0.load(Ordering::Relaxed));
            assert_eq!(wait.as_mut().poll(&mut cx), Poll::Ready(Ok(1)));
        }
        live.flush().unwrap();
        assert_eq!(render::<1>(), [[0; 2]]);
        live.set_speed(500).unwrap();
        live.set_paused(true).unwrap();
        live.try_write(&[[17; 2]]).unwrap();
        assert_eq!(render::<1>(), [[0; 2]]);
        live.finish().unwrap();
        assert_eq!(render::<3>(), [[17; 2], [17; 2], [0; 2]]);
        drop(other);
        set_available(false);
    }
}
