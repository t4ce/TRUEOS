#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

//! Allocation-free, phase-aware tensor memory for sealed Kokoro programs.
//!
//! The AOT parser proves storage ownership and maximum spans, while the
//! executor owns exact logical shapes and phase-one slot admission. This crate
//! joins those proofs to caller-owned memory. Public access is callback-scoped
//! or guarded by runtime leases, so no safe tensor borrow can outlive the
//! validated arena, DATA section, or external binding that backs it.

#[cfg(not(target_endian = "little"))]
compile_error!("the Kokoro AOT typed-memory ABI requires a little-endian target");

use core::array;
use core::cell::Cell;
use core::marker::PhantomData;
use core::mem::{align_of, size_of};
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;
use core::slice;

use trueos_kokoro_aot::{
    ARENA_ALIGNMENT, DType, OpDesc, Phase, Program, SectionKind, SlotKind, StorageKind,
    StorageOwner, TensorDesc, UNRESOLVED_SLOT_BASE,
};
use trueos_kokoro_exec::{ResolvedPhase, RuntimeShape, ShapeError, TensorShapeTable};

/// All failures are detected before a callback or lease is issued.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryError {
    ForeignShapeTable,
    ForeignExternalBindings,
    Shape(ShapeError),
    TensorOutOfBounds,
    OperationOutOfBounds,
    WrongPhase,
    InvalidStorage,
    AddressSpaceOverflow,
    LogicalSpanExceedsCapacity,
    NonContiguousView,
    ArenaMisaligned,
    ArenaTooSmall,
    DataMisaligned,
    TensorMisaligned,
    PhaseAdmissionMismatch,
    SlotBasesTooSmall,
    UnresolvedSlot,
    SlotBaseMisaligned,
    SlotOutOfBounds,
    OverlappingLiveSlots,
    ExternalTableFull,
    DuplicateExternal,
    ExternalNotBound,
    ExternalNotInput,
    ExternalNotOutput,
    ExternalWriteRequired,
    ExternalBufferTooSmall,
    ExternalMisaligned,
    DTypeMismatch,
    ReadOnlyWrite,
    BindingCapacityTooSmall,
    BindingOutOfBounds,
    OutputOverlap,
    InputOutputOverlap,
    InPlaceNotSealed,
    BorrowConflict,
    BorrowCountOverflow,
    InvalidBoolValue,
}

mod element_seal {
    pub trait Sealed {}

    impl Sealed for f32 {}
    impl Sealed for i32 {}
    impl Sealed for i64 {}
    impl Sealed for u8 {}
    impl Sealed for i8 {}
    impl Sealed for bool {}
}

/// Rust element types with an exact sealed-AOT dtype representation.
///
/// The trait is sealed because adding an implementation would change the
/// validity proof used when raw backing bytes become a typed slice.
pub trait TensorElement: element_seal::Sealed + Copy + 'static {
    const DTYPE: DType;
    const VALIDATE_BOOL: bool = false;
}

impl TensorElement for f32 {
    const DTYPE: DType = DType::F32;
}

impl TensorElement for i32 {
    const DTYPE: DType = DType::I32;
}

impl TensorElement for i64 {
    const DTYPE: DType = DType::I64;
}

impl TensorElement for u8 {
    const DTYPE: DType = DType::U8;
}

impl TensorElement for i8 {
    const DTYPE: DType = DType::I8;
}

impl TensorElement for bool {
    const DTYPE: DType = DType::Bool;
    const VALIDATE_BOOL: bool = true;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExternalMode {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Copy, Debug)]
struct ExternalEntry {
    tensor_id: u32,
    pointer: NonNull<u8>,
    bytes: usize,
    dtype: DType,
    mode: ExternalMode,
}

impl ExternalEntry {
    const EMPTY: Self = Self {
        tensor_id: u32::MAX,
        pointer: NonNull::dangling(),
        bytes: 0,
        dtype: DType::U8,
        mode: ExternalMode::ReadOnly,
    };

    const fn writable(self) -> bool {
        matches!(self.mode, ExternalMode::ReadWrite)
    }
}

/// Caller-owned fixed-capacity external tensor bindings.
///
/// Input bindings retain a shared borrow and output bindings retain an
/// exclusive borrow for `'buffers`. A tensor ID can be bound exactly once.
#[derive(Debug)]
pub struct ExternalBindings<'buffers, const CAPACITY: usize> {
    entries: [ExternalEntry; CAPACITY],
    len: usize,
    artifact_sha256: Option<[u8; 32]>,
    _buffers: PhantomData<&'buffers mut [u8]>,
}

