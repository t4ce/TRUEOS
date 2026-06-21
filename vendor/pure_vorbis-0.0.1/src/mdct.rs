//! This is a direct port of the reference inverse MDCT implementation in [libvorbis].
//! [libvorbis]: https://www.xiph.org/vorbis/doc/libvorbis/
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::f32::consts::PI;

use util::Bits;

const PI3_8: f32 = 0.38268343236508977175;
const PI2_8: f32 = 0.70710678118654752441;
const PI1_8: f32 = 0.92387953251128675613;

pub struct Mdct {
    len: usize,
    log2len: usize,
    trig: Box<[f32]>,
    bitrev: Box<[usize]>,
}

impl Mdct {
    pub fn new(len: usize) -> Self {
        assert!(len >= 32 && len % 2 == 0);

        let mut trig = vec![0_f32; len + len / 4];
        let half_len = len / 2;
        for i in 0..len / 4 {
            let len = len as f32;
            let i2 = i as f32 * 2.0;
            trig[i * 2] = libm::cosf((PI / len) * (2.0 * i2));
            trig[i * 2 + 1] = -libm::sinf((PI / len) * (2.0 * i2));
            trig[half_len + i * 2] = libm::cosf((PI / (2.0 * len)) * (i2 + 1.0));
            trig[half_len + i * 2 + 1] = libm::sinf((PI / (2.0 * len)) * (i2 + 1.0));
        }
        for i in 0..len / 8 {
            let i2 = i as f32 * 2.0;
            trig[len + i * 2] = libm::cosf((PI / len as f32) * (2.0 * i2 + 2.0)) * 0.5;
            trig[len + i * 2 + 1] = -libm::sinf((PI / len as f32) * (2.0 * i2 + 2.0)) * 0.5;
        }

        let log2len = (Bits::ilog(len as u32) - 1) as usize;
        let mut bitrev = Vec::with_capacity(len / 4);
        {
            let mask = (1 << (log2len - 1)) - 1;
            let msb = 1 << (log2len - 2);
            for i in 0..len / 8 {
                let mut acc = 0;
                let mut j = 0;
                while msb >> j != 0 {
                    if (msb >> j) & i != 0 {
                        acc |= 1 << j;
                    }
                    j += 1;
                }
                bitrev.push(((!acc) & mask) - 1);
                bitrev.push(acc);
            }
        }

        Mdct {
            len: len,
            log2len: log2len,
            trig: trig.into_boxed_slice(),
            bitrev: bitrev.into_boxed_slice(),
        }
    }

