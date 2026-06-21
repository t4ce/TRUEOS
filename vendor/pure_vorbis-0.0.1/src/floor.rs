use alloc::boxed::Box;
use alloc::vec::Vec;

use bitstream::BitRead;
use codebook::Codebook;
use error::{Error, ErrorKind, ExpectEof, Result};
use util::Bits;

#[derive(Clone, Copy, Debug)]
pub enum FloorKind {
    Floor0,
    Floor1,
}

impl FloorKind {
    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(FloorKind::Floor0),
            1 => Some(FloorKind::Floor1),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct Floor {
    mult: u8,
    range: u16,
    // [0..15]{1..31}.
    part_classes: Box<[usize]>,
    classes: Box<[Class]>,
    pub x_list: Box<[u16]>,
    sorted_x_list: Box<[(usize, u16)]>,
    neighbors: Box<[(usize, usize)]>,
}

#[derive(Debug)]
struct Class {
    dim_count: usize,
    subclass_bit_count: usize,
    master_book: Option<usize>,
    subclass_books: Box<[Option<usize>]>,
}

impl Floor {
    pub fn read<R: BitRead>(reader: &mut R, codebooks_len: usize) -> Result<Self> {
        match FloorKind::from_u16(try!(reader.read_u16())) {
            Some(FloorKind::Floor0) => return Err(Error::Undecodable("Floor 0 is not supported")),
            Some(FloorKind::Floor1) => {},
            None => return Err(Error::Undecodable("Unsupported floor type")),
        }

        let part_count = try!(reader.read_u32_bits(5)) as usize;
        if part_count == 0 {
            return Err(Error::Undecodable("Invalid floor partition count"));
        }
        let mut part_classes = Vec::with_capacity(part_count);
        let mut max_class = -1;
        for _ in 0..part_count {
            let part_class = try!(reader.read_u8_bits(4));
            if part_class as i8 > max_class {
                max_class = part_class as i8;
            }
            part_classes.push(part_class as usize);
        }

        let class_count = max_class as usize + 1;
        let mut classes = Vec::with_capacity(max_class as usize + 1);
        for _ in 0..class_count {
            let dim_count = try!(reader.read_u8_bits(3)) as usize + 1;

            let subclass_bit_count = try!(reader.read_u8_bits(2)) as usize;
            let master_book = if subclass_bit_count != 0 {
                let master_book = try!(reader.read_u8()) as usize;
                if master_book >= codebooks_len {
                    return Err(Error::Undecodable("Invalid codebook index in floor class master book"));
                }
                Some(master_book)
            } else {
                None
            };

            let subclass_books_count = 1 << subclass_bit_count;
            let mut subclass_books = Vec::with_capacity(subclass_books_count);
            for _ in 0..subclass_books_count {
                let subclass_book = match try!(reader.read_u8()) as usize {
                    0 => None,
                    classbook_idx => {
                        let classbook_idx = classbook_idx - 1;
                        if classbook_idx >= codebooks_len {
                            return Err(Error::Undecodable(
                                "Invalid codebook index in floor subclass books"));
                        }
                        Some(classbook_idx)
                    },
                };
                subclass_books.push(subclass_book);
            }

            classes.push(Class {
                dim_count: dim_count,
                subclass_bit_count: subclass_bit_count,
                master_book: master_book,
                subclass_books: subclass_books.into_boxed_slice(),
            })
        }

        let mult = try!(reader.read_u8_bits(2)) + 1;
        let range = [256, 128, 86, 64][mult as usize - 1];
        let rangebits = try!(reader.read_u8_bits(4)) as usize;
        let mut x_list = Vec::with_capacity(65);
        x_list.push(0);
        x_list.push(1 << rangebits);
        for &part_class in &part_classes {
            for _ in 0..classes[part_class].dim_count {
                let x = try!(reader.read_u16_bits(rangebits));
                if x_list.len() >= 65 {
                    return Err(Error::Undecodable("Too many elements in floor X list"));
                }
                x_list.push(x);
            }
        }

        let mut sorted_x_list = x_list.iter().cloned().enumerate().collect::<Vec<_>>();
        sorted_x_list.sort_by_key(|v| v.1);

        // Check x_list values are unique.
        {
            let mut last = sorted_x_list[0].1;
            for &x in sorted_x_list.iter().skip(1) {
                if x.1 == last {
                    return Err(Error::Undecodable("Floor X list contains duplicates"));
                }
                last = x.1;
            }
        }

        // Precompute neighbors.
        let mut neighbors = Vec::with_capacity(x_list.len() - 2);
        for i in 2..x_list.len() {
            neighbors.push(Self::find_neighbors(&x_list, i));
        }

        Ok(Floor {
            mult: mult,
            range: range,
            part_classes: part_classes.into_boxed_slice(),
            classes: classes.into_boxed_slice(),
            x_list: x_list.into_boxed_slice(),
            sorted_x_list: sorted_x_list.into_boxed_slice(),
            neighbors: neighbors.into_boxed_slice(),
        })
    }

