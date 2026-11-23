//! Wi-Fi microphone presence and on-demand capture of stereo RTP L16.

use alloc::{collections::VecDeque, vec::Vec};
use spin::Mutex;
use v::vnet::EndpointV4;

pub(crate) const CAPTURE_SAMPLE_RATE: usize = super::SAMPLE_RATE;
const RTP_HEADER_BYTES: usize = 12;
const RTP_PAYLOAD_TYPE: u8 = 96;
const STREAM_TIMEOUT_MS: u64 = 2_000;
const REORDER_WAIT_MS: u64 = 60;
const REORDER_PACKETS: i16 = 16;
const BUFFER_FRAMES: usize = 2 * CAPTURE_SAMPLE_RATE;
const FRAME_BYTES: usize = super::FRAME_BYTES;

type Frame = [i16; 2];

static RECEIVER: Mutex<Receiver> = Mutex::new(Receiver {
    stream: None,
    capture: None,
});

/// A single recording holds this guard. Idle receivers retain no audio.
pub(crate) struct Capture(());

pub(crate) fn start_capture() -> Result<Capture, &'static str> {
    RECEIVER.lock().start_capture(super::monotonic_millis())?;
    Ok(Capture(()))
}

impl Capture {
    pub(crate) fn drain(&self, out: &mut [[i16; 2]]) -> usize {
        let mut receiver = RECEIVER.lock();
        let now_ms = super::monotonic_millis();
        receiver.expire_stream(now_ms);
        receiver.capture.as_mut().map_or(0, |capture| {
            capture.flush_reorder(now_ms, false);
            capture.drain(out)
        })
    }

    pub(crate) fn input_error(&self) -> Option<&'static str> {
        let mut receiver = RECEIVER.lock();
        receiver.expire_stream(super::monotonic_millis());
        receiver.capture.as_ref().and_then(|capture| capture.error)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let capture = RECEIVER.lock().capture.take();
        drop(capture);
    }
}

pub(crate) fn status() -> Option<EndpointV4> {
    let mut receiver = RECEIVER.lock();
    receiver.expire_stream(super::monotonic_millis());
    receiver.stream.map(|stream| stream.peer)
}

pub(super) fn receive(peer: EndpointV4, bytes: &[u8]) {
    let Some(packet) = Packet::parse(bytes) else {
        return;
    };
    let connected = RECEIVER
        .lock()
        .receive(peer, &packet, super::monotonic_millis());
    if connected {
        let [a, b, c, d] = peer.addr;
        crate::log_info!(target: "net";
            "audio-http: ESP32 Wi-Fi microphone connected from {}.{}.{}.{}:{} (RTP SSRC {:08x})\n",
            a, b, c, d, peer.port, packet.ssrc
        );
    }
}

struct Packet<'a> {
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    pcm: &'a [u8],
}

impl<'a> Packet<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        let header = bytes.get(..RTP_HEADER_BYTES)?;
        if header[0] >> 6 != 2 || header[1] & 0x7f != RTP_PAYLOAD_TYPE {
            return None;
        }

        let csrc_bytes = usize::from(header[0] & 0x0f) * 4;
        let mut pcm = bytes.get(RTP_HEADER_BYTES + csrc_bytes..)?;
        if header[0] & 0x10 != 0 {
            let extension_words = u16::from_be_bytes([*pcm.get(2)?, *pcm.get(3)?]);
            pcm = pcm.get(4 + usize::from(extension_words) * 4..)?;
        }
        if header[0] & 0x20 != 0 {
            let padding = usize::from(*pcm.last()?);
            if padding == 0 {
                return None;
            }
            pcm = &pcm[..pcm.len().checked_sub(padding)?];
        }
        if pcm.is_empty() || !pcm.len().is_multiple_of(FRAME_BYTES) {
            return None;
        }

        Some(Self {
            sequence: u16::from_be_bytes([header[2], header[3]]),
            timestamp: u32::from_be_bytes([header[4], header[5], header[6], header[7]]),
            ssrc: u32::from_be_bytes([header[8], header[9], header[10], header[11]]),
            pcm,
        })
    }

    fn frames(&self) -> impl ExactSizeIterator<Item = Frame> + '_ {
        self.pcm.chunks_exact(FRAME_BYTES).map(|frame| {
            [
                i16::from_be_bytes([frame[0], frame[1]]),
                i16::from_be_bytes([frame[2], frame[3]]),
            ]
        })
    }
}