    pub fn inverse(&self, buf: &mut [f32]) {
        assert!(buf.len() == self.len);
        let n = self.len;
        let n2 = n >> 1;
        let n4 = n >> 2;

        let tri = &self.trig;

        /* rotate */
        let mut i_x = n2 - 7;
        let mut o_x = n2 + n4;
        let mut t = n4;

        loop {
            o_x          -= 4;
            buf[o_x + 0]  = -buf[i_x + 2] * tri[t + 3] - buf[i_x + 0]  * tri[t + 2];
            buf[o_x + 1]  =  buf[i_x + 0] * tri[t + 3] - buf[i_x + 2]  * tri[t + 2];
            buf[o_x + 2]  = -buf[i_x + 6] * tri[t + 1] - buf[i_x + 4]  * tri[t + 0];
            buf[o_x + 3]  =  buf[i_x + 4] * tri[t + 1] - buf[i_x + 6]  * tri[t + 0];

            if i_x < 8 {
                break;
            }

            i_x          -= 8;
            t           += 4;
        }

        let mut i_x = n2 - 8;
        let mut o_x = n2 + n4;
        let mut t  = n4;

        loop {
            t           -= 4;
            buf[o_x + 0]  = buf[i_x + 4] * tri[t + 3] + buf[i_x + 6] * tri[t + 2];
            buf[o_x + 1]  = buf[i_x + 4] * tri[t + 2] - buf[i_x + 6] * tri[t + 3];
            buf[o_x + 2]  = buf[i_x + 0] * tri[t + 1] + buf[i_x + 2] * tri[t + 0];
            buf[o_x + 3]  = buf[i_x + 0] * tri[t + 0] - buf[i_x + 2] * tri[t + 1];

            if i_x < 8 {
                break;
            }

            i_x          -= 8;
            o_x          += 4;
        }

        self.butterflies(&mut buf[n2..]);
        self.bitreverse(buf);

        /* rotate + window */

        let mut o_x1 = n2 + n4;
        let mut o_x2 = n2 + n4;
        let mut i_x  = 0;
        let mut t   = n2;

        loop {
            o_x1    -= 4;

            buf[o_x1 + 3]  =   buf[i_x + 0] * tri[t + 1] - buf[i_x + 1] * tri[t + 0];
            buf[o_x2 + 0]  = -(buf[i_x + 0] * tri[t + 0] + buf[i_x + 1] * tri[t + 1]);

            buf[o_x1 + 2]  =   buf[i_x + 2] * tri[t + 3] - buf[i_x + 3] * tri[t + 2];
            buf[o_x2 + 1]  = -(buf[i_x + 2] * tri[t + 2] + buf[i_x + 3] * tri[t + 3]);

            buf[o_x1 + 1]  =   buf[i_x + 4] * tri[t + 5] - buf[i_x + 5] * tri[t + 4];
            buf[o_x2 + 2]  = -(buf[i_x + 4] * tri[t + 4] + buf[i_x + 5] * tri[t + 5]);

            buf[o_x1 + 0]  =   buf[i_x + 6] * tri[t + 7] - buf[i_x + 7] * tri[t + 6];
            buf[o_x2 + 3]  = -(buf[i_x + 6] * tri[t + 6] + buf[i_x + 7] * tri[t + 7]);

            o_x2    += 4;
            i_x     += 8;
            t      += 8;

            if i_x >= o_x1 {
                break;
            }
        }

        let mut i_x = n2 + n4;
        let mut o_x1 = n4;
        let mut o_x2 = o_x1;

        loop {
            o_x1          -= 4;
            i_x           -= 4;

            let v         =  buf[i_x + 3];
            buf[o_x1 + 3]  =  v;
            buf[o_x2 + 0]  = -v;

            let v         =  buf[i_x + 2];
            buf[o_x1 + 2]  =  v;
            buf[o_x2 + 1]  = -v;

            let v         =  buf[i_x + 1];
            buf[o_x1 + 1]  =  v;
            buf[o_x2 + 2]  = -v;

            let v         =  buf[i_x + 0];
            buf[o_x1 + 0]  =  v;
            buf[o_x2 + 3]  = -v;

            o_x2          +=  4;

            if o_x2 >= i_x {
                break;
            }
        }

        let mut i_x        = n2 + n4;
        let mut o_x1       = n2 + n4;
        let o_x2           = n2;
        loop {
            o_x1          -= 4;
            buf[o_x1 + 0]  = buf[i_x + 3];
            buf[o_x1 + 1]  = buf[i_x + 2];
            buf[o_x1 + 2]  = buf[i_x + 1];
            buf[o_x1 + 3]  = buf[i_x + 0];
            i_x           += 4;

            if o_x1 <= o_x2 {
                break;
            }
        }
    }

    fn butterflies(&self, x: &mut [f32]) {
        let stages = self.log2len - 5;

        if stages > 1 {
            self.butterfly_first(x);
        }

        let len = x.len();
        for i in 1..stages - 1 {
            for j in 0..(1 << i) {
                let l = len >> i;
                let start = l * j;
                self.butterfly_generic(&mut x[start..start + l], 4 << i);
            }
        }

        let mut j = 0;
        while j < x.len() {
            Self::butterfly_32(&mut x[j..]);
            j += 32;
        }
    }

