//! Native-row AVX-VNNI projection kernel for the fixed LFM2.5 Q8_0 contract.
//!
//! One Q8_0 block is exactly one F16 scale followed by 32 signed bytes. The
//! activation uses the same block shape. AVX-VNNI supplies `u8 × i8 -> i32`,
//! so the signed activation is represented as an unsigned magnitude and its
//! sign is applied to the weight with `VPSIGNB`:
//!
//! `abs(q) * sign(weight, q) == q * weight`.
//!
//! The kernel deliberately keeps the eight dot4 results as eight F32 lanes
//! across all K blocks and applies the pinned LFM reduction tree only once at
//! the end of each output row.

use alloc::vec::Vec;
use half::f16;

use crate::{Error, Q8_BLOCK_BYTES, Q8_BLOCK_VALUES, q8_row_bytes, quantize_q8};

pub const Q8_VNNI_ROWS_PER_TILE: usize = 4;
const Q8_VNNI_F32_LANES: usize = 8;

const _: () = assert!(Q8_BLOCK_VALUES == 32);
const _: () = assert!(Q8_BLOCK_BYTES == 34);
const _: () = assert!(Q8_VNNI_F32_LANES * 4 == Q8_BLOCK_VALUES);

/// One contiguous output-row ownership range produced by
/// [`Q8VnniRowPlan::lower`].
///
/// A range never overlaps another range in the same plan.  Every range except
/// the final one ends on a native four-row VNNI tile boundary, so a future AP
/// pool can assign one range to one worker without changing the established
/// per-row reduction order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Q8VnniRowRange {
    first_row: usize,
    row_count: usize,
}

impl Q8VnniRowRange {
    pub const fn first_row(self) -> usize {
        self.first_row
    }

    pub const fn row_count(self) -> usize {
        self.row_count
    }

    pub const fn end_row(self) -> usize {
        self.first_row + self.row_count
    }
}

/// Lower one native Q8_0 projection into independent contiguous row ranges.
///
/// This is deliberately only a numerical-work plan: it does not select CPUs,
/// spawn tasks, or make a scheduling policy decision.  The kernel supplies
/// that policy and a bounded worker lease; callers then use the plan's ranges
/// with [`Q8VnniProjector::project_rows`].  That separation prevents a session
/// task-pool limit from being mistaken for intra-projection parallelism.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Q8VnniRowPlan {
    rows: usize,
    ranges: Vec<Q8VnniRowRange>,
}

impl Q8VnniRowPlan {
    /// Produce at most `worker_cap` ranges, further bounded by the number of
    /// native four-row tiles in `rows`.
    pub fn lower(rows: usize, worker_cap: usize) -> Result<Self, Error> {
        if rows == 0 || worker_cap == 0 {
            return Err(Error::Shape);
        }
        let tile_count = rows.div_ceil(Q8_VNNI_ROWS_PER_TILE);
        let range_count = core::cmp::min(worker_cap, tile_count);
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(range_count)
            .map_err(|_| Error::Allocation)?;

        let mut first_row = 0usize;
        let mut remaining_tiles = tile_count;
        for range_index in 0..range_count {
            let remaining_ranges = range_count - range_index;
            let tiles = remaining_tiles.div_ceil(remaining_ranges);
            let nominal_end = first_row
                .checked_add(
                    tiles
                        .checked_mul(Q8_VNNI_ROWS_PER_TILE)
                        .ok_or(Error::Shape)?,
                )
                .ok_or(Error::Shape)?;
            let end_row = core::cmp::min(nominal_end, rows);
            ranges.push(Q8VnniRowRange {
                first_row,
                row_count: end_row - first_row,
            });
            first_row = end_row;
            remaining_tiles -= tiles;
        }
        if first_row != rows || ranges.iter().any(|range| range.row_count == 0) {
            return Err(Error::Shape);
        }
        Ok(Self { rows, ranges })
    }

    pub const fn rows(&self) -> usize {
        self.rows
    }

