use alloc::boxed::Box;
use alloc::vec::Vec;
use core::mem;

use bitstream::BitRead;
use codebook::Codebook;
use error::{Error, Result};
use floor::Floor;
use header::{Comments, FrameKind, Header};
use mapping::Mapping;
use mdct::Mdct;
use mode::Mode;
use residue::Residue;
use util::Bits;
use window::{OverlapTarget, Window, WindowRange, Windows};

const MAGIC_LEN: usize = 6;
const MAGIC: &'static [u8] = b"vorbis";

/// Low-level Vorbis decoder.
///
/// Decodes Vorbis audio packets into audio samples. Note the decoder works directly with
/// Vorbis packet data extracted from container (like Ogg).
///
/// # Example
/// See [module reference](index.html).
pub struct Decoder {
    header: Header,
    comments: Option<Comments>,
    setup: Setup,
    windows: Windows,
    mdct: [Mdct; 2],

    floor_y_list: Box<[Vec<(u16, bool)>]>,
    prev_frame: Box<[Box<[f32]>]>,
    prev_frame_kind: Option<FrameKind>,
    frame: Box<[Box<[f32]>]>,
    frame_kind: Option<FrameKind>,
    pos: u64,
}

impl Decoder {
    pub fn builder() -> DecoderBuilder {
        DecoderBuilder {
            header: None,
            comments: None,
            setup: None,
        }
    }

    /// Decodes an audio packet. Note if this is the first audio packet (either for a newly initialized
    /// decoder instance or after a call to `reset()`) the returned samples will
    /// be empty.
    pub fn decode<R: BitRead>(&mut self, reader: &mut R) -> Result<Samples> {
        self.swap_frames();

        let packet_kind = try!(reader.read_u8_bits(1));
        if packet_kind != PacketKind::Audio as u8 {
            return Err(Error::WrongPacketKind("Expected audio packet"));
        }
        let mode_count = self.setup.modes.len();
        let mode_idx =
            try!(reader.read_u8_bits(Bits::ilog(mode_count as u8) as usize - 1)) as usize;
        if mode_idx >= mode_count {
            return Err(Error::Undecodable("Invalid packet mode number"));
        }
        let mode = &self.setup.modes[mode_idx];

        if mode.frame_kind == FrameKind::Long {
            /* let is_prev_long_frame = */ try!(reader.read_bool());
            /* let is_next_long_frame = */ try!(reader.read_bool());
        }

        let frame_lens = self.header.frame_lens();
        let frame_len = frame_lens.get(mode.frame_kind);
        let frame_half_len = frame_len / 2;

        let mapping = &self.setup.mappings[mode.mapping as usize];

        // Begin decoding floors.
        for (channel, floor_y_list) in self.floor_y_list.iter_mut().enumerate() {
            let submap_idx = mapping.channel_to_submap[channel];
            let floor_idx = mapping.submaps[submap_idx].floor;
            let floor = &self.setup.floors[floor_idx];
            try!(floor.begin_decode(floor_y_list, reader, &self.setup.codebooks));
        }

        // Decode residues.
        {
            let mut zero_channels: Vec<_> = self.floor_y_list.iter().map(|f| f.is_empty()).collect();

            mapping.unzero_coupled_channels(&mut zero_channels);

            for submap in mapping.submaps.iter() {
                let residue_idx = submap.residue;
                let residue = &self.setup.residues[residue_idx];
                try!(residue.decode(reader,
                            &mut self.frame,
                            frame_half_len,
                            &submap.channels,
                            &zero_channels,
                            &self.setup.codebooks));
            }
        }

        mapping.decouple_channels(&mut self.frame, frame_half_len);

        // Finish decoding floors (synthesize and perform dot product with residues).
        for ((channel, result), floor_y_list) in self.frame.iter_mut().enumerate()
                                                        .zip(self.floor_y_list.iter()) {
            if !floor_y_list.is_empty() {
                let submap_idx = mapping.channel_to_submap[channel];
                let floor_idx = mapping.submaps[submap_idx].floor;
                let floor = &self.setup.floors[floor_idx];
                floor.finish_decode(result, floor_y_list);
            } else {
                for r in result[..frame_half_len].as_mut().iter_mut() {
                    *r = 0.0;
                }
            }
        }

        for channel in self.frame.iter_mut() {
            self.mdct[mode.frame_kind as usize].inverse(&mut channel[..frame_len]);
        }

        if let Some(prev_frame_kind) = self.prev_frame_kind {
            let window = self.windows.get(prev_frame_kind, mode.frame_kind);
            for (mut l, mut r) in self.prev_frame.iter_mut().zip(self.frame.iter_mut()) {
                window.overlap(&mut l, &mut r);
            }
            self.pos += window.len() as u64;
        }

        self.frame_kind = Some(mode.frame_kind);

        Ok(self.samples())
    }

