use core::convert::TryFrom;

use crate::format::{
    ARENA_ALIGNMENT, BINDING_RECORD_BYTES, HEADER_BYTES, LITTLE_ENDIAN_TAG, MAGIC,
    MODEL_SHA256_OFFSET, OP_FLAG_IN_PLACE, OP_RECORD_BYTES, OpCode, PAYLOAD_SHA256_OFFSET,
    PHASE_COUNT, PHASE_FLAG_RUNTIME_SIZED, PHASE_RECORD_BYTES, SECTION_COUNT,
    SECTION_DIRECTORY_OFFSET, SECTION_ENTRY_BYTES, SHA256_BYTES, SLOT_RECORD_BYTES, SectionKind,
    TENSOR_RECORD_BYTES, UNRESOLVED_SLOT_BASE, VERSION, VOICES_SHA256_OFFSET, checked_align_up,
    hash_eq, is_zero, payload_sha256, read_i64, read_u16, read_u32, read_u64,
};
use crate::tensor::{
    DType, Phase, StorageKind, TensorDesc, TensorError, TensorFlags, access_span,
    checked_storage_end, effective_view_alignment, phase_compatible, validate_aligned_size,
};

#[derive(Clone, Copy, Debug)]
pub struct ParseOptions<'a> {
    pub expected_payload_sha256: Option<&'a [u8; 32]>,
    pub expected_model_sha256: Option<&'a [u8; 32]>,
    pub expected_voices_sha256: Option<&'a [u8; 32]>,
    pub max_tensors: u32,
    pub max_slots: u32,
    pub max_ops: u32,
    pub max_bindings: u32,
    pub max_data_bytes: u64,
    pub max_arena_bytes: u64,
}

impl ParseOptions<'_> {
    pub const fn permissive() -> Self {
        Self {
            expected_payload_sha256: None,
            expected_model_sha256: None,
            expected_voices_sha256: None,
            max_tensors: u32::MAX,
            max_slots: u32::MAX,
            max_ops: u32::MAX,
            max_bindings: u32::MAX,
            max_data_bytes: u64::MAX,
            max_arena_bytes: u64::MAX,
        }
    }
}

