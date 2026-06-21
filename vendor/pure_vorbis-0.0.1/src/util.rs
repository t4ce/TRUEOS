use alloc::boxed::Box;

pub trait Bits {
    fn ilog(self) -> usize;
    fn is_bit_set(self, offset: usize) -> bool;
    fn reverse_bits(self) -> Self;
    fn ls_bits(self, _len: usize) -> Self
            where Self: core::marker::Sized {
        unimplemented!();
    }
}

impl Bits for u32 {
    #[inline]
    fn ilog(self) -> usize {
        32 - self.leading_zeros() as usize
    }

    #[inline]
    fn is_bit_set(self, offset: usize) -> bool {
        self & (1 << offset) != 0
    }

    #[inline]
    fn reverse_bits(self) -> Self {
        ((((self >> 16) & 0xFFFF) as u16).reverse_bits() as u32) |
        (((self & 0xFFFF) as u16).reverse_bits() as u32) << 16
    }

    #[inline]
    fn ls_bits(self, len: usize) -> Self {
        match len {
            0 => 0,
            32 => self,
            1..=31 => self & lsb_mask(len),
            _ => panic!("Length must be in [0..32] range"),
        }
    }
}

impl Bits for u16 {
    #[inline]
    fn ilog(self) -> usize {
        16 - self.leading_zeros() as usize
    }

    #[inline]
    fn is_bit_set(self, offset: usize) -> bool {
        self & (1 << offset) != 0
    }

    #[inline]
    fn reverse_bits(self) -> Self {
        ((((self >> 8) & 0xFF) as u8).reverse_bits() as u16) |
        (((self & 0xFF) as u8).reverse_bits() as u16) << 8
    }
}

impl Bits for u8 {
    #[inline]
    fn ilog(self) -> usize {
        8 - self.leading_zeros() as usize
    }

    #[inline]
    fn is_bit_set(self, offset: usize) -> bool {
        self & (1 << offset) != 0
    }

    #[inline]
    fn reverse_bits(self) -> Self {
        static REVERSE_BIT_TABLE: [u8; 256] = [
            0x00, 0x80, 0x40, 0xC0, 0x20, 0xA0, 0x60, 0xE0, 0x10, 0x90, 0x50, 0xD0, 0x30, 0xB0, 0x70, 0xF0,
            0x08, 0x88, 0x48, 0xC8, 0x28, 0xA8, 0x68, 0xE8, 0x18, 0x98, 0x58, 0xD8, 0x38, 0xB8, 0x78, 0xF8,
            0x04, 0x84, 0x44, 0xC4, 0x24, 0xA4, 0x64, 0xE4, 0x14, 0x94, 0x54, 0xD4, 0x34, 0xB4, 0x74, 0xF4,
            0x0C, 0x8C, 0x4C, 0xCC, 0x2C, 0xAC, 0x6C, 0xEC, 0x1C, 0x9C, 0x5C, 0xDC, 0x3C, 0xBC, 0x7C, 0xFC,
            0x02, 0x82, 0x42, 0xC2, 0x22, 0xA2, 0x62, 0xE2, 0x12, 0x92, 0x52, 0xD2, 0x32, 0xB2, 0x72, 0xF2,
            0x0A, 0x8A, 0x4A, 0xCA, 0x2A, 0xAA, 0x6A, 0xEA, 0x1A, 0x9A, 0x5A, 0xDA, 0x3A, 0xBA, 0x7A, 0xFA,
            0x06, 0x86, 0x46, 0xC6, 0x26, 0xA6, 0x66, 0xE6, 0x16, 0x96, 0x56, 0xD6, 0x36, 0xB6, 0x76, 0xF6,
            0x0E, 0x8E, 0x4E, 0xCE, 0x2E, 0xAE, 0x6E, 0xEE, 0x1E, 0x9E, 0x5E, 0xDE, 0x3E, 0xBE, 0x7E, 0xFE,
            0x01, 0x81, 0x41, 0xC1, 0x21, 0xA1, 0x61, 0xE1, 0x11, 0x91, 0x51, 0xD1, 0x31, 0xB1, 0x71, 0xF1,
            0x09, 0x89, 0x49, 0xC9, 0x29, 0xA9, 0x69, 0xE9, 0x19, 0x99, 0x59, 0xD9, 0x39, 0xB9, 0x79, 0xF9,
            0x05, 0x85, 0x45, 0xC5, 0x25, 0xA5, 0x65, 0xE5, 0x15, 0x95, 0x55, 0xD5, 0x35, 0xB5, 0x75, 0xF5,
            0x0D, 0x8D, 0x4D, 0xCD, 0x2D, 0xAD, 0x6D, 0xED, 0x1D, 0x9D, 0x5D, 0xDD, 0x3D, 0xBD, 0x7D, 0xFD,
            0x03, 0x83, 0x43, 0xC3, 0x23, 0xA3, 0x63, 0xE3, 0x13, 0x93, 0x53, 0xD3, 0x33, 0xB3, 0x73, 0xF3,
            0x0B, 0x8B, 0x4B, 0xCB, 0x2B, 0xAB, 0x6B, 0xEB, 0x1B, 0x9B, 0x5B, 0xDB, 0x3B, 0xBB, 0x7B, 0xFB,
            0x07, 0x87, 0x47, 0xC7, 0x27, 0xA7, 0x67, 0xE7, 0x17, 0x97, 0x57, 0xD7, 0x37, 0xB7, 0x77, 0xF7,
            0x0F, 0x8F, 0x4F, 0xCF, 0x2F, 0xAF, 0x6F, 0xEF, 0x1F, 0x9F, 0x5F, 0xDF, 0x3F, 0xBF, 0x7F, 0xFF,
        ];
        REVERSE_BIT_TABLE[self as usize]
    }
}