    pub fn begin_decode<R: BitRead>(
                &self,
                result_y_list: &mut Vec<(u16, bool)>,
                reader: &mut R,
                codebooks: &[Codebook]) -> Result<()> {
        match self.do_begin_decode(result_y_list, reader, codebooks).expect_eof() {
            Err(ref e) if e.kind() == ErrorKind::ExpectedEof => {
                result_y_list.truncate(0);
                Ok(())
            },
            r @ _ => r,
        }
    }

    pub fn finish_decode(&self, result: &mut [f32], y_list: &[(u16, bool)]) {
        let mut hx = 0_i32;
        let mut hy = 0_i32;
        let mut lx = 0_i32;
        let mult = self.mult as i32;
        let mut ly = y_list[self.sorted_x_list[0].0].0 as i32 * mult;
        for &(i, x) in self.sorted_x_list.iter().skip(1) {
            let y = y_list[i];
            if y.1 {
                hy = y.0 as i32 * mult;
                hx = x as i32;
                Self::render_line(result, lx, ly, hx, hy);
                lx = hx;
                ly = hy;
            }
        }
        if hx < result.len() as i32 {
            let len = result.len() as i32;
            Self::render_line(result, hx, hy, len, hy);
        }
    }

    fn do_begin_decode<R: BitRead>(
                &self,
                result_y_list: &mut Vec<(u16, bool)>,
                reader: &mut R,
                codebooks: &[Codebook]) -> Result<()> {
        result_y_list.truncate(0);

        let non_zero = try!(reader.read_bool());
        if !non_zero {
            return Ok(());
        }

        let len_bits = Bits::ilog(self.range - 1) as usize;
        result_y_list.push((try!(reader.read_u16_bits(len_bits)), true));
        result_y_list.push((try!(reader.read_u16_bits(len_bits)), true));
        for &part_class in self.part_classes.iter() {
            let part_class = part_class;
            let class = &self.classes[part_class];
            let cbits = class.subclass_bit_count;
            let csub = (1 << cbits) - 1;
            let mut cval = if cbits > 0 {
                let codebook_idx = class.master_book.unwrap();
                let codebook = &codebooks[codebook_idx];
                try!(codebook.decode_scalar(reader)) as usize
            } else {
                0
            };
            for _ in 0..class.dim_count {
                let codebook = class.subclass_books[cval & csub].map(|i| &codebooks[i]);
                cval >>= cbits;
                let y = try!(codebook.map(|c| c.decode_scalar(reader)).unwrap_or(Ok(0)));
                result_y_list.push((y as u16, true));
            }
        }

        self.decode_amplitude(result_y_list);

        Ok(())
    }