    /* N point first stage butterfly */
    #[inline]
    fn butterfly_first(&self, x: &mut [f32]) {
        let tri = &self.trig;
        let mut t = 0;
        let mut x1 = x.len() - 8;
        let mut x2 = (x.len() >> 1) - 8;

        loop {
            let r0      = x[x1 + 6]      -  x[x2 + 6];
            let r1      = x[x1 + 7]      -  x[x2 + 7];
            x[x1 + 6]  += x[x2 + 6];
            x[x1 + 7]  += x[x2 + 7];
            x[x2 + 6]   = r1 * tri[t + 1]  +  r0 * tri[t + 0];
            x[x2 + 7]   = r1 * tri[t + 0]  -  r0 * tri[t + 1];

            let r0      = x[x1 + 4]      -  x[x2 + 4];
            let r1      = x[x1 + 5]      -  x[x2 + 5];
            x[x1 + 4]  += x[x2 + 4];
            x[x1 + 5]  += x[x2 + 5];
            x[x2 + 4]   = r1 * tri[t + 5]  +  r0 * tri[t + 4];
            x[x2 + 5]   = r1 * tri[t + 4]  -  r0 * tri[t + 5];

            let r0      = x[x1 + 2]      -  x[x2 + 2];
            let r1      = x[x1 + 3]      -  x[x2 + 3];
            x[x1 + 2]  += x[x2 + 2];
            x[x1 + 3]  += x[x2 + 3];
            x[x2 + 2]   = r1 * tri[t + 9]  +  r0 * tri[t + 8];
            x[x2 + 3]   = r1 * tri[t + 8]  -  r0 * tri[t + 9];

            let r0      = x[x1 + 0]      -  x[x2 + 0];
            let r1      = x[x1 + 1]      -  x[x2 + 1];
            x[x1 + 0]  += x[x2 + 0];
            x[x1 + 1]  += x[x2 + 1];
            x[x2 + 0]   = r1 * tri[t + 13] +  r0 * tri[t + 12];
            x[x2 + 1]   = r1 * tri[t + 12] -  r0 * tri[t + 13];

            if x2 < 8 {
                break;
            }

            x1-=8;
            x2-=8;
            t+=16;
        }
    }

    /* N/stage point generic N stage butterfly */
    #[inline]
    fn butterfly_generic(&self, x: &mut [f32], trigint: usize) {
        let tri = &self.trig;

        let mut x1 = x.len() - 8;
        let mut x2 = (x.len() >> 1) - 8;
        let mut t = 0;

        loop {
            let r0      = x[x1 + 6]      -  x[x2 + 6];
            let r1      = x[x1 + 7]      -  x[x2 + 7];
            x[x1 + 6]  += x[x2 + 6];
            x[x1 + 7]  += x[x2 + 7];
            x[x2 + 6]   = r1 * tri[t + 1]  +  r0 * tri[t + 0];
            x[x2 + 7]   = r1 * tri[t + 0]  -  r0 * tri[t + 1];

            t += trigint;

            let r0      = x[x1 + 4]      -  x[x2 + 4];
            let r1      = x[x1 + 5]      -  x[x2 + 5];
            x[x1 + 4]  += x[x2 + 4];
            x[x1 + 5]  += x[x2 + 5];
            x[x2 + 4]   = r1 * tri[t + 1]  +  r0 * tri[t + 0];
            x[x2 + 5]   = r1 * tri[t + 0]  -  r0 * tri[t + 1];

            t += trigint;

            let r0      = x[x1 + 2]      -  x[x2 + 2];
            let r1      = x[x1 + 3]      -  x[x2 + 3];
            x[x1 + 2]  += x[x2 + 2];
            x[x1 + 3]  += x[x2 + 3];
            x[x2 + 2]   = r1 * tri[t + 1]  +  r0 * tri[t + 0];
            x[x2 + 3]   = r1 * tri[t + 0]  -  r0 * tri[t + 1];

            t += trigint;

            let r0      = x[x1 + 0]      -  x[x2 + 0];
            let r1      = x[x1 + 1]      -  x[x2 + 1];
            x[x1 + 0]  += x[x2 + 0];
            x[x1 + 1]  += x[x2 + 1];
            x[x2 + 0]   = r1 * tri[t + 1]  +  r0 * tri[t + 0];
            x[x2 + 1]   = r1 * tri[t + 0]  -  r0 * tri[t + 1];

            t+=trigint;
            if x2 < 8 {
                break;
            }
            x1 -= 8;
            x2 -= 8;
        }
    }