    pub fn ranges(&self) -> &[Q8VnniRowRange] {
        &self.ranges
    }

    pub fn worker_count(&self) -> usize {
        self.ranges.len()
    }
}

/// CPU and OS state required by the native-row LFM Q8 projection kernel.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Q8VnniCapabilities {
    ymm_state: bool,
    avx2: bool,
    avx_vnni: bool,
    fma: bool,
}

impl Q8VnniCapabilities {
    pub const fn ymm_state(self) -> bool {
        self.ymm_state
    }

    pub const fn avx2(self) -> bool {
        self.avx2
    }

    pub const fn avx_vnni(self) -> bool {
        self.avx_vnni
    }

    pub const fn fma(self) -> bool {
        self.fma
    }

    pub const fn supported(self) -> bool {
        self.ymm_state && self.avx2 && self.avx_vnni && self.fma
    }
}

/// A runtime-admitted native Q8_0 projector.
///
/// Construction checks CPUID as well as the OS-owned XMM/YMM state. Safe entry
/// points revalidate the current worker before entering target-feature code, so
/// a future row-range scheduler cannot accidentally carry this handle onto an
/// inadmissible CPU.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Q8VnniProjector {
    capabilities: Q8VnniCapabilities,
}

impl Q8VnniProjector {
    pub fn detect() -> Result<Self, Error> {
        let capabilities = detect_q8_vnni_capabilities();
        if !capabilities.supported() {
            return Err(Error::UnsupportedCpu);
        }
        Ok(Self { capabilities })
    }

    pub const fn capabilities(self) -> Q8VnniCapabilities {
        self.capabilities
    }

    /// Project every row in one native Q8_0 matrix.
    pub fn project(
        self,
        matrix: &[u8],
        rows: usize,
        columns: usize,
        activation: &Q8VnniActivation,
        output: &mut [f32],
    ) -> Result<(), Error> {
        if output.len() != rows {
            return Err(Error::Shape);
        }
        self.project_rows(matrix, rows, columns, activation, 0, output)
    }

    /// Execute a previously lowered row plan synchronously.
    ///
    /// This is the single-worker fallback and validation path.  A pool-backed
    /// caller may instead hand each [`Q8VnniRowRange`] to a distinct worker and
    /// invoke [`Self::project_rows`] on that worker's disjoint output slice.
    /// Both paths retain the exact per-row numerical result.
    pub fn project_plan(
        self,
        matrix: &[u8],
        rows: usize,
        columns: usize,
        activation: &Q8VnniActivation,
        plan: &Q8VnniRowPlan,
        output: &mut [f32],
    ) -> Result<(), Error> {
        if plan.rows != rows || output.len() != rows {
            return Err(Error::Shape);
        }
        for range in plan.ranges() {
            let output_end = range.end_row();
            let output = output
                .get_mut(range.first_row()..output_end)
                .ok_or(Error::Shape)?;
            self.project_rows(matrix, rows, columns, activation, range.first_row(), output)?;
        }
        Ok(())
    }

    /// Project one contiguous output-row range.
    ///
    /// `output.len()` is the row count. This is the only partitioning surface
    /// later AP fan-out needs; the arithmetic and reduction contract are
    /// unchanged regardless of which worker owns the range.
    pub fn project_rows(
        self,
        matrix: &[u8],
        rows: usize,
        columns: usize,
        activation: &Q8VnniActivation,
        first_row: usize,
        output: &mut [f32],
    ) -> Result<(), Error> {
        let current_capabilities = detect_q8_vnni_capabilities();
        if !self.capabilities.supported() || !current_capabilities.supported() {
            return Err(Error::UnsupportedCpu);
        }
        if rows == 0 || output.is_empty() {
            return Err(Error::Shape);
        }
        let row_bytes = q8_row_bytes(columns)?;
        let matrix_bytes = rows.checked_mul(row_bytes).ok_or(Error::Shape)?;
        let last_row = first_row.checked_add(output.len()).ok_or(Error::Shape)?;
        if matrix.len() != matrix_bytes || last_row > rows || activation.columns() != columns {
            return Err(Error::Shape);
        }

        #[cfg(target_arch = "x86_64")]
        {
            // SAFETY: `detect()` proved AVX2, AVX-VNNI, FMA and enabled YMM
            // state on this CPU. Complete shape validation above bounds every
            // native row and every 32-byte block load.
            unsafe { project_rows_avx_vnni(matrix, row_bytes, activation, first_row, output) }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = (matrix, row_bytes, activation, first_row, output);
            Err(Error::UnsupportedCpu)
        }
    }
}

