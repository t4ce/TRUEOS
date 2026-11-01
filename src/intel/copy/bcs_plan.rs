//! Pure planning for ordered linear RGBA BLTs on the single GuC BCS0 lane.
//!
//! A plan owns descriptors, not the allocations they reference. The caller must
//! retain their backing through exact retirement, including cancellation and
//! ambiguous-submission quarantine. Planning never mutates GPU storage.

use alloc::vec::Vec;

use super::blt::{
    GucBcs0CopySubmitError, GucBcs0RgbaCopy, GucBcs0RgbaFill, GucBcs0RgbaSurface,
    guc_blt_source_mappings_compatible, guc_blt_valid_copy, guc_blt_valid_fill_rect,
    guc_blt_valid_legacy_copy, guc_blt_valid_surface,
};

pub(crate) const BCS_PLAN_BATCH_BYTES: usize = 8192;
pub(crate) const BCS_PLAN_ENVELOPE_DWORDS: usize = 12 + 8;
pub(crate) const BCS_PLAN_MAX_OPERATIONS: usize = 160;
const BODY_DWORDS: usize = BCS_PLAN_BATCH_BYTES / 4 - BCS_PLAN_ENVELOPE_DWORDS;
const MI_FLUSH_DW: u32 = (0x26 << 23) | 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceCache {
    WriteBack,
    ScanoutUncached,
}

