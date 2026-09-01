// TRUEOS Intel entropy research probe.
//
// This WGSL is a mathematical/oracle lane, not the production kernel ABI.
// One workgroup owns one independent input chunk. Thirty-two invocations
// deliberately mirror the planned 32-state entropy decomposition while the
// production ADL-S artifact remains free to use the repo's required SIMD16
// hardware-thread ABI and multiple hardware threads per chunk.
//
// Binding 0 is a little-endian u32 view of input bytes. The caller must pad
// the backing allocation to a full u32; `input_bytes` remains the true length.
// Binding 1 is a u32 result buffer with OUTPUT_STRIDE_WORDS words per chunk.
// Binding 2 is Params.

const MAGIC_EPR1: u32 = 0x31525045u; // "EPR1" little endian
const LANES: u32 = 32u;
const HISTOGRAM_WORDS: u32 = 256u;
const BITPLANE_WORDS: u32 = 8u;
const CONTEXT2_WORDS: u32 = 8u;   // four 2-bit contexts x {0,1}
const CONTEXT4_WORDS: u32 = 32u;  // sixteen 4-bit contexts x {0,1}
const SUMMARY_WORDS: u32 = 16u;
const HISTOGRAM_BASE: u32 = SUMMARY_WORDS;
const BITPLANE_BASE: u32 = HISTOGRAM_BASE + HISTOGRAM_WORDS;
const CONTEXT2_BASE: u32 = BITPLANE_BASE + BITPLANE_WORDS;
const CONTEXT4_BASE: u32 = CONTEXT2_BASE + CONTEXT2_WORDS;
const OUTPUT_STRIDE_WORDS: u32 = CONTEXT4_BASE + CONTEXT4_WORDS;

struct Params {
    input_bytes: u32,
    chunk_bytes: u32,
    chunk_count: u32,
    flags: u32,
};

@group(0) @binding(0)
var<storage, read> input_words: array<u32>;

@group(0) @binding(1)
var<storage, read_write> output_words: array<u32>;

@group(0) @binding(2)
var<uniform> params: Params;

var<workgroup> histogram: array<atomic<u32>, 256>;
var<workgroup> bitplane_ones: array<atomic<u32>, 8>;
var<workgroup> context2_counts: array<atomic<u32>, 8>;
var<workgroup> context4_counts: array<atomic<u32>, 32>;
var<workgroup> byte_sum: atomic<u32>;
var<workgroup> byte_xor: atomic<u32>;

fn load_byte(byte_index: u32) -> u32 {
    let word = input_words[byte_index >> 2u];
    let shift = (byte_index & 3u) << 3u;
    return (word >> shift) & 0xffu;
}

fn load_bit(bit_index: u32) -> u32 {
    let byte_index = bit_index >> 3u;
    let bit_in_byte = bit_index & 7u;
    let value = load_byte(byte_index);
    return (value >> (7u - bit_in_byte)) & 1u;
}

fn binary_entropy_bits(ones: u32, total: u32) -> f32 {
    if (total == 0u || ones == 0u || ones == total) {
        return 0.0;
    }
    let p = f32(ones) / f32(total);
    let q = 1.0 - p;
    return -f32(ones) * log2(p) - f32(total - ones) * log2(q);
}

fn markov_bits_2() -> f32 {
    var bits = 0.0;
    for (var context = 0u; context < 4u; context = context + 1u) {
        let zeroes = atomicLoad(&context2_counts[context * 2u]);
        let ones = atomicLoad(&context2_counts[context * 2u + 1u]);
        bits = bits + binary_entropy_bits(ones, zeroes + ones);
    }
    return bits;
}

fn markov_bits_4() -> f32 {
    var bits = 0.0;
    for (var context = 0u; context < 16u; context = context + 1u) {
        let zeroes = atomicLoad(&context4_counts[context * 2u]);
        let ones = atomicLoad(&context4_counts[context * 2u + 1u]);
        bits = bits + binary_entropy_bits(ones, zeroes + ones);
    }
    return bits;
}