    // Resets this decoder's state as it would be after a newly initialized decoder instance.
    pub fn reset(&mut self) {
        self.prev_frame_kind = None;
        self.frame_kind = None;
        self.pos = 0;
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn comments(&self) -> Option<&Comments> {
        self.comments.as_ref()
    }

    pub fn samples(&self) -> Samples {
        self.window().map(|w| match w.overlap_target {
            OverlapTarget::Left => Samples { frame: &self.prev_frame, range: w.left },
            OverlapTarget::Right => Samples { frame: &self.frame, range: w.right },
        }).unwrap_or_else(|| Samples { frame: &self.frame, range: WindowRange { start: 0, end: 0 } })
    }

    // Returns sample position - the number of sample this decoder produced so far.
    pub fn pos(&self) -> u64 {
        self.pos
    }

    fn window(&self) -> Option<&Window> {
        if let (Some(prev_frame_kind), Some(frame_kind)) = (self.prev_frame_kind, self.frame_kind) {
            Some(self.windows.get(prev_frame_kind, frame_kind))
        } else {
            None
        }
    }

    fn swap_frames(&mut self) {
        if self.frame_kind.is_some() {
            mem::swap(&mut self.frame, &mut self.prev_frame);
            self.prev_frame_kind = self.frame_kind;
            self.frame_kind = None;
        }
    }
}

/// Contains decoded sample data for all channels returned by the [Decoder::decode()] method.
/// [Decoder::decode()]: struct.Decoder.html#method.decode
pub struct Samples<'a> {
    frame: &'a [Box<[f32]>],
    range: WindowRange,
}

impl<'a> Samples<'a> {
    /// Returns the number of samples each channel has.
    pub fn len(&self) -> usize {
        self.range.len()
    }

    /// Returns `true` if the `len() == 0`.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns iterator over the samples in all channels interleaved.
    pub fn interleave(&self) -> InterleavedSamplesIter<'a> {
        InterleavedSamplesIter {
            frame: self.frame,
            range: self.range,
            pos: (0, self.range.start),
        }
    }

    /// Returns the number of channels. This is the same as `Header::channel_count()`.
    pub fn channel_count(&self) -> usize {
        self.frame.len()
    }

    /// Returns iterator over the samples for each channel in order.
    pub fn channels(&self) -> ChannelIter<'a> {
        ChannelIter {
            frame_iter: self.frame.iter(),
            range: self.range,
        }
    }

    // Returns samples slice for the specified zero-based channel index.
    pub fn channel(&self, index: usize) -> &[f32] {
        &self.frame[index][self.range.start..self.range.end]
    }
}

pub struct ChannelIter<'a> {
    frame_iter: core::slice::Iter<'a, Box<[f32]>>,
    range: WindowRange,
}

impl<'a> Iterator for ChannelIter<'a> {
    type Item = &'a [f32];

    fn next(&mut self) -> Option<Self::Item> {
        self.frame_iter.next().map(|c| &c[self.range.start..self.range.end])
    }
}

