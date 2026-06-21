use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cmp;

use bitstream::BitRead;
use error::{Error, Result};
use util::{self, Bits};

#[derive(Debug)]
pub struct HuffmanDecoder {
    lookup_table: LookupTable,
    long_codes: Box<[LongCode]>,
    max_code_len: usize,
}

impl HuffmanDecoder {
    pub fn builder(lookup_table_bits: usize) -> HuffmanDecoderBuilder {
        assert!(lookup_table_bits > 0 && lookup_table_bits < 32);
        let lookup_table_len = if lookup_table_bits == 0 {
            0
        } else {
            1 << lookup_table_bits
        };
        let lookup_entries = vec![LookupEntry::Null; lookup_table_len];

        let long_codes = Vec::new();

        HuffmanDecoderBuilder {
            lookup_table: LookupTable {
                entries: lookup_entries.into_boxed_slice(),
                len_bits: lookup_table_bits,
            },
            long_codes: long_codes,
            cur_codes: [None; 31],
            max_code_len: 0,
        }
    }

    pub fn decode<R: BitRead>(&self, reader: &mut R) -> Result<u32> {
        let lookup_len_bits = cmp::min(self.max_code_len, self.lookup_table.len_bits);
        let (mut code_bits, mut read) = try!(reader.try_read_u32_bits(lookup_len_bits));
        if read == 0 {
            return Err(Error::Undecodable("Unexpected EOF while reading Huffman code"));
        }
        let entry = &self.lookup_table.entries[code_bits as usize];
        let code = match entry {
            &LookupEntry::Code(code) => code,
            &LookupEntry::LongCode => {
                let r = try!(reader.try_read_u32_bits(self.max_code_len - lookup_len_bits));
                read += r.1;
                if read == 0 {
                    return Err(Error::Undecodable("Incomplete Huffman code"));
                }
                code_bits |= r.0 << lookup_len_bits;

                try!(self.find_long_code(code_bits, read))
            },
            &LookupEntry::Null => return Err(Error::Undecodable("Matched a null Huffman code entry")),
        };
        if code.len < read {
            let unread_len = read - code.len;
            let unread_bits = code_bits >> code.len;
            reader.unread_u32_bits(unread_bits, unread_len);
        } else if code.len > read {
            return Err(Error::Undecodable("Incomplete Huffman code"));
        }
        Ok(code.value)
    }

    fn find_long_code(&self, bits: u32, len: usize) -> Result<CodeValue> {
        // TODO: Use binary search here.
        self.long_codes.iter()
            .filter(|lc| lc.len <= len &&
                    lc.code.ls_bits(lc.len) == bits.ls_bits(lc.len))
            .next()
            .map(|lc| CodeValue {
                value: lc.value,
                len: lc.len,
            })
            .ok_or_else(|| Error::Undecodable("Incomplete or unknown Huffman code"))
    }
}

pub struct HuffmanDecoderBuilder {
    lookup_table: LookupTable,
    long_codes: Vec<LongCode>,
    /// Current lowest codes for each code length (length 1 is at index 0).
    cur_codes: [Option<u32>; 31],
    max_code_len: usize,
}

impl HuffmanDecoderBuilder {
    pub fn create_code(&mut self, value: u32, len: usize) -> Result<()> {
        let code_straight = try!(self.next_code(len));
        let code = code_straight.reverse_bits() >> (32 - len);
        let code = Code { code: code, len: len };
        let value = CodeValue {
            value: value,
            len: len,
        };

        let is_long_code = if !self.lookup_table.is_empty() && len > 0 {
            let lookup_table_len = self.lookup_table.len_bits;
            let (entry, is_long_code) = if len <= lookup_table_len {
                (LookupEntry::Code(value), false)
            } else {
                (LookupEntry::LongCode, true)
            };
            self.lookup_table.set(code.truncate(lookup_table_len), entry);
            is_long_code
        } else {
            true
        };

        if is_long_code {
            let lc = LongCode {
                sort_key: code_straight,
                code: code.code,
                value: value.value,
                len: len,
            };
            self.long_codes.push(lc);
        }

        Ok(())
    }

    pub fn build(mut self) -> HuffmanDecoder {
        for lc in self.long_codes.iter_mut() {
            lc.pad_sort_key(self.max_code_len);
        }
        self.long_codes.sort_by_key(|lc| lc.sort_key);

        HuffmanDecoder {
            lookup_table: self.lookup_table,
            long_codes: self.long_codes.into_boxed_slice(),
            max_code_len: self.max_code_len,
        }
    }