/// One dynamically quantized activation row plus its reusable unsigned bytes.
pub struct Q8VnniActivation {
    blocks: Vec<[u8; Q8_BLOCK_BYTES]>,
    magnitudes: Vec<[u8; Q8_BLOCK_VALUES]>,
    scales: Vec<f32>,
}

impl Q8VnniActivation {
    pub fn quantize(values: &[f32]) -> Result<Self, Error> {
        let blocks = quantize_q8(values)?;
        let mut magnitudes = Vec::new();
        let mut scales = Vec::new();
        magnitudes
            .try_reserve_exact(blocks.len())
            .map_err(|_| Error::Allocation)?;
        scales
            .try_reserve_exact(blocks.len())
            .map_err(|_| Error::Allocation)?;

        for block in &blocks {
            let scale = f16::from_bits(u16::from_le_bytes([block[0], block[1]])).to_f32();
            if !scale.is_finite() {
                return Err(Error::NonFinite);
            }
            scales.push(scale);
            let mut magnitude = [0u8; Q8_BLOCK_VALUES];
            for (destination, &encoded) in magnitude.iter_mut().zip(&block[2..]) {
                let quant = encoded as i8;
                if quant == i8::MIN {
                    return Err(Error::Encoding);
                }
                *destination = if quant < 0 {
                    (-i16::from(quant)) as u8
                } else {
                    quant as u8
                };
            }
            magnitudes.push(magnitude);
        }

        Ok(Self {
            blocks,
            magnitudes,
            scales,
        })
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    pub fn columns(&self) -> usize {
        self.blocks.len() * Q8_BLOCK_VALUES
    }

    pub fn native_blocks(&self) -> &[[u8; Q8_BLOCK_BYTES]] {
        &self.blocks
    }
}

/// Validate the one-time weight-side precondition required by `VPSIGNB`.
///
/// The sealed LFM image already records this invariant, but model admission
/// scans it once so the hot projection loop never pays for per-byte checks.
pub fn validate_q8_vnni_matrix(matrix: &[u8], rows: usize, columns: usize) -> Result<(), Error> {
    if rows == 0 {
        return Err(Error::Shape);
    }
    let row_bytes = q8_row_bytes(columns)?;
    if matrix.len() != rows.checked_mul(row_bytes).ok_or(Error::Shape)? {
        return Err(Error::Shape);
    }
    for block in matrix.chunks_exact(Q8_BLOCK_BYTES) {
        let scale = f16::from_bits(u16::from_le_bytes([block[0], block[1]])).to_f32();
        if !scale.is_finite() {
            return Err(Error::NonFinite);
        }
        if block[2..].contains(&0x80) {
            return Err(Error::Encoding);
        }
    }
    Ok(())
}

#[inline]
fn reduce_q8_lanes(lanes: [f32; Q8_VNNI_F32_LANES]) -> f32 {
    let a0 = lanes[0] + lanes[4];
    let a1 = lanes[1] + lanes[5];
    let a2 = lanes[2] + lanes[6];
    let a3 = lanes[3] + lanes[7];
    let b0 = a0 + a2;
    let b1 = a1 + a3;
    b0 + b1
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,avxvnni,fma")]
unsafe fn project_rows_avx_vnni(
    matrix: &[u8],
    row_bytes: usize,
    activation: &Q8VnniActivation,
    first_row: usize,
    output: &mut [f32],
) -> Result<(), Error> {
    use core::arch::x86_64::{
        __m256i, _mm256_cvtepi32_ps, _mm256_dpbusd_avx_epi32, _mm256_fmadd_ps, _mm256_loadu_si256,
        _mm256_set1_ps, _mm256_setzero_ps, _mm256_setzero_si256, _mm256_sign_epi8,
        _mm256_storeu_ps,
    };

    macro_rules! combined_scale {
        ($weight_block:expr, $activation_scale:expr) => {{
            let weight_block = $weight_block;
            let low = unsafe { *weight_block };
            let high = unsafe { *weight_block.add(1) };
            f16::from_bits(u16::from_le_bytes([low, high])).to_f32() * $activation_scale
        }};
    }

    macro_rules! update_row {
        ($accumulator:expr, $magnitude:expr, $activation_signs:expr, $weight_block:expr, $scale:expr) => {{
            let weight_block = $weight_block;
            let weights = unsafe { _mm256_loadu_si256(weight_block.add(2).cast::<__m256i>()) };
            let signed_weights = _mm256_sign_epi8(weights, $activation_signs);
            let dots = _mm256_dpbusd_avx_epi32(_mm256_setzero_si256(), $magnitude, signed_weights);
            let dots = _mm256_cvtepi32_ps(dots);
            _mm256_fmadd_ps(dots, _mm256_set1_ps($scale), $accumulator)
        }};
    }

    let mut relative_row = 0usize;
    while relative_row + Q8_VNNI_ROWS_PER_TILE <= output.len() {
        let row0 = unsafe { matrix.as_ptr().add((first_row + relative_row) * row_bytes) };
        let row1 = unsafe { row0.add(row_bytes) };
        let row2 = unsafe { row1.add(row_bytes) };
        let row3 = unsafe { row2.add(row_bytes) };
        let mut accumulator0 = _mm256_setzero_ps();
        let mut accumulator1 = _mm256_setzero_ps();
        let mut accumulator2 = _mm256_setzero_ps();
        let mut accumulator3 = _mm256_setzero_ps();

        for block_index in 0..activation.block_count() {
            let activation_block = &activation.blocks[block_index];
            let activation_signs =
                unsafe { _mm256_loadu_si256(activation_block.as_ptr().add(2).cast::<__m256i>()) };
            let magnitude = unsafe {
                _mm256_loadu_si256(
                    activation.magnitudes[block_index]
                        .as_ptr()
                        .cast::<__m256i>(),
                )
            };
            let activation_scale = activation.scales[block_index];
            let block_offset = block_index * Q8_BLOCK_BYTES;
            let weight0 = unsafe { row0.add(block_offset) };
            let weight1 = unsafe { row1.add(block_offset) };
            let weight2 = unsafe { row2.add(block_offset) };
            let weight3 = unsafe { row3.add(block_offset) };

            accumulator0 = update_row!(
                accumulator0,
                magnitude,
                activation_signs,
                weight0,
                combined_scale!(weight0, activation_scale)
            );
            accumulator1 = update_row!(
                accumulator1,
                magnitude,
                activation_signs,
                weight1,
                combined_scale!(weight1, activation_scale)
            );
            accumulator2 = update_row!(
                accumulator2,
                magnitude,
                activation_signs,
                weight2,
                combined_scale!(weight2, activation_scale)
            );
            accumulator3 = update_row!(
                accumulator3,
                magnitude,
                activation_signs,
                weight3,
                combined_scale!(weight3, activation_scale)
            );
        }

        let mut lanes0 = [0.0f32; Q8_VNNI_F32_LANES];
        let mut lanes1 = [0.0f32; Q8_VNNI_F32_LANES];
        let mut lanes2 = [0.0f32; Q8_VNNI_F32_LANES];
        let mut lanes3 = [0.0f32; Q8_VNNI_F32_LANES];
        unsafe {
            _mm256_storeu_ps(lanes0.as_mut_ptr(), accumulator0);
            _mm256_storeu_ps(lanes1.as_mut_ptr(), accumulator1);
            _mm256_storeu_ps(lanes2.as_mut_ptr(), accumulator2);
            _mm256_storeu_ps(lanes3.as_mut_ptr(), accumulator3);
        }
        for (destination, lanes) in output[relative_row..relative_row + 4]
            .iter_mut()
            .zip([lanes0, lanes1, lanes2, lanes3])
        {
            let value = reduce_q8_lanes(lanes);
            if !value.is_finite() {
                return Err(Error::NonFinite);
            }
            *destination = value;
        }
        relative_row += Q8_VNNI_ROWS_PER_TILE;
    }

    while relative_row < output.len() {
        let row = unsafe { matrix.as_ptr().add((first_row + relative_row) * row_bytes) };
        let mut accumulator = _mm256_setzero_ps();
        for block_index in 0..activation.block_count() {
            let activation_block = &activation.blocks[block_index];
            let activation_signs =
                unsafe { _mm256_loadu_si256(activation_block.as_ptr().add(2).cast::<__m256i>()) };
            let magnitude = unsafe {
                _mm256_loadu_si256(
                    activation.magnitudes[block_index]
                        .as_ptr()
                        .cast::<__m256i>(),
                )
            };
            let activation_scale = activation.scales[block_index];
            let weight = unsafe { row.add(block_index * Q8_BLOCK_BYTES) };
            accumulator = update_row!(
                accumulator,
                magnitude,
                activation_signs,
                weight,
                combined_scale!(weight, activation_scale)
            );
        }
        let mut lanes = [0.0f32; Q8_VNNI_F32_LANES];
        unsafe { _mm256_storeu_ps(lanes.as_mut_ptr(), accumulator) };
        let value = reduce_q8_lanes(lanes);
        if !value.is_finite() {
            return Err(Error::NonFinite);
        }
        output[relative_row] = value;
        relative_row += 1;
    }
    Ok(())
}

#[cfg(target_arch = "x86_64")]
fn detect_q8_vnni_capabilities() -> Q8VnniCapabilities {
    use core::arch::x86_64::{__cpuid, __cpuid_count};

    const CPUID_1_ECX_FMA: u32 = 1 << 12;
    const CPUID_1_ECX_OSXSAVE: u32 = 1 << 27;
    const CPUID_1_ECX_AVX: u32 = 1 << 28;
    const CPUID_7_0_EBX_AVX2: u32 = 1 << 5;
    const CPUID_7_1_EAX_AVX_VNNI: u32 = 1 << 4;
    const XCR0_XMM_YMM: u64 = (1 << 1) | (1 << 2);

    let maximum_leaf = __cpuid(0).eax;
    if maximum_leaf < 1 {
        return Q8VnniCapabilities::default();
    }
    let leaf1 = __cpuid(1);
    let avx_state_contract = CPUID_1_ECX_OSXSAVE | CPUID_1_ECX_AVX;
    if leaf1.ecx & avx_state_contract != avx_state_contract {
        return Q8VnniCapabilities::default();
    }
    let xcr0 = unsafe { read_xcr0() };
    if xcr0 & XCR0_XMM_YMM != XCR0_XMM_YMM || maximum_leaf < 7 {
        return Q8VnniCapabilities::default();
    }

    let leaf7 = __cpuid_count(7, 0);
    let avx2 = leaf7.ebx & CPUID_7_0_EBX_AVX2 != 0;
    let avx_vnni = avx2 && leaf7.eax >= 1 && __cpuid_count(7, 1).eax & CPUID_7_1_EAX_AVX_VNNI != 0;
    Q8VnniCapabilities {
        ymm_state: true,
        avx2,
        avx_vnni,
        fma: leaf1.ecx & CPUID_1_ECX_FMA != 0,
    }
}

#[cfg(not(target_arch = "x86_64"))]
const fn detect_q8_vnni_capabilities() -> Q8VnniCapabilities {
    Q8VnniCapabilities {
        ymm_state: false,
        avx2: false,
        avx_vnni: false,
        fma: false,
    }
}

#[cfg(target_arch = "x86_64")]
#[inline]
unsafe fn read_xcr0() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!(
            "xgetbv",
            in("ecx") 0u32,
            out("eax") low,
            out("edx") high,
            options(nostack, preserves_flags),
        );
    }
    (u64::from(high) << 32) | u64::from(low)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    fn native_matrix(rows: usize, columns: usize) -> Vec<u8> {
        let row_bytes = q8_row_bytes(columns).unwrap();
        let mut matrix = vec![0u8; rows * row_bytes];
        let mut state = 0x6a09_e667_f3bc_c909u64;
        for row in 0..rows {
            for block in 0..columns / Q8_BLOCK_VALUES {
                let offset = row * row_bytes + block * Q8_BLOCK_BYTES;
                let scale_bits = match (row + block) % 5 {
                    0 => f16::from_f32(0.0).to_bits(),
                    1 => f16::from_bits(1).to_bits(),
                    2 => f16::from_f32(0.000_976_562_5).to_bits(),
                    3 => f16::from_f32(0.125).to_bits(),
                    _ => f16::from_f32(1.75).to_bits(),
                };
                matrix[offset..offset + 2].copy_from_slice(&scale_bits.to_le_bytes());
                for destination in &mut matrix[offset + 2..offset + Q8_BLOCK_BYTES] {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    *destination = (((state % 255) as i16) - 127) as i8 as u8;
                }
            }
        }
        matrix
    }