#[derive(Clone, Copy)]
struct Stream {
    peer: EndpointV4,
    ssrc: u32,
    sequence: u16,
    last_advance_ms: u64,
}

struct Receiver {
    stream: Option<Stream>,
    capture: Option<CaptureBuffer>,
}

impl Receiver {
    fn start_capture(&mut self, now_ms: u64) -> Result<(), &'static str> {
        self.expire_stream(now_ms);
        if self.capture.is_some() {
            return Err("Wi-Fi microphone is already recording");
        }
        let mut pcm = VecDeque::new();
        let mut reorder = Vec::new();
        pcm.try_reserve_exact(BUFFER_FRAMES)
            .map_err(|_| "Wi-Fi microphone recording buffer allocation failed")?;
        reorder
            .try_reserve_exact(REORDER_PACKETS as usize)
            .map_err(|_| "Wi-Fi microphone reorder buffer allocation failed")?;
        self.capture = Some(CaptureBuffer {
            pcm,
            reorder,
            next_packet: None,
            error: None,
        });
        Ok(())
    }

    fn expire_stream(&mut self, now_ms: u64) {
        if self.stream.is_some_and(|stream| {
            now_ms.saturating_sub(stream.last_advance_ms) >= STREAM_TIMEOUT_MS
        }) {
            self.stream = None;
            if let Some(capture) = self.capture.as_mut() {
                capture.error.get_or_insert("Wi-Fi microphone disconnected");
            }
        }
    }

    /// Keep one sender until its sequence stops progressing. This also avoids
    /// mixing or repeatedly logging the same multicast stream across interfaces.
    fn receive(&mut self, peer: EndpointV4, packet: &Packet<'_>, now_ms: u64) -> bool {
        self.expire_stream(now_ms);
        let connected = self.stream.is_none();
        if let Some(stream) = self.stream.as_mut() {
            if stream.peer != peer || stream.ssrc != packet.ssrc {
                return false;
            }
            let advance = sequence_delta(packet.sequence, stream.sequence);
            if advance == 0 || advance < -REORDER_PACKETS {
                return false;
            }
            if advance > 0 {
                stream.sequence = packet.sequence;
                stream.last_advance_ms = now_ms;
            }
        } else {
            self.stream = Some(Stream {
                peer,
                ssrc: packet.ssrc,
                sequence: packet.sequence,
                last_advance_ms: now_ms,
            });
        }
        if let Some(capture) = self.capture.as_mut() {
            capture.receive(packet, now_ms);
        }
        connected
    }
}

fn sequence_delta(sequence: u16, reference: u16) -> i16 {
    sequence.wrapping_sub(reference) as i16
}

struct Pending {
    sequence: u16,
    timestamp: u32,
    arrived_ms: u64,
    frames: Vec<Frame>,
}

struct CaptureBuffer {
    pcm: VecDeque<Frame>,
    reorder: Vec<Pending>,
    next_packet: Option<(u16, u32)>,
    error: Option<&'static str>,
}

impl CaptureBuffer {
    fn receive(&mut self, packet: &Packet<'_>, now_ms: u64) {
        self.flush_reorder(now_ms, false);
        if self.error.is_some() {
            return;
        }
        if let Some((next, _)) = self.next_packet {
            let ahead = sequence_delta(packet.sequence, next);
            if ahead < 0 {
                return;
            }
            if (1..=REORDER_PACKETS).contains(&ahead) {
                self.queue_reorder(packet, next, ahead, now_ms);
                return;
            }
            if ahead > REORDER_PACKETS {
                self.flush_reorder(now_ms, true);
            }
        }
        self.append(packet.sequence, packet.timestamp, packet.frames());
        self.flush_reorder(now_ms, false);
    }