    fn next_code(&mut self, len: usize) -> Result<u32> {
        let r = try!(self.do_next_code(len));
        if len > self.max_code_len {
            self.max_code_len = len;
        }
        Ok(r)
    }

    fn do_next_code(&mut self, len: usize) -> Result<u32> {
        assert!(len > 0 && len < 32);

        let idx = len - 1;

        if self.cur_codes[idx].is_none() {
            let r = if idx > 0 {
                try!(self.do_next_code(idx)) << 1
            } else {
                0
            };
            self.cur_codes[idx] = Some(r);
            return Ok(r);
        }

        let cur_code_bits = self.cur_codes[idx].unwrap();
        if cur_code_bits & 1 == 0 {
            let cur_code_bits = cur_code_bits | 1;
            self.cur_codes[idx] = Some(cur_code_bits);
            return Ok(cur_code_bits);
        }

        if len == 1 {
            return Err(Error::Undecodable("Overspecified Huffman tree"));
        }
        let cur_code_bits = try!(self.do_next_code(idx)) << 1;
        self.cur_codes[idx] = Some(cur_code_bits);
        Ok(cur_code_bits)
    }
}

#[derive(Clone, Copy, Debug)]
struct Code {
    code: u32,
    len: usize,
}

impl Code {
    pub fn truncate(&self, len: usize) -> Self {
        if self.len <= len {
            *self
        } else {
            Code {
                code: self.code.ls_bits(len),
                len: len,
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CodeValue {
    value: u32,
    len: usize,
}

#[derive(Clone, Copy, Debug)]
struct LongCode {
    sort_key: u32,
    code: u32,
    value: u32,
    len: usize,
}

impl LongCode {
    pub fn pad_sort_key(&mut self, len: usize) {
        assert!(len >= self.len && len <= 32);
        self.sort_key <<= len - self.len;
    }
}

#[derive(Debug)]
struct LookupTable {
    entries: Box<[LookupEntry]>,
    len_bits: usize,
}

impl LookupTable {
    pub fn is_empty(&self) -> bool {
        self.len_bits == 0
    }

    pub fn set(&mut self, code: Code, entry: LookupEntry) {
        assert!(code.len <= self.len_bits);
        let mut index = code.code as usize;
        let last_index = ((self.entries.len() - 1) & !util::lsb_mask(code.len) as usize) | index;
        let step = 1 << code.len;
        loop {
            assert!(match self.entries[index] {
                LookupEntry::Null | LookupEntry::LongCode => true,
                _ => false,
            });
            self.entries[index] = entry;
            if index == last_index {
                break;
            }
            index += step;
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum LookupEntry {
    Null,
    Code(CodeValue),
    LongCode,
}

#[cfg(test)]
mod tests {
    use std::cmp;
    use std::io::Cursor;

    use super::*;
    use bitstream::BitReader;
    use error::ErrorKind;

    fn new_bit_reader(bits: &str) -> BitReader<Cursor<Vec<u8>>> {
        let mut buf = Vec::new();
        let mut byte = 0;
        let mut bit_pos = 0;
        for c in bits.chars() {
            match c {
                '0' => {},
                '1' => byte |= 1 << bit_pos,
                _   => continue,
            }
            if bit_pos == 7 {
                buf.push(byte);
                byte = 0;
                bit_pos = 0;
            } else {
                bit_pos += 1;
            }
        }
        if bit_pos != 0 {
            buf.push(byte);
        }
        BitReader::new(Cursor::new(buf))
    }

    fn test_next_code(check_underspec: bool, input: &[usize], expected: &[u32]) {
        assert!(!input.is_empty());
        assert_eq!(input.len(), expected.len());
        let mut b = HuffmanDecoder::builder(1);
        for (&inp, &exp) in input.iter().zip(expected.iter()) {
            let act = b.next_code(inp).unwrap();

            /*let code_str = format!("{:032b}", act);
            println!("{:2} {}", inp, &code_str[code_str.len() - inp as usize..]);

            println!("cur_codes:");
            for (i, &c) in b.cur_codes.iter().enumerate() {
                if let Some(c) = c {
                    println!("  {:2} {:b}", i + 1, c);
                }
            }*/

            assert_eq!(act, exp);
        }
        assert_eq!(b.max_code_len, *input.iter().max().unwrap());
        if check_underspec {
            for i in 1..32 {
                let c = b.next_code(i);
                if c.is_ok() {
                    println!("Underspecified: {} -> {:b}", i, c.as_ref().unwrap());
                }
                assert_eq!(c.err().unwrap().kind(), ErrorKind::Undecodable);
            }
        }
    }

    #[test]
    fn next_code_1() {
        test_next_code(true,
                       &[2, 4, 4, 4, 4, 2, 3, 3],
                       &[0b00, 0b0100, 0b0101, 0b0110, 0b0111, 0b10, 0b110, 0b111]);
    }

    #[test]
    fn next_code_2() {
        test_next_code(true,
                       &[3,       1,      2,      3],
                       &[0b000,   0b1,    0b01,   0b001]);
    }

    #[test]
    fn next_code_3() {
        test_next_code(false,
                       &[10, 7, 8, 13, 9, 6, 7, 11, 10, 8, 8, 12, 17, 17, 17, 17, 7, 5, 5, 9, 6, 4, 4, 8, 8, 5, 5, 8, 16, 14, 13, 16, 7, 5, 5, 7, 6, 3, 3, 5, 8, 5],
                       &[0b0000000000, 0b0000001, 0b00000001, 0b0000000001000, 0b000000001, 0b000001, 0b0000100, 0b00000000011, 0b0000101000, 0b00001011, 0b00001100, 0b000000000101, 0b00000000010010000, 0b00000000010010001, 0b00000000010010010, 0b00000000010010011, 0b0000111, 0b00010, 0b00011, 0b000010101, 0b001000, 0b0011, 0b0100, 0b00001101, 0b00100100, 0b00101, 0b01010, 0b00100101, 0b0000000001001010, 0b00000000010011, 0b0000101001000, 0b0000000001001011, 0b0010011, 0b01011, 0b01100, 0b0110100, 0b011011, 0b100, 0b101, 0b01110, 0b01101010, 0b01111]);
    }

    #[test]
    fn overspecified() {
        let mut b = HuffmanDecoder::builder(1);
        b.next_code(1).unwrap();
        b.next_code(1).unwrap();
        assert_eq!(b.next_code(1).err().unwrap().kind(), ErrorKind::Undecodable);
    }

    fn test_decode(code_lens: &[usize], input: &str, expected: &[u32]) {
        let max_code_len = *code_lens.iter().max().unwrap();
        // Without long codes.
        test_decode_(max_code_len, code_lens, input, expected);

        // With long codes.
        if max_code_len > 1 {
            test_decode_(cmp::max(max_code_len as isize - 4, 1) as usize, code_lens, input, expected);
        }
    }

    fn test_decode_(lookup_table_bits: usize, code_lens: &[usize], input: &str, expected: &[u32]) {
        let mut b = HuffmanDecoder::builder(lookup_table_bits);
        for (i, &code_len) in code_lens.iter().enumerate() {
            b.create_code(i as u32, code_len).unwrap();
        }
        let d = b.build();

        let mut reader = new_bit_reader(input);

        for exp in expected {
            assert_eq!(d.decode(&mut reader).unwrap(), *exp);
        }
    }

    #[test]
    fn decode_1() {
        /*
            0 2 codeword 00
            1 4 codeword 0100
            2 4 codeword 0101
            3 4 codeword 0110
            4 4 codeword 0111
            5 2 codeword 10
            6 3 codeword 110
            7 3 codeword 111 */
        test_decode(&[2, 4, 4, 4, 4, 2, 3, 3],
                     "00 111 0111 0110 110 110 111",
                    &[0, 7,  4,   3,   6,  6,  7]);
    }

    #[test]
    fn decode_2() {
        test_decode(&[10, 7, 8, 13, 9, 6, 7, 11, 10, 8, 8, 12, 17, 17, 17, 17, 7, 5, 5, 9, 6, 4, 4, 8, 8, 5, 5, 8, 16, 14, 13, 16, 7, 5, 5, 7, 6, 3, 3, 5, 8, 5],
                     "001000 0000000001001011 100 000001 0000000000 01111 00010 unused: 011011",
                    &[20,    31,              37, 5,     0,         41,   17]);
    }
}