impl<'buffers, const CAPACITY: usize> ExternalBindings<'buffers, CAPACITY> {
    pub const fn new() -> Self {
        Self {
            entries: [ExternalEntry::EMPTY; CAPACITY],
            len: 0,
            artifact_sha256: None,
            _buffers: PhantomData,
        }
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub const fn capacity(&self) -> usize {
        CAPACITY
    }

    /// Bind a sealed graph input to an immutable typed buffer.
    pub fn bind_input<T: TensorElement, const SHAPES: usize>(
        &mut self,
        program: &Program<'_>,
        shapes: &TensorShapeTable<SHAPES>,
        tensor_id: u32,
        buffer: &'buffers [T],
    ) -> Result<(), MemoryError> {
        let descriptor = external_descriptor(program, tensor_id)?;
        if !descriptor.is_input() {
            return Err(MemoryError::ExternalNotInput);
        }
        if descriptor.is_output() {
            return Err(MemoryError::ExternalWriteRequired);
        }
        self.bind::<T, SHAPES>(
            program,
            shapes,
            tensor_id,
            descriptor,
            NonNull::new(buffer.as_ptr().cast_mut().cast()).expect("slice pointers are non-null"),
            buffer.len(),
            ExternalMode::ReadOnly,
        )
    }

    /// Bind a sealed graph output, or an input/output tensor, to a mutable
    /// typed buffer.
    pub fn bind_output<T: TensorElement, const SHAPES: usize>(
        &mut self,
        program: &Program<'_>,
        shapes: &TensorShapeTable<SHAPES>,
        tensor_id: u32,
        buffer: &'buffers mut [T],
    ) -> Result<(), MemoryError> {
        let descriptor = external_descriptor(program, tensor_id)?;
        if !descriptor.is_output() {
            return Err(MemoryError::ExternalNotOutput);
        }
        if descriptor.is_read_only() {
            return Err(MemoryError::ReadOnlyWrite);
        }
        self.bind::<T, SHAPES>(
            program,
            shapes,
            tensor_id,
            descriptor,
            NonNull::new(buffer.as_mut_ptr().cast()).expect("slice pointers are non-null"),
            buffer.len(),
            ExternalMode::ReadWrite,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn bind<T: TensorElement, const SHAPES: usize>(
        &mut self,
        program: &Program<'_>,
        shapes: &TensorShapeTable<SHAPES>,
        tensor_id: u32,
        descriptor: TensorDesc,
        pointer: NonNull<u8>,
        elements: usize,
        mode: ExternalMode,
    ) -> Result<(), MemoryError> {
        self.check_program(program)?;
        if self.entries[..self.len]
            .iter()
            .any(|entry| entry.tensor_id == tensor_id)
        {
            return Err(MemoryError::DuplicateExternal);
        }
        if self.len == CAPACITY {
            return Err(MemoryError::ExternalTableFull);
        }
        if descriptor.dtype != T::DTYPE {
            return Err(MemoryError::DTypeMismatch);
        }
        if !(pointer.as_ptr() as usize).is_multiple_of(descriptor.guaranteed_alignment as usize) {
            return Err(MemoryError::ExternalMisaligned);
        }
        let shape = shapes.shape(program, tensor_id).map_err(map_shape_error)?;
        let logical_bytes = shape
            .logical_bytes(descriptor.dtype)
            .map_err(MemoryError::Shape)?;
        if logical_bytes > descriptor.byte_capacity {
            return Err(MemoryError::LogicalSpanExceedsCapacity);
        }
        let bytes = elements
            .checked_mul(size_of::<T>())
            .ok_or(MemoryError::AddressSpaceOverflow)?;
        if u64::try_from(bytes).map_err(|_| MemoryError::AddressSpaceOverflow)? < logical_bytes {
            return Err(MemoryError::ExternalBufferTooSmall);
        }

        self.entries[self.len] = ExternalEntry {
            tensor_id,
            pointer,
            bytes,
            dtype: T::DTYPE,
            mode,
        };
        self.len += 1;
        self.artifact_sha256 = Some(*program.artifact_sha256());
        Ok(())
    }

    fn check_program(&self, program: &Program<'_>) -> Result<(), MemoryError> {
        if self
            .artifact_sha256
            .is_some_and(|hash| hash != *program.artifact_sha256())
        {
            Err(MemoryError::ForeignExternalBindings)
        } else {
            Ok(())
        }
    }

    fn entry(&self, tensor_id: u32) -> Option<ExternalEntry> {
        self.entries[..self.len]
            .iter()
            .copied()
            .find(|entry| entry.tensor_id == tensor_id)
    }
}

impl<const CAPACITY: usize> Default for ExternalBindings<'_, CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

fn external_descriptor(program: &Program<'_>, tensor_id: u32) -> Result<TensorDesc, MemoryError> {
    let descriptor = program
        .tensor(tensor_id)
        .ok_or(MemoryError::TensorOutOfBounds)?;
    if descriptor.storage != StorageKind::External {
        return Err(MemoryError::InvalidStorage);
    }
    Ok(descriptor)
}

/// Validated tensor memory for one active execution phase.
///
/// `BINDINGS` is the maximum input-plus-output count accepted by
/// [`with_op`](Self::with_op). It bounds both stack metadata and runtime lease
/// state; no allocation occurs.
pub struct TensorMemory<
    'memory,
    'artifact,
    'buffers,
    const SHAPES: usize,
    const EXTERNALS: usize,
    const BINDINGS: usize,
> {
    program: &'memory Program<'artifact>,
    shapes: &'memory mut TensorShapeTable<SHAPES>,
    arena: NonNull<u8>,
    arena_len: usize,
    externals: &'memory ExternalBindings<'buffers, EXTERNALS>,
    phase: Phase,
    frame_count: u32,
    arena_bytes: u64,
    slot_bases: &'memory [u64],
    _arena: PhantomData<&'memory mut [u8]>,
    _binding_capacity: PhantomData<[(); BINDINGS]>,
}

impl<
    'memory,
    'artifact,
    'buffers,
    const SHAPES: usize,
    const EXTERNALS: usize,
    const BINDINGS: usize,
> TensorMemory<'memory, 'artifact, 'buffers, SHAPES, EXTERNALS, BINDINGS>
{
    /// Construct phase-zero memory from the sealed fixed arena plan.
    pub fn phase_zero(
        program: &'memory Program<'artifact>,
        shapes: &'memory mut TensorShapeTable<SHAPES>,
        arena: &'memory mut [u8],
        externals: &'memory mut ExternalBindings<'buffers, EXTERNALS>,
    ) -> Result<Self, MemoryError> {
        let arena_bytes = program
            .phase(Phase::Phase0)
            .ok_or(MemoryError::PhaseAdmissionMismatch)?
            .arena_min_bytes;
        Self::construct(program, shapes, arena, externals, Phase::Phase0, 0, arena_bytes, &[])
    }

    /// Construct phase-one memory from facts emitted by [`trueos_kokoro_exec::Executor`].
    ///
    /// The slot bases are independently checked for count, resolution,
    /// alignment, bounds, liveness overlap, and exact admitted high-water mark.
    #[allow(clippy::too_many_arguments)]
    pub fn phase_one(
        program: &'memory Program<'artifact>,
        shapes: &'memory mut TensorShapeTable<SHAPES>,
        arena: &'memory mut [u8],
        admission: ResolvedPhase,
        slot_bases: &'memory [u64],
        externals: &'memory mut ExternalBindings<'buffers, EXTERNALS>,
    ) -> Result<Self, MemoryError> {
        validate_phase_one(program, admission, slot_bases)?;
        Self::construct(
            program,
            shapes,
            arena,
            externals,
            Phase::Phase1,
            admission.frame_count(),
            admission.arena_bytes(),
            slot_bases,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn construct(
        program: &'memory Program<'artifact>,
        shapes: &'memory mut TensorShapeTable<SHAPES>,
        arena: &'memory mut [u8],
        externals: &'memory mut ExternalBindings<'buffers, EXTERNALS>,
        phase: Phase,
        frame_count: u32,
        arena_bytes: u64,
        slot_bases: &'memory [u64],
    ) -> Result<Self, MemoryError> {
        validate_shape_table(program, shapes)?;
        externals.check_program(program)?;
        let arena_pointer = NonNull::new(arena.as_mut_ptr()).expect("slice pointers are non-null");
        if !(arena_pointer.as_ptr() as usize).is_multiple_of(ARENA_ALIGNMENT as usize) {
            return Err(MemoryError::ArenaMisaligned);
        }
        let required =
            usize::try_from(arena_bytes).map_err(|_| MemoryError::AddressSpaceOverflow)?;
        if arena.len() < required {
            return Err(MemoryError::ArenaTooSmall);
        }
        if !(program.data().as_ptr() as usize)
            .is_multiple_of(SectionKind::Data.alignment() as usize)
        {
            return Err(MemoryError::DataMisaligned);
        }

        Ok(Self {
            program,
            shapes,
            arena: arena_pointer,
            arena_len: arena.len(),
            externals,
            phase,
            frame_count,
            arena_bytes,
            slot_bases,
            _arena: PhantomData,
            _binding_capacity: PhantomData,
        })
    }

    pub const fn phase(&self) -> Phase {
        self.phase
    }

    pub const fn frame_count(&self) -> u32 {
        self.frame_count
    }

    pub const fn arena_bytes(&self) -> u64 {
        self.arena_bytes
    }

    /// Return one already-declared exact logical shape.
    ///
    /// Dispatchers use this before preparing an operation so output shapes can
    /// be derived from initialized inputs without resolving writable outputs.
    pub fn tensor_shape(&self, tensor_id: u32) -> Result<RuntimeShape, MemoryError> {
        self.shapes
            .shape(self.program, tensor_id)
            .map_err(map_shape_error)
    }

    /// Atomically declare the exact logical output shapes for one operation.
    ///
    /// This is intentionally separate from [`with_op`](Self::with_op): shape
    /// inference may inspect small control tensors first, while `with_op`
    /// remains the point that resolves and alias-checks the complete binding
    /// set. A rejected declaration leaves the shape table unchanged.
    pub fn declare_op_outputs(
        &mut self,
        op_index: u32,
        shapes: &[RuntimeShape],
    ) -> Result<(), MemoryError> {
        let op = self
            .program
            .op(op_index)
            .ok_or(MemoryError::OperationOutOfBounds)?;
        if op.phase != self.phase {
            return Err(MemoryError::WrongPhase);
        }
        self.shapes
            .declare_op_outputs(self.program, op_index, shapes)
            .map_err(map_shape_error)
    }

    /// Read one exact contiguous tensor during a non-escaping callback.
    pub fn with_read<T: TensorElement, R, F>(
        &self,
        tensor_id: u32,
        callback: F,
    ) -> Result<R, MemoryError>
    where
        F: for<'scope> FnOnce(&'scope [T], RuntimeShape) -> R,
    {
        let region = self.resolve_region(tensor_id)?;
        validate_typed_region::<T>(region)?;
        // SAFETY: `resolve_region` proves live backing storage and exact bounds;
        // `validate_typed_region` proves dtype, alignment, length, and bool
        // validity. The higher-ranked callback prevents the slice escaping.
        let values = unsafe { region_slice::<T>(region) };
        Ok(callback(values, region.shape))
    }

    /// Mutate one exact contiguous tensor during a non-escaping callback.
    pub fn with_write<T: TensorElement, R, F>(
        &mut self,
        tensor_id: u32,
        callback: F,
    ) -> Result<R, MemoryError>
    where
        F: for<'scope> FnOnce(&'scope mut [T], RuntimeShape) -> R,
    {
        let region = self.resolve_region(tensor_id)?;
        if !region.writable {
            return Err(MemoryError::ReadOnlyWrite);
        }
        validate_typed_region::<T>(region)?;
        // SAFETY: this method exclusively borrows the entire memory bridge;
        // the validated region is writable, aligned, in-bounds, and valid for
        // T. The callback cannot retain the resulting mutable slice.
        let values = unsafe { region_slice_mut::<T>(region) };
        Ok(callback(values, region.shape))
    }

    /// Resolve every sealed binding, reject physical aliases transactionally,
    /// and run a callback with guarded typed binding access.
    pub fn with_op<R, F>(&mut self, op_index: u32, callback: F) -> Result<R, MemoryError>
    where
        F: for<'scope> FnOnce(&'scope OpAccess<BINDINGS>) -> R,
    {
        let access = self.prepare_op(op_index)?;
        Ok(callback(&access))
    }

    fn prepare_op(&self, op_index: u32) -> Result<OpAccess<BINDINGS>, MemoryError> {
        let op = self
            .program
            .op(op_index)
            .ok_or(MemoryError::OperationOutOfBounds)?;
        if op.phase != self.phase {
            return Err(MemoryError::WrongPhase);
        }
        self.shapes
            .validate_inputs(self.program, op_index)
            .map_err(map_shape_error)?;
        let binding_count = op
            .binding_count()
            .ok_or(MemoryError::BindingCapacityTooSmall)?;
        let binding_count =
            usize::try_from(binding_count).map_err(|_| MemoryError::BindingCapacityTooSmall)?;
        if binding_count > BINDINGS {
            return Err(MemoryError::BindingCapacityTooSmall);
        }

        let mut regions = [Region::EMPTY; BINDINGS];
        let mut tensor_ids = [u32::MAX; BINDINGS];
        for binding in 0..binding_count {
            let tensor_id = self
                .program
                .binding(
                    op.binding_start
                        .checked_add(binding as u32)
                        .ok_or(MemoryError::BindingOutOfBounds)?,
                )
                .ok_or(MemoryError::BindingOutOfBounds)?;
            let region = self.resolve_region(tensor_id)?;
            if binding >= usize::from(op.input_count) && !region.writable {
                return Err(MemoryError::ReadOnlyWrite);
            }
            regions[binding] = region;
            tensor_ids[binding] = tensor_id;
        }
        validate_op_aliases(op, &regions[..binding_count], &tensor_ids[..binding_count])?;

        Ok(OpAccess {
            op,
            regions,
            tensor_ids,
            binding_count,
            borrows: array::from_fn(|_| Cell::new(BorrowState::Unused)),
        })
    }

    fn resolve_region(&self, tensor_id: u32) -> Result<Region, MemoryError> {
        let descriptor = self
            .program
            .tensor(tensor_id)
            .ok_or(MemoryError::TensorOutOfBounds)?;
        if descriptor.phase != Phase::Shared && descriptor.phase != self.phase {
            return Err(MemoryError::WrongPhase);
        }
        let shape = self
            .shapes
            .shape(self.program, tensor_id)
            .map_err(map_shape_error)?;
        let logical_bytes = shape
            .logical_bytes(descriptor.dtype)
            .map_err(MemoryError::Shape)?;
        if logical_bytes > descriptor.byte_capacity {
            return Err(MemoryError::LogicalSpanExceedsCapacity);
        }
        if descriptor.storage == StorageKind::View && !view_is_contiguous(descriptor, shape)? {
            return Err(MemoryError::NonContiguousView);
        }
        let storage = self
            .program
            .resolve_storage(tensor_id)
            .map_err(|_| MemoryError::InvalidStorage)?;
        if logical_bytes > storage.span {
            return Err(MemoryError::LogicalSpanExceedsCapacity);
        }
        let length =
            usize::try_from(logical_bytes).map_err(|_| MemoryError::AddressSpaceOverflow)?;

        let (pointer, writable) = match storage.owner {
            StorageOwner::Slot(slot_id) => {
                let slot = self
                    .program
                    .slot(slot_id)
                    .ok_or(MemoryError::InvalidStorage)?;
                let base = self.slot_base(slot_id, slot.kind, slot.phase)?;
                let relative_end = storage
                    .offset
                    .checked_add(logical_bytes)
                    .ok_or(MemoryError::AddressSpaceOverflow)?;
                if relative_end
                    > slot
                        .bytes_at(self.frame_count)
                        .map_err(|_| MemoryError::SlotOutOfBounds)?
                {
                    return Err(MemoryError::SlotOutOfBounds);
                }
                let absolute = base
                    .checked_add(storage.offset)
                    .ok_or(MemoryError::AddressSpaceOverflow)?;
                let absolute_end = absolute
                    .checked_add(logical_bytes)
                    .ok_or(MemoryError::AddressSpaceOverflow)?;
                if absolute_end > self.arena_bytes || absolute_end > self.arena_len as u64 {
                    return Err(MemoryError::SlotOutOfBounds);
                }
                let offset =
                    usize::try_from(absolute).map_err(|_| MemoryError::AddressSpaceOverflow)?;
                // SAFETY: `absolute_end <= arena_len` proves `offset` is in or
                // one-past the original caller-owned arena allocation.
                let pointer = unsafe { NonNull::new_unchecked(self.arena.as_ptr().add(offset)) };
                (pointer, !descriptor.is_read_only())
            }
            StorageOwner::Constant(_) => {
                let start = usize::try_from(storage.offset)
                    .map_err(|_| MemoryError::AddressSpaceOverflow)?;
                let end = start
                    .checked_add(length)
                    .ok_or(MemoryError::AddressSpaceOverflow)?;
                let bytes = self
                    .program
                    .data()
                    .get(start..end)
                    .ok_or(MemoryError::LogicalSpanExceedsCapacity)?;
                (NonNull::from(bytes).cast(), false)
            }
            StorageOwner::External(owner_id) => {
                let entry = self
                    .externals
                    .entry(owner_id)
                    .ok_or(MemoryError::ExternalNotBound)?;
                if entry.dtype != descriptor.dtype {
                    return Err(MemoryError::DTypeMismatch);
                }
                let start = usize::try_from(storage.offset)
                    .map_err(|_| MemoryError::AddressSpaceOverflow)?;
                let end = start
                    .checked_add(length)
                    .ok_or(MemoryError::AddressSpaceOverflow)?;
                if end > entry.bytes {
                    return Err(MemoryError::ExternalBufferTooSmall);
                }
                // SAFETY: the binding retained the source slice for
                // `'buffers`; `end <= entry.bytes` proves this offset is in or
                // one-past that allocation.
                let pointer = unsafe { NonNull::new_unchecked(entry.pointer.as_ptr().add(start)) };
                (pointer, entry.writable() && !descriptor.is_read_only())
            }
        };

        if !(pointer.as_ptr() as usize).is_multiple_of(descriptor.guaranteed_alignment as usize) {
            return Err(MemoryError::TensorMisaligned);
        }
        let elements = usize::try_from(shape.element_count().map_err(MemoryError::Shape)?)
            .map_err(|_| MemoryError::AddressSpaceOverflow)?;
        Ok(Region {
            pointer,
            bytes: length,
            elements,
            dtype: descriptor.dtype,
            shape,
            writable,
        })
    }

    fn slot_base(
        &self,
        slot_id: u32,
        kind: SlotKind,
        slot_phase: Phase,
    ) -> Result<u64, MemoryError> {
        match self.phase {
            Phase::Phase0 => {
                if kind != SlotKind::Fixed || !matches!(slot_phase, Phase::Phase0 | Phase::Shared) {
                    return Err(MemoryError::WrongPhase);
                }
                self.program
                    .slot(slot_id)
                    .map(|slot| slot.fixed_offset)
                    .ok_or(MemoryError::InvalidStorage)
            }
            Phase::Phase1 => {
                if !matches!(slot_phase, Phase::Phase1 | Phase::Shared) {
                    return Err(MemoryError::WrongPhase);
                }
                self.slot_bases
                    .get(slot_id as usize)
                    .copied()
                    .filter(|base| *base != UNRESOLVED_SLOT_BASE)
                    .ok_or(MemoryError::UnresolvedSlot)
            }
            Phase::Shared => Err(MemoryError::WrongPhase),
        }
    }
}

fn validate_shape_table<const SHAPES: usize>(
    program: &Program<'_>,
    shapes: &TensorShapeTable<SHAPES>,
) -> Result<(), MemoryError> {
    if shapes.tensor_count() != program.tensor_count() {
        return Err(MemoryError::ForeignShapeTable);
    }
    if program.tensor_count() != 0
        && matches!(shapes.shape(program, 0), Err(ShapeError::ForeignProgram))
    {
        return Err(MemoryError::ForeignShapeTable);
    }
    Ok(())
}

fn map_shape_error(error: ShapeError) -> MemoryError {
    if error == ShapeError::ForeignProgram {
        MemoryError::ForeignShapeTable
    } else {
        MemoryError::Shape(error)
    }
}

fn validate_phase_one(
    program: &Program<'_>,
    admission: ResolvedPhase,
    slot_bases: &[u64],
) -> Result<(), MemoryError> {
    if admission.slot_count() != program.slot_count() {
        return Err(MemoryError::PhaseAdmissionMismatch);
    }
    let slot_count = program.slot_count() as usize;
    if slot_bases.len() < slot_count {
        return Err(MemoryError::SlotBasesTooSmall);
    }
    let phase = program
        .phase(Phase::Phase1)
        .ok_or(MemoryError::PhaseAdmissionMismatch)?;
    let frame_count = admission.frame_count();
    if frame_count < phase.frame_count_min || frame_count > phase.frame_count_max {
        return Err(MemoryError::PhaseAdmissionMismatch);
    }

    let mut high_water = 0_u64;
    for slot_id in 0..program.slot_count() {
        let slot = program.slot(slot_id).ok_or(MemoryError::InvalidStorage)?;
        let base = slot_bases[slot_id as usize];
        match (slot.kind, slot.phase) {
            (SlotKind::Fixed, Phase::Phase0) => {
                if base != UNRESOLVED_SLOT_BASE {
                    return Err(MemoryError::PhaseAdmissionMismatch);
                }
                continue;
            }
            (SlotKind::Fixed, Phase::Shared) => {
                if base != slot.fixed_offset {
                    return Err(MemoryError::PhaseAdmissionMismatch);
                }
            }
            (SlotKind::Dynamic, Phase::Phase1) => {
                if base == UNRESOLVED_SLOT_BASE {
                    return Err(MemoryError::UnresolvedSlot);
                }
            }
            _ => return Err(MemoryError::PhaseAdmissionMismatch),
        }
        if !base.is_multiple_of(u64::from(slot.alignment)) {
            return Err(MemoryError::SlotBaseMisaligned);
        }
        let end = base
            .checked_add(
                slot.bytes_at(frame_count)
                    .map_err(|_| MemoryError::SlotOutOfBounds)?,
            )
            .ok_or(MemoryError::AddressSpaceOverflow)?;
        if end > admission.arena_bytes() {
            return Err(MemoryError::SlotOutOfBounds);
        }
        high_water = high_water.max(end);
    }

    for first_id in 0..program.slot_count() {
        let first = program.slot(first_id).ok_or(MemoryError::InvalidStorage)?;
        let first_base = slot_bases[first_id as usize];
        if first_base == UNRESOLVED_SLOT_BASE {
            continue;
        }
        let first_end = first_base
            .checked_add(
                first
                    .bytes_at(frame_count)
                    .map_err(|_| MemoryError::SlotOutOfBounds)?,
            )
            .ok_or(MemoryError::AddressSpaceOverflow)?;
        for second_id in first_id + 1..program.slot_count() {
            let second = program.slot(second_id).ok_or(MemoryError::InvalidStorage)?;
            let second_base = slot_bases[second_id as usize];
            if second_base == UNRESOLVED_SLOT_BASE || !first.liveness_overlaps(second) {
                continue;
            }
            let second_end = second_base
                .checked_add(
                    second
                        .bytes_at(frame_count)
                        .map_err(|_| MemoryError::SlotOutOfBounds)?,
                )
                .ok_or(MemoryError::AddressSpaceOverflow)?;
            if ranges_overlap(first_base, first_end, second_base, second_end) {
                return Err(MemoryError::OverlappingLiveSlots);
            }
        }
    }

    let expected = align_up(high_water.max(phase.arena_min_bytes), ARENA_ALIGNMENT)
        .ok_or(MemoryError::AddressSpaceOverflow)?;
    if admission.arena_bytes() != expected || expected > phase.arena_max_bytes {
        return Err(MemoryError::PhaseAdmissionMismatch);
    }
    Ok(())
}

fn align_up(value: u64, alignment: u32) -> Option<u64> {
    let mask = u64::from(alignment.checked_sub(1)?);
    value.checked_add(mask).map(|value| value & !mask)
}

fn view_is_contiguous(descriptor: TensorDesc, shape: RuntimeShape) -> Result<bool, MemoryError> {
    // Shape-only aliases (Reshape/Squeeze/Unsqueeze) carry canonical strides
    // for their maximum-capacity descriptor. Their invocation-local logical
    // dimensions live in TensorShapeTable and therefore may be smaller; the
    // contiguous strides are implicit for that logical shape rather than a
    // second mutable descriptor. Preserve support for an explicitly encoded
    // runtime-sized stride set as well, but never admit a genuinely strided
    // maximum view as contiguous merely because one degenerate shape happens
    // to hide its stride.
    if strides_are_contiguous(
        descriptor,
        &descriptor.max_dims[..usize::from(descriptor.rank)],
    )? {
        return Ok(true);
    }
    strides_are_contiguous(descriptor, shape.dims())
}

fn strides_are_contiguous(descriptor: TensorDesc, dims: &[u32]) -> Result<bool, MemoryError> {
    let mut stride = descriptor.dtype.element_bytes();
    for axis in (0..usize::from(descriptor.rank)).rev() {
        if descriptor.max_byte_strides[axis] != stride {
            return Ok(false);
        }
        stride = stride
            .checked_mul(u64::from(dims[axis]))
            .ok_or(MemoryError::AddressSpaceOverflow)?;
    }
    Ok(true)
}

#[derive(Clone, Copy, Debug)]
struct Region {
    pointer: NonNull<u8>,
    bytes: usize,
    elements: usize,
    dtype: DType,
    shape: RuntimeShape,
    writable: bool,
}

impl Region {
    const EMPTY: Self = Self {
        pointer: NonNull::dangling(),
        bytes: 0,
        elements: 0,
        dtype: DType::U8,
        shape: RuntimeShape::scalar(),
        writable: false,
    };

    fn end_address(self) -> Result<usize, MemoryError> {
        (self.pointer.as_ptr() as usize)
            .checked_add(self.bytes)
            .ok_or(MemoryError::AddressSpaceOverflow)
    }
}

fn validate_typed_region<T: TensorElement>(region: Region) -> Result<(), MemoryError> {
    if region.dtype != T::DTYPE || size_of::<T>() as u64 != region.dtype.element_bytes() {
        return Err(MemoryError::DTypeMismatch);
    }
    if !(region.pointer.as_ptr() as usize).is_multiple_of(align_of::<T>()) {
        return Err(MemoryError::TensorMisaligned);
    }
    if region
        .elements
        .checked_mul(size_of::<T>())
        .ok_or(MemoryError::AddressSpaceOverflow)?
        != region.bytes
    {
        return Err(MemoryError::LogicalSpanExceedsCapacity);
    }
    if T::VALIDATE_BOOL && !valid_bool_bytes(region.pointer, region.bytes) {
        return Err(MemoryError::InvalidBoolValue);
    }
    Ok(())
}

fn valid_bool_bytes(pointer: NonNull<u8>, bytes: usize) -> bool {
    for index in 0..bytes {
        // SAFETY: every Region is proved in-bounds for `bytes`; reading its
        // representation as u8 is valid for every supported backing store.
        let value = unsafe { pointer.as_ptr().add(index).read() };
        if value > 1 {
            return false;
        }
    }
    true
}

/// Convert a validated region to a callback- or lease-scoped shared slice.
///
/// # Safety
///
/// The caller must keep every backing allocation alive and immutable for `'a`,
/// and must first call `validate_typed_region::<T>`.
unsafe fn region_slice<'a, T: TensorElement>(region: Region) -> &'a [T] {
    let pointer = if region.elements == 0 {
        NonNull::<T>::dangling().as_ptr()
    } else {
        region.pointer.cast::<T>().as_ptr()
    };
    // SAFETY: required by this function's contract; zero-length slices use an
    // aligned non-null dangling pointer as required by `from_raw_parts`.
    unsafe { slice::from_raw_parts(pointer, region.elements) }
}

/// Convert a validated writable region to a callback- or lease-scoped slice.
///
/// # Safety
///
/// The caller must additionally hold exclusive access to this physical region
/// for `'a` and ensure that `region.writable` is true.
unsafe fn region_slice_mut<'a, T: TensorElement>(region: Region) -> &'a mut [T] {
    let pointer = if region.elements == 0 {
        NonNull::<T>::dangling().as_ptr()
    } else {
        region.pointer.cast::<T>().as_ptr()
    };
    // SAFETY: required by this function's contract; the pointer is non-null
    // and aligned even for an empty slice.
    unsafe { slice::from_raw_parts_mut(pointer, region.elements) }
}

fn ranges_overlap(first_start: u64, first_end: u64, second_start: u64, second_end: u64) -> bool {
    first_start < second_end && second_start < first_end
}

fn regions_overlap(first: Region, second: Region) -> Result<bool, MemoryError> {
    let first_start = first.pointer.as_ptr() as usize;
    let second_start = second.pointer.as_ptr() as usize;
    let first_end = first.end_address()?;
    let second_end = second.end_address()?;
    Ok(first_start < second_end && second_start < first_end)
}

fn same_region(first: Region, second: Region) -> bool {
    first.pointer == second.pointer
        && first.bytes == second.bytes
        && first.dtype == second.dtype
        && first.shape == second.shape
}

fn validate_op_aliases(
    op: OpDesc,
    regions: &[Region],
    tensor_ids: &[u32],
) -> Result<(), MemoryError> {
    let inputs = usize::from(op.input_count);
    let outputs = usize::from(op.output_count);
    for output in 0..outputs {
        let output_index = inputs + output;
        for earlier in 0..output {
            if regions_overlap(regions[output_index], regions[inputs + earlier])? {
                return Err(MemoryError::OutputOverlap);
            }
        }
        for input in 0..inputs {
            if !regions_overlap(regions[output_index], regions[input])? {
                continue;
            }
            let exact_in_place = op.allows_in_place()
                && tensor_ids[output_index] == tensor_ids[input]
                && same_region(regions[output_index], regions[input]);
            if !exact_in_place {
                return Err(MemoryError::InputOutputOverlap);
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BorrowState {
    Unused,
    Readers(u16),
    Writer,
}

/// Runtime-leased binding set for one sealed operation.
///
/// Shared input leases may coexist. Mutable leases require physical
/// exclusivity, including for explicitly sealed in-place bindings.
pub struct OpAccess<const CAPACITY: usize> {
    op: OpDesc,
    regions: [Region; CAPACITY],
    tensor_ids: [u32; CAPACITY],
    binding_count: usize,
    borrows: [Cell<BorrowState>; CAPACITY],
}

impl<const CAPACITY: usize> OpAccess<CAPACITY> {
    pub const fn input_count(&self) -> u16 {
        self.op.input_count
    }

    pub const fn output_count(&self) -> u16 {
        self.op.output_count
    }

    pub fn input_tensor_id(&self, input: u16) -> Option<u32> {
        (input < self.op.input_count).then(|| self.tensor_ids[usize::from(input)])
    }

    pub fn output_tensor_id(&self, output: u16) -> Option<u32> {
        if output >= self.op.output_count {
            return None;
        }
        Some(self.tensor_ids[usize::from(self.op.input_count) + usize::from(output)])
    }

    pub fn input<T: TensorElement>(&self, input: u16) -> Result<ReadLease<'_, T>, MemoryError> {
        if input >= self.op.input_count {
            return Err(MemoryError::BindingOutOfBounds);
        }
        let index = usize::from(input);
        let region = self.regions[index];
        validate_typed_region::<T>(region)?;
        self.acquire_read(index)?;
        // SAFETY: the operation retains every backing allocation; validation
        // proves the typed representation and `acquire_read` excludes writers.
        let values = unsafe { region_slice::<T>(region) };
        Ok(ReadLease {
            values,
            shape: region.shape,
            state: &self.borrows[index],
        })
    }

    /// Acquire one mutable output. Runtime lease checks make interior
    /// mutability here sound even though the binding set itself is shared.
    #[allow(clippy::mut_from_ref)]
    pub fn output<T: TensorElement>(&self, output: u16) -> Result<WriteLease<'_, T>, MemoryError> {
        if output >= self.op.output_count {
            return Err(MemoryError::BindingOutOfBounds);
        }
        let index = usize::from(self.op.input_count) + usize::from(output);
        let region = self.regions[index];
        if !region.writable {
            return Err(MemoryError::ReadOnlyWrite);
        }
        validate_typed_region::<T>(region)?;
        self.acquire_write(index)?;
        // SAFETY: alias validation rejected forbidden op aliases, while the
        // runtime lease excludes every currently overlapping read or write.
        let values = unsafe { region_slice_mut::<T>(region) };
        Ok(WriteLease {
            values,
            shape: region.shape,
            state: &self.borrows[index],
        })
    }

    /// Acquire the single mutable view of an exact same-ID in-place binding.
    #[allow(clippy::mut_from_ref)]
    pub fn in_place<T: TensorElement>(
        &self,
        input: u16,
        output: u16,
    ) -> Result<WriteLease<'_, T>, MemoryError> {
        if input >= self.op.input_count || output >= self.op.output_count {
            return Err(MemoryError::BindingOutOfBounds);
        }
        let input_index = usize::from(input);
        let output_index = usize::from(self.op.input_count) + usize::from(output);
        if !self.op.allows_in_place()
            || self.tensor_ids[input_index] != self.tensor_ids[output_index]
            || !same_region(self.regions[input_index], self.regions[output_index])
        {
            return Err(MemoryError::InPlaceNotSealed);
        }
        self.output(output)
    }

    fn acquire_read(&self, index: usize) -> Result<(), MemoryError> {
        for other in 0..self.binding_count {
            if self.borrows[other].get() == BorrowState::Writer
                && regions_overlap(self.regions[index], self.regions[other])?
            {
                return Err(MemoryError::BorrowConflict);
            }
        }
        let next = match self.borrows[index].get() {
            BorrowState::Unused => BorrowState::Readers(1),
            BorrowState::Readers(readers) => BorrowState::Readers(
                readers
                    .checked_add(1)
                    .ok_or(MemoryError::BorrowCountOverflow)?,
            ),
            BorrowState::Writer => return Err(MemoryError::BorrowConflict),
        };
        self.borrows[index].set(next);
        Ok(())
    }

    fn acquire_write(&self, index: usize) -> Result<(), MemoryError> {
        for other in 0..self.binding_count {
            if self.borrows[other].get() != BorrowState::Unused
                && regions_overlap(self.regions[index], self.regions[other])?
            {
                return Err(MemoryError::BorrowConflict);
            }
        }
        self.borrows[index].set(BorrowState::Writer);
        Ok(())
    }
}

/// Shared typed tensor lease. Dropping it releases the runtime read count.
#[derive(Debug)]
pub struct ReadLease<'access, T> {
    values: &'access [T],
    shape: RuntimeShape,
    state: &'access Cell<BorrowState>,
}

impl<T> ReadLease<'_, T> {
    pub const fn shape(&self) -> RuntimeShape {
        self.shape
    }
}

impl<T> Deref for ReadLease<'_, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.values
    }
}

impl<T> Drop for ReadLease<'_, T> {
    fn drop(&mut self) {
        match self.state.get() {
            BorrowState::Readers(1) => self.state.set(BorrowState::Unused),
            BorrowState::Readers(readers) => {
                self.state.set(BorrowState::Readers(readers - 1));
            }
            BorrowState::Unused | BorrowState::Writer => {
                debug_assert!(false, "invalid read-lease state");
            }
        }
    }
}

/// Exclusive typed tensor lease. Dropping it releases the runtime writer.
#[derive(Debug)]
pub struct WriteLease<'access, T> {
    values: &'access mut [T],
    shape: RuntimeShape,
    state: &'access Cell<BorrowState>,
}

impl<T> WriteLease<'_, T> {
    pub const fn shape(&self) -> RuntimeShape {
        self.shape
    }
}

impl<T> Deref for WriteLease<'_, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.values
    }
}

impl<T> DerefMut for WriteLease<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.values
    }
}

impl<T> Drop for WriteLease<'_, T> {
    fn drop(&mut self) {
        debug_assert_eq!(self.state.get(), BorrowState::Writer);
        self.state.set(BorrowState::Unused);
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
