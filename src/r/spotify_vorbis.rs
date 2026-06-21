extern crate alloc;

use alloc::vec::Vec;

use vorbis::{BitReader, Decoder};

#[derive(Clone, Debug)]
pub struct PreparedVorbisDecoderInput {
    ident: Vec<u8>,
    comment: Vec<u8>,
    setup: Vec<u8>,
    audio_packets: Vec<Vec<u8>>,
    source_channels: usize,
    source_sample_rate: u32,
    sink_channels: usize,
    sink_sample_rate: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareVorbisError {
    MissingIdent,
    MissingComment,
    MissingSetup,
    MissingAudio,
    InvalidFormat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcmAdapterError {
    UnsupportedChannels,
    BadFrameAlignment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VorbisDecoderError {
    Headers,
    Packet,
    Pcm(PcmAdapterError),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PcmAdapterStats {
    pub source_frames: usize,
    pub sink_frames: usize,
    pub clipped_samples: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VorbisDecodeStats {
    pub packets_seen: usize,
    pub packets_decoded: usize,
    pub empty_packets: usize,
    pub source_frames: usize,
    pub sink_frames: usize,
    pub clipped_samples: usize,
    pub pcm_samples: usize,
}

impl VorbisDecodeStats {
    pub fn add(&mut self, other: VorbisDecodeStats) {
        self.packets_seen = self.packets_seen.saturating_add(other.packets_seen);
        self.packets_decoded = self.packets_decoded.saturating_add(other.packets_decoded);
        self.empty_packets = self.empty_packets.saturating_add(other.empty_packets);
        self.source_frames = self.source_frames.saturating_add(other.source_frames);
        self.sink_frames = self.sink_frames.saturating_add(other.sink_frames);
        self.clipped_samples = self.clipped_samples.saturating_add(other.clipped_samples);
        self.pcm_samples = self.pcm_samples.saturating_add(other.pcm_samples);
    }
}

impl PreparedVorbisDecoderInput {
    pub fn new(
        ident: &[u8],
        comment: &[u8],
        setup: &[u8],
        audio_packets: &[Vec<u8>],
        source_channels: usize,
        source_sample_rate: u32,
        sink_channels: usize,
        sink_sample_rate: u32,
    ) -> Result<Self, PrepareVorbisError> {
        if ident.is_empty() {
            return Err(PrepareVorbisError::MissingIdent);
        }
        if comment.is_empty() {
            return Err(PrepareVorbisError::MissingComment);
        }
        if setup.is_empty() {
            return Err(PrepareVorbisError::MissingSetup);
        }
        if audio_packets.is_empty() {
            return Err(PrepareVorbisError::MissingAudio);
        }
        if source_channels == 0
            || source_sample_rate == 0
            || sink_channels != 2
            || sink_sample_rate == 0
        {
            return Err(PrepareVorbisError::InvalidFormat);
        }

        Ok(Self {
            ident: ident.to_vec(),
            comment: comment.to_vec(),
            setup: setup.to_vec(),
            audio_packets: audio_packets.to_vec(),
            source_channels,
            source_sample_rate,
            sink_channels,
            sink_sample_rate,
        })
    }

    pub fn ident_len(&self) -> usize {
        self.ident.len()
    }

    pub fn comment_len(&self) -> usize {
        self.comment.len()
    }

    pub fn setup_len(&self) -> usize {
        self.setup.len()
    }

    pub fn audio_packet_count(&self) -> usize {
        self.audio_packets.len()
    }

    pub fn audio_bytes(&self) -> usize {
        self.audio_packets
            .iter()
            .fold(0usize, |sum, packet| sum.saturating_add(packet.len()))
    }

    pub fn ident(&self) -> &[u8] {
        self.ident.as_slice()
    }

    pub fn comment(&self) -> &[u8] {
        self.comment.as_slice()
    }

    pub fn setup(&self) -> &[u8] {
        self.setup.as_slice()
    }

    pub fn audio_packets(&self) -> &[Vec<u8>] {
        self.audio_packets.as_slice()
    }

    pub fn source_channels(&self) -> usize {
        self.source_channels
    }

    pub fn source_sample_rate(&self) -> u32 {
        self.source_sample_rate
    }

    pub fn sink_channels(&self) -> usize {
        self.sink_channels
    }

    pub fn sink_sample_rate(&self) -> u32 {
        self.sink_sample_rate
    }

    pub fn needs_rate_match(&self) -> bool {
        self.source_sample_rate != self.sink_sample_rate
    }

    pub fn needs_channel_map(&self) -> bool {
        self.source_channels != self.sink_channels
    }
}

pub struct VorbisPacketDecoder {
    decoder: Decoder,
    pcm: PcmSinkAdapter,
    packet_f32: Vec<f32>,
}

impl VorbisPacketDecoder {
    pub fn new(input: &PreparedVorbisDecoderInput) -> Result<Self, VorbisDecoderError> {
        let mut builder = Decoder::builder();
        builder
            .read_ident_packet(&mut BitReader::new(input.ident()))
            .map_err(|_| VorbisDecoderError::Headers)?;
        builder
            .read_comment_packet(&mut BitReader::new(input.comment()))
            .map_err(|_| VorbisDecoderError::Headers)?;
        builder
            .read_setup_packet(&mut BitReader::new(input.setup()))
            .map_err(|_| VorbisDecoderError::Headers)?;

        let decoder = builder.build();
        let pcm = PcmSinkAdapter::new(
            input.source_channels(),
            input.source_sample_rate(),
            input.sink_sample_rate(),
        )
        .map_err(VorbisDecoderError::Pcm)?;

        Ok(Self {
            decoder,
            pcm,
            packet_f32: Vec::with_capacity(8192),
        })
    }

    pub fn decode_captured_to_i16(
        &mut self,
        input: &PreparedVorbisDecoderInput,
        out: &mut Vec<i16>,
    ) -> Result<VorbisDecodeStats, VorbisDecoderError> {
        let mut stats = VorbisDecodeStats::default();
        for packet in input.audio_packets() {
            stats.add(self.decode_packet_to_i16(packet.as_slice(), out)?);
        }
        Ok(stats)
    }

    pub fn decode_packet_to_i16(
        &mut self,
        packet: &[u8],
        out: &mut Vec<i16>,
    ) -> Result<VorbisDecodeStats, VorbisDecoderError> {
        let start_len = out.len();
        let mut stats = VorbisDecodeStats {
            packets_seen: 1,
            ..VorbisDecodeStats::default()
        };
        let samples = self
            .decoder
            .decode(&mut BitReader::new(packet))
            .map_err(|_| VorbisDecoderError::Packet)?;
        if samples.is_empty() {
            stats.empty_packets = 1;
            return Ok(stats);
        }

        self.packet_f32.clear();
        self.packet_f32.extend(samples.interleave());
        let packet_stats = self
            .pcm
            .convert_packet(self.packet_f32.as_slice(), out)
            .map_err(VorbisDecoderError::Pcm)?;
        stats.packets_decoded = 1;
        stats.source_frames = packet_stats.source_frames;
        stats.sink_frames = packet_stats.sink_frames;
        stats.clipped_samples = packet_stats.clipped_samples;
        stats.pcm_samples = out.len().saturating_sub(start_len);
        Ok(stats)
    }
}

#[derive(Clone, Debug)]
pub struct PcmSinkAdapter {
    source_channels: usize,
    source_sample_rate: u32,
    sink_sample_rate: u32,
    source_frame_phase: u64,
}

impl PcmSinkAdapter {
    pub fn new(
        source_channels: usize,
        source_sample_rate: u32,
        sink_sample_rate: u32,
    ) -> Result<Self, PcmAdapterError> {
        if source_channels == 0 || source_channels > 2 {
            return Err(PcmAdapterError::UnsupportedChannels);
        }
        Ok(Self {
            source_channels,
            source_sample_rate: source_sample_rate.max(1),
            sink_sample_rate: sink_sample_rate.max(1),
            source_frame_phase: 0,
        })
    }

    pub fn convert_packet(
        &mut self,
        interleaved_f32: &[f32],
        out: &mut Vec<i16>,
    ) -> Result<PcmAdapterStats, PcmAdapterError> {
        if interleaved_f32.len() % self.source_channels != 0 {
            return Err(PcmAdapterError::BadFrameAlignment);
        }
        let source_frames = interleaved_f32.len() / self.source_channels;
        if source_frames == 0 {
            return Ok(PcmAdapterStats::default());
        }

        let start_len = out.len();
        let mut clipped_samples = 0usize;
        while (self.source_frame_phase / self.sink_sample_rate as u64) < source_frames as u64 {
            let source_frame = (self.source_frame_phase / self.sink_sample_rate as u64) as usize;
            let base = source_frame * self.source_channels;
            let (left, right) = if self.source_channels == 1 {
                let sample = interleaved_f32[base];
                (sample, sample)
            } else {
                (interleaved_f32[base], interleaved_f32[base + 1])
            };
            let (left, left_clipped) = f32_to_i16(left);
            let (right, right_clipped) = f32_to_i16(right);
            clipped_samples = clipped_samples
                .saturating_add(left_clipped as usize)
                .saturating_add(right_clipped as usize);
            out.push(left);
            out.push(right);
            self.source_frame_phase = self
                .source_frame_phase
                .saturating_add(self.source_sample_rate as u64);
        }
        let consumed_phase = (source_frames as u64).saturating_mul(self.sink_sample_rate as u64);
        self.source_frame_phase = self.source_frame_phase.saturating_sub(consumed_phase);

        Ok(PcmAdapterStats {
            source_frames,
            sink_frames: (out.len().saturating_sub(start_len)) / 2,
            clipped_samples,
        })
    }
}

fn f32_to_i16(sample: f32) -> (i16, bool) {
    let clipped = !(-1.0..=1.0).contains(&sample);
    let sample = sample.clamp(-1.0, 1.0);
    let scaled = sample * 32767.0;
    let rounded = if scaled >= 0.0 {
        scaled + 0.5
    } else {
        scaled - 0.5
    };
    (rounded as i16, clipped)
}