    fn input(columns: usize) -> Vec<f32> {
        (0..columns)
            .map(|index| {
                let centered = (index.wrapping_mul(97).wrapping_add(31) % 251) as i32 - 125;
                centered as f32 * 0.007_812_5
            })
            .collect()
    }

    #[test]
    fn fixed_reduction_tree_is_observable() {
        let lanes = [-3.0, 1.0, -0.000_01, -0.5, 1.0e20, -1.0e20, -0.000_01, -0.5];
        assert_eq!(reduce_q8_lanes(lanes).to_bits(), 0.0f32.to_bits());

        let sequential = lanes.into_iter().fold(0.0f32, |sum, value| sum + value);
        assert_ne!(sequential.to_bits(), 0.0f32.to_bits());
    }

    #[test]
    fn activation_quantization_excludes_signed_byte_minimum() {
        let values: Vec<f32> = (0..Q8_BLOCK_VALUES)
            .map(|index| match index % 5 {
                0 => -127.0,
                1 => 127.0,
                2 => -0.5,
                3 => 0.0,
                _ => 0.5,
            })
            .collect();
        let activation = Q8VnniActivation::quantize(&values).unwrap();
        for (block, magnitudes) in activation
            .native_blocks()
            .iter()
            .zip(&activation.magnitudes)
        {
            for (&encoded, &magnitude) in block[2..].iter().zip(magnitudes) {
                let quant = encoded as i8;
                assert_ne!(quant, i8::MIN);
                assert_eq!(magnitude, quant.unsigned_abs());
            }
        }
    }