pub struct InterleavedSamplesIter<'a> {
    frame: &'a [Box<[f32]>],
    range: WindowRange,
    pos: (usize, usize),
}

impl<'a> Iterator for InterleavedSamplesIter<'a> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos.1 == self.range.end {
            return None;
        }
        let r = self.frame[self.pos.0][self.pos.1];
        self.pos.0 += 1;
        if self.pos.0 >= self.frame.len() {
            self.pos.0 = 0;
            self.pos.1 += 1;
        }
        Some(r)
    }
}

pub struct DecoderBuilder {
    header: Option<Header>,
    comments: Option<Comments>,
    setup: Option<Setup>,
}

impl DecoderBuilder {
    pub fn read_ident_packet<R: BitRead>(&mut self, reader: &mut R) -> Result<()> {
        self.header = Some(try!(PacketKind::Ident.read(reader, |r| Header::read(r))));
        Ok(())
    }

    pub fn read_comment_packet<R: BitRead>(&mut self, reader: &mut R) -> Result<()> {
        self.comments = Some(try!(PacketKind::Comment.read(reader, |r| Comments::read(r))));
        Ok(())
    }

    pub fn read_setup_packet<R: BitRead>(&mut self, reader: &mut R) -> Result<()> {
        let header = self.header.as_ref()
                .expect("You need to call read_ident_packet() before read_setup_packet()");
        self.setup = Some(try!(PacketKind::Setup.read(reader, |r| Setup::read(r, header))));
        Ok(())
    }

    pub fn build(mut self) -> Decoder {
        assert!(self.setup.is_some(),
            "You need to call read_ident_packet() and read_setup_packet() first");
        let header = self.header.take().unwrap();
        let setup = self.setup.take().unwrap();

        let max_floor_len = setup.floors.iter().max_by_key(|f| f.x_list.len()).unwrap().x_list.len();

        let windows = Windows::new(header.frame_lens());

        let mdct = [Mdct::new(header.frame_lens().short()),
                    Mdct::new(header.frame_lens().long())];

        let mut floor_y_list = Vec::with_capacity(header.channel_count());
        let mut prev_frame = Vec::with_capacity(header.channel_count());
        let mut frame = Vec::with_capacity(header.channel_count());
        for _ in 0..header.channel_count() {
            floor_y_list.push(Vec::with_capacity(max_floor_len));
            prev_frame.push(vec![0_f32; header.frame_lens().long()].into_boxed_slice());
            frame.push(vec![0_f32; header.frame_lens().long()].into_boxed_slice());
        }

        Decoder {
            header: header,
            comments: self.comments,
            setup: setup,
            windows: windows,
            mdct: mdct,

            floor_y_list: floor_y_list.into_boxed_slice(),
            prev_frame: prev_frame.into_boxed_slice(),
            prev_frame_kind: None,
            frame: frame.into_boxed_slice(),
            frame_kind: None,
            pos: 0,
        }
    }

    pub fn header(&self) -> Option<&Header> {
        self.header.as_ref()
    }

    pub fn comments(&self) -> Option<&Comments> {
        self.comments.as_ref()
    }
}

struct Setup {
    codebooks: Box<[Codebook]>,
    floors: Box<[Floor]>,
    residues: Box<[Residue]>,
    mappings: Box<[Mapping]>,
    modes: Box<[Mode]>,
}

impl Setup {
    fn read<R: BitRead>(reader: &mut R, header: &Header) -> Result<Self> {
        let codebooks = try!(Self::read_codebooks(reader));

        try!(Self::skip_time_domain_trans(reader));

        let floors = try!(Self::read_floors(reader, codebooks.len()));

        let residues = try!(Self::read_residues(reader, codebooks.len()));

        let mappings = try!(Self::read_mappings(reader, header.channel_count(),
                                                floors.len(), residues.len()));

        let modes = try!(Self::read_modes(reader, mappings.len()));

        Ok(Setup {
            codebooks: codebooks.into_boxed_slice(),
            floors: floors.into_boxed_slice(),
            residues: residues.into_boxed_slice(),
            mappings: mappings.into_boxed_slice(),
            modes: modes.into_boxed_slice(),
        })
    }

