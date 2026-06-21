use alloc::boxed::Box;
use alloc::vec::Vec;

use bitstream::BitRead;
use error::{Error, Result};
use util::Bits;

#[derive(Debug)]
pub struct Mapping {
    channel_couplings: Box<[ChannelCoupling]>,
    /// Channel index -> submap index in self.submaps.
    pub channel_to_submap: Box<[usize]>,
    pub submaps: Box<[Submap]>,
}

#[derive(Debug)]
struct ChannelCoupling {
    mag_channel: usize,
    ang_channel: usize,
}

#[derive(Debug)]
pub struct Submap {
    pub channels: Box<[usize]>,
    pub floor: usize,
    pub residue: usize,
}

impl Mapping {
    pub fn read<R: BitRead>(reader: &mut R, channel_count: usize, floor_count: usize, residue_count: usize) -> Result<Self> {
        assert!(channel_count > 0 && channel_count <= 255);

        if try!(reader.read_u16()) != 0 {
            return Err(Error::Undecodable("Unsupported mapping type"));
        }

        let submap_count = if try!(reader.read_bool()) {
            try!(reader.read_u8_bits(4)) as usize
        } else {
            1
        };
        let has_channel_couplings = try!(reader.read_bool());
        let channel_couplings = if has_channel_couplings {
            let len = try!(reader.read_u8()) as usize + 1;
            let mut channel_couplings = Vec::with_capacity(len);
            let channel_index_bits = Bits::ilog(channel_count as u32 - 1) as usize;
            for _ in 0..channel_couplings.capacity() {
                let mag_channel = try!(reader.read_u8_bits(channel_index_bits)) as usize;
                let ang_channel = try!(reader.read_u8_bits(channel_index_bits)) as usize;
                if mag_channel == ang_channel ||
                        mag_channel >= channel_count ||
                        ang_channel >= channel_count {
                    return Err(Error::Undecodable("Invalid values of (magnitude, angle) channel pair"));
                }
                channel_couplings.push(ChannelCoupling {
                    mag_channel: mag_channel,
                    ang_channel: ang_channel,
                });
            }
            channel_couplings
        } else {
            Vec::new()
        };

        // Reserved.
        if try!(reader.read_u8_bits(2)) != 0 {
            return Err(Error::Undecodable("Unexpected data in reserved field"));
        }

        let channel_to_submap = if submap_count > 1 {
            let mut channel_to_submap = Vec::with_capacity(channel_count);
            for _ in 0..channel_count {
                let submap_idx = try!(reader.read_u8_bits(4)) as usize;
                if submap_idx >= submap_count {
                    return Err(Error::Undecodable("Invalid mapping mux value"));
                }
                channel_to_submap.push(submap_idx)
            }
            channel_to_submap
        } else {
            // This is missing from the specs.
            vec![0; channel_count]
        };

        let mut submaps = Vec::with_capacity(submap_count);
        for submap_idx in 0..submap_count {
            // Unused.
            try!(reader.read_u8());

            let floor = try!(reader.read_u8()) as usize;
            if floor >= floor_count {
                return Err(Error::Undecodable("Invalid mapping floor value"));
            }

            let residue = try!(reader.read_u8()) as usize;
            if residue >= residue_count {
                return Err(Error::Undecodable("Invalid mapping residue value"));
            }

            let channels: Vec<_> = channel_to_submap.iter().enumerate()
                    .filter_map(|(i, &v)| if v == submap_idx {
                        Some(i)
                    } else {
                        None
                    })
                    .collect();

            submaps.push(Submap {
                floor: floor,
                residue: residue,
                channels: channels.into_boxed_slice(),
            });
        }

        Ok(Mapping {
            channel_couplings: channel_couplings.into_boxed_slice(),
            channel_to_submap: channel_to_submap.into_boxed_slice(),
            submaps: submaps.into_boxed_slice(),
        })
    }

    pub fn unzero_coupled_channels(&self, zero_channels: &mut [bool]) {
        for c in self.channel_couplings.iter() {
            let m = c.mag_channel;
            let a = c.ang_channel;
            if !zero_channels[m] || !zero_channels[a] {
                zero_channels[m] = false;
                zero_channels[a] = false;
            }
        }
    }

    pub fn decouple_channels(&self, channels: &mut [Box<[f32]>], channel_len: usize) {
        for c in self.channel_couplings.iter() {
            for i in 0..channel_len {
                let m = channels[c.mag_channel][i];
                let a = channels[c.ang_channel][i];
                let (new_m, new_a) = if m > 0.0 {
                    if a > 0.0 {
                        (m, m - a)
                    } else {
                        (m + a, m)
                    }
                } else if a > 0.0 {
                    (m, m + a)
                } else {
                    (m - a, m)
                };
                channels[c.mag_channel][i] = new_m;
                channels[c.ang_channel][i] = new_a;
            }
        }
    }
}
