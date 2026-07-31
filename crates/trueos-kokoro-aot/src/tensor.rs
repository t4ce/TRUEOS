use core::convert::TryFrom;

use crate::format::{ARENA_ALIGNMENT, NO_SLOT, NO_TENSOR, STATIC_DIM, checked_align_up};

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DType {
    F32 = 1,
    I32 = 2,
    I64 = 3,
    U8 = 4,
    I8 = 5,
    Bool = 6,
}

impl DType {
    pub const fn element_bytes(self) -> u64 {
        match self {
            Self::F32 | Self::I32 => 4,
            Self::I64 => 8,
            Self::U8 | Self::I8 | Self::Bool => 1,
        }
    }
}

impl TryFrom<u8> for DType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::F32),
            2 => Ok(Self::I32),
            3 => Ok(Self::I64),
            4 => Ok(Self::U8),
            5 => Ok(Self::I8),
            6 => Ok(Self::Bool),
            _ => Err(()),
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageKind {
    Slot = 1,
    View = 2,
    Constant = 3,
    External = 4,
}

impl TryFrom<u8> for StorageKind {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Slot),
            2 => Ok(Self::View),
            3 => Ok(Self::Constant),
            4 => Ok(Self::External),
            _ => Err(()),
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Phase0 = 0,
    Phase1 = 1,
    Shared = 2,
}

impl TryFrom<u8> for Phase {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Phase0),
            1 => Ok(Self::Phase1),
            2 => Ok(Self::Shared),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TensorFlags(u32);

impl TensorFlags {
    pub const READ_ONLY: Self = Self(1 << 0);
    pub const INPUT: Self = Self(1 << 1);
    pub const OUTPUT: Self = Self(1 << 2);
    pub const ALL_BITS: u32 = Self::READ_ONLY.0 | Self::INPUT.0 | Self::OUTPUT.0;

    pub const fn from_bits(bits: u32) -> Option<Self> {
        if bits & !Self::ALL_BITS == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 == flag.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TensorError {
    UnknownDType,
    UnknownStorage,
    UnknownPhase,
    UnknownFlags,
    RankTooLarge,
    NonCanonicalInactiveDimension,
    NonCanonicalInactiveStride,
    InvalidSymbolicDimension,
    SymbolicExpressionOverflow,
    SymbolicDimensionNegative,
    SymbolicDimensionExceedsMaximum,
    SymbolicMaximumMismatch,
    DynamicTensorOutsidePhase1,
    DynamicViewUnsupported,
    ElementCountOverflow,
    ByteLengthOverflow,
    ByteCapacityMismatch,
    StrideOverflow,
    NonCanonicalContiguousStride,
    InvalidAlignment,
    MisalignedStorage,
    InvalidSlotReference,
    InvalidViewReference,
    InvalidStorageFields,
    ConstantNotReadOnly,
    ConstantOutOfBounds,
    ViewDTypeMismatch,
    ViewPhaseMismatch,
    ViewOutOfBounds,
    ViewOfView,
    WritableStridedView,
    ReadOnlyWrite,
}

/// One validated maximum-capacity tensor descriptor from the sealed program.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TensorDesc {
    pub dtype: DType,
    pub rank: u8,
    pub storage: StorageKind,
    pub phase: Phase,
    pub flags: TensorFlags,
    pub slot_id: u32,
    pub view_of: u32,
    /// Slot-relative offset, view-relative offset, or DATA-relative offset.
    pub storage_offset: u64,
    pub byte_capacity: u64,
    pub max_dims: [u32; 4],
    pub max_byte_strides: [u64; 4],
    pub symbolic_dim: u8,
    pub frame_multiplier: u32,
    pub frame_addend: i64,
    pub guaranteed_alignment: u32,
}

impl TensorDesc {
    pub const fn is_dynamic(self) -> bool {
        self.symbolic_dim != STATIC_DIM
    }

    pub const fn is_read_only(self) -> bool {
        self.flags.contains(TensorFlags::READ_ONLY)
    }

    pub const fn is_input(self) -> bool {
        self.flags.contains(TensorFlags::INPUT)
    }

    pub const fn is_output(self) -> bool {
        self.flags.contains(TensorFlags::OUTPUT)
    }