    fn decode_amplitude(&self, result_y_list: &mut [(u16, bool)]) {
        for i in 2..result_y_list.len() {
            let (low_neighbor, high_neighbor) = self.neighbors[i - 2];
            let predicted = Self::render_point(
                    self.x_list[low_neighbor] as i32,
                    result_y_list[low_neighbor].0 as i32,
                    self.x_list[high_neighbor] as i32,
                    result_y_list[high_neighbor].0 as i32,
                    self.x_list[i] as i32) as i32;
            let high_room = self.range as i32 - predicted;
            let low_room = predicted;
            let room = if high_room < low_room {
                high_room * 2
            } else {
                low_room * 2
            };
            let y = result_y_list[i].0 as i32;
            let final_y = if y != 0 {
                result_y_list[low_neighbor].1 = true;
                result_y_list[high_neighbor].1 = true;
                result_y_list[i].1 = true;
                if y >= room {
                    if high_room > low_room {
                        predicted + y - low_room
                    } else {
                        predicted - y + high_room - 1
                    }
                } else {
                    if y % 2 == 0 {
                        predicted + y / 2
                    } else {
                        predicted - (y + 1) / 2
                    }
                }
            } else {
                result_y_list[i].1 = false;
                predicted
            };
            result_y_list[i].0 = final_y as u16;
        }
    }

    fn find_neighbors(arr: &[u16], end: usize) -> (usize, usize) {
        let v = arr[end];
        let mut low: Option<(usize, u16)> = None;
        let mut high: Option<(usize, u16)> = None;
        let arr = &arr[..end];
        for (arr_i, &arr_v) in arr.iter().enumerate() {
            if arr_v < v {
                if let Some(ref mut low) = low {
                    if arr_v > low.1 {
                        *low = (arr_i, arr_v);
                    }
                } else {
                    low = Some((arr_i, arr_v))
                }
            } else if arr_v > v {
                if let Some(ref mut high) = high {
                    if arr_v < high.1 {
                        *high = (arr_i, arr_v);
                    }
                } else {
                    high = Some((arr_i, arr_v));
                }
            }
        }
        (low.unwrap().0, high.unwrap().0)
    }

    fn render_point(x0: i32, y0: i32, x1: i32, y1: i32, x: i32) -> i32 {
        let dy = y1 - y0;
        let adx = x1 - x0;
        let ady = dy.abs();
        let err = ady * (x - x0);
        let off = err / adx;
        if dy < 0 {
            y0 - off
        } else {
            y0 + off
        }
    }