    fn read_codebooks<R: BitRead>(reader: &mut R) -> Result<Vec<Codebook>> {
        let count = try!(reader.read_u8()) as usize + 1;
        let mut r = Vec::with_capacity(count);
        for _ in 0..count {
            let mut codebook = try!(Codebook::read(reader));
            codebook.idx = r.len();
            r.push(codebook);
        }
        Ok(r)
    }

    fn skip_time_domain_trans<R: BitRead>(reader: &mut R) -> Result<()> {
        let len = try!(reader.read_u8_bits(6)) as usize + 1;
        for _ in 0..len {
            let value = try!(reader.read_u32_bits(16));
            if value != 0 {
                return Err(Error::Undecodable("Non-zero value in time domain transforms"));
            }
        }
        Ok(())
    }

    fn read_floors<R: BitRead>(reader: &mut R, codebook_count: usize) -> Result<Vec<Floor>> {
        let count = try!(reader.read_u8_bits(6)) as usize + 1;
        let mut floors = Vec::with_capacity(count);
        for _ in 0..count {
            let floor = try!(Floor::read(reader, codebook_count));
            floors.push(floor);
        }
        Ok(floors)
    }

    fn read_residues<R: BitRead>(reader: &mut R, codebook_count: usize) -> Result<Vec<Residue>> {
        let count = try!(reader.read_u8_bits(6)) as usize + 1;
        let mut residues = Vec::with_capacity(count);
        for _ in 0..count {
            let residue = try!(Residue::read(reader, codebook_count));
            residues.push(residue);
        }
        Ok(residues)
    }

    fn read_mappings<R: BitRead>(reader: &mut R, channel_count: usize,
            floor_count: usize, residue_count: usize) -> Result<Vec<Mapping>> {
        let count = try!(reader.read_u8_bits(6)) as usize + 1;
        let mut mappings = Vec::with_capacity(count);
        for _ in 0..count {
            let mapping = try!(Mapping::read(reader, channel_count, floor_count, residue_count));
            mappings.push(mapping);
        }
        Ok(mappings)
    }

    fn read_modes<R: BitRead>(reader: &mut R, mapping_count: usize) -> Result<Vec<Mode>> {
        let count = try!(reader.read_u8_bits(6)) as usize + 1;
        let mut modes = Vec::with_capacity(count);
        for _ in 0..count {
            let mode = try!(Mode::read(reader, mapping_count));
            modes.push(mode);
        }
        if !try!(reader.read_bool()) {
            return Err(Error::Undecodable("Invalid framing bit"));
        }
        Ok(modes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PacketKind {
    Audio   = 0,
    Ident   = 1,
    Comment = 3,
    Setup   = 5,
}

impl PacketKind {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(PacketKind::Audio),
            1 => Some(PacketKind::Ident),
            3 => Some(PacketKind::Comment),
            5 => Some(PacketKind::Setup),
            _ => None,
        }
    }
}

impl PacketKind {
    fn read<BR: BitRead, R, F>(self, reader: &mut BR, f: F) -> Result<R>
            where F: FnOnce(&mut BR) -> Result<R> {
        let packet_kind = try!(PacketKind::from_u8(try!(reader.read_u8()))
                    .ok_or(Error::Undecodable("Invalid packet kind")));
        if packet_kind != self {
            return Err(Error::WrongPacketKind("Unexpected packet kind"));
        }

        let mut magic = [0; MAGIC_LEN];
        try!(reader.read_exact(&mut magic));
        if magic != MAGIC {
            return Err(Error::Undecodable("Invalid packet magic value"));
        }

        f(reader)
    }
}