@compute @workgroup_size(32, 1, 1)
fn entropy_probe(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let chunk = workgroup_id.x;
    let lane = local_id.x;
    if (chunk >= params.chunk_count || params.chunk_bytes == 0u) {
        return;
    }

    for (var bin = lane; bin < 256u; bin = bin + LANES) {
        atomicStore(&histogram[bin], 0u);
    }
    if (lane < 8u) {
        atomicStore(&bitplane_ones[lane], 0u);
        atomicStore(&context2_counts[lane], 0u);
    }
    if (lane < 32u) {
        atomicStore(&context4_counts[lane], 0u);
    }
    if (lane == 0u) {
        atomicStore(&byte_sum, 0u);
        atomicStore(&byte_xor, 0u);
    }
    workgroupBarrier();

    let chunk_start = chunk * params.chunk_bytes;
    let chunk_end = min(chunk_start + params.chunk_bytes, params.input_bytes);
    var byte_index = chunk_start + lane;
    while (byte_index < chunk_end) {
        let value = load_byte(byte_index);
        atomicAdd(&histogram[value], 1u);
        atomicAdd(&byte_sum, value);
        atomicXor(&byte_xor, value);

        for (var bit_in_byte = 0u; bit_in_byte < 8u; bit_in_byte = bit_in_byte + 1u) {
            let symbol = (value >> (7u - bit_in_byte)) & 1u;
            atomicAdd(&bitplane_ones[bit_in_byte], symbol);

            let local_bit = (byte_index - chunk_start) * 8u + bit_in_byte;
            let global_bit = byte_index * 8u + bit_in_byte;
            if (local_bit >= 2u) {
                let context2 = (load_bit(global_bit - 2u) << 1u) | load_bit(global_bit - 1u);
                atomicAdd(&context2_counts[context2 * 2u + symbol], 1u);
            }
            if (local_bit >= 4u) {
                let context4 =
                    (load_bit(global_bit - 4u) << 3u) |
                    (load_bit(global_bit - 3u) << 2u) |
                    (load_bit(global_bit - 2u) << 1u) |
                    load_bit(global_bit - 1u);
                atomicAdd(&context4_counts[context4 * 2u + symbol], 1u);
            }
        }
        byte_index = byte_index + LANES;
    }
    workgroupBarrier();

    if (lane == 0u) {
        let output_base = chunk * OUTPUT_STRIDE_WORDS;
        let bytes = chunk_end - chunk_start;
        var unique = 0u;
        var h0_bits = 0.0;
        for (var bin = 0u; bin < 256u; bin = bin + 1u) {
            let count = atomicLoad(&histogram[bin]);
            output_words[output_base + HISTOGRAM_BASE + bin] = count;
            if (count != 0u) {
                unique = unique + 1u;
                let p = f32(count) / f32(bytes);
                h0_bits = h0_bits - f32(count) * log2(p);
            }
        }

        var bitplane_bound = 0.0;
        for (var bit = 0u; bit < 8u; bit = bit + 1u) {
            let ones = atomicLoad(&bitplane_ones[bit]);
            output_words[output_base + BITPLANE_BASE + bit] = ones;
            bitplane_bound = bitplane_bound + binary_entropy_bits(ones, bytes);
        }
        for (var index = 0u; index < CONTEXT2_WORDS; index = index + 1u) {
            output_words[output_base + CONTEXT2_BASE + index] = atomicLoad(&context2_counts[index]);
        }
        for (var index = 0u; index < CONTEXT4_WORDS; index = index + 1u) {
            output_words[output_base + CONTEXT4_BASE + index] = atomicLoad(&context4_counts[index]);
        }

        // Summary floats are stored as IEEE-754 bit patterns. They are model
        // scores, not container sizes: the host reference computes exact
        // combinatorial ranks and accounts for model metadata separately.
        output_words[output_base + 0u] = MAGIC_EPR1;
        output_words[output_base + 1u] = bytes;
        output_words[output_base + 2u] = unique;
        output_words[output_base + 3u] = atomicLoad(&histogram[0u]);
        output_words[output_base + 4u] = atomicLoad(&histogram[255u]);
        output_words[output_base + 5u] = bytes * 8u;
        output_words[output_base + 6u] = bitcast<u32>(h0_bits);
        output_words[output_base + 7u] = bitcast<u32>(bitplane_bound);
        output_words[output_base + 8u] = bitcast<u32>(markov_bits_2());
        output_words[output_base + 9u] = bitcast<u32>(markov_bits_4());
        output_words[output_base + 10u] = 0x0000000fu; // raw/enum/ctx2/ctx4 candidates
        output_words[output_base + 11u] = params.flags;
        var first_byte = 0u;
        var last_byte = 0u;
        if (bytes != 0u) {
            first_byte = load_byte(chunk_start);
            last_byte = load_byte(chunk_end - 1u);
        }
        output_words[output_base + 12u] = first_byte;
        output_words[output_base + 13u] = last_byte;
        output_words[output_base + 14u] = atomicLoad(&byte_sum);
        output_words[output_base + 15u] = atomicLoad(&byte_xor);
    }
}