pub trait Push<T> {
    fn push(&mut self, value: T);
}

impl<'a, T: 'a, I> Push<T> for I where I: Iterator<Item=&'a mut T> {
    fn push(&mut self, value: T) {
        let r = self.next().unwrap();
        *r = value;
    }
}

pub enum Pusher2dStep {
    RightDown(usize, usize),
    DownRight(usize, usize),
}

pub struct Pusher2d<'a, T: 'a, F> {
    array2d: &'a mut [Box<[T]>],
    index_map: &'a [usize],
    len: (usize, usize),
    step: Pusher2dStep,
    pos: (usize, usize),
    mutator: F,
}

impl<'a, T, F: FnMut(&mut T, T)> Pusher2d<'a, T, F> {
    pub fn new(
            array2d: &'a mut [Box<[T]>],
            index_map: &'a [usize],
            pos: (usize, usize),
            step: Pusher2dStep,
            mutator: F) -> Self {
        let len = (index_map.len(), array2d[0].len());
        Pusher2d {
            array2d: array2d,
            index_map: index_map,
            len: len,
            pos: pos,
            step: step,
            mutator: mutator,
        }
    }

    pub fn set_pos(&mut self, pos: (usize, usize)) {
        self.pos = pos;
    }

    pub fn advance_flat_pos(&mut self, flat_offset: usize) {
        match self.step {
            Pusher2dStep::RightDown(_, _) => {
                let pos_1 = self.pos.1 + flat_offset;
                self.pos.0 = pos_1 / self.len.1;
                self.pos.1 += pos_1 % self.len.1;
            },
            Pusher2dStep::DownRight(_, _) => {
                let pos_0 = self.pos.0 + flat_offset;
                self.pos.0 = pos_0 % self.len.0;
                self.pos.1 += pos_0 / self.len.0;
            },
        }
    }
}

impl<'a, T, F: FnMut(&mut T, T)> Push<T> for Pusher2d<'a, T, F> {
    fn push(&mut self, value: T) {
        let index = self.index_map[self.pos.0];
        let r = &mut self.array2d[index][self.pos.1];
        (self.mutator)(r, value);
        match self.step {
            Pusher2dStep::RightDown(step_0, step_1) => {
                self.pos.1 += step_1;
                if self.pos.1 >= self.len.1 {
                    self.pos.1 = 0;
                    self.pos.0 += step_0;
                }
            },
            Pusher2dStep::DownRight(step_0, step_1) => {
                self.pos.0 += step_0;
                if self.pos.0 >= self.len.0 {
                    self.pos.0 = 0;
                    self.pos.1 += step_1;
                }
            },
        }
    }
}

#[inline]
pub fn lsb_mask(len: usize) -> u32 {
    0xFFFF_FFFF >> (32 - len)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::mem;

    #[test]
    fn bits_ilog() {
        const TEST_SET_LEN: usize = 6;
        let inp: [u64; TEST_SET_LEN]   = [0, 1, 2, 3, 4, 7];
        let exp: [usize; TEST_SET_LEN] = [0, 1, 2, 2, 3, 3];
        for i in 0..TEST_SET_LEN {
            if mem::size_of::<u8>() >= inp[i] as usize {
                assert_eq!((inp[i] as u8).ilog(), exp[i]);
            }
            if mem::size_of::<u16>() >= inp[i] as usize {
                assert_eq!((inp[i] as u16).ilog(), exp[i]);
            }
            if mem::size_of::<u32>() >= inp[i] as usize {
                assert_eq!((inp[i] as u32).ilog(), exp[i]);
            }
        }
    }

    #[test]
    fn bits_reverse() {
        assert_eq!(0b10111001_u8.reverse_bits(),
                   0b10011101);
        assert_eq!(0b11001011_00011001_u16.reverse_bits(),
                   0b10011000_11010011);
        assert_eq!(0b00110111_11010110_10101100_00000001_u32.reverse_bits(),
                   0b10000000_00110101_01101011_11101100);
    }
}
