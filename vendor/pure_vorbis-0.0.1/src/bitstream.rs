use core::cmp;

use error::{Error, Result};
use util::Bits;

/// A bit reader for Vorbis packet payloads.
pub trait BitRead {
    fn try_read_u32_bits(&mut self, len_bits: usize) -> Result<(u32, usize)>;

    fn read_u32_bits(&mut self, len_bits: usize) -> Result<u32> {
        let (r, r_len) = try!(self.try_read_u32_bits(len_bits));
        if r_len == len_bits {
            Ok(r)
        } else {
            Err(Error::Undecodable("Unexpected EOF"))
        }
    }

    fn unread_u32_bits(&mut self, bits: u32, len_bits: usize);

    fn read_exact(&mut self, out: &mut [u8]) -> Result<()> {
        for byte in out {
            *byte = try!(self.read_u8());
        }
        Ok(())
    }

    fn read_u8_bits(&mut self, len_bits: usize) -> Result<u8> {
        assert!(len_bits <= 8);
        self.read_u32_bits(len_bits).map(|v| v as u8)
    }

    fn read_u8(&mut self) -> Result<u8> {
        self.read_u8_bits(8)
    }

    fn read_u16_bits(&mut self, len_bits: usize) -> Result<u16> {
        assert!(len_bits <= 16);
        self.read_u32_bits(len_bits).map(|v| v as u16)
    }

    fn read_u16(&mut self) -> Result<u16> {
        self.read_u16_bits(16)
    }

    fn read_i32_bits(&mut self, len_bits: usize) -> Result<i32> {
        assert!(len_bits >= 2);
        let u = try!(self.read_u32_bits(len_bits - 1));
        let sign = try!(self.read_bool());
        if sign {
            Ok(-(u as i32))
        } else {
            Ok(u as i32)
        }
    }

    fn read_u32(&mut self) -> Result<u32> {
        self.read_u32_bits(32)
    }

    fn read_i32(&mut self) -> Result<i32> {
        self.read_i32_bits(32)
    }

    fn read_bool(&mut self) -> Result<bool> {
        self.read_u8_bits(1).map(|v| v & 1 == 1)
    }

    fn read_f32(&mut self) -> Result<f32> {
        self.read_u32().map(|v| f32_unpack(v))
    }
}

pub struct BitReader<'a> {
    data: &'a [u8],
    offset: usize,
    bit_buf: u64,
    bit_buf_left: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BitReader {
            data,
            offset: 0,
            bit_buf: 0,
            bit_buf_left: 0,
        }
    }

    fn fill_bit_buf(&mut self) -> Result<()> {
        assert_eq!(self.bit_buf_left, 0);
        let remaining = self.data.len().saturating_sub(self.offset);
        let read = remaining.min(4);
        self.bit_buf_left = read * 8;

        if read == 0 {
            return Ok(());
        }

        let mut bit_buf = self.data[self.offset] as u64;
        if read > 1 {
            bit_buf |= (self.data[self.offset + 1] as u64) << 8;
        }
        if read > 2 {
            bit_buf |= (self.data[self.offset + 2] as u64) << 16;
        }
        if read > 3 {
            bit_buf |= (self.data[self.offset + 3] as u64) << 24;
        }
        self.offset += read;
        self.bit_buf = bit_buf;

        Ok(())
    }

    fn read_bit_buf(&mut self, target: &mut u32, offset: usize, len: usize) -> usize {
        assert!(offset + len <= 32);
        if len == 0 || self.bit_buf_left == 0 {
            return 0;
        }
        let can_read = cmp::min(self.bit_buf_left, len);
        let bits = (self.bit_buf as u32).ls_bits(can_read);
        *target = if offset == 0 {
            bits
        } else {
            target.ls_bits(offset) | (bits << offset)
        };
        if can_read == self.bit_buf_left {
            self.bit_buf = 0;
            self.bit_buf_left = 0;
        } else {
            self.bit_buf >>= can_read;
            self.bit_buf_left -= can_read;
        }
        can_read
    }
}

impl<'a> BitRead for BitReader<'a> {
    fn try_read_u32_bits(&mut self, len_bits: usize) -> Result<(u32, usize)> {
        if len_bits == 0 {
            return Ok((0, 0));
        }
        assert!(len_bits <= 32);
        if self.bit_buf_left == 0 {
            try!(self.fill_bit_buf());
        }
        let mut r = 0;
        let mut read_bits = self.read_bit_buf(&mut r, 0, len_bits);
        if read_bits != 0 && read_bits < len_bits && self.bit_buf_left == 0 {
            try!(self.fill_bit_buf());
            read_bits += self.read_bit_buf(&mut r, read_bits, len_bits - read_bits);
        }
        Ok((r, read_bits))
    }

    fn unread_u32_bits(&mut self, bits: u32, len_bits: usize) {
        if len_bits == 0 {
            return;
        }
        assert!(self.bit_buf_left + len_bits <= 64);
        self.bit_buf = (self.bit_buf << len_bits) | bits.ls_bits(len_bits) as u64;
        self.bit_buf_left += len_bits;
    }
}

fn f32_unpack(val: u32) -> f32 {
    let mut mantissa = (val & 0x1F_FFFF) as f32;
    let sign = val & 0x8000_0000;
    if sign != 0 {
        mantissa = -mantissa;
    }
    let exponent = ((val & 0x7FE0_0000) >> 21) as f32;
    mantissa * libm::powf(2_f32, exponent - 788_f32)
}
