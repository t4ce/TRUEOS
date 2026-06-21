use alloc::boxed::Box;
use alloc::vec::Vec;

use bitstream::BitRead;
use codebook::Codebook;
use error::{Error, ErrorKind, ExpectEof, Result};
use util::{Bits, Push, Pusher2d, Pusher2dStep};

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub enum ResidueKind {
    Residue0,
    Residue1,
    Residue2,
}

impl ResidueKind {
    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(ResidueKind::Residue0),
            1 => Some(ResidueKind::Residue1),
            2 => Some(ResidueKind::Residue2),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct Residue {
    kind: ResidueKind,
    start: usize,
    end: usize,
    part_len: usize,
    classbook: usize,
    class_codebooks: Box<[[Option<usize>; 8]]>,
}

impl Residue {
    pub fn read<R: BitRead>(reader: &mut R, codebook_count: usize) -> Result<Self> {
        let kind = if let Some(kind) = ResidueKind::from_u16(try!(reader.read_u16())) {
            kind
        } else {
            return Err(Error::Undecodable("Unsupported residue type"));
        };
        let start = try!(reader.read_u32_bits(24)) as usize;
        let end = try!(reader.read_u32_bits(24)) as usize;
        if end < start {
            return Err(Error::Undecodable("Invalid residue range"));
        }

        let part_len = try!(reader.read_u32_bits(24)) as usize + 1;
        let class_count = try!(reader.read_u8_bits(6)) as usize + 1;
        let classbook = try!(reader.read_u8_bits(8)) as usize;
        if classbook >= codebook_count {
            return Err(Error::Undecodable("Invalid codebook index in residue classbook"));
        }

        let mut cascade = Vec::with_capacity(class_count);
        for _ in 0..class_count {
            let low_bits = try!(reader.read_u8_bits(3));
            let has_high_bits = try!(reader.read_bool());
            let high_bits = if has_high_bits {
                try!(reader.read_u8_bits(5))
            } else {
                0
            };
            cascade.push(high_bits << 3 | low_bits);
        }

        let mut class_codebooks = Vec::with_capacity(class_count);
        for c in &cascade {
            let mut book_set = [None; 8];
            for bit in 0..8 {
                if c.is_bit_set(bit) {
                    let codebook_idx = try!(reader.read_u8()) as usize;
                    if codebook_idx >= codebook_count {
                        return Err(Error::Undecodable("Invalid codebook index in residue"));
                    }
                    book_set[bit] = Some(codebook_idx);
                }
            }
            class_codebooks.push(book_set);
        }

        // TODO The presence of codebook in array [residue_books] without a value mapping (maptype equals zero) renders the stream undecodable.

        Ok(Residue {
            kind: kind,
            start: start,
            end: end,
            part_len: part_len,
            classbook: classbook,
            class_codebooks: class_codebooks.into_boxed_slice(),
        })
    }

    pub fn decode<R: BitRead>(&self,
            reader: &mut R,
            result: &mut [Box<[f32]>],
            len: usize,
            channels: &[usize],
            zero_channels: &[bool],
            codebooks: &[Codebook]) -> Result<()> {
        match self.do_decode(reader, result, len, channels, zero_channels, codebooks).expect_eof() {
            Err(ref e) if e.kind() == ErrorKind::ExpectedEof => Ok(()),
            r @ _ => r,
        }
    }

    fn do_decode<R: BitRead>(&self,
            reader: &mut R,
            result: &mut [Box<[f32]>],
            len: usize,
            channels: &[usize],
            zero_channels: &[bool],
            codebooks: &[Codebook]) -> Result<()> {
        let n_to_read = self.end - self.start;

        for &c in channels {
            for r in &mut result[c][..len] {
                *r = 0.0;
            }
        }

        if n_to_read == 0 {
            return Ok(());
        }

        let all_channels_zero = !zero_channels.iter().any(|c| !c);
        if all_channels_zero {
            return Ok(());
        }

        let codebook = &codebooks[self.classbook];
        let classwords_per_codeword = codebook.dim_count as usize;
        let parts_to_read = n_to_read / self.part_len;

        let is_residue2 = self.kind == ResidueKind::Residue2;

        let mut classes = Vec::with_capacity(channels.len());
        for _ in 0..channels.len() {
            classes.push(vec![0; classwords_per_codeword + parts_to_read - 1]);
        }

        for pass in 0..8 {
            let mut part_count = 0;
            let (pusher_pos, pusher_step) = match self.kind {
                ResidueKind::Residue0 => unimplemented!(),
                ResidueKind::Residue1 => ((0, 0),
                                          Pusher2dStep::RightDown(0, 1)),
                ResidueKind::Residue2 => ((self.start % channels.len(), self.start / channels.len()),
                                          Pusher2dStep::DownRight(1, 1)),
            };
            let mut pusher = Pusher2d::new(&mut result[..], channels, pusher_pos, pusher_step,
                    |r, v| *r += v);
            'outer: while part_count < parts_to_read {
                if pass == 0 {
                    for (i, &c) in channels.iter().enumerate() {
                        if !is_residue2 && zero_channels[c] {
                            continue;
                        }
                        let mut temp = try!(codebook.decode_scalar(reader)) as usize;
                        for cw in (0..classwords_per_codeword).rev() {
                            classes[i][cw + part_count] =
                                temp % self.class_codebooks.len();
                            temp /= self.class_codebooks.len();
                        }
                        if is_residue2 {
                            // In Residue2 all channel partitions share a single classword.
                            break;
                        }
                    }
                }

                for _ in 0..classwords_per_codeword {
                    for (i, &c) in channels.iter().enumerate() {
                        if !is_residue2 && zero_channels[c] {
                            continue;
                        }
                        let vq_class = classes[i][part_count];
                        let vq_book = self.class_codebooks[vq_class][pass];
                        if let Some(vq_book) = vq_book {
                            let codebook = &codebooks[vq_book];
                            match self.kind {
                                ResidueKind::Residue0 => unimplemented!(),
                                ResidueKind::Residue1 =>
                                    pusher.set_pos((c, self.start + part_count * self.part_len)),
                                ResidueKind::Residue2 => {},
                            }
                            try!(self.codebook_decode(&mut pusher, reader, codebook));
                        } else {
                            pusher.advance_flat_pos(self.part_len);
                        }
                        if is_residue2 {
                            // In Residue2 all channels are in a single partition.
                            break;
                        }
                    }
                    part_count += 1;
                    if part_count >= parts_to_read {
                        break 'outer;
                    }
                }
            }
        }

        Ok(())
    }

    fn codebook_decode<P: Push<f32>, R: BitRead>(&self, result: &mut P, reader: &mut R, codebook: &Codebook) -> Result<()> {
        assert!(self.part_len % codebook.dim_count == 0);
        for _ in 0..self.part_len / codebook.dim_count {
            try!(codebook.decode_vq(reader, result));
        }
        Ok(())
    }
}