    #[test]
    fn signed_magnitude_mapping_is_exhaustive_over_the_admitted_domain() {
        for activation in -127i16..=127 {
            for weight in -127i16..=127 {
                let magnitude = activation.unsigned_abs() as i32;
                let signed_weight = if activation < 0 {
                    -weight
                } else if activation == 0 {
                    0
                } else {
                    weight
                };
                assert_eq!(magnitude * i32::from(signed_weight), i32::from(activation * weight));
            }
        }
    }

    #[test]
    fn one_time_matrix_admission_rejects_signed_byte_minimum() {
        let mut matrix = native_matrix(1, 32);
        matrix[2] = 0x80;
        assert_eq!(validate_q8_vnni_matrix(&matrix, 1, 32), Err(Error::Encoding));
    }

    #[test]
    fn every_available_vnni_tile_matches_the_pinned_scalar_oracle_bitwise() {
        let Ok(projector) = Q8VnniProjector::detect() else {
            return;
        };
        for (rows, columns) in [(1, 1024), (4, 1024), (7, 1024), (5, 4608)] {
            let matrix = native_matrix(rows, columns);
            validate_q8_vnni_matrix(&matrix, rows, columns).unwrap();
            let input = input(columns);
            let expected =
                crate::q8_matrix_vector_quantized(&matrix, rows, columns, &input).unwrap();
            let activation = Q8VnniActivation::quantize(&input).unwrap();
            let mut observed = vec![0.0f32; rows];
            projector
                .project(&matrix, rows, columns, &activation, &mut observed)
                .unwrap();
            assert_eq!(
                observed
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                expected
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                "rows={rows} columns={columns}"
            );
        }
    }