    /* 8 point butterfly */
    #[inline]
    fn butterfly_8(x: &mut [f32]) {
        let r0   = x[6] + x[2];
        let r1   = x[6] - x[2];
        let r2   = x[4] + x[0];
        let r3   = x[4] - x[0];

        x[6] = r0   + r2;
        x[4] = r0   - r2;

        let r0   = x[5] - x[1];
        let r2   = x[7] - x[3];
        x[0] = r1   + r0;
        x[2] = r1   - r0;

        let r0   = x[5] + x[1];
        let r1   = x[7] + x[3];
        x[3] = r2   + r3;
        x[1] = r2   - r3;
        x[7] = r1   + r0;
        x[5] = r1   - r0;
    }

    /* 16 point butterfly */
    #[inline]
    fn butterfly_16(x: &mut [f32]){
        let r0     = x[1]  - x[9];
        let r1     = x[0]  - x[8];

        x[8]  += x[0];
        x[9]  += x[1];
        x[0]   = (r0   + r1) * PI2_8;
        x[1]   = (r0   - r1) * PI2_8;

        let r0     = x[3]  - x[11];
        let r1     = x[10] - x[2];
        x[10] += x[2];
        x[11] += x[3];
        x[2]   = r0;
        x[3]   = r1;

        let r0     = x[12] - x[4];
        let r1     = x[13] - x[5];
        x[12] += x[4];
        x[13] += x[5];
        x[4]   = (r0   - r1) * PI2_8;
        x[5]   = (r0   + r1) * PI2_8;

        let r0     = x[14] - x[6];
        let r1     = x[15] - x[7];
        x[14] += x[6];
        x[15] += x[7];
        x[6]  = r0;
        x[7]  = r1;


        Self::butterfly_8(x);
        Self::butterfly_8(&mut x[8..]);
    }

    /* 32 point butterfly */
    #[inline]
    fn butterfly_32(x: &mut [f32]) {
        let r0 = x[30] - x[14];
        let r1 = x[31] - x[15];

        x[30] +=         x[14];
        x[31] +=         x[15];
        x[14]  =         r0;
        x[15]  =         r1;

        let r0 = x[28] - x[12];
        let r1 = x[29] - x[13];
        x[28] +=         x[12];
        x[29] +=         x[13];
        x[12]  =  r0 * PI1_8  -  r1 * PI3_8;
        x[13]  =  r0 * PI3_8  +  r1 * PI1_8;

        let r0 = x[26] - x[10];
        let r1 = x[27] - x[11];
        x[26] +=         x[10];
        x[27] +=         x[11];
        x[10]  = ( r0  - r1 ) * PI2_8;
        x[11]  = ( r0  + r1 ) * PI2_8;

        let r0 = x[24] - x[8];
        let r1 = x[25] - x[9];
        x[24] += x[8];
        x[25] += x[9];
        x[8]   =  r0 * PI3_8  -  r1 * PI1_8;
        x[9]   =  r1 * PI3_8  +  r0 * PI1_8;

        let r0 = x[22] - x[6];
        let r1 = x[7]  - x[23];
        x[22] += x[6];
        x[23] += x[7];
        x[6]   = r1;
        x[7]   = r0;

        let r0 = x[4]  - x[20];
        let r1 = x[5]  - x[21];
        x[20] += x[4];
        x[21] += x[5];
        x[4]   =  r1 * PI1_8  +  r0 * PI3_8;
        x[5]   =  r1 * PI3_8  -  r0 * PI1_8;

        let r0 = x[2]  - x[18];
        let r1 = x[3]  - x[19];
        x[18] += x[2];
        x[19] += x[3];
        x[2]   = ( r1  + r0 ) * PI2_8;
        x[3]   = ( r1  - r0 ) * PI2_8;

        let r0 = x[0]  - x[16];
        let r1 = x[1]  - x[17];
        x[16] += x[0];
        x[17] += x[1];
        x[0]   =  r1 * PI3_8  +  r0 * PI1_8;
        x[1]   =  r1 * PI1_8  -  r0 * PI3_8;

        Self::butterfly_16(x);
        Self::butterfly_16(&mut x[16..]);
    }