    pub fn resolve(self, frame_count: u32) -> Result<ResolvedTensorDesc, TensorError> {
        let mut dims = self.max_dims;
        if self.is_dynamic() {
            let index = usize::from(self.symbolic_dim);
            if index >= usize::from(self.rank) {
                return Err(TensorError::InvalidSymbolicDimension);
            }
            let multiplied = i128::from(self.frame_multiplier)
                .checked_mul(i128::from(frame_count))
                .ok_or(TensorError::SymbolicExpressionOverflow)?;
            let value = multiplied
                .checked_add(i128::from(self.frame_addend))
                .ok_or(TensorError::SymbolicExpressionOverflow)?;
            if value < 0 {
                return Err(TensorError::SymbolicDimensionNegative);
            }
            let value =
                u32::try_from(value).map_err(|_| TensorError::SymbolicExpressionOverflow)?;
            if value > self.max_dims[index] {
                return Err(TensorError::SymbolicDimensionExceedsMaximum);
            }
            dims[index] = value;
        }

        let logical_bytes = logical_bytes(self.dtype, self.rank, &dims)?;
        if logical_bytes > self.byte_capacity {
            return Err(TensorError::ByteCapacityMismatch);
        }
        let byte_strides = if self.storage == StorageKind::View {
            self.max_byte_strides
        } else {
            contiguous_strides(self.dtype, self.rank, &dims)?
        };
        let access_span = access_span(self.dtype, self.rank, &dims, &byte_strides)?;
        if access_span > self.byte_capacity && self.storage != StorageKind::View {
            return Err(TensorError::ByteCapacityMismatch);
        }
        Ok(ResolvedTensorDesc {
            dtype: self.dtype,
            rank: self.rank,
            storage: self.storage,
            phase: self.phase,
            flags: self.flags,
            dims,
            byte_strides,
            logical_bytes,
            access_span,
            guaranteed_alignment: self.guaranteed_alignment,
        })
    }

