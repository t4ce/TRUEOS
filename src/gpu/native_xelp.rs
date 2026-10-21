//! Admission for odin-son's bounded scalar Xe-LP ABI v1.

pub(crate) const MAX_CODE_BYTES: usize = 24 * 1024;
pub(crate) const PREFIX: [u32; 4] = [0x80000166, 0x80018220, 0x01008000, 0x04c004c0];
pub(crate) const SUFFIX: [[u32; 4]; 8] = [
    [0x00000101, 0x00010000, 0, 0],
    [0x00034031, 0, 0xcc00030c, 0x009a040c],
    [0x00003001, 0x00010000, 0, 0],
    [0x80034131, 0x7e0c0000, 0xa000000c, 0x02780000],
    [0x80002101, 0x00010000, 0, 0],
    [0x80030161, 0x7f050220, 0x00460005, 0],
    [0x80040061, 0x20014aa0, 0, 0],
    [0x80034131, 0x00000004, 0x70207f0c, 0],
];

fn words(bytes: &[u8]) -> [u32; 4] {
    core::array::from_fn(|i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
}

pub(crate) fn validate(code: &[u8]) -> bool {
    if !(160..=MAX_CODE_BYTES).contains(&code.len()) || code.len() % 16 != 0 {
        return false;
    }
    let tail = code.len() - SUFFIX.len() * 16;
    if words(&code[..16]) != PREFIX
        || code[tail..]
            .chunks_exact(16)
            .zip(SUFFIX)
            .any(|(a, b)| words(a) != b)
    {
        return false;
    }
    let mut initialized = [false; 128];
    initialized[1] = true;
    initialized[2] = true;
    for instruction in code[16..tail].chunks_exact(16) {
        let w = words(instruction);
        let op = w[0] & 0x7f;
        let dst = (w[1] >> 24) as usize;
        let ty = (w[1] >> 4) & 15;
        if w[0] != 0x00030100 | op || !(4..112).contains(&dst) || !matches!(ty, 2 | 10) {
            return false;
        }
        let base = (dst as u32) << 24 | 0x50000 | ty << 4 | ty << 8;
        let src0 = ((w[2] >> 8) & 255) as usize;
        let src1 = ((w[3] >> 8) & 255) as usize;
        let readable = |r: usize| r < initialized.len() && initialized[r];
        if op == 0x61 {
            if w[1] == base | 0x4000 {
                if w[2] != 0 {
                    return false;
                }
            } else if w[1] != base
                || w[3] != 0
                || !readable(src0)
                || w[2] != 0x00460005 | (src0 as u32) << 8
            {
                return false;
            }
        } else {
            if !matches!(op, 0x40 | 0x41 | 0x65 | 0x66 | 0x67 | 0x68 | 0x69)
                || (op >= 0x65 && ty != 2)
                || !readable(src0)
                || w[2] != ty << 24 | 0x00460005 | (src0 as u32) << 8
            {
                return false;
            }
            if w[1] != base | 0x8000
                && (w[1] != base || !readable(src1) || w[3] != 0x00460005 | (src1 as u32) << 8)
            {
                return false;
            }
        }
        initialized[dst] = true;
    }
    initialized[4]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kernel() -> Vec<u8> {
        PREFIX
            .into_iter()
            .chain([0x00030140, 0x04050aa0, 0x0a460105, 0x00460205])
            .chain(SUFFIX.into_iter().flatten())
            .flat_map(u32::to_le_bytes)
            .collect()
    }

    #[test]
    fn admits_scalar_add_and_rejects_privileged_or_uninitialized_access() {
        let original = kernel();
        assert!(validate(&original));
        for (offset, value) in [(16, 0x31), (23, 0), (25, 80), (29, 80), (24, 4)] {
            let mut corrupt = original.clone();
            corrupt[offset] = value;
            assert!(!validate(&corrupt), "offset {offset}");
        }
        for i in 0..original.len() - 16 {
            if (16..32).contains(&i) {
                continue;
            }
            let mut corrupt = original.clone();
            corrupt[i] ^= 1;
            assert!(!validate(&corrupt), "envelope byte {i}");
        }
        assert!(!validate(&original[..original.len() - 1]));
    }
}