    fn queue_reorder(&mut self, packet: &Packet<'_>, next: u16, ahead: i16, now_ms: u64) {
        let Err(index) = self
            .reorder
            .binary_search_by_key(&ahead, |pending| sequence_delta(pending.sequence, next))
        else {
            return;
        };
        let mut frames = Vec::new();
        let incoming = packet.frames();
        if frames.try_reserve_exact(incoming.len()).is_err() {
            self.error = Some("Wi-Fi microphone reorder buffer allocation failed");
            return;
        }
        frames.extend(incoming);
        self.reorder.insert(
            index,
            Pending {
                sequence: packet.sequence,
                timestamp: packet.timestamp,
                arrived_ms: now_ms,
                frames,
            },
        );
    }

    fn flush_reorder(&mut self, now_ms: u64, force: bool) {
        while self.error.is_none() {
            let (Some(packet), Some((next, _))) = (self.reorder.first(), self.next_packet) else {
                break;
            };
            if !force
                && packet.sequence != next
                && now_ms.saturating_sub(packet.arrived_ms) < REORDER_WAIT_MS
            {
                break;
            }
            let packet = self.reorder.remove(0);
            self.append(packet.sequence, packet.timestamp, packet.frames.into_iter());
        }
    }

    fn append(
        &mut self,
        sequence: u16,
        timestamp: u32,
        frames: impl ExactSizeIterator<Item = Frame>,
    ) {
        if self.error.is_some() {
            return;
        }
        let expected = self.next_packet.map_or(timestamp, |(_, stamp)| stamp);
        let offset = timestamp.wrapping_sub(expected) as i32;
        // RTP ticks count stereo frames. Preserve genuine packet gaps without
        // letting an unrelated clock jump enqueue an arbitrary silence span.
        if offset.unsigned_abs() as usize > BUFFER_FRAMES {
            self.error = Some("Wi-Fi microphone RTP timestamp discontinuity");
            return;
        }
        let silence = offset.max(0) as usize;
        let skip = (offset.saturating_neg().max(0) as usize).min(frames.len());
        let count = frames.len() - skip;
        if silence + count > BUFFER_FRAMES - self.pcm.len() {
            self.error = Some("Wi-Fi microphone recording buffer overrun");
            return;
        }
        self.pcm.resize(self.pcm.len() + silence, [0, 0]);
        self.pcm.extend(frames.skip(skip));
        self.next_packet = Some((
            sequence.wrapping_add(1),
            expected.wrapping_add((silence + count) as u32),
        ));
    }