    pub(crate) fn validate_local(
        self,
        phase1_frame_max: u32,
        slot_count: u32,
        tensor_count: u32,
        data_bytes: u64,
    ) -> Result<(), TensorError> {
        if self.rank > 4 {
            return Err(TensorError::RankTooLarge);
        }
        for index in usize::from(self.rank)..4 {
            if self.max_dims[index] != 1 {
                return Err(TensorError::NonCanonicalInactiveDimension);
            }
            if self.max_byte_strides[index] != 0 {
                return Err(TensorError::NonCanonicalInactiveStride);
            }
        }
        if self.guaranteed_alignment == 0
            || !self.guaranteed_alignment.is_power_of_two()
            || self.guaranteed_alignment > ARENA_ALIGNMENT
            || u64::from(self.guaranteed_alignment) < self.dtype.element_bytes()
        {
            return Err(TensorError::InvalidAlignment);
        }

        if self.is_dynamic() {
            if self.phase != Phase::Phase1 {
                return Err(TensorError::DynamicTensorOutsidePhase1);
            }
            if self.storage == StorageKind::View {
                return Err(TensorError::DynamicViewUnsupported);
            }
            if usize::from(self.symbolic_dim) >= usize::from(self.rank)
                || self.frame_multiplier == 0
            {
                return Err(TensorError::InvalidSymbolicDimension);
            }
            let resolved = self.resolve(phase1_frame_max)?;
            if resolved.dims[usize::from(self.symbolic_dim)]
                != self.max_dims[usize::from(self.symbolic_dim)]
            {
                return Err(TensorError::SymbolicMaximumMismatch);
            }
        } else if self.frame_multiplier != 0 || self.frame_addend != 0 {
            return Err(TensorError::InvalidSymbolicDimension);
        }

        let max_logical = logical_bytes(self.dtype, self.rank, &self.max_dims)?;
        if max_logical != self.byte_capacity {
            return Err(TensorError::ByteCapacityMismatch);
        }
        if self.storage != StorageKind::View {
            let expected = contiguous_strides(self.dtype, self.rank, &self.max_dims)?;
            if expected != self.max_byte_strides {
                return Err(TensorError::NonCanonicalContiguousStride);
            }
        } else {
            let _ = access_span(self.dtype, self.rank, &self.max_dims, &self.max_byte_strides)?;
            if !is_contiguous(self.dtype, self.rank, &self.max_dims, &self.max_byte_strides)
                && !self.is_read_only()
            {
                return Err(TensorError::WritableStridedView);
            }
        }

        match self.storage {
            StorageKind::Slot => {
                if self.slot_id >= slot_count
                    || self.view_of != NO_TENSOR
                    || self.storage_offset % u64::from(self.guaranteed_alignment) != 0
                {
                    return Err(TensorError::InvalidSlotReference);
                }
            }
            StorageKind::View => {
                if self.slot_id != NO_SLOT || self.view_of >= tensor_count {
                    return Err(TensorError::InvalidViewReference);
                }
            }
            StorageKind::Constant => {
                if self.slot_id != NO_SLOT || self.view_of != NO_TENSOR || !self.is_read_only() {
                    return Err(TensorError::ConstantNotReadOnly);
                }
                let end = self
                    .storage_offset
                    .checked_add(self.byte_capacity)
                    .ok_or(TensorError::ByteLengthOverflow)?;
                if end > data_bytes {
                    return Err(TensorError::ConstantOutOfBounds);
                }
            }
            StorageKind::External => {
                if self.slot_id != NO_SLOT || self.view_of != NO_TENSOR || self.storage_offset != 0
                {
                    return Err(TensorError::InvalidStorageFields);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedTensorDesc {
    pub dtype: DType,
    pub rank: u8,
    pub storage: StorageKind,
    pub phase: Phase,
    pub flags: TensorFlags,
    pub dims: [u32; 4],
    pub byte_strides: [u64; 4],
    pub logical_bytes: u64,
    pub access_span: u64,
    pub guaranteed_alignment: u32,
}

impl ResolvedTensorDesc {
    pub fn is_contiguous(self) -> bool {
        is_contiguous(self.dtype, self.rank, &self.dims, &self.byte_strides)
    }

    pub fn materialization(
        self,
        requirement: LayoutRequirement,
    ) -> Result<Materialization, TensorError> {
        match requirement {
            LayoutRequirement::StridedRead => Ok(Materialization::Direct),
            LayoutRequirement::ContiguousRead { alignment } => {
                validate_requested_alignment(alignment)?;
                if self.is_contiguous() && self.guaranteed_alignment >= alignment {
                    Ok(Materialization::Direct)
                } else {
                    Ok(Materialization::Required {
                        bytes: self.logical_bytes,
                        alignment,
                    })
                }
            }
            LayoutRequirement::ContiguousWrite { alignment } => {
                validate_requested_alignment(alignment)?;
                if self.flags.contains(TensorFlags::READ_ONLY) {
                    return Err(TensorError::ReadOnlyWrite);
                }
                if self.is_contiguous() && self.guaranteed_alignment >= alignment {
                    Ok(Materialization::Direct)
                } else {
                    Ok(Materialization::Required {
                        bytes: self.logical_bytes,
                        alignment,
                    })
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutRequirement {
    StridedRead,
    ContiguousRead { alignment: u32 },
    ContiguousWrite { alignment: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Materialization {
    Direct,
    Required { bytes: u64, alignment: u32 },
}

fn validate_requested_alignment(alignment: u32) -> Result<(), TensorError> {
    if alignment == 0 || !alignment.is_power_of_two() || alignment > ARENA_ALIGNMENT {
        Err(TensorError::InvalidAlignment)
    } else {
        Ok(())
    }
}

pub(crate) fn logical_bytes(dtype: DType, rank: u8, dims: &[u32; 4]) -> Result<u64, TensorError> {
    let mut elements = 1u64;
    for dim in &dims[..usize::from(rank)] {
        elements = elements
            .checked_mul(u64::from(*dim))
            .ok_or(TensorError::ElementCountOverflow)?;
    }
    elements
        .checked_mul(dtype.element_bytes())
        .ok_or(TensorError::ByteLengthOverflow)
}

pub(crate) fn contiguous_strides(
    dtype: DType,
    rank: u8,
    dims: &[u32; 4],
) -> Result<[u64; 4], TensorError> {
    let mut strides = [0u64; 4];
    let mut stride = dtype.element_bytes();
    for index in (0..usize::from(rank)).rev() {
        strides[index] = stride;
        stride = stride
            .checked_mul(u64::from(dims[index]))
            .ok_or(TensorError::StrideOverflow)?;
    }
    Ok(strides)
}

pub(crate) fn access_span(
    dtype: DType,
    rank: u8,
    dims: &[u32; 4],
    strides: &[u64; 4],
) -> Result<u64, TensorError> {
    if dims[..usize::from(rank)].contains(&0) {
        return Ok(0);
    }
    let mut last = 0u64;
    for index in 0..usize::from(rank) {
        let contribution = u64::from(dims[index] - 1)
            .checked_mul(strides[index])
            .ok_or(TensorError::StrideOverflow)?;
        last = last
            .checked_add(contribution)
            .ok_or(TensorError::StrideOverflow)?;
    }
    last.checked_add(dtype.element_bytes())
        .ok_or(TensorError::StrideOverflow)
}

pub(crate) fn is_contiguous(dtype: DType, rank: u8, dims: &[u32; 4], strides: &[u64; 4]) -> bool {
    contiguous_strides(dtype, rank, dims).is_ok_and(|expected| expected == *strides)
}

pub(crate) fn phase_compatible(owner: Phase, view: Phase) -> bool {
    owner == view || owner == Phase::Shared
}

pub(crate) fn checked_storage_end(offset: u64, span: u64) -> Result<u64, TensorError> {
    offset
        .checked_add(span)
        .ok_or(TensorError::ByteLengthOverflow)
}

pub(crate) fn effective_view_alignment(owner_alignment: u32, offset: u64) -> u32 {
    if offset == 0 {
        return owner_alignment;
    }
    let offset_alignment = 1u64 << offset.trailing_zeros().min(31);
    owner_alignment.min(offset_alignment as u32)
}

pub(crate) fn validate_aligned_size(value: u64, alignment: u32) -> bool {
    checked_align_up(value, alignment) == Some(value)
}