    #[test]
    fn row_range_surface_preserves_global_row_order() {
        let Ok(projector) = Q8VnniProjector::detect() else {
            return;
        };
        let rows = 9;
        let columns = 1024;
        let matrix = native_matrix(rows, columns);
        let input = input(columns);
        let activation = Q8VnniActivation::quantize(&input).unwrap();
        let mut all = vec![0.0f32; rows];
        projector
            .project(&matrix, rows, columns, &activation, &mut all)
            .unwrap();
        let mut middle = vec![0.0f32; 5];
        projector
            .project_rows(&matrix, rows, columns, &activation, 2, &mut middle)
            .unwrap();
        assert_eq!(
            middle
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            all[2..7]
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn row_plan_is_contiguous_and_tile_aligned_before_the_tail() {
        let plan = Q8VnniRowPlan::lower(17, 3).unwrap();
        assert_eq!(plan.rows(), 17);
        assert_eq!(plan.worker_count(), 3);
        assert_eq!(
            plan.ranges(),
            &[
                Q8VnniRowRange {
                    first_row: 0,
                    row_count: 8,
                },
                Q8VnniRowRange {
                    first_row: 8,
                    row_count: 8,
                },
                Q8VnniRowRange {
                    first_row: 16,
                    row_count: 1,
                },
            ]
        );
        assert!(
            plan.ranges()
                .iter()
                .take(plan.worker_count() - 1)
                .all(|range| range.end_row().is_multiple_of(Q8_VNNI_ROWS_PER_TILE))
        );
    }

    #[test]
    fn row_plan_caps_to_available_native_tiles() {
        let plan = Q8VnniRowPlan::lower(3, 32).unwrap();
        assert_eq!(plan.worker_count(), 1);
        assert_eq!(plan.ranges()[0].first_row(), 0);
        assert_eq!(plan.ranges()[0].row_count(), 3);
        assert_eq!(Q8VnniRowPlan::lower(0, 1), Err(Error::Shape));
        assert_eq!(Q8VnniRowPlan::lower(1, 0), Err(Error::Shape));
    }

    #[test]
    fn serial_row_plan_keeps_the_projection_oracle_bits() {
        let Ok(projector) = Q8VnniProjector::detect() else {
            return;
        };
        let rows = 9;
        let columns = 1024;
        let matrix = native_matrix(rows, columns);
        let input = input(columns);
        let activation = Q8VnniActivation::quantize(&input).unwrap();
        let mut direct = vec![0.0f32; rows];
        projector
            .project(&matrix, rows, columns, &activation, &mut direct)
            .unwrap();
        let mut planned = vec![0.0f32; rows];
        let plan = Q8VnniRowPlan::lower(rows, 3).unwrap();
        projector
            .project_plan(&matrix, rows, columns, &activation, &plan, &mut planned)
            .unwrap();
        assert_eq!(
            planned
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            direct
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
    }
}