    fn bitreverse(&self, x: &mut [f32]){
        let n       = self.len;
        let n2 = n >> 1;
        let brv = &self.bitrev;
        let mut bit = 0;
        let mut w0      = 0;
        let mut w1      = n2;
        let tri = &self.trig;
        let mut t       = n;

        loop {
            let x0    = n2 + brv[bit + 0];
            let x1    = n2 + brv[bit + 1];

            let r0     = x[x0 + 1]  - x[x1 + 1];
            let r1     = x[x0 + 0]  + x[x1 + 0];
            let r2     = r1     * tri[t + 0]   + r0 * tri[t + 1];
            let r3     = r1     * tri[t + 1]   - r0 * tri[t + 0];

            w1    -= 4;

            let r0     = (x[x0 + 1] + x[x1 + 1]) * 0.5;
            let r1     = (x[x0 + 0] - x[x1 + 0]) * 0.5;

            x[w0 + 0]  = r0     + r2;
            x[w1 + 2]  = r0     - r2;
            x[w0 + 1]  = r1     + r3;
            x[w1 + 3]  = r3     - r1;

            let x0     = n2 + brv[bit + 2];
            let x1     = n2 + brv[bit + 3];

            let r0     = x[x0 + 1]  - x[x1 + 1];
            let r1     = x[x0 + 0]  + x[x1 + 0];
            let r2     = r1     * tri[t + 2]   + r0 * tri[t + 3];
            let r3     = r1     * tri[t + 3]   - r0 * tri[t + 2];

            let r0     = (x[x0 + 1] + x[x1 + 1]) * 0.5;
            let r1     = (x[x0 + 0] - x[x1 + 0]) * 0.5;

            x[w0 + 2]  = r0     + r2;
            x[w1 + 0]  = r0     - r2;
            x[w0 + 3]  = r1     + r3;
            x[w1 + 1]  = r3     - r1;

            t     += 4;
            bit   += 4;
            w0    += 4;

            if w0 >= w1 {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32;
    use std::f32::consts::PI;

    use super::*;

    pub fn inverse_mdct_slow(buf: &mut [f32]) {
        assert!(buf.len() % 2 == 0);
        let n = buf.len();
        let n2 = n / 2;
        let inp = buf[..n2].as_ref().to_vec();
        for i in 0..n {
            let mut acc = 0_f32;
            for (j, x) in inp.iter().enumerate() {
                let n = n as f32;
                acc += x * (PI / 2.0 / n * (2.0 * i as f32 + 1.0 + n / 2.0) * (2.0 * j as f32 + 1.0)).cos();
            }
            buf[i] = acc;
        }
    }

    static INPUT: [f32; 64] = [-0.69401383, 0.03862691, -0.55153704, -0.78269863, -0.09741044, -0.49561787, 0.42875743, -0.19526768, -0.06347418, -0.00010037422, 0.6325817, -0.48571062, -0.8504288, -0.28039575, -0.6088922, 0.95481896, -0.1591835, 0.9108696, -0.54748464, -0.11515808, -0.985873, -0.1792016, 0.10024643, -0.65555835, 0.4586711, -0.28872848, 0.09826708, -0.19525862, 0.833838, -0.36552095, 0.037439585, 0.40315723, -0.96927285, 0.41392016, 0.408257, 0.15481758, 0.9985726, -0.98773885, 0.82968235, 0.46624875, 0.49264956, 0.11497569, -0.006861925, -0.9980333, -0.22240639, -0.6312058, 0.4906652, -0.010108948, -0.8477638, -0.056087017, -0.7326493, -0.73279214, -0.68954086, -0.4644475, 0.6687648, 0.62569046, -0.5956092, 0.9961209, -0.29823017, -0.03980136, -0.12348294, 0.83054876, 0.32812834, 0.3774073];
    static EXPECTED: [f32; 128] = [-0.04398486, -0.104446724, -3.534832, 3.8501837, -0.14957228, 0.7534752, -2.6459243, 0.3395752, -0.40157068, 1.3667705, -1.5802002, -5.155503, -1.9898258, -0.3746807, 2.723372, -7.4657774, 1.1178919, 4.2596145, -4.2643995, 0.32841936, 0.72192276, 1.5253807, -5.8298798, -4.7367554, 2.3636713, 6.5154843, 3.032085, 2.8470132, 2.1626804, -6.993517, 2.662696, -0.41398838, 0.41398835, -2.6627026, 6.9935184, -2.1627154, -2.8469687, -3.032117, -6.51548, -2.363654, 4.7367563, 5.8298445, -1.5253813, -0.7219145, -0.32840136, 4.264414, -4.2596507, -1.1178186, 7.4657693, -2.7233686, 0.3747228, 1.9898224, 5.1555324, 1.5802336, -1.3667517, 0.40155572, -0.33958945, 2.6459208, -0.75346154, 0.149549, -3.8501651, 3.5348828, 0.104403034, 0.0439485, 4.050725, 0.5420946, 2.4831505, -0.5343465, 1.7392917, 0.9157535, -2.3912883, -1.3115467, 0.78983486, -4.5483594, -1.4655226, 3.1918535, 4.476434, -2.6109004, 4.347729, -5.4297366, -2.3821006, -2.3284597, -3.6841853, 3.1392276, 3.3745584, 0.91208255, -0.056582414, 0.049863316, 3.0820458, -3.0675306, 6.783364, -0.14948165, -2.019868, 4.173112, 1.8012438, 4.0068555, 4.0068464, 1.8012108, 4.173142, -2.019923, -0.14935923, 6.783282, -3.0675812, 3.0820832, 0.04983966, -0.056600958, 0.9120948, 3.3745806, 3.1391861, -3.684268, -2.3284128, -2.3821485, -5.4296513, 4.3477545, -2.6109486, 4.476468, 3.1918228, -1.4655291, -4.5483932, 0.7899155, -1.3116122, -2.3912494, 0.9158027, 1.7392603, -0.5343737, 2.483176, 0.542063, 4.0507092];

    #[test] #[ignore]
    fn test_inverse_mdct_slow() {
        let mut actual = vec![0_f32; INPUT.len() * 2];
        actual[..INPUT.len()].as_mut().clone_from_slice(&INPUT);

        inverse_mdct_slow(&mut actual);

        assert_eq!(actual.len(), EXPECTED.len());
        for (&a, &e) in actual.iter().zip(EXPECTED.iter()) {
            assert!((a - e).abs() < 1e-11);
        }
    }

    #[test]
    fn inverse() {
        let mut actual = vec![0_f32; INPUT.len() * 2];
        actual[..INPUT.len()].as_mut().clone_from_slice(&INPUT);

        Mdct::new(actual.len()).inverse(&mut actual);

        assert_eq!(actual.len(), EXPECTED.len());
        for (&a, &e) in actual.iter().zip(EXPECTED.iter()) {
            assert!((a - e).abs() < 1e-3);
        }
    }
}