impl SourceCache {
    pub(crate) fn is_uncached(self) -> bool {
        self == Self::ScanoutUncached
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CopyBackend {
    /// Preserve the warmed glyph path's legacy SRCCOPY specialization.
    Legacy,
    /// Use fast copy when its alignment and coordinate constraints are met;
    /// otherwise use legal legacy SRCCOPY without modifying the rectangle.
    PreferFast,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BcsOp {
    Fill(GucBcs0RgbaFill),
    Copy {
        copy: GucBcs0RgbaCopy,
        backend: CopyBackend,
        source_cache: SourceCache,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Fill(GucBcs0RgbaFill),
    LegacyCopy(GucBcs0RgbaCopy),
    FastCopy(GucBcs0RgbaCopy),
}

impl Operation {
    fn dwords(self) -> usize {
        match self {
            Self::Fill(_) => 11,
            Self::LegacyCopy(_) | Self::FastCopy(_) => 10,
        }
    }

    fn bytes(self) -> u64 {
        let (width, height) = match self {
            Self::Fill(fill) => (fill.width, fill.height),
            Self::LegacyCopy(copy) | Self::FastCopy(copy) => (copy.width, copy.height),
        };
        u64::from(width) * u64::from(height) * 4
    }

    fn fast_source(self) -> Option<FastSource> {
        match self {
            Self::Fill(fill) => Some(FastSource::Color(fill.color)),
            Self::FastCopy(copy) => Some(FastSource::Surface(copy.source)),
            Self::LegacyCopy(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FastSource {
    Color(u32),
    Surface(GucBcs0RgbaSurface),
}

#[derive(Clone, Copy, Debug)]
struct PlannedOperation {
    operation: Operation,
    flush_before: bool,
}

#[derive(Debug)]
pub(crate) struct BcsPlan {
    batches: Vec<BcsBatch>,
}

impl BcsPlan {
    pub(crate) fn new(
        destination: GucBcs0RgbaSurface,
        ops: &[BcsOp],
    ) -> Result<Self, GucBcs0CopySubmitError> {
        let invalid = GucBcs0CopySubmitError::InvalidRequest;
        if !guc_blt_valid_surface(destination) {
            return Err(invalid);
        }
        let mut mappings = Vec::new();
        let mut resolved = Vec::with_capacity(ops.len());
        // Validate the complete request before returning any batch. Even
        // mappings used in different chunks must describe stable live sources.
        for &op in ops {
            let operation = match op {
                BcsOp::Fill(fill) => {
                    if !guc_blt_valid_fill_rect(destination, fill)
                        || fill.x.checked_add(fill.width).is_none_or(|x| x > 16384)
                        || fill.y.checked_add(fill.height).is_none_or(|y| y > 16384)
                    {
                        return Err(invalid);
                    }
                    Operation::Fill(fill)
                }
                BcsOp::Copy {
                    copy,
                    backend,
                    source_cache,
                } => {
                    if !guc_blt_valid_copy(destination, copy) {
                        return Err(invalid);
                    }
                    add_mapping(&mut mappings, copy.source, source_cache)?;
                    match backend {
                        CopyBackend::PreferFast if fast_copy_eligible(destination, copy) => {
                            Operation::FastCopy(copy)
                        }
                        _ if guc_blt_valid_legacy_copy(destination, copy) => {
                            Operation::LegacyCopy(copy)
                        }
                        _ => return Err(invalid),
                    }
                }
            };
            resolved.push((operation, op));
        }

        let mut batches = Vec::new();
        let mut batch = BcsBatch::new(destination);
        for (operation, original) in resolved {
            let mut flush_before = batch.needs_flush(operation);
            let mut added_dwords = operation.dwords() + usize::from(flush_before) * 5;
            if batch.operation_count() == BCS_PLAN_MAX_OPERATIONS
                || batch.encoded_dwords + added_dwords > BODY_DWORDS
            {
                batches.push(batch);
                batch = BcsBatch::new(destination);
                // The preceding batch's ordered completion flush drains all
                // commands. No internal barrier is needed at the next start.
                flush_before = false;
                added_dwords = operation.dwords();
            }
            if let BcsOp::Copy {
                copy, source_cache, ..
            } = original
            {
                add_mapping(&mut batch.source_mappings, copy.source, source_cache)?;
            }
            batch.encoded_dwords += added_dwords;
            batch.copied_bytes += operation.bytes();
            batch.explicit_flushes += usize::from(flush_before);
            batch.last_fast_source = operation.fast_source();
            batch.operations.push(PlannedOperation {
                operation,
                flush_before,
            });
        }
        if batch.operation_count() != 0 {
            batches.push(batch);
        }
        Ok(Self { batches })
    }

    pub(crate) fn batches(&self) -> &[BcsBatch] {
        &self.batches
    }
}

/// One bounded, fully validated command body and its unique source mappings.
/// The hardware backend adds the TLB/entry and exact-completion envelope.
#[derive(Debug)]
pub(crate) struct BcsBatch {
    destination: GucBcs0RgbaSurface,
    source_mappings: Vec<(GucBcs0RgbaSurface, SourceCache)>,
    operations: Vec<PlannedOperation>,
    last_fast_source: Option<FastSource>,
    encoded_dwords: usize,
    copied_bytes: u64,
    explicit_flushes: usize,
}

impl BcsBatch {
    fn new(destination: GucBcs0RgbaSurface) -> Self {
        Self {
            destination,
            source_mappings: Vec::new(),
            operations: Vec::new(),
            last_fast_source: None,
            encoded_dwords: 0,
            copied_bytes: 0,
            explicit_flushes: 0,
        }
    }

    fn needs_flush(&self, operation: Operation) -> bool {
        // TGL Vol 10 p.38 requires a flush for different fast-copy sources
        // writing the same destination. Conservatively apply the boundary to
        // every change of fast source, including inline colors. This also
        // drains older outstanding writes, rather than checking just the last
        // pair of rectangles. Legacy/fast transitions already flush in HW
        // (Vol 2a pp.1367,1371), so a legacy operation resets this frontier.
        match (self.last_fast_source, operation.fast_source()) {
            (Some(previous), Some(next)) => previous != next,
            _ => false,
        }
    }

    pub(crate) fn destination(&self) -> GucBcs0RgbaSurface {
        self.destination
    }

    pub(crate) fn source_mappings(&self) -> &[(GucBcs0RgbaSurface, SourceCache)] {
        &self.source_mappings
    }

    pub(crate) fn operation_count(&self) -> usize {
        self.operations.len()
    }

    pub(crate) fn copied_bytes(&self) -> u64 {
        self.copied_bytes
    }

    pub(crate) fn copy_count(&self) -> usize {
        self.operations
            .iter()
            .filter(|op| !matches!(op.operation, Operation::Fill(_)))
            .count()
    }

    pub(crate) fn fill_count(&self) -> usize {
        self.operation_count() - self.copy_count()
    }

    pub(crate) fn explicit_flushes(&self) -> usize {
        self.explicit_flushes
    }

    /// Body size, including internal flushes; excludes the 20-DWORD envelope.
    pub(crate) fn encoded_dwords(&self) -> usize {
        self.encoded_dwords
    }

    /// Encode only after checking the entire destination slice fits. This
    /// method is pure with respect to GPU state and leaves short slices intact.
    pub(crate) fn encode_body(&self, output: &mut [u32]) -> Option<usize> {
        if output.len() < self.encoded_dwords {
            return None;
        }
        let destination = self.destination;
        let mut cursor = 0;
        for &PlannedOperation {
            operation,
            flush_before,
        } in &self.operations
        {
            if flush_before {
                output[cursor..cursor + 5].copy_from_slice(&[MI_FLUSH_DW, 0, 0, 0, 0]);
                cursor += 5;
            }
            match operation {
                Operation::Fill(fill) => {
                    output[cursor..cursor + 11].copy_from_slice(&[
                        (2 << 29) | (0x44 << 22) | (2 << 19) | 9,
                        destination.pitch_bytes - 1,
                        fill.x | (fill.y << 16),
                        (fill.x + fill.width) | ((fill.y + fill.height) << 16),
                        destination.gpu as u32,
                        (destination.gpu >> 32) as u32,
                        0,
                        fill.color,
                        0,
                        0,
                        0,
                    ]);
                    cursor += 11;
                }
                Operation::FastCopy(copy) | Operation::LegacyCopy(copy) => {
                    let legacy = matches!(operation, Operation::LegacyCopy(_));
                    output[cursor..cursor + 10].copy_from_slice(&[
                        if legacy {
                            (2 << 29) | (0x53 << 22) | (3 << 20) | 8
                        } else {
                            (2 << 29) | (0x42 << 22) | 8
                        },
                        destination.pitch_bytes
                            | if legacy {
                                (3 << 24) | (0xCC << 16)
                            } else {
                                3 << 24
                            },
                        copy.destination_x | (copy.destination_y << 16),
                        (copy.destination_x + copy.width)
                            | ((copy.destination_y + copy.height) << 16),
                        destination.gpu as u32,
                        (destination.gpu >> 32) as u32,
                        copy.source_x | (copy.source_y << 16),
                        copy.source.pitch_bytes,
                        copy.source.gpu as u32,
                        (copy.source.gpu >> 32) as u32,
                    ]);
                    cursor += 10;
                }
            }
        }
        debug_assert_eq!(cursor, self.encoded_dwords);
        Some(cursor)
    }
}

fn fast_copy_eligible(destination: GucBcs0RgbaSurface, copy: GucBcs0RgbaCopy) -> bool {
    // Vol 2a pp.1371–1373 specify OWord-aligned source/destination starts.
    // Keep every row aligned and limit this initial path to positive signed
    // coordinates. PreferFast may fall back without changing coverage.
    copy.source_x.is_multiple_of(4)
        && copy.destination_x.is_multiple_of(4)
        && copy.source.pitch_bytes.is_multiple_of(16)
        && destination.pitch_bytes.is_multiple_of(16)
        && copy.source_x + copy.width <= i16::MAX as u32
        && copy.source_y + copy.height <= i16::MAX as u32
        && copy.destination_x + copy.width <= i16::MAX as u32
        && copy.destination_y + copy.height <= i16::MAX as u32
}

fn same_mapping(first: GucBcs0RgbaSurface, second: GucBcs0RgbaSurface) -> bool {
    first.gpu == second.gpu
        && first.phys == second.phys
        && first.bytes.div_ceil(4096) == second.bytes.div_ceil(4096)
}

fn add_mapping(
    mappings: &mut Vec<(GucBcs0RgbaSurface, SourceCache)>,
    surface: GucBcs0RgbaSurface,
    cache: SourceCache,
) -> Result<(), GucBcs0CopySubmitError> {
    let mut duplicate = false;
    for &(previous, previous_cache) in mappings.iter() {
        if !guc_blt_source_mappings_compatible(
            surface,
            cache.is_uncached(),
            previous,
            previous_cache.is_uncached(),
        ) {
            return Err(GucBcs0CopySubmitError::InvalidRequest);
        }
        duplicate |= same_mapping(surface, previous) && cache == previous_cache;
    }
    if !duplicate {
        mappings.push((surface, cache));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn surface(gpu: u64) -> GucBcs0RgbaSurface {
        GucBcs0RgbaSurface {
            phys: gpu + 0x1_0000_0000,
            gpu,
            bytes: 64 * 256,
            width: 64,
            height: 64,
            pitch_bytes: 256,
        }
    }

    fn copy(source: GucBcs0RgbaSurface, backend: CopyBackend) -> BcsOp {
        BcsOp::Copy {
            copy: GucBcs0RgbaCopy {
                source,
                source_x: 0,
                source_y: 0,
                destination_x: 0,
                destination_y: 0,
                width: 16,
                height: 16,
            },
            backend,
            source_cache: SourceCache::WriteBack,
        }
    }

    fn fill(color: u32) -> BcsOp {
        BcsOp::Fill(GucBcs0RgbaFill {
            x: 0,
            y: 0,
            width: 32,
            height: 32,
            color,
        })
    }

    fn body(batch: &BcsBatch) -> Vec<u32> {
        let mut words = vec![0xdead_beef; batch.encoded_dwords()];
        assert_eq!(batch.encode_body(&mut words), Some(words.len()));
        words
    }

    fn opcode(word: u32) -> u32 {
        (word >> 22) & 0x7f
    }

    #[test]
    fn mixed_fill_and_legacy_glyphs_share_one_ordered_batch() {
        let source = surface(0x300_0000);
        let plan = BcsPlan::new(
            surface(0x500_0000),
            &[
                fill(0x8040_2010),
                copy(source, CopyBackend::Legacy),
                fill(0),
            ],
        )
        .unwrap();
        assert_eq!(plan.batches().len(), 1);
        let batch = &plan.batches()[0];
        assert_eq!(batch.operation_count(), 3);
        assert_eq!(batch.copy_count(), 1);
        assert_eq!(batch.fill_count(), 2);
        assert_eq!(batch.explicit_flushes(), 0);
        assert_eq!(batch.copied_bytes(), 2 * 32 * 32 * 4 + 16 * 16 * 4);
        let words = body(batch);
        assert_eq!(opcode(words[0]), 0x44);
        assert_eq!(words[7], 0x8040_2010);
        assert_eq!(opcode(words[11]), 0x53);
        assert_eq!(opcode(words[21]), 0x44);
        assert_eq!(words[28], 0);
        assert_eq!(batch.destination(), surface(0x500_0000));
    }

    #[test]
    fn fast_source_changes_and_clear_dependencies_get_internal_flushes() {
        let a = surface(0x300_0000);
        let b = surface(0x400_0000);
        let plan = BcsPlan::new(
            surface(0x500_0000),
            &[
                fill(1),
                copy(a, CopyBackend::PreferFast),
                copy(a, CopyBackend::PreferFast),
                copy(b, CopyBackend::PreferFast),
                fill(2),
            ],
        )
        .unwrap();
        let batch = &plan.batches()[0];
        assert_eq!(batch.explicit_flushes(), 3);
        let words = body(batch);
        assert_eq!(&words[11..16], &[MI_FLUSH_DW, 0, 0, 0, 0]);
        assert_eq!(opcode(words[16]), 0x42);
        assert_eq!(opcode(words[26]), 0x42);
        assert_eq!(&words[36..41], &[MI_FLUSH_DW, 0, 0, 0, 0]);
        assert_eq!(opcode(words[41]), 0x42);
        assert_eq!(&words[51..56], &[MI_FLUSH_DW, 0, 0, 0, 0]);
        assert_eq!(opcode(words[56]), 0x44);
    }

    #[test]
    fn repeated_glyphs_map_the_atlas_once_and_retain_cache_policy() {
        let source = surface(0x300_0000);
        let ops = vec![copy(source, CopyBackend::Legacy); 160];
        let plan = BcsPlan::new(surface(0x500_0000), &ops).unwrap();
        let batch = &plan.batches()[0];
        assert_eq!(batch.source_mappings(), &[(source, SourceCache::WriteBack)]);
        assert_eq!(batch.explicit_flushes(), 0);
        assert_eq!(batch.encoded_dwords(), 1600);
    }

    #[test]
    fn conflicting_gpu_backing_and_physical_cache_aliases_are_rejected() {
        let a = surface(0x300_0000);
        let mut changed = a;
        changed.phys += 0x100_0000;
        assert!(
            BcsPlan::new(
                surface(0x500_0000),
                &[
                    copy(a, CopyBackend::Legacy),
                    copy(changed, CopyBackend::Legacy)
                ]
            )
            .is_err()
        );
        let mut alias = a;
        alias.gpu = 0x400_0000;
        let mut uncached = copy(alias, CopyBackend::Legacy);
        if let BcsOp::Copy { source_cache, .. } = &mut uncached {
            *source_cache = SourceCache::ScanoutUncached;
        }
        assert!(
            BcsPlan::new(
                surface(0x500_0000),
                &[copy(a, CopyBackend::Legacy), uncached]
            )
            .is_err()
        );
    }

    #[test]
    fn alias_conflicts_across_batch_boundaries_fail_the_whole_plan() {
        let a = surface(0x300_0000);
        let mut changed = a;
        changed.phys += 0x100_0000;
        let mut ops = vec![copy(a, CopyBackend::Legacy); 160];
        ops.push(copy(changed, CopyBackend::Legacy));
        assert!(BcsPlan::new(surface(0x500_0000), &ops).is_err());
    }

    #[test]
    fn unaligned_fast_request_uses_legacy_without_changing_coordinates() {
        let mut unaligned = copy(surface(0x300_0000), CopyBackend::PreferFast);
        if let BcsOp::Copy { copy, .. } = &mut unaligned {
            copy.destination_x = 1;
        }
        let plan = BcsPlan::new(surface(0x500_0000), &[unaligned]).unwrap();
        let words = body(&plan.batches()[0]);
        assert_eq!(opcode(words[0]), 0x53);
        assert_eq!(words[2], 1);
        assert_eq!(words[3], 17 | (16 << 16));
    }

    #[test]
    fn barriers_count_toward_capacity_and_every_batch_fits_the_envelope() {
        let a = surface(0x300_0000);
        let b = surface(0x400_0000);
        let ops: Vec<_> = (0..160)
            .map(|index| copy(if index % 2 == 0 { a } else { b }, CopyBackend::PreferFast))
            .collect();
        let plan = BcsPlan::new(surface(0x500_0000), &ops).unwrap();
        assert_eq!(plan.batches().len(), 2);
        assert_eq!(plan.batches()[0].operation_count(), 135);
        assert_eq!(plan.batches()[1].operation_count(), 25);
        assert_eq!(plan.batches()[0].explicit_flushes(), 134);
        assert_eq!(plan.batches()[1].explicit_flushes(), 24);
        for batch in plan.batches() {
            assert!(batch.encoded_dwords() + BCS_PLAN_ENVELOPE_DWORDS <= BCS_PLAN_BATCH_BYTES / 4);
            assert_eq!(body(batch).len(), batch.encoded_dwords());
        }
    }

    #[test]
    fn operation_limit_splits_even_when_encoded_bytes_fit() {
        let ops = vec![copy(surface(0x300_0000), CopyBackend::Legacy); 161];
        let plan = BcsPlan::new(surface(0x500_0000), &ops).unwrap();
        assert_eq!(plan.batches().len(), 2);
        assert_eq!(plan.batches()[0].operation_count(), 160);
        assert_eq!(plan.batches()[1].operation_count(), 1);
    }

    #[test]
    fn invalid_bounds_zero_rectangles_and_destination_aliases_are_rejected() {
        let destination = surface(0x500_0000);
        for (x, width) in [(u32::MAX, 1), (0, 0), (63, 2)] {
            let mut invalid = copy(surface(0x300_0000), CopyBackend::PreferFast);
            if let BcsOp::Copy { copy, .. } = &mut invalid {
                copy.destination_x = x;
                copy.width = width;
            }
            assert!(BcsPlan::new(destination, &[invalid]).is_err());
        }
        assert!(BcsPlan::new(destination, &[copy(destination, CopyBackend::Legacy)]).is_err());
        let mut invalid_fill = fill(1);
        if let BcsOp::Fill(fill) = &mut invalid_fill {
            fill.height = 0;
        }
        assert!(BcsPlan::new(destination, &[invalid_fill]).is_err());
    }

    #[test]
    fn empty_plan_has_no_submission_and_short_encoding_is_atomic() {
        let destination = surface(0x500_0000);
        assert!(BcsPlan::new(destination, &[]).unwrap().batches().is_empty());
        let plan = BcsPlan::new(destination, &[fill(1)]).unwrap();
        let mut short = [0x1234_5678; 10];
        assert_eq!(plan.batches()[0].encode_body(&mut short), None);
        assert_eq!(short, [0x1234_5678; 10]);
    }

    #[test]
    fn mixed_encoded_commands_match_pixels_alpha_and_padded_guards() {
        // Decode the emitted command stream into an ordinary host image and
        // compare it with a scalar operation-list reference. This checks the
        // actual encoding's addresses, pitches, endpoints and painter order.
        let mut destination = surface(0x500_0000);
        destination.width = 17;
        destination.height = 8;
        destination.pitch_bytes = 80;
        destination.bytes = 80 * 10;
        let mut a = destination;
        a.gpu = 0x300_0000;
        a.phys = a.gpu + 0x1_0000_0000;
        let mut b = a;
        b.gpu = 0x400_0000;
        b.phys = b.gpu + 0x1_0000_0000;
        let sources: Vec<_> = [a, b]
            .into_iter()
            .enumerate()
            .map(|(source_index, s)| {
                let pixels: Vec<_> = (0..200u32)
                    .map(|pixel| {
                        let alpha = [0, 0x80, 0xff][pixel as usize % 3];
                        (alpha << 24) | ((source_index as u32 + 1) << 16) | pixel
                    })
                    .collect();
                (s.gpu, pixels)
            })
            .collect();
        let rect_copy = |source, backend, source_x, source_y, x, y, width, height| BcsOp::Copy {
            copy: GucBcs0RgbaCopy {
                source,
                source_x,
                source_y,
                destination_x: x,
                destination_y: y,
                width,
                height,
            },
            backend,
            source_cache: SourceCache::WriteBack,
        };
        let ops = [
            BcsOp::Fill(GucBcs0RgbaFill {
                x: 0,
                y: 0,
                width: 17,
                height: 8,
                color: 0,
            }),
            rect_copy(a, CopyBackend::Legacy, 2, 1, 1, 1, 9, 4),
            BcsOp::Fill(GucBcs0RgbaFill {
                x: 4,
                y: 2,
                width: 5,
                height: 3,
                color: 0x007a_2519,
            }),
            rect_copy(b, CopyBackend::PreferFast, 4, 0, 4, 3, 8, 2),
            rect_copy(a, CopyBackend::Legacy, 3, 2, 7, 2, 5, 3),
            rect_copy(a, CopyBackend::PreferFast, 0, 6, 0, 6, 16, 2),
        ];
        let poison = 0xa55a_c33c;
        let mut expected = vec![poison; 200];
        for &op in &ops {
            match op {
                BcsOp::Fill(f) => {
                    for y in f.y..f.y + f.height {
                        for x in f.x..f.x + f.width {
                            expected[(y * 20 + x) as usize] = f.color;
                        }
                    }
                }
                BcsOp::Copy { copy, .. } => {
                    let pixels = &sources.iter().find(|s| s.0 == copy.source.gpu).unwrap().1;
                    for y in 0..copy.height {
                        for x in 0..copy.width {
                            expected[((copy.destination_y + y) * 20 + copy.destination_x + x)
                                as usize] =
                                pixels[((copy.source_y + y) * 20 + copy.source_x + x) as usize];
                        }
                    }
                }
            }
        }
        let plan = BcsPlan::new(destination, &ops).unwrap();
        let mut actual = vec![poison; 200];
        for batch in plan.batches() {
            let words = body(batch);
            let mut cursor = 0;
            while cursor < words.len() {
                let command = &words[cursor..];
                if command[0] == MI_FLUSH_DW {
                    cursor += 5;
                    continue;
                }
                let x1 = (command[2] & 0xffff) as usize;
                let y1 = (command[2] >> 16) as usize;
                let x2 = (command[3] & 0xffff) as usize;
                let y2 = (command[3] >> 16) as usize;
                let destination_gpu = u64::from(command[4]) | (u64::from(command[5]) << 32);
                assert_eq!(destination_gpu, destination.gpu);
                match opcode(command[0]) {
                    0x44 => {
                        let pitch = (command[1] as usize + 1) / 4;
                        for y in y1..y2 {
                            actual[y * pitch + x1..y * pitch + x2].fill(command[7]);
                        }
                        cursor += 11;
                    }
                    0x42 | 0x53 => {
                        let pitch = (command[1] & 0xffff) as usize / 4;
                        let source_pitch = command[7] as usize / 4;
                        let source_x = (command[6] & 0xffff) as usize;
                        let source_y = (command[6] >> 16) as usize;
                        let source_gpu = u64::from(command[8]) | (u64::from(command[9]) << 32);
                        let pixels = &sources.iter().find(|s| s.0 == source_gpu).unwrap().1;
                        for y in y1..y2 {
                            let source = (source_y + y - y1) * source_pitch + source_x;
                            actual[y * pitch + x1..y * pitch + x2]
                                .copy_from_slice(&pixels[source..source + x2 - x1]);
                        }
                        cursor += 10;
                    }
                    other => panic!("unexpected BLT opcode {other:x}"),
                }
            }
        }
        assert_eq!(actual, expected);
        assert!(actual[160..].iter().all(|&pixel| pixel == poison));
        assert!((0..8).all(|y| {
            actual[y * 20 + 17..y * 20 + 20]
                .iter()
                .all(|&pixel| pixel == poison)
        }));
        assert_eq!(actual[2 * 20 + 4], 0x007a_2519);
    }
}