    fn drain(&mut self, out: &mut [Frame]) -> usize {
        let count = out.len().min(self.pcm.len());
        let (first, second) = self.pcm.as_slices();
        let left = count.min(first.len());
        out[..left].copy_from_slice(&first[..left]);
        out[left..count].copy_from_slice(&second[..count - left]);
        self.pcm.drain(..count);
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER: EndpointV4 = EndpointV4::new([192, 0, 2, 5], 5004);

    fn packet_bytes(sequence: u16, timestamp: u32, ssrc: u32, frame: Frame) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0x80, RTP_PAYLOAD_TYPE]);
        bytes.extend_from_slice(&sequence.to_be_bytes());
        bytes.extend_from_slice(&timestamp.to_be_bytes());
        bytes.extend_from_slice(&ssrc.to_be_bytes());
        bytes.extend_from_slice(&frame[0].to_be_bytes());
        bytes.extend_from_slice(&frame[1].to_be_bytes());
        bytes
    }

    fn feed(receiver: &mut Receiver, sequence: u16, timestamp: u32, now_ms: u64) -> bool {
        let sample = sequence as i16;
        let bytes = packet_bytes(sequence, timestamp, 45, [sample, !sample]);
        receiver.receive(PEER, &Packet::parse(&bytes).unwrap(), now_ms)
    }

    fn receiver() -> Receiver {
        Receiver {
            stream: None,
            capture: None,
        }
    }

    #[test]
    fn decodes_signed_l16_and_validates_rtp_optional_headers() {
        let mut bytes = packet_bytes(4, 9, 42, [i16::MIN, 0x1234]);
        bytes[0] = 0xb1; // RTP v2 with padding, extension, and one CSRC.
        bytes[1] |= 0x80; // Marker is independent of the payload type.
        bytes.splice(12..12, [0, 0, 0, 1, 0xbe, 0xde, 0, 1, 7, 6, 5, 4]);
        bytes.extend_from_slice(&[0, 0, 0, 4]);
        let packet = Packet::parse(&bytes).unwrap();
        assert_eq!((packet.sequence, packet.timestamp, packet.ssrc), (4, 9, 42));
        assert_eq!(packet.frames().collect::<Vec<_>>(), [[i16::MIN, 0x1234]]);

        assert!(Packet::parse(&bytes[..20]).is_none());
        let last = bytes.len() - 1;
        bytes[last] = 0;
        assert!(Packet::parse(&bytes).is_none());
        bytes[last] = 5;
        assert!(Packet::parse(&bytes).is_none());
        bytes[last] = 4;
        bytes[1] = RTP_PAYLOAD_TYPE + 1;
        assert!(Packet::parse(&bytes).is_none());
    }

    #[test]
    fn tracks_one_stream_and_leaves_idle_audio_unbuffered() {
        let mut receiver = receiver();
        assert!(feed(&mut receiver, 5, 100, 0));
        assert!(!feed(&mut receiver, 6, 101, 10));
        assert!(receiver.capture.is_none());
        receiver.start_capture(15).unwrap();
        assert!(receiver.start_capture(15).is_err());

        let bytes = packet_bytes(7, 102, 46, [100, 200]);
        assert!(!receiver.receive(PEER, &Packet::parse(&bytes).unwrap(), 20));
        let bytes = packet_bytes(7, 102, 45, [100, 200]);
        let other = EndpointV4::new([192, 0, 2, 6], 5004);
        assert!(!receiver.receive(other, &Packet::parse(&bytes).unwrap(), 20));
        assert!(!feed(&mut receiver, 6, 101, 10 + STREAM_TIMEOUT_MS - 1));
        assert!(receiver.capture.as_ref().unwrap().pcm.is_empty());

        assert!(feed(&mut receiver, 7, 102, 10 + STREAM_TIMEOUT_MS));
        let capture = receiver.capture.as_ref().unwrap();
        assert_eq!(capture.error, Some("Wi-Fi microphone disconnected"));
        assert!(capture.pcm.is_empty());
    }

    #[test]
    fn reorders_sequences_across_wrap_and_conceals_expired_gaps() {
        let mut receiver = receiver();
        receiver.start_capture(0).unwrap();
        assert!(feed(&mut receiver, 65534, u32::MAX - 1, 0));
        assert!(!feed(&mut receiver, 0, 0, 1));
        assert!(!feed(&mut receiver, 0, 0, 2));
        assert!(!feed(&mut receiver, 65535, u32::MAX, 3));
        assert!(!feed(&mut receiver, 2, 2, 4));

        let capture = receiver.capture.as_mut().unwrap();
        capture.flush_reorder(4 + REORDER_WAIT_MS - 1, false);
        let mut output = [[0; 2]; 8];
        assert_eq!(capture.drain(&mut output), 3);
        assert_eq!(output[..3], [[-2, 1], [-1, 0], [0, -1]]);
        capture.flush_reorder(4 + REORDER_WAIT_MS, false);
        assert_eq!(capture.drain(&mut output), 2);
        assert_eq!(output[..2], [[0, 0], [2, -3]]);
        assert!(!feed(&mut receiver, 1, 1, 4 + REORDER_WAIT_MS + 1));
        assert!(receiver.capture.as_ref().unwrap().pcm.is_empty());
    }

    #[test]
    fn detects_buffer_overrun_and_oversized_timestamp_jumps() {
        let mut receiver = receiver();
        receiver.start_capture(0).unwrap();
        assert!(feed(&mut receiver, 1, 0, 0));
        assert!(!feed(&mut receiver, 2, BUFFER_FRAMES as u32 + 1, 1));
        let capture = receiver.capture.as_ref().unwrap();
        assert_eq!(capture.pcm.len(), 1);
        assert_eq!(
            capture.error,
            Some("Wi-Fi microphone recording buffer overrun")
        );

        receiver.capture = None;
        receiver.start_capture(2).unwrap();
        assert!(!feed(&mut receiver, 3, 0, 2));
        assert!(!feed(&mut receiver, 4, BUFFER_FRAMES as u32 * 2, 3));
        let capture = receiver.capture.as_ref().unwrap();
        assert_eq!(capture.pcm.len(), 1);
        assert_eq!(
            capture.error,
            Some("Wi-Fi microphone RTP timestamp discontinuity")
        );
    }
}