impl Default for ParseOptions<'_> {
    fn default() -> Self {
        Self::permissive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError {
    HeaderTruncated,
    BadMagic,
    UnsupportedVersion,
    WrongEndianness,
    BadHeaderSize,
    BadArtifactSize,
    BadHeaderFlags,
    BadHeaderRecordSize,
    HeaderReservedNonZero,
    HashMissing,
    PayloadHashMismatch,
    ExpectedPayloadHashMismatch,
    ExpectedModelHashMismatch,
    ExpectedVoicesHashMismatch,
    BadSectionKind,
    BadSectionFlags,
    BadSectionAlignment,
    BadSectionStride,
    SectionLengthOverflow,
    SectionOutOfBounds,
    NonCanonicalSectionOffset,
    NonZeroSectionPadding,
    SectionCountTooLarge,
    DataSectionTooLarge,
    BadPhase,
    BadSlot { slot: u32, reason: ArenaPlanError },
    FixedSlotAlias { first: u32, second: u32 },
    BadTensor { tensor: u32, reason: TensorError },
    SlotTensorOutOfBounds { tensor: u32, slot: u32 },
    TensorStorageAlias { first: u32, second: u32 },
    BadOp { op: u32, reason: OpError },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArenaPlanError {
    UnknownSlotKind,
    UnknownPhase,
    UnknownFlags,
    InvalidAlignment,
    InvalidPhaseKind,
    InvalidFixedOffset,
    InvalidAffineSize,
    SizeOverflow,
    InvalidLiveness,
    SlotOutOfPhaseBounds,
    SlotBasesTooSmall,
    FrameCountOutOfRange,
    ArenaLimitExceeded,
    PackingDidNotConverge,
    ResolvedTensorOutOfBounds,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpError {
    UnknownOpcode,
    UnknownFlags,
    UnknownPhase,
    ReservedNonZero,
    EmptyOutputs,
    EmptyWork,
    BindingRangeOverflow,
    BindingOutOfBounds,
    TensorIdOutOfBounds,
    TensorWrongPhase,
    TensorNotLive,
    ReadOnlyOutput,
    DuplicateOutput,
    AliasingRequiresInPlace,
    OutputAlias,
    AttributeRangeOverflow,
    AttributeOutOfBounds,
    NonCanonicalEmptyAttribute,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectionDesc {
    pub kind: SectionKind,
    pub alignment: u32,
    pub offset: u64,
    pub count: u64,
    pub stride: u32,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhasePlan {
    pub phase: Phase,
    pub runtime_sized: bool,
    pub op_start: u32,
    pub op_end: u32,
    pub arena_min_bytes: u64,
    pub arena_max_bytes: u64,
    pub arena_alignment: u32,
    pub frame_count_min: u32,
    pub frame_count_max: u32,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotKind {
    Fixed = 1,
    Dynamic = 2,
}

impl TryFrom<u8> for SlotKind {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Fixed),
            2 => Ok(Self::Dynamic),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotDesc {
    pub kind: SlotKind,
    pub phase: Phase,
    pub alignment: u32,
    pub fixed_offset: u64,
    pub byte_multiplier: u64,
    pub byte_addend: i64,
    pub live_start: u32,
    pub live_end: u32,
}

impl SlotDesc {
    pub fn bytes_at(self, frame_count: u32) -> Result<u64, ArenaPlanError> {
        let value = i128::from(self.byte_multiplier)
            .checked_mul(i128::from(frame_count))
            .and_then(|value| value.checked_add(i128::from(self.byte_addend)))
            .ok_or(ArenaPlanError::SizeOverflow)?;
        if value < 0 {
            return Err(ArenaPlanError::InvalidAffineSize);
        }
        u64::try_from(value).map_err(|_| ArenaPlanError::SizeOverflow)
    }

    pub const fn liveness_overlaps(self, other: Self) -> bool {
        self.live_start < other.live_end && other.live_start < self.live_end
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpDesc {
    pub opcode: OpCode,
    pub flags: u16,
    pub phase: Phase,
    pub binding_start: u32,
    pub input_count: u16,
    pub output_count: u16,
    pub attribute_offset: u64,
    pub attribute_len: u32,
    pub work_units: u32,
}

impl OpDesc {
    pub const fn allows_in_place(self) -> bool {
        self.flags & OP_FLAG_IN_PLACE != 0
    }

    pub fn binding_count(self) -> Option<u32> {
        u32::from(self.input_count).checked_add(u32::from(self.output_count))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageOwner {
    Slot(u32),
    Constant(u32),
    External(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedStorage {
    pub owner: StorageOwner,
    pub offset: u64,
    pub span: u64,
    pub alignment: u32,
}

#[derive(Debug)]
pub struct Program<'a> {
    artifact: &'a [u8],
    sections: [SectionDesc; SECTION_COUNT],
    tensors: &'a [u8],
    slots: &'a [u8],
    ops: &'a [u8],
    bindings: &'a [u8],
    data: &'a [u8],
    phases: [PhasePlan; PHASE_COUNT],
    payload_sha256: [u8; 32],
    model_sha256: [u8; 32],
    voices_sha256: [u8; 32],
}

impl<'a> Program<'a> {
    pub fn parse(artifact: &'a [u8]) -> Result<Self, ParseError> {
        Self::parse_with_options(artifact, ParseOptions::default())
    }

    pub fn parse_with_options(
        artifact: &'a [u8],
        options: ParseOptions<'_>,
    ) -> Result<Self, ParseError> {
        if artifact.len() < HEADER_BYTES {
            return Err(ParseError::HeaderTruncated);
        }
        if artifact.get(..8) != Some(MAGIC.as_slice()) {
            return Err(ParseError::BadMagic);
        }
        if read_u16(artifact, 8) != Some(VERSION) {
            return Err(ParseError::UnsupportedVersion);
        }
        if read_u16(artifact, 10) != Some(LITTLE_ENDIAN_TAG) {
            return Err(ParseError::WrongEndianness);
        }
        if read_u32(artifact, 12) != Some(HEADER_BYTES as u32) {
            return Err(ParseError::BadHeaderSize);
        }
        let artifact_bytes = read_u64(artifact, 16).ok_or(ParseError::HeaderTruncated)?;
        if usize::try_from(artifact_bytes).ok() != Some(artifact.len()) {
            return Err(ParseError::BadArtifactSize);
        }
        if read_u16(artifact, 24) != Some(SECTION_COUNT as u16)
            || read_u16(artifact, 26) != Some(PHASE_COUNT as u16)
            || read_u32(artifact, 28) != Some(0)
            || read_u32(artifact, 32) != Some(ARENA_ALIGNMENT)
        {
            return Err(ParseError::BadHeaderFlags);
        }
        if read_u16(artifact, 36) != Some(TENSOR_RECORD_BYTES as u16)
            || read_u16(artifact, 38) != Some(SLOT_RECORD_BYTES as u16)
            || read_u16(artifact, 40) != Some(OP_RECORD_BYTES as u16)
            || read_u16(artifact, 42) != Some(PHASE_RECORD_BYTES as u16)
            || read_u16(artifact, 44) != Some(BINDING_RECORD_BYTES as u16)
            || read_u16(artifact, 46) != Some(0)
        {
            return Err(ParseError::BadHeaderRecordSize);
        }
        if !is_zero(&artifact[48..64]) {
            return Err(ParseError::HeaderReservedNonZero);
        }

        let payload_hash: [u8; 32] = artifact
            [PAYLOAD_SHA256_OFFSET..PAYLOAD_SHA256_OFFSET + SHA256_BYTES]
            .try_into()
            .expect("header hash");
        let model_hash: [u8; 32] = artifact
            [MODEL_SHA256_OFFSET..MODEL_SHA256_OFFSET + SHA256_BYTES]
            .try_into()
            .expect("header hash");
        let voices_hash: [u8; 32] = artifact
            [VOICES_SHA256_OFFSET..VOICES_SHA256_OFFSET + SHA256_BYTES]
            .try_into()
            .expect("header hash");
        if is_zero(&payload_hash) || is_zero(&model_hash) || is_zero(&voices_hash) {
            return Err(ParseError::HashMissing);
        }
        let observed_payload_hash = payload_sha256(&artifact[HEADER_BYTES..]);
        if !hash_eq(&observed_payload_hash, &payload_hash) {
            return Err(ParseError::PayloadHashMismatch);
        }
        if options
            .expected_payload_sha256
            .is_some_and(|expected| !hash_eq(expected, &payload_hash))
        {
            return Err(ParseError::ExpectedPayloadHashMismatch);
        }
        if options
            .expected_model_sha256
            .is_some_and(|expected| !hash_eq(expected, &model_hash))
        {
            return Err(ParseError::ExpectedModelHashMismatch);
        }
        if options
            .expected_voices_sha256
            .is_some_and(|expected| !hash_eq(expected, &voices_hash))
        {
            return Err(ParseError::ExpectedVoicesHashMismatch);
        }

        let mut sections = [SectionDesc {
            kind: SectionKind::Tensors,
            alignment: 1,
            offset: 0,
            count: 0,
            stride: 0,
            bytes: 0,
        }; SECTION_COUNT];
        let mut previous_end = HEADER_BYTES as u64;
        for (index, expected_kind) in SectionKind::ALL.iter().copied().enumerate() {
            let entry_offset = SECTION_DIRECTORY_OFFSET + index * SECTION_ENTRY_BYTES;
            let raw_kind = read_u16(artifact, entry_offset).ok_or(ParseError::HeaderTruncated)?;
            let kind = SectionKind::try_from(raw_kind).map_err(|_| ParseError::BadSectionKind)?;
            if kind != expected_kind {
                return Err(ParseError::BadSectionKind);
            }
            if read_u16(artifact, entry_offset + 2) != Some(0)
                || read_u32(artifact, entry_offset + 28) != Some(0)
            {
                return Err(ParseError::BadSectionFlags);
            }
            let alignment =
                read_u32(artifact, entry_offset + 4).ok_or(ParseError::HeaderTruncated)?;
            let offset = read_u64(artifact, entry_offset + 8).ok_or(ParseError::HeaderTruncated)?;
            let count = read_u64(artifact, entry_offset + 16).ok_or(ParseError::HeaderTruncated)?;
            let stride =
                read_u32(artifact, entry_offset + 24).ok_or(ParseError::HeaderTruncated)?;
            if alignment != kind.alignment() {
                return Err(ParseError::BadSectionAlignment);
            }
            if stride != kind.stride() {
                return Err(ParseError::BadSectionStride);
            }
            let bytes = count
                .checked_mul(u64::from(stride))
                .ok_or(ParseError::SectionLengthOverflow)?;
            let expected_offset = checked_align_up(previous_end, alignment)
                .ok_or(ParseError::SectionLengthOverflow)?;
            if offset != expected_offset {
                return Err(ParseError::NonCanonicalSectionOffset);
            }
            let padding_start =
                usize::try_from(previous_end).map_err(|_| ParseError::SectionOutOfBounds)?;
            let padding_end =
                usize::try_from(offset).map_err(|_| ParseError::SectionOutOfBounds)?;
            if !is_zero(
                artifact
                    .get(padding_start..padding_end)
                    .ok_or(ParseError::SectionOutOfBounds)?,
            ) {
                return Err(ParseError::NonZeroSectionPadding);
            }
            let end = offset
                .checked_add(bytes)
                .ok_or(ParseError::SectionLengthOverflow)?;
            if end > artifact_bytes {
                return Err(ParseError::SectionOutOfBounds);
            }
            sections[index] = SectionDesc {
                kind,
                alignment,
                offset,
                count,
                stride,
                bytes,
            };
            previous_end = end;
        }
        if previous_end != artifact_bytes {
            return Err(ParseError::SectionOutOfBounds);
        }

        let tensor_count = checked_count(sections[0].count, options.max_tensors)?;
        let slot_count = checked_count(sections[1].count, options.max_slots)?;
        let op_count = checked_count(sections[2].count, options.max_ops)?;
        let _binding_count = checked_count(sections[3].count, options.max_bindings)?;
        if sections[4].count != PHASE_COUNT as u64 {
            return Err(ParseError::BadPhase);
        }
        if sections[5].count > options.max_data_bytes {
            return Err(ParseError::DataSectionTooLarge);
        }

        let tensors = section_slice(artifact, sections[0])?;
        let slots = section_slice(artifact, sections[1])?;
        let ops = section_slice(artifact, sections[2])?;
        let bindings = section_slice(artifact, sections[3])?;
        let phase_bytes = section_slice(artifact, sections[4])?;
        let data = section_slice(artifact, sections[5])?;
        let phases = parse_phases(phase_bytes, op_count, options.max_arena_bytes)?;

        let program = Self {
            artifact,
            sections,
            tensors,
            slots,
            ops,
            bindings,
            data,
            phases,
            payload_sha256: payload_hash,
            model_sha256: model_hash,
            voices_sha256: voices_hash,
        };
        program.validate_slots(slot_count)?;
        program.validate_tensors(tensor_count, slot_count)?;
        program.validate_ops(op_count)?;
        Ok(program)
    }

    pub const fn artifact(&self) -> &'a [u8] {
        self.artifact
    }

    pub const fn sections(&self) -> &[SectionDesc; SECTION_COUNT] {
        &self.sections
    }

    pub const fn payload_sha256(&self) -> &[u8; 32] {
        &self.payload_sha256
    }

    pub const fn model_sha256(&self) -> &[u8; 32] {
        &self.model_sha256
    }

    pub const fn voices_sha256(&self) -> &[u8; 32] {
        &self.voices_sha256
    }

    pub const fn phases(&self) -> &[PhasePlan; PHASE_COUNT] {
        &self.phases
    }

    pub const fn phase(&self, phase: Phase) -> Option<PhasePlan> {
        match phase {
            Phase::Phase0 => Some(self.phases[0]),
            Phase::Phase1 => Some(self.phases[1]),
            Phase::Shared => None,
        }
    }

    pub fn tensor_count(&self) -> u32 {
        self.sections[0].count as u32
    }

    pub fn slot_count(&self) -> u32 {
        self.sections[1].count as u32
    }

    pub fn op_count(&self) -> u32 {
        self.sections[2].count as u32
    }

    pub fn binding_count(&self) -> u32 {
        self.sections[3].count as u32
    }

    pub const fn data(&self) -> &'a [u8] {
        self.data
    }

    pub fn tensor(&self, tensor: u32) -> Option<TensorDesc> {
        let record = record(self.tensors, tensor, TENSOR_RECORD_BYTES)?;
        decode_tensor(record).ok()
    }

    pub fn slot(&self, slot: u32) -> Option<SlotDesc> {
        let record = record(self.slots, slot, SLOT_RECORD_BYTES)?;
        decode_slot(record).ok()
    }

    pub fn op(&self, op: u32) -> Option<OpDesc> {
        let record = record(self.ops, op, OP_RECORD_BYTES)?;
        decode_op(record).ok()
    }

    pub fn binding(&self, binding: u32) -> Option<u32> {
        let offset = usize::try_from(binding)
            .ok()?
            .checked_mul(BINDING_RECORD_BYTES)?;
        read_u32(self.bindings, offset)
    }

    pub fn op_input(&self, op: OpDesc, input: u16) -> Option<u32> {
        if input >= op.input_count {
            return None;
        }
        self.binding(op.binding_start.checked_add(u32::from(input))?)
    }

    pub fn op_output(&self, op: OpDesc, output: u16) -> Option<u32> {
        if output >= op.output_count {
            return None;
        }
        let index = op
            .binding_start
            .checked_add(u32::from(op.input_count))?
            .checked_add(u32::from(output))?;
        self.binding(index)
    }

    pub fn op_attributes(&self, op: OpDesc) -> Option<&'a [u8]> {
        let start = usize::try_from(op.attribute_offset).ok()?;
        let end = start.checked_add(op.attribute_len as usize)?;
        self.data.get(start..end)
    }

    pub fn resolve_storage(&self, tensor_id: u32) -> Result<ResolvedStorage, TensorError> {
        let tensor = self
            .tensor(tensor_id)
            .ok_or(TensorError::InvalidViewReference)?;
        let max_span =
            access_span(tensor.dtype, tensor.rank, &tensor.max_dims, &tensor.max_byte_strides)?;
        match tensor.storage {
            StorageKind::Slot => Ok(ResolvedStorage {
                owner: StorageOwner::Slot(tensor.slot_id),
                offset: tensor.storage_offset,
                span: max_span,
                alignment: tensor.guaranteed_alignment,
            }),
            StorageKind::Constant => Ok(ResolvedStorage {
                owner: StorageOwner::Constant(tensor_id),
                offset: tensor.storage_offset,
                span: max_span,
                alignment: tensor.guaranteed_alignment,
            }),
            StorageKind::External => Ok(ResolvedStorage {
                owner: StorageOwner::External(tensor_id),
                offset: 0,
                span: max_span,
                alignment: tensor.guaranteed_alignment,
            }),
            StorageKind::View => {
                let owner = self
                    .tensor(tensor.view_of)
                    .ok_or(TensorError::InvalidViewReference)?;
                if owner.storage == StorageKind::View {
                    return Err(TensorError::ViewOfView);
                }
                let base = self.resolve_storage(tensor.view_of)?;
                let offset = base
                    .offset
                    .checked_add(tensor.storage_offset)
                    .ok_or(TensorError::ByteLengthOverflow)?;
                Ok(ResolvedStorage {
                    owner: base.owner,
                    offset,
                    span: max_span,
                    alignment: tensor.guaranteed_alignment,
                })
            }
        }
    }

    /// Resolve and deterministically interval-pack all phase-one slots.
    ///
    /// `slot_bases` is caller-owned scratch and remains borrowed by the returned
    /// proof. Phase-zero-only slots are marked [`UNRESOLVED_SLOT_BASE`].
    pub fn resolve_phase_two<'p, 's>(
        &'p self,
        frame_count: u32,
        slot_bases: &'s mut [u64],
    ) -> Result<ResolvedArenaPlan<'p, 'a, 's>, ArenaPlanError> {
        let phase = self.phases[1];
        if frame_count < phase.frame_count_min || frame_count > phase.frame_count_max {
            return Err(ArenaPlanError::FrameCountOutOfRange);
        }
        let slot_count = self.slot_count() as usize;
        if slot_bases.len() < slot_count {
            return Err(ArenaPlanError::SlotBasesTooSmall);
        }
        slot_bases[..slot_count].fill(UNRESOLVED_SLOT_BASE);

        let mut high_water = 0u64;
        for slot_id in 0..self.slot_count() {
            let slot = self.slot(slot_id).ok_or(ArenaPlanError::UnknownSlotKind)?;
            if slot.kind == SlotKind::Fixed && slot.phase == Phase::Shared {
                slot_bases[slot_id as usize] = slot.fixed_offset;
                high_water = high_water.max(
                    slot.fixed_offset
                        .checked_add(slot.bytes_at(frame_count)?)
                        .ok_or(ArenaPlanError::SizeOverflow)?,
                );
            }
        }

        for slot_id in 0..self.slot_count() {
            let slot = self.slot(slot_id).ok_or(ArenaPlanError::UnknownSlotKind)?;
            if slot.kind != SlotKind::Dynamic {
                continue;
            }
            let bytes = slot.bytes_at(frame_count)?;
            let mut candidate = 0u64;
            let mut moves = 0u32;
            loop {
                candidate = checked_align_up(candidate, slot.alignment)
                    .ok_or(ArenaPlanError::SizeOverflow)?;
                let candidate_end = candidate
                    .checked_add(bytes)
                    .ok_or(ArenaPlanError::SizeOverflow)?;
                let mut conflict_end = None;
                for other_id in 0..self.slot_count() {
                    if other_id == slot_id {
                        continue;
                    }
                    let other_base = slot_bases[other_id as usize];
                    if other_base == UNRESOLVED_SLOT_BASE {
                        continue;
                    }
                    let other = self.slot(other_id).ok_or(ArenaPlanError::UnknownSlotKind)?;
                    if !slot.liveness_overlaps(other) {
                        continue;
                    }
                    let other_end = other_base
                        .checked_add(other.bytes_at(frame_count)?)
                        .ok_or(ArenaPlanError::SizeOverflow)?;
                    if ranges_overlap(candidate, candidate_end, other_base, other_end) {
                        conflict_end = Some(other_end);
                        break;
                    }
                }
                match conflict_end {
                    None => {
                        slot_bases[slot_id as usize] = candidate;
                        high_water = high_water.max(candidate_end);
                        break;
                    }
                    Some(end) => {
                        candidate = end;
                        moves = moves.saturating_add(1);
                        if moves > self.slot_count().saturating_add(1) {
                            return Err(ArenaPlanError::PackingDidNotConverge);
                        }
                    }
                }
            }
        }

        let arena_bytes = checked_align_up(high_water.max(phase.arena_min_bytes), ARENA_ALIGNMENT)
            .ok_or(ArenaPlanError::SizeOverflow)?;
        if arena_bytes > phase.arena_max_bytes {
            return Err(ArenaPlanError::ArenaLimitExceeded);
        }

        for tensor_id in 0..self.tensor_count() {
            let tensor = self
                .tensor(tensor_id)
                .ok_or(ArenaPlanError::ResolvedTensorOutOfBounds)?;
            if tensor.storage != StorageKind::Slot
                || !matches!(tensor.phase, Phase::Phase1 | Phase::Shared)
            {
                continue;
            }
            let slot = self
                .slot(tensor.slot_id)
                .ok_or(ArenaPlanError::ResolvedTensorOutOfBounds)?;
            let base = slot_bases[tensor.slot_id as usize];
            if base == UNRESOLVED_SLOT_BASE {
                return Err(ArenaPlanError::ResolvedTensorOutOfBounds);
            }
            let resolved = tensor
                .resolve(frame_count)
                .map_err(|_| ArenaPlanError::ResolvedTensorOutOfBounds)?;
            let relative_end = tensor
                .storage_offset
                .checked_add(resolved.access_span)
                .ok_or(ArenaPlanError::SizeOverflow)?;
            if relative_end > slot.bytes_at(frame_count)?
                || base
                    .checked_add(relative_end)
                    .ok_or(ArenaPlanError::SizeOverflow)?
                    > arena_bytes
            {
                return Err(ArenaPlanError::ResolvedTensorOutOfBounds);
            }
        }

        Ok(ResolvedArenaPlan {
            program: self,
            frame_count,
            arena_bytes,
            slot_bases: &slot_bases[..slot_count],
        })
    }

    fn validate_slots(&self, slot_count: u32) -> Result<(), ParseError> {
        for slot_id in 0..slot_count {
            let slot = self.slot(slot_id).ok_or(ParseError::BadSlot {
                slot: slot_id,
                reason: ArenaPlanError::UnknownSlotKind,
            })?;
            validate_slot(slot, self.phases).map_err(|reason| ParseError::BadSlot {
                slot: slot_id,
                reason,
            })?;
        }
        for first_id in 0..slot_count {
            let first = self.slot(first_id).expect("validated slot");
            if first.kind != SlotKind::Fixed {
                continue;
            }
            let first_end = first
                .fixed_offset
                .checked_add(first.bytes_at(0).map_err(|reason| ParseError::BadSlot {
                    slot: first_id,
                    reason,
                })?)
                .ok_or(ParseError::BadSlot {
                    slot: first_id,
                    reason: ArenaPlanError::SizeOverflow,
                })?;
            for second_id in first_id + 1..slot_count {
                let second = self.slot(second_id).expect("validated slot");
                if second.kind != SlotKind::Fixed || !first.liveness_overlaps(second) {
                    continue;
                }
                let second_end = second
                    .fixed_offset
                    .checked_add(second.bytes_at(0).map_err(|reason| ParseError::BadSlot {
                        slot: second_id,
                        reason,
                    })?)
                    .ok_or(ParseError::BadSlot {
                        slot: second_id,
                        reason: ArenaPlanError::SizeOverflow,
                    })?;
                if ranges_overlap(first.fixed_offset, first_end, second.fixed_offset, second_end) {
                    return Err(ParseError::FixedSlotAlias {
                        first: first_id,
                        second: second_id,
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_tensors(&self, tensor_count: u32, slot_count: u32) -> Result<(), ParseError> {
        let phase1_frame_max = self.phases[1].frame_count_max;
        for tensor_id in 0..tensor_count {
            let tensor = self.tensor(tensor_id).ok_or(ParseError::BadTensor {
                tensor: tensor_id,
                reason: TensorError::UnknownDType,
            })?;
            tensor
                .validate_local(phase1_frame_max, slot_count, tensor_count, self.data.len() as u64)
                .map_err(|reason| ParseError::BadTensor {
                    tensor: tensor_id,
                    reason,
                })?;
            self.validate_tensor_storage(tensor_id, tensor)?;
        }

        // A slot is one allocation interval. Materialized tensors within it
        // must occupy disjoint subranges; intentional aliases are Views.
        for first_id in 0..tensor_count {
            let first = self.tensor(first_id).expect("validated tensor");
            if first.storage != StorageKind::Slot {
                continue;
            }
            let first_end = first
                .storage_offset
                .checked_add(first.byte_capacity)
                .ok_or(ParseError::BadTensor {
                    tensor: first_id,
                    reason: TensorError::ByteLengthOverflow,
                })?;
            for second_id in first_id + 1..tensor_count {
                let second = self.tensor(second_id).expect("validated tensor");
                if second.storage != StorageKind::Slot || second.slot_id != first.slot_id {
                    continue;
                }
                let second_end = second
                    .storage_offset
                    .checked_add(second.byte_capacity)
                    .ok_or(ParseError::BadTensor {
                        tensor: second_id,
                        reason: TensorError::ByteLengthOverflow,
                    })?;
                if ranges_overlap(
                    first.storage_offset,
                    first_end,
                    second.storage_offset,
                    second_end,
                ) {
                    return Err(ParseError::TensorStorageAlias {
                        first: first_id,
                        second: second_id,
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_tensor_storage(
        &self,
        tensor_id: u32,
        tensor: TensorDesc,
    ) -> Result<(), ParseError> {
        match tensor.storage {
            StorageKind::Slot => {
                let slot = self.slot(tensor.slot_id).ok_or(ParseError::BadTensor {
                    tensor: tensor_id,
                    reason: TensorError::InvalidSlotReference,
                })?;
                if tensor.guaranteed_alignment > slot.alignment {
                    return Err(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::MisalignedStorage,
                    });
                }
                if slot.phase != tensor.phase {
                    return Err(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::ViewPhaseMismatch,
                    });
                }
                for frame_count in slot_frame_endpoints(slot, self.phases) {
                    let resolved =
                        tensor
                            .resolve(frame_count)
                            .map_err(|reason| ParseError::BadTensor {
                                tensor: tensor_id,
                                reason,
                            })?;
                    let end = tensor
                        .storage_offset
                        .checked_add(resolved.access_span)
                        .ok_or(ParseError::SlotTensorOutOfBounds {
                            tensor: tensor_id,
                            slot: tensor.slot_id,
                        })?;
                    let slot_bytes =
                        slot.bytes_at(frame_count)
                            .map_err(|reason| ParseError::BadSlot {
                                slot: tensor.slot_id,
                                reason,
                            })?;
                    if end > slot_bytes {
                        return Err(ParseError::SlotTensorOutOfBounds {
                            tensor: tensor_id,
                            slot: tensor.slot_id,
                        });
                    }
                }
                if tensor.is_input() && slot.live_start != 0 {
                    return Err(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::InvalidStorageFields,
                    });
                }
                if tensor.is_output() && slot.live_end != self.op_count().saturating_add(1) {
                    return Err(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::InvalidStorageFields,
                    });
                }
            }
            StorageKind::View => {
                if tensor.view_of == tensor_id {
                    return Err(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::InvalidViewReference,
                    });
                }
                let owner = self.tensor(tensor.view_of).ok_or(ParseError::BadTensor {
                    tensor: tensor_id,
                    reason: TensorError::InvalidViewReference,
                })?;
                if owner.storage == StorageKind::View {
                    return Err(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::ViewOfView,
                    });
                }
                if owner.is_dynamic() {
                    return Err(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::DynamicViewUnsupported,
                    });
                }
                if owner.dtype != tensor.dtype {
                    return Err(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::ViewDTypeMismatch,
                    });
                }
                if !phase_compatible(owner.phase, tensor.phase) {
                    return Err(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::ViewPhaseMismatch,
                    });
                }
                if owner.is_read_only() && !tensor.is_read_only() {
                    return Err(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::ReadOnlyWrite,
                    });
                }
                let view_span = access_span(
                    tensor.dtype,
                    tensor.rank,
                    &tensor.max_dims,
                    &tensor.max_byte_strides,
                )
                .map_err(|reason| ParseError::BadTensor {
                    tensor: tensor_id,
                    reason,
                })?;
                let end =
                    checked_storage_end(tensor.storage_offset, view_span).map_err(|reason| {
                        ParseError::BadTensor {
                            tensor: tensor_id,
                            reason,
                        }
                    })?;
                if end > owner.byte_capacity {
                    return Err(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::ViewOutOfBounds,
                    });
                }
                let effective =
                    effective_view_alignment(owner.guaranteed_alignment, tensor.storage_offset);
                if tensor.guaranteed_alignment > effective
                    || tensor.storage_offset % u64::from(tensor.guaranteed_alignment) != 0
                {
                    return Err(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::MisalignedStorage,
                    });
                }
            }
            StorageKind::Constant => {
                let absolute_offset = self.sections[5]
                    .offset
                    .checked_add(tensor.storage_offset)
                    .ok_or(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::ByteLengthOverflow,
                    })?;
                if absolute_offset % u64::from(tensor.guaranteed_alignment) != 0 {
                    return Err(ParseError::BadTensor {
                        tensor: tensor_id,
                        reason: TensorError::MisalignedStorage,
                    });
                }
            }
            StorageKind::External => {}
        }
        Ok(())
    }

    fn validate_ops(&self, op_count: u32) -> Result<(), ParseError> {
        for op_id in 0..op_count {
            let op = self.op(op_id).ok_or(ParseError::BadOp {
                op: op_id,
                reason: OpError::UnknownOpcode,
            })?;
            let expected_phase = if op_id < self.phases[0].op_end {
                Phase::Phase0
            } else {
                Phase::Phase1
            };
            if op.phase != expected_phase {
                return Err(ParseError::BadOp {
                    op: op_id,
                    reason: OpError::TensorWrongPhase,
                });
            }
            let binding_count = op.binding_count().ok_or(ParseError::BadOp {
                op: op_id,
                reason: OpError::BindingRangeOverflow,
            })?;
            let binding_end =
                op.binding_start
                    .checked_add(binding_count)
                    .ok_or(ParseError::BadOp {
                        op: op_id,
                        reason: OpError::BindingRangeOverflow,
                    })?;
            if binding_end > self.binding_count() {
                return Err(ParseError::BadOp {
                    op: op_id,
                    reason: OpError::BindingOutOfBounds,
                });
            }
            let attribute_end = op
                .attribute_offset
                .checked_add(u64::from(op.attribute_len))
                .ok_or(ParseError::BadOp {
                    op: op_id,
                    reason: OpError::AttributeRangeOverflow,
                })?;
            if attribute_end > self.data.len() as u64 {
                return Err(ParseError::BadOp {
                    op: op_id,
                    reason: OpError::AttributeOutOfBounds,
                });
            }
            if op.attribute_len == 0 && op.attribute_offset != 0 {
                return Err(ParseError::BadOp {
                    op: op_id,
                    reason: OpError::NonCanonicalEmptyAttribute,
                });
            }

            for binding_offset in 0..binding_count {
                let binding = op.binding_start + binding_offset;
                let tensor_id = self.binding(binding).ok_or(ParseError::BadOp {
                    op: op_id,
                    reason: OpError::BindingOutOfBounds,
                })?;
                let tensor = self.tensor(tensor_id).ok_or(ParseError::BadOp {
                    op: op_id,
                    reason: OpError::TensorIdOutOfBounds,
                })?;
                if tensor.phase != Phase::Shared && tensor.phase != op.phase {
                    return Err(ParseError::BadOp {
                        op: op_id,
                        reason: OpError::TensorWrongPhase,
                    });
                }
                if let Some(slot) = self.tensor_slot(tensor)? {
                    if op_id < slot.live_start || op_id >= slot.live_end {
                        return Err(ParseError::BadOp {
                            op: op_id,
                            reason: OpError::TensorNotLive,
                        });
                    }
                }
                if binding_offset >= u32::from(op.input_count) && tensor.is_read_only() {
                    return Err(ParseError::BadOp {
                        op: op_id,
                        reason: OpError::ReadOnlyOutput,
                    });
                }
            }

            for output in 0..op.output_count {
                let output_id = self.op_output(op, output).expect("validated binding");
                let output_storage =
                    self.resolve_storage(output_id)
                        .map_err(|_| ParseError::BadOp {
                            op: op_id,
                            reason: OpError::OutputAlias,
                        })?;
                for earlier in 0..output {
                    let earlier_id = self.op_output(op, earlier).expect("validated binding");
                    if output_id == earlier_id {
                        return Err(ParseError::BadOp {
                            op: op_id,
                            reason: OpError::DuplicateOutput,
                        });
                    }
                    let earlier_storage =
                        self.resolve_storage(earlier_id)
                            .map_err(|_| ParseError::BadOp {
                                op: op_id,
                                reason: OpError::OutputAlias,
                            })?;
                    if storage_overlaps(output_storage, earlier_storage).ok_or(
                        ParseError::BadOp {
                            op: op_id,
                            reason: OpError::OutputAlias,
                        },
                    )? {
                        return Err(ParseError::BadOp {
                            op: op_id,
                            reason: OpError::OutputAlias,
                        });
                    }
                }
                for input in 0..op.input_count {
                    let input_id = self.op_input(op, input).expect("validated binding");
                    let input_storage =
                        self.resolve_storage(input_id)
                            .map_err(|_| ParseError::BadOp {
                                op: op_id,
                                reason: OpError::OutputAlias,
                            })?;
                    if storage_overlaps(output_storage, input_storage).ok_or(ParseError::BadOp {
                        op: op_id,
                        reason: OpError::OutputAlias,
                    })? && !op.allows_in_place()
                    {
                        return Err(ParseError::BadOp {
                            op: op_id,
                            reason: OpError::AliasingRequiresInPlace,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn tensor_slot(&self, tensor: TensorDesc) -> Result<Option<SlotDesc>, ParseError> {
        let slot_id = match tensor.storage {
            StorageKind::Slot => Some(tensor.slot_id),
            StorageKind::View => {
                let owner = self.tensor(tensor.view_of).ok_or(ParseError::BadTensor {
                    tensor: tensor.view_of,
                    reason: TensorError::InvalidViewReference,
                })?;
                (owner.storage == StorageKind::Slot).then_some(owner.slot_id)
            }
            StorageKind::Constant | StorageKind::External => None,
        };
        Ok(slot_id.and_then(|id| self.slot(id)))
    }
}

#[derive(Debug)]
pub struct ResolvedArenaPlan<'p, 'a, 's> {
    program: &'p Program<'a>,
    frame_count: u32,
    arena_bytes: u64,
    slot_bases: &'s [u64],
}

impl ResolvedArenaPlan<'_, '_, '_> {
    pub const fn frame_count(&self) -> u32 {
        self.frame_count
    }

    pub const fn arena_bytes(&self) -> u64 {
        self.arena_bytes
    }

    pub fn slot_base(&self, slot: u32) -> Option<u64> {
        self.slot_bases
            .get(slot as usize)
            .copied()
            .filter(|base| *base != UNRESOLVED_SLOT_BASE)
    }

    pub fn program(&self) -> &Program<'_> {
        self.program
    }

    pub fn tensor_arena_offset(&self, tensor_id: u32) -> Option<u64> {
        let tensor = self.program.tensor(tensor_id)?;
        if tensor.storage != StorageKind::Slot {
            return None;
        }
        self.slot_base(tensor.slot_id)?
            .checked_add(tensor.storage_offset)
    }
}

fn checked_count(count: u64, limit: u32) -> Result<u32, ParseError> {
    let count = u32::try_from(count).map_err(|_| ParseError::SectionCountTooLarge)?;
    if count > limit {
        Err(ParseError::SectionCountTooLarge)
    } else {
        Ok(count)
    }
}

fn section_slice(artifact: &[u8], section: SectionDesc) -> Result<&[u8], ParseError> {
    let start = usize::try_from(section.offset).map_err(|_| ParseError::SectionOutOfBounds)?;
    let end = usize::try_from(
        section
            .offset
            .checked_add(section.bytes)
            .ok_or(ParseError::SectionLengthOverflow)?,
    )
    .map_err(|_| ParseError::SectionOutOfBounds)?;
    artifact
        .get(start..end)
        .ok_or(ParseError::SectionOutOfBounds)
}

fn record(bytes: &[u8], index: u32, stride: usize) -> Option<&[u8]> {
    let start = usize::try_from(index).ok()?.checked_mul(stride)?;
    bytes.get(start..start.checked_add(stride)?)
}

fn parse_phases(
    bytes: &[u8],
    op_count: u32,
    max_arena_bytes: u64,
) -> Result<[PhasePlan; 2], ParseError> {
    let phase0 = decode_phase(record(bytes, 0, PHASE_RECORD_BYTES).ok_or(ParseError::BadPhase)?)?;
    let phase1 = decode_phase(record(bytes, 1, PHASE_RECORD_BYTES).ok_or(ParseError::BadPhase)?)?;
    if phase0.phase != Phase::Phase0
        || phase0.runtime_sized
        || phase0.op_start != 0
        || phase0.op_end != phase1.op_start
        || phase0.arena_min_bytes != phase0.arena_max_bytes
        || phase0.frame_count_min != 0
        || phase0.frame_count_max != 0
        || phase1.phase != Phase::Phase1
        || !phase1.runtime_sized
        || phase1.op_end != op_count
        || phase1.frame_count_min > phase1.frame_count_max
        || phase0.arena_max_bytes > max_arena_bytes
        || phase1.arena_max_bytes > max_arena_bytes
    {
        return Err(ParseError::BadPhase);
    }
    for phase in [phase0, phase1] {
        if phase.op_start > phase.op_end
            || phase.arena_min_bytes > phase.arena_max_bytes
            || phase.arena_alignment != ARENA_ALIGNMENT
            || !validate_aligned_size(phase.arena_min_bytes, ARENA_ALIGNMENT)
            || !validate_aligned_size(phase.arena_max_bytes, ARENA_ALIGNMENT)
        {
            return Err(ParseError::BadPhase);
        }
    }
    Ok([phase0, phase1])
}

fn decode_phase(bytes: &[u8]) -> Result<PhasePlan, ParseError> {
    if bytes.len() != PHASE_RECORD_BYTES
        || read_u16(bytes, 2) != Some(0)
        || read_u32(bytes, 12) != Some(0)
        || read_u32(bytes, 44) != Some(0)
    {
        return Err(ParseError::BadPhase);
    }
    let phase = Phase::try_from(bytes[0]).map_err(|_| ParseError::BadPhase)?;
    if phase == Phase::Shared || bytes[1] & !PHASE_FLAG_RUNTIME_SIZED != 0 {
        return Err(ParseError::BadPhase);
    }
    Ok(PhasePlan {
        phase,
        runtime_sized: bytes[1] & PHASE_FLAG_RUNTIME_SIZED != 0,
        op_start: read_u32(bytes, 4).ok_or(ParseError::BadPhase)?,
        op_end: read_u32(bytes, 8).ok_or(ParseError::BadPhase)?,
        arena_min_bytes: read_u64(bytes, 16).ok_or(ParseError::BadPhase)?,
        arena_max_bytes: read_u64(bytes, 24).ok_or(ParseError::BadPhase)?,
        arena_alignment: read_u32(bytes, 32).ok_or(ParseError::BadPhase)?,
        frame_count_min: read_u32(bytes, 36).ok_or(ParseError::BadPhase)?,
        frame_count_max: read_u32(bytes, 40).ok_or(ParseError::BadPhase)?,
    })
}

fn decode_slot(bytes: &[u8]) -> Result<SlotDesc, ArenaPlanError> {
    if bytes.len() != SLOT_RECORD_BYTES || !is_zero(&bytes[40..64]) {
        return Err(ArenaPlanError::UnknownFlags);
    }
    let kind = SlotKind::try_from(bytes[0]).map_err(|_| ArenaPlanError::UnknownSlotKind)?;
    let phase = Phase::try_from(bytes[1]).map_err(|_| ArenaPlanError::UnknownPhase)?;
    if read_u16(bytes, 2) != Some(0) {
        return Err(ArenaPlanError::UnknownFlags);
    }
    Ok(SlotDesc {
        kind,
        phase,
        alignment: read_u32(bytes, 4).ok_or(ArenaPlanError::UnknownFlags)?,
        fixed_offset: read_u64(bytes, 8).ok_or(ArenaPlanError::UnknownFlags)?,
        byte_multiplier: read_u64(bytes, 16).ok_or(ArenaPlanError::UnknownFlags)?,
        byte_addend: read_i64(bytes, 24).ok_or(ArenaPlanError::UnknownFlags)?,
        live_start: read_u32(bytes, 32).ok_or(ArenaPlanError::UnknownFlags)?,
        live_end: read_u32(bytes, 36).ok_or(ArenaPlanError::UnknownFlags)?,
    })
}

fn validate_slot(slot: SlotDesc, phases: [PhasePlan; 2]) -> Result<(), ArenaPlanError> {
    if slot.alignment == 0 || !slot.alignment.is_power_of_two() || slot.alignment > ARENA_ALIGNMENT
    {
        return Err(ArenaPlanError::InvalidAlignment);
    }
    let operation_limit = phases[1].op_end.saturating_add(1);
    if slot.live_start >= slot.live_end || slot.live_end > operation_limit {
        return Err(ArenaPlanError::InvalidLiveness);
    }
    match (slot.kind, slot.phase) {
        (SlotKind::Fixed, Phase::Phase0) => {
            if slot.byte_multiplier != 0
                || slot.fixed_offset % u64::from(slot.alignment) != 0
                || slot.live_end > phases[0].op_end
            {
                return Err(ArenaPlanError::InvalidPhaseKind);
            }
            let end = slot
                .fixed_offset
                .checked_add(slot.bytes_at(0)?)
                .ok_or(ArenaPlanError::SizeOverflow)?;
            if end > phases[0].arena_min_bytes {
                return Err(ArenaPlanError::SlotOutOfPhaseBounds);
            }
        }
        (SlotKind::Fixed, Phase::Shared) => {
            if slot.byte_multiplier != 0
                || slot.fixed_offset % u64::from(slot.alignment) != 0
                || slot.live_start >= phases[0].op_end
                || slot.live_end <= phases[1].op_start
            {
                return Err(ArenaPlanError::InvalidPhaseKind);
            }
            let end = slot
                .fixed_offset
                .checked_add(slot.bytes_at(0)?)
                .ok_or(ArenaPlanError::SizeOverflow)?;
            if end > phases[0].arena_min_bytes || end > phases[1].arena_min_bytes {
                return Err(ArenaPlanError::SlotOutOfPhaseBounds);
            }
        }
        (SlotKind::Dynamic, Phase::Phase1) => {
            if slot.fixed_offset != 0 || slot.live_start < phases[1].op_start {
                return Err(ArenaPlanError::InvalidPhaseKind);
            }
            let _ = slot.bytes_at(phases[1].frame_count_min)?;
            let _ = slot.bytes_at(phases[1].frame_count_max)?;
        }
        _ => return Err(ArenaPlanError::InvalidPhaseKind),
    }
    Ok(())
}

fn decode_tensor(bytes: &[u8]) -> Result<TensorDesc, TensorError> {
    if bytes.len() != TENSOR_RECORD_BYTES
        || !is_zero(&bytes[81..84])
        || read_u32(bytes, 100) != Some(0)
        || !is_zero(&bytes[104..128])
    {
        return Err(TensorError::InvalidStorageFields);
    }
    let dtype = DType::try_from(bytes[0]).map_err(|_| TensorError::UnknownDType)?;
    let storage = StorageKind::try_from(bytes[2]).map_err(|_| TensorError::UnknownStorage)?;
    let phase = Phase::try_from(bytes[3]).map_err(|_| TensorError::UnknownPhase)?;
    let flags = TensorFlags::from_bits(read_u32(bytes, 4).ok_or(TensorError::UnknownFlags)?)
        .ok_or(TensorError::UnknownFlags)?;
    let mut dims = [0u32; 4];
    let mut strides = [0u64; 4];
    for index in 0..4 {
        dims[index] = read_u32(bytes, 32 + index * 4).ok_or(TensorError::UnknownFlags)?;
        strides[index] = read_u64(bytes, 48 + index * 8).ok_or(TensorError::UnknownFlags)?;
    }
    Ok(TensorDesc {
        dtype,
        rank: bytes[1],
        storage,
        phase,
        flags,
        slot_id: read_u32(bytes, 8).ok_or(TensorError::InvalidStorageFields)?,
        view_of: read_u32(bytes, 12).ok_or(TensorError::InvalidStorageFields)?,
        storage_offset: read_u64(bytes, 16).ok_or(TensorError::InvalidStorageFields)?,
        byte_capacity: read_u64(bytes, 24).ok_or(TensorError::InvalidStorageFields)?,
        max_dims: dims,
        max_byte_strides: strides,
        symbolic_dim: bytes[80],
        frame_multiplier: read_u32(bytes, 84).ok_or(TensorError::InvalidStorageFields)?,
        frame_addend: read_i64(bytes, 88).ok_or(TensorError::InvalidStorageFields)?,
        guaranteed_alignment: read_u32(bytes, 96).ok_or(TensorError::InvalidStorageFields)?,
    })
}

fn decode_op(bytes: &[u8]) -> Result<OpDesc, OpError> {
    if bytes.len() != OP_RECORD_BYTES || !is_zero(&bytes[5..8]) || !is_zero(&bytes[32..40]) {
        return Err(OpError::ReservedNonZero);
    }
    let opcode = OpCode::try_from(read_u16(bytes, 0).ok_or(OpError::UnknownOpcode)?)
        .map_err(|_| OpError::UnknownOpcode)?;
    let flags = read_u16(bytes, 2).ok_or(OpError::UnknownFlags)?;
    if flags & !OP_FLAG_IN_PLACE != 0 {
        return Err(OpError::UnknownFlags);
    }
    let phase = Phase::try_from(bytes[4]).map_err(|_| OpError::UnknownPhase)?;
    if phase == Phase::Shared {
        return Err(OpError::UnknownPhase);
    }
    let output_count = read_u16(bytes, 14).ok_or(OpError::EmptyOutputs)?;
    if output_count == 0 {
        return Err(OpError::EmptyOutputs);
    }
    let work_units = read_u32(bytes, 28).ok_or(OpError::EmptyWork)?;
    if work_units == 0 {
        return Err(OpError::EmptyWork);
    }
    Ok(OpDesc {
        opcode,
        flags,
        phase,
        binding_start: read_u32(bytes, 8).ok_or(OpError::BindingRangeOverflow)?,
        input_count: read_u16(bytes, 12).ok_or(OpError::BindingRangeOverflow)?,
        output_count,
        attribute_offset: read_u64(bytes, 16).ok_or(OpError::AttributeRangeOverflow)?,
        attribute_len: read_u32(bytes, 24).ok_or(OpError::AttributeRangeOverflow)?,
        work_units,
    })
}

fn slot_frame_endpoints(slot: SlotDesc, phases: [PhasePlan; 2]) -> [u32; 2] {
    if slot.phase == Phase::Phase1 {
        [phases[1].frame_count_min, phases[1].frame_count_max]
    } else {
        [0, 0]
    }
}

fn ranges_overlap(first_start: u64, first_end: u64, second_start: u64, second_end: u64) -> bool {
    first_start < second_end && second_start < first_end
}

fn storage_overlaps(first: ResolvedStorage, second: ResolvedStorage) -> Option<bool> {
    if first.owner != second.owner {
        return Some(false);
    }
    let first_end = first.offset.checked_add(first.span)?;
    let second_end = second.offset.checked_add(second.span)?;
    Some(ranges_overlap(first.offset, first_end, second.offset, second_end))
}
