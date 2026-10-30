//! Admission for odin-son's bounded scalar Xe-LP ABI v1.

pub(crate) const MAX_CODE_BYTES: usize = 24 * 1024;
pub(crate) const PREFIX: [u32; 4] = [0x80000166, 0x80018220, 0x01008000, 0x04c004c0];
pub(crate) const SUFFIX: [[u32; 4]; 9] = [
    [0x00000101, 0x00010000, 0, 0],
    [0x00034031, 0, 0xcc00030c, 0x009a040c],
    [0x00003001, 0x00010000, 0, 0],
    [0x80034131, 0x7e0c0000, 0xa000000c, 0x02780000],
    [0x80002101, 0x00010000, 0, 0],
    [0x80030161, 0x7f050220, 0x00460005, 0],
    [0x80040061, 0x20014aa0, 0, 0],
    [0x00000101, 0x00010000, 0, 0],
    [0x80034131, 0x00000004, 0x70207f0c, 0],
];

fn words(bytes: &[u8]) -> [u32; 4] {
    core::array::from_fn(|i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
}

pub(crate) fn validate(code: &[u8]) -> bool {
    if !(176..=MAX_CODE_BYTES).contains(&code.len()) || code.len() % 16 != 0 {
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
        for i in 0..original.len() {
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

/// Complete upstream SIMD8 kernels: direct GRF arithmetic/control flow and one
/// fixed BT0 media-block result store. Hashes identify uploads, not admission.
pub(crate) fn validate_simd8(code: &[u8]) -> bool {
    const HEAD: [[u32; 4]; 3] = [
        [0x00030061, 0x77050220, 0x00460005, 0],
        [0x00010061, 0x77054220, 0, 0],
        [0x00000061, 0x77454220, 0, 31],
    ];
    const TAIL: [[u32; 4]; 3] = [
        [0x00039031, 0, 0xc0007714, 0x02a00000],
        [0x00030061, 0x76050220, 0x00460005, 0],
        [0x00039131, 4, 0x7020760c, 0x10000000],
    ];
    if !(112..=MAX_CODE_BYTES).contains(&code.len()) || code.len() % 16 != 0 {
        return false;
    }
    let end = code.len() - 48;
    if code[..48]
        .chunks_exact(16)
        .zip(HEAD)
        .any(|(a, b)| words(a) != b)
        || code[end..]
            .chunks_exact(16)
            .zip(TAIL)
            .any(|(a, b)| words(a) != b)
    {
        return false;
    }
    let grf = |r: u32| (1..118).contains(&r) || r == 120;
    let source = |w: u32| grf((w >> 8) & 255) && w & 0x00ff_00ff == 0x0046_0005;
    for (index, bytes) in code[48..end].chunks_exact(16).enumerate() {
        let w = words(bytes);
        let op = w[0] & 0x7f;
        // The compiler's reciprocal is the only admitted out-of-order ALU op.
        // Its fixed SBID must be retired before any result can be consumed.
        if op == 0x38 {
            if w[0] != 0x00039238 || !grf(w[1] >> 24)
                || w[1] & 0x00ffffff != 0x00050aa0
                || !source(w[2]) || w[2] >> 24 != 0x10 || w[3] != 0
                || code.get(64 + index*16..80 + index*16)
                    .map(words) != Some([0x00002201, 0x00010000, 0, 0])
            { return false; }
            continue;
        }
        if op == 0x01 {
            if w != [0x00002201, 0x00010000, 0, 0] || index == 0
                || words(&code[32 + index*16..48 + index*16])[0] != 0x00039238
            { return false; }
            continue;
        }
        if w[0] & !0x11c0_0000 != 0x00030100 | op || w[0] & 0x1100_0000 == 0x1000_0000 {
            return false;
        }
        if matches!(op, 0x22 | 0x24 | 0x25 | 0x27 | 0x2e | 0x2f) {
            if !matches!(w[1], 0x4000 | 0xc000) || (w[1] == 0x4000 && w[2] != 0) {
                return false;
            }
            for offset in [w[3], w[2]]
                .into_iter()
                .take(if w[1] == 0xc000 { 2 } else { 1 })
            {
                let target = (index as i64 + 3) * 16 + i64::from(offset as i32);
                if target < 48 || target > end as i64 || target % 16 != 0 {
                    return false;
                }
            }
            continue;
        }
        let dst = w[1] >> 24;
        let ty = (w[1] >> 4) & 15;
        let null_cmp = op == 0x70 && dst == 0;
        if (!null_cmp && !grf(dst)) || !matches!(ty, 2 | 6 | 10) {
            return false;
        }
        let base = dst << 24 | if null_cmp { 0x10000 } else { 0x50000 } | ty << 4 | ty << 8;
        if op == 0x61 {
            if ty != 2
                || !((w[1] == base | 0x4000 && w[2] == 0)
                    || (w[1] == base && w[2] >> 24 == 0 && source(w[2]) && w[3] == 0))
            {
                return false;
            }
        } else {
            if !matches!(op, 0x40 | 0x41 | 0x65 | 0x66 | 0x67 | 0x68 | 0x69 | 0x70)
                || (op == 0x41 && ty != 10)
                || (matches!(op, 0x65..=0x69) && ty != 2)
            {
                return false;
            }
            let condition = if op == 0x70 { w[2] >> 28 } else { 0 };
            if (op == 0x70 && !(1..=6).contains(&condition))
                || !source(w[2])
                || w[2] >> 24 != condition << 4 | ty
            {
                return false;
            }
            let immediate = w[1] == base | 0x8000;
            if !immediate && (w[1] != base || w[3] & 0xfc00_0000 != 0 || !source(w[3])) {
                return false;
            }
        }
    }
    true
}

/// The complete POC arithmetic/control-flow body can also run between the
/// driver's coordinate and framebuffer stages. Keep their persistent GRFs
/// inaccessible, including as sources, across every grid-stride iteration.
pub(crate) fn pixel_body_simd8(code: &[u8]) -> Option<&[u8]> {
    if !validate_simd8(code) {
        return None;
    }
    let body = &code[48..code.len() - 48];
    let allowed = |r: u32| (1..112).contains(&r) || r == 120;
    for bytes in body.chunks_exact(16) {
        let w = words(bytes);
        let op = w[0] & 0x7f;
        if matches!(op, 0x01 | 0x22 | 0x24 | 0x25 | 0x27 | 0x2e | 0x2f) {
            continue;
        }
        if !(op == 0x70 && w[1] >> 24 == 0) && !allowed(w[1] >> 24) {
            return None;
        }
        if op == 0x61 && w[1] & 0x4000 != 0 {
            continue;
        }
        if !allowed(w[2] >> 8 & 255)
            || (!matches!(op, 0x61 | 0x38) && w[1] & 0x8000 == 0 && !allowed(w[3] >> 8 & 255))
        {
            return None;
        }
    }
    Some(body)
}
