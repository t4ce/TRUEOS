use half::f16;
use std::env;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

const VALUES: usize = 32;
const TRACE_HEADER_BYTES: usize = 256;
const TRACE_RECORD_HEADER_BYTES: usize = 72;
const Q30_ONE: f32 = (1u64 << 30) as f32;

struct Case {
    id: u32,
    input: [i64; VALUES],
}

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let trace_path = args.next().ok_or("missing TGALDEC1 trace path")?;
    let output_path = args.next().ok_or("missing vector output path")?;
    if args.next().is_some() {
        return Err("usage: truega-q30-q8-vectors TRACE.bin OUTPUT.txt".into());
    }

    let trace = fs::read(&trace_path).map_err(|error| format!("read {trace_path}: {error}"))?;
    let mut cases = synthetic_cases();
    for (id, checkpoint) in [(100, 0usize), (101, 6), (102, 96), (103, 97)] {
        cases.push(Case {
            id,
            input: trace_block_as_q30(&trace, checkpoint)?,
        });
    }
    write_vectors(Path::new(&output_path), &cases)
}

fn synthetic_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    cases.push(Case {
        id: 0,
        input: [0; VALUES],
    });

    let mut ordinary = [0i64; VALUES];
    let one = 1i64 << 30;
    for (index, value) in ordinary.iter_mut().enumerate() {
        *value = match index % 8 {
            0 => one,
            1 => -one,
            2 => one / 2,
            3 => -(one / 2),
            4 => one / 127,
            5 => -(one / 127),
            6 => index as i64 * 1_000_003,
            _ => -(index as i64 * 999_983),
        };
    }
    cases.push(Case {
        id: 1,
        input: ordinary,
    });

    // max=254*2^20 makes odd multiples of 2^20 exact half-integer quants.
    let unit = 1i64 << 20;
    let maximum = 254 * unit;
    let tie_targets = [
        1i64, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 31,
    ];
    let mut ties = [0i64; VALUES];
    for index in 0..15 {
        ties[index] = tie_targets[index] * unit;
        ties[15 + index] = -tie_targets[index] * unit;
    }
    ties[30] = maximum;
    ties[31] = -maximum;
    cases.push(Case { id: 2, input: ties });

    let mut sparse = [0i64; VALUES];
    sparse[0] = 127 * 32; // scale=2^-25: binary16 tie rounds to zero.
    sparse[1] = -(127 * 32);
    cases.push(Case {
        id: 3,
        input: sparse,
    });

    let mut min_subnormal = [0i64; VALUES];
    min_subnormal[0] = 127 * 64; // scale=2^-24: minimum binary16 subnormal.
    min_subnormal[1] = -(127 * 64);
    cases.push(Case {
        id: 4,
        input: min_subnormal,
    });

    let mut carry = [0i64; VALUES];
    // scale=1.99951171875, exactly halfway to binary16 2.0; even is 2.0.
    carry[0] = 127 * ((2i64 << 30) - (1i64 << 19));
    carry[1] = -carry[0];
    cases.push(Case {
        id: 5,
        input: carry,
    });

    let mut signs = [0i64; VALUES];
    signs[0] = i64::MAX;
    signs[1] = -i64::MAX;
    signs[2] = i64::MIN;
    signs[3] = 1;
    signs[4] = -1;
    cases.push(Case {
        id: 6,
        input: signs,
    });

    let mut fp32_boundary = [0i64; VALUES];
    // The Rust/ggml boundary first casts Q30 i64 samples to F32. At this range,
    // +/-1 is below one F32 ULP: exact-rational hardware must not use it to move
    // a half-way quant away from ties-to-even.
    fp32_boundary[0] = 127i64 << 53;
    fp32_boundary[1] = (1i64 << 52) + 1;
    fp32_boundary[2] = -((1i64 << 52) + 1);
    cases.push(Case {
        id: 7,
        input: fp32_boundary,
    });
    cases
}

fn trace_block_as_q30(trace: &[u8], wanted: usize) -> Result<[i64; VALUES], String> {
    if trace.get(..8) != Some(b"TGALDEC1") || trace.len() < TRACE_HEADER_BYTES {
        return Err("invalid TGALDEC1 trace".into());
    }
    let records = read_u32(trace, 20)? as usize;
    if wanted >= records {
        return Err(format!("trace checkpoint {wanted} out of range {records}"));
    }
    let mut offset = TRACE_HEADER_BYTES;
    for index in 0..records {
        let elements = read_u32(trace, offset + 64)? as usize;
        let payload_bytes = read_u32(trace, offset + 68)? as usize;
        if payload_bytes != elements.checked_mul(4).ok_or("trace size overflow")? {
            return Err(format!("bad trace payload at checkpoint {index}"));
        }
        let payload = offset + TRACE_RECORD_HEADER_BYTES;
        let end = payload
            .checked_add(payload_bytes)
            .ok_or("trace offset overflow")?;
        if end > trace.len() {
            return Err(format!("truncated trace checkpoint {index}"));
        }
        if index == wanted {
            if elements < VALUES {
                return Err(format!("checkpoint {index} has only {elements} elements"));
            }
            let mut output = [0i64; VALUES];
            for (element, value) in output.iter_mut().enumerate() {
                let bits = read_u32(trace, payload + element * 4)?;
                let fp32 = f32::from_bits(bits);
                if !fp32.is_finite() {
                    return Err(format!("checkpoint {index} contains non-finite F32"));
                }
                *value = (fp32 * Q30_ONE).round_ties_even() as i64;
            }
            return Ok(output);
        }
        offset = end;
    }
    Err(format!("checkpoint {wanted} missing"))
}

fn reference(input: &[i64; VALUES]) -> (u16, [u8; VALUES]) {
    let mut fp32 = [0f32; VALUES];
    for (destination, source) in fp32.iter_mut().zip(input) {
        *destination = *source as f32 / Q30_ONE;
    }
    let maximum = fp32
        .iter()
        .fold(0.0f32, |current, value| current.max(value.abs()));
    let scale = maximum / 127.0;
    let inverse = if maximum == 0.0 { 0.0 } else { 127.0 / maximum };
    let mut quants = [0u8; VALUES];
    for (quant, value) in quants.iter_mut().zip(fp32) {
        *quant = (value * inverse).round_ties_even() as i8 as u8;
    }
    (f16::from_f32(scale).to_bits(), quants)
}

fn write_vectors(path: &Path, cases: &[Case]) -> Result<(), String> {
    let file = fs::File::create(path).map_err(|error| format!("create {}: {error}", path.display()))?;
    let mut output = BufWriter::new(file);
    writeln!(output, "{}", cases.len()).map_err(|error| error.to_string())?;
    for case in cases {
        let (scale, quants) = reference(&case.input);
        writeln!(output, "{} {:04x}", case.id, scale).map_err(|error| error.to_string())?;
        for quant in quants {
            write!(output, "{:02x} ", quant).map_err(|error| error.to_string())?;
        }
        writeln!(output).map_err(|error| error.to_string())?;
        for sample in case.input {
            write!(output, "{:016x} ", sample as u64).map_err(|error| error.to_string())?;
        }
        writeln!(output).map_err(|error| error.to_string())?;
    }
    output.flush().map_err(|error| error.to_string())
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let bytes: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or("truncated u32")?
        .try_into()
        .map_err(|_| "invalid u32")?;
    Ok(u32::from_le_bytes(bytes))
}