    fn render_line(result: &mut [f32], x0: i32, y0: i32, x1: i32, y1: i32) {
        let dy = y1 - y0;
        let adx = x1 - x0;
        let base = dy / adx;
        let ady = dy.abs() - base.abs() * adx;
        let sy = if dy < 0 {
            base - 1
        } else {
            base + 1
        };

        result[x0 as usize] *= INVERSE_DB_TABLE[y0 as usize];

        let mut y = y0;
        let mut err = 0;
        for x in x0 + 1..x1 {
            err += ady;
            if err >= adx {
                err -= adx;
                y += sy;
            } else {
                y += base;
            }
            result[x as usize] *= INVERSE_DB_TABLE[y as usize];
        }
    }
}

const INVERSE_DB_TABLE: [f32; 256] = [
    1.0649863E-07, 1.1341951e-07, 1.2079015e-07, 1.2863978e-07,
    1.3699951e-07, 1.4590251e-07, 1.5538408e-07, 1.6548181e-07,
    1.7623575e-07, 1.8768855e-07, 1.9988561e-07, 2.1287530e-07,
    2.2670913e-07, 2.4144197e-07, 2.5713223e-07, 2.7384213e-07,
    2.9163793e-07, 3.1059021e-07, 3.3077411e-07, 3.5226968e-07,
    3.7516214e-07, 3.9954229e-07, 4.2550680e-07, 4.5315863e-07,
    4.8260743e-07, 5.1396998e-07, 5.4737065e-07, 5.8294187e-07,
    6.2082472e-07, 6.6116941e-07, 7.0413592e-07, 7.4989464e-07,
    7.9862701e-07, 8.5052630e-07, 9.0579828e-07, 9.6466216e-07,
    1.0273513e-06, 1.0941144e-06, 1.1652161e-06, 1.2409384e-06,
    1.3215816e-06, 1.4074654e-06, 1.4989305e-06, 1.5963394e-06,
    1.7000785e-06, 1.8105592e-06, 1.9282195e-06, 2.0535261e-06,
    2.1869758e-06, 2.3290978e-06, 2.4804557e-06, 2.6416497e-06,
    2.8133190e-06, 2.9961443e-06, 3.1908506e-06, 3.3982101e-06,
    3.6190449e-06, 3.8542308e-06, 4.1047004e-06, 4.3714470e-06,
    4.6555282e-06, 4.9580707e-06, 5.2802740e-06, 5.6234160e-06,
    5.9888572e-06, 6.3780469e-06, 6.7925283e-06, 7.2339451e-06,
    7.7040476e-06, 8.2047000e-06, 8.7378876e-06, 9.3057248e-06,
    9.9104632e-06, 1.0554501e-05, 1.1240392e-05, 1.1970856e-05,
    1.2748789e-05, 1.3577278e-05, 1.4459606e-05, 1.5399272e-05,
    1.6400004e-05, 1.7465768e-05, 1.8600792e-05, 1.9809576e-05,
    2.1096914e-05, 2.2467911e-05, 2.3928002e-05, 2.5482978e-05,
    2.7139006e-05, 2.8902651e-05, 3.0780908e-05, 3.2781225e-05,
    3.4911534e-05, 3.7180282e-05, 3.9596466e-05, 4.2169667e-05,
    4.4910090e-05, 4.7828601e-05, 5.0936773e-05, 5.4246931e-05,
    5.7772202e-05, 6.1526565e-05, 6.5524908e-05, 6.9783085e-05,
    7.4317983e-05, 7.9147585e-05, 8.4291040e-05, 8.9768747e-05,
    9.5602426e-05, 0.00010181521, 0.00010843174, 0.00011547824,
    0.00012298267, 0.00013097477, 0.00013948625, 0.00014855085,
    0.00015820453, 0.00016848555, 0.00017943469, 0.00019109536,
    0.00020351382, 0.00021673929, 0.00023082423, 0.00024582449,
    0.00026179955, 0.00027881276, 0.00029693158, 0.00031622787,
    0.00033677814, 0.00035866388, 0.00038197188, 0.00040679456,
    0.00043323036, 0.00046138411, 0.00049136745, 0.00052329927,
    0.00055730621, 0.00059352311, 0.00063209358, 0.00067317058,
    0.00071691700, 0.00076350630, 0.00081312324, 0.00086596457,
    0.00092223983, 0.00098217216, 0.0010459992,  0.0011139742,
    0.0011863665,  0.0012634633,  0.0013455702,  0.0014330129,
    0.0015261382,  0.0016253153,  0.0017309374,  0.0018434235,
    0.0019632195,  0.0020908006,  0.0022266726,  0.0023713743,
    0.0025254795,  0.0026895994,  0.0028643847,  0.0030505286,
    0.0032487691,  0.0034598925,  0.0036847358,  0.0039241906,
    0.0041792066,  0.0044507950,  0.0047400328,  0.0050480668,
    0.0053761186,  0.0057254891,  0.0060975636,  0.0064938176,
    0.0069158225,  0.0073652516,  0.0078438871,  0.0083536271,
    0.0088964928,  0.009474637,   0.010090352,   0.010746080,
    0.011444421,   0.012188144,   0.012980198,   0.013823725,
    0.014722068,   0.015678791,   0.016697687,   0.017782797,
    0.018938423,   0.020169149,   0.021479854,   0.022875735,
    0.024362330,   0.025945531,   0.027631618,   0.029427276,
    0.031339626,   0.033376252,   0.035545228,   0.037855157,
    0.040315199,   0.042935108,   0.045725273,   0.048696758,
    0.051861348,   0.055231591,   0.058820850,   0.062643361,
    0.066714279,   0.071049749,   0.075666962,   0.080584227,
    0.085821044,   0.091398179,   0.097337747,   0.10366330,
    0.11039993,    0.11757434,    0.12521498,    0.13335215,
    0.14201813,    0.15124727,    0.16107617,    0.17154380,
    0.18269168,    0.19456402,    0.20720788,    0.22067342,
    0.23501402,    0.25028656,    0.26655159,    0.28387361,
    0.30232132,    0.32196786,    0.34289114,    0.36517414,
    0.38890521,    0.41417847,    0.44109412,    0.46975890,
    0.50028648,    0.53279791,    0.56742212,    0.60429640,
    0.64356699,    0.68538959,    0.72993007,    0.77736504,
    0.82788260,    0.88168307,    0.9389798,     1.0
];
