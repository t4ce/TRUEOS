//! Bounded control plane for semi-persistent GPU-font producers.
//!
//! This module deliberately contains no GPU or UI4 types.  It owns producer
//! leases, row ownership, credits, and generation/sequence validation.  The
//! GPU adapter carries [`FontRowToken`] and [`FontRowCompletion`] across the
//! existing Font RCS lane; UI4 returns the same token on acknowledgement.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the producer registry is a newly landed kernel service API awaiting its first UI4 consumer"
    )
)]

extern crate alloc;

use alloc::vec::Vec;
#[cfg(not(test))]
use spin::Mutex;

#[cfg(test)]
struct Mutex<T>(std::sync::Mutex<T>);

#[cfg(test)]
impl<T> Mutex<T> {
    const fn new(value: T) -> Self {
        Self(std::sync::Mutex::new(value))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, T> {
        self.0.lock().expect("font producer test mutex poisoned")
    }
}

pub const FONT_PRODUCER_MAX_SLOTS: usize = 32;
pub const FONT_PRODUCER_MAX_ROW_RING_DEPTH: usize = 4;
pub const FONT_PRODUCER_MAX_ROW_WIDTH: u32 = 4_096;
pub const FONT_PRODUCER_MAX_ROW_HEIGHT: u32 = 4_096;
pub const FONT_PRODUCER_MAX_CHARS: usize = 4_096;
pub const FONT_PRODUCER_MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontProducerFormat {
    Rgba8Premultiplied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontProducerRegistration {
    pub face: u16,
    pub tier: u16,
    /// Static em size multiplied by 1,000 so registration stays exactly
    /// comparable without storing a floating-point lifecycle key.
    pub font_pixels_milli: u32,
    pub row_width_px: u32,
    pub row_height_px: u32,
    pub format: FontProducerFormat,
    pub max_chars: usize,
    pub row_ring_depth: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontProducerLease {
    producer_id: u16,
    generation: u64,
    registration: FontProducerRegistration,
}

impl FontProducerLease {
    pub const fn producer_id(self) -> u16 {
        self.producer_id
    }
    pub const fn generation(self) -> u64 {
        self.generation
    }
    pub const fn registration(self) -> FontProducerRegistration {
        self.registration
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontRowToken {
    producer_id: u16,
    generation: u64,
    row_index: u16,
    sequence: u64,
}

impl FontRowToken {
    pub const fn producer_id(self) -> u16 {
        self.producer_id
    }
    pub const fn generation(self) -> u64 {
        self.generation
    }
    pub const fn row_index(self) -> u16 {
        self.row_index
    }
    pub const fn sequence(self) -> u64 {
        self.sequence
    }
}

/// Metadata proving that the GPU has finished the exact retained row.
///
/// The actual `GpgpuRgba8ReleaseFence` remains in the GPU adapter.  Its
/// adapter must only construct this record after checking that fence against
/// the physical allocation and byte length; this core never treats an
/// address or an unrelated completion as proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontRowCompletion {
    pub release_fence: u64,
    pub metadata: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontRowState {
    Idle,
    GpuOwned,
    Produced,
    SurfLive,
    Retiring,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontProducerStatus {
    pub lease: FontProducerLease,
    pub state: FontProducerState,
    pub credits: usize,
    pub ring_depth: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontProducerState {
    Active,
    Retiring,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontProducerError {
    RegistryFull,
    InvalidRegistration(&'static str),
    InvalidLease,
    InvalidToken,
    NoCredits,
    InvalidCharCount,
    InputTooLong,
    RowNotGpuOwned,
    RowNotProduced,
    RowNotSurfLive,
    CompletionMismatch,
    AlreadyReleased,
}

struct RowSlot {
    state: FontRowState,
    sequence: u64,
    char_count: usize,
    input: Vec<u8>,
    completion: Option<FontRowCompletion>,
}

struct ProducerSlot {
    lease: FontProducerLease,
    state: FontProducerState,
    rows: Vec<RowSlot>,
}

/// In-memory registry.  Construct one per kernel/control-plane owner.
pub struct FontProducerRegistry {
    slots: Vec<Option<ProducerSlot>>,
    next_generations: [u64; FONT_PRODUCER_MAX_SLOTS],
}

impl FontProducerRegistry {
    pub fn new() -> Self {
        let mut slots = Vec::with_capacity(FONT_PRODUCER_MAX_SLOTS);
        slots.resize_with(FONT_PRODUCER_MAX_SLOTS, || None);
        Self {
            slots,
            next_generations: [0; FONT_PRODUCER_MAX_SLOTS],
        }
    }

    pub fn register(
        &mut self,
        registration: FontProducerRegistration,
    ) -> Result<FontProducerLease, FontProducerError> {
        validate_registration(registration)?;
        let index = self
            .slots
            .iter()
            .position(Option::is_none)
            .ok_or(FontProducerError::RegistryFull)?;
        let generation = self.next_generations[index].wrapping_add(1).max(1);
        self.next_generations[index] = generation;
        let lease = FontProducerLease {
            producer_id: index as u16,
            generation,
            registration,
        };
        let mut rows = Vec::with_capacity(registration.row_ring_depth);
        for _ in 0..registration.row_ring_depth {
            rows.push(RowSlot {
                state: FontRowState::Idle,
                sequence: 0,
                char_count: 0,
                input: Vec::with_capacity(registration.max_chars.saturating_mul(4)),
                completion: None,
            });
        }
        self.slots[index] = Some(ProducerSlot {
            lease,
            state: FontProducerState::Active,
            rows,
        });
        Ok(lease)
    }

    /// Request final retirement.  In-flight rows remain owned until ACKed;
    /// once all rows are idle the slot is removed and its generation advances
    /// on the next registration.
    pub fn release(&mut self, lease: FontProducerLease) -> Result<bool, FontProducerError> {
        let slot = self.slot_mut(lease)?;
        if slot.state == FontProducerState::Retiring {
            return Err(FontProducerError::AlreadyReleased);
        }
        slot.state = FontProducerState::Retiring;
        Ok(self.finish_retirement(lease.producer_id as usize))
    }

    pub fn status(
        &self,
        lease: FontProducerLease,
    ) -> Result<FontProducerStatus, FontProducerError> {
        let slot = self.slot(lease)?;
        Ok(FontProducerStatus {
            lease: slot.lease,
            state: slot.state,
            credits: slot
                .rows
                .iter()
                .filter(|row| row.state == FontRowState::Idle)
                .count(),
            ring_depth: slot.rows.len(),
        })
    }

    /// Reserve one retained row.  This is the producer's credit-consuming
    /// transition; the row remains GPU-owned until its exact completion.
    pub fn reserve_row(
        &mut self,
        lease: FontProducerLease,
        char_count: usize,
    ) -> Result<FontRowToken, FontProducerError> {
        let slot = self.slot_mut(lease)?;
        if slot.state != FontProducerState::Active {
            return Err(FontProducerError::AlreadyReleased);
        }
        if !(1..=slot.lease.registration.max_chars).contains(&char_count) {
            return Err(FontProducerError::InvalidCharCount);
        }
        let index = slot
            .rows
            .iter()
            .position(|row| row.state == FontRowState::Idle)
            .ok_or(FontProducerError::NoCredits)?;
        Self::reserve_slot(slot, lease, index, char_count)
    }

    /// Reserve a particular retained row.  GPU/UI adapters use this when a
    /// preallocated row is paired with a concrete display buffer index.
    pub fn reserve_specific_row(
        &mut self,
        lease: FontProducerLease,
        row_index: usize,
        char_count: usize,
    ) -> Result<FontRowToken, FontProducerError> {
        let slot = self.slot_mut(lease)?;
        if slot.state != FontProducerState::Active {
            return Err(FontProducerError::AlreadyReleased);
        }
        if !(1..=slot.lease.registration.max_chars).contains(&char_count) {
            return Err(FontProducerError::InvalidCharCount);
        }
        if row_index >= slot.rows.len() {
            return Err(FontProducerError::InvalidToken);
        }
        if slot.rows[row_index].state != FontRowState::Idle {
            return Err(FontProducerError::NoCredits);
        }
        Self::reserve_slot(slot, lease, row_index, char_count)
    }

    fn reserve_slot(
        slot: &mut ProducerSlot,
        lease: FontProducerLease,
        index: usize,
        char_count: usize,
    ) -> Result<FontRowToken, FontProducerError> {
        let row = &mut slot.rows[index];
        row.sequence = row.sequence.wrapping_add(1).max(1);
        row.state = FontRowState::GpuOwned;
        row.char_count = char_count;
        row.input.clear();
        row.completion = None;
        Ok(FontRowToken {
            producer_id: lease.producer_id,
            generation: lease.generation,
            row_index: index as u16,
            sequence: row.sequence,
        })
    }

    /// Reserve and copy the bounded input descriptor.  The retained output
    /// allocation is not recreated by this operation.
    pub fn submit_row(
        &mut self,
        lease: FontProducerLease,
        text: &[u8],
        char_count: usize,
    ) -> Result<FontRowToken, FontProducerError> {
        if text.len() > lease.registration.max_chars.saturating_mul(4) {
            return Err(FontProducerError::InputTooLong);
        }
        let token = self.reserve_row(lease, char_count)?;
        let row = self.row_mut(token)?;
        row.input.extend_from_slice(text);
        Ok(token)
    }

    pub fn row_input(&self, token: FontRowToken) -> Result<&[u8], FontProducerError> {
        Ok(&self.row(token)?.input)
    }

    pub fn gpu_produced(
        &mut self,
        token: FontRowToken,
        completion: FontRowCompletion,
    ) -> Result<(), FontProducerError> {
        let row = self.row_mut(token)?;
        if row.state != FontRowState::GpuOwned {
            return Err(FontProducerError::RowNotGpuOwned);
        }
        row.completion = Some(completion);
        row.state = FontRowState::Produced;
        Ok(())
    }

    /// Undo admission only while the row is still a pre-submit reservation.
    /// Callers must never use this after a command may have reached hardware.
    pub fn cancel_reserved(&mut self, token: FontRowToken) -> Result<(), FontProducerError> {
        let id = token.producer_id as usize;
        let row = self.row_mut(token)?;
        if row.state != FontRowState::GpuOwned || row.completion.is_some() {
            return Err(FontProducerError::RowNotGpuOwned);
        }
        row.state = FontRowState::Idle;
        row.char_count = 0;
        row.input.clear();
        if self.slots[id]
            .as_ref()
            .is_some_and(|slot| slot.state == FontProducerState::Retiring)
        {
            self.finish_retirement(id);
        }
        Ok(())
    }

    /// Pin a row whose GPU retirement is ambiguous.  It never becomes a
    /// credit again and therefore keeps a retiring producer from being freed.
    pub fn quarantine(&mut self, token: FontRowToken) -> Result<(), FontProducerError> {
        let row = self.row_mut(token)?;
        if row.state != FontRowState::GpuOwned {
            return Err(FontProducerError::RowNotGpuOwned);
        }
        row.state = FontRowState::Retiring;
        row.input.clear();
        row.completion = None;
        Ok(())
    }

    /// Permanently retire a produced/display-owned row whose consumer drops
    /// the exact acknowledgement capability.  No later token can reuse it.
    pub fn abandon(&mut self, token: FontRowToken) -> Result<(), FontProducerError> {
        let row = self.row_mut(token)?;
        if matches!(row.state, FontRowState::Idle | FontRowState::Retiring) {
            return Err(FontProducerError::InvalidToken);
        }
        row.state = FontRowState::Retiring;
        row.input.clear();
        row.completion = None;
        Ok(())
    }

    pub fn publish_surflive(
        &mut self,
        token: FontRowToken,
        expected: FontRowCompletion,
    ) -> Result<FontRowCompletion, FontProducerError> {
        let row = self.row_mut(token)?;
        if row.state != FontRowState::Produced {
            return Err(FontProducerError::RowNotProduced);
        }
        if row.completion != Some(expected) {
            return Err(FontProducerError::CompletionMismatch);
        }
        row.state = FontRowState::SurfLive;
        Ok(expected)
    }

    /// Return one exact row credit.  A token from an older generation, a
    /// different sequence, or a duplicate ACK cannot reopen a row.
    pub fn acknowledge(&mut self, token: FontRowToken) -> Result<(), FontProducerError> {
        let id = token.producer_id as usize;
        let row = self.row_mut(token)?;
        if row.state != FontRowState::SurfLive {
            return Err(FontProducerError::RowNotSurfLive);
        }
        row.state = FontRowState::Idle;
        row.char_count = 0;
        row.input.clear();
        row.completion = None;
        if let Some(slot) = self.slots[id].as_ref() {
            if slot.state == FontProducerState::Retiring {
                self.finish_retirement(id);
            }
        }
        Ok(())
    }

    /// Return a GPU-complete row which was published into a UI4 Frame but was
    /// superseded before any compositor transaction presented that publish
    /// serial. The caller must hold the exact reacquired frame buffer; the
    /// completion tuple prevents a stale token from reopening newer work.
    pub fn acknowledge_unpresented(
        &mut self,
        token: FontRowToken,
        expected: FontRowCompletion,
    ) -> Result<(), FontProducerError> {
        let id = token.producer_id as usize;
        let row = self.row_mut(token)?;
        if row.state != FontRowState::Produced {
            return Err(FontProducerError::RowNotProduced);
        }
        if row.completion != Some(expected) {
            return Err(FontProducerError::CompletionMismatch);
        }
        row.state = FontRowState::Idle;
        row.char_count = 0;
        row.input.clear();
        row.completion = None;
        if self.slots[id]
            .as_ref()
            .is_some_and(|slot| slot.state == FontProducerState::Retiring)
        {
            self.finish_retirement(id);
        }
        Ok(())
    }

    /// Return a row after its entire backing container has been retired.
    /// This is the teardown counterpart of a display ACK: an exact completion
    /// is still required, but a row which completed without publication may
    /// retire directly from `Produced` once UI4 proves the Frame is destroyed.
    pub fn acknowledge_retired(
        &mut self,
        token: FontRowToken,
        expected: FontRowCompletion,
    ) -> Result<(), FontProducerError> {
        let id = token.producer_id as usize;
        let row = self.row_mut(token)?;
        if !matches!(row.state, FontRowState::Produced | FontRowState::SurfLive) {
            return Err(FontProducerError::InvalidToken);
        }
        if row.completion != Some(expected) {
            return Err(FontProducerError::CompletionMismatch);
        }
        row.state = FontRowState::Idle;
        row.char_count = 0;
        row.input.clear();
        row.completion = None;
        if self.slots[id]
            .as_ref()
            .is_some_and(|slot| slot.state == FontProducerState::Retiring)
        {
            self.finish_retirement(id);
        }
        Ok(())
    }

    pub fn row_state(&self, token: FontRowToken) -> Result<FontRowState, FontProducerError> {
        Ok(self.row(token)?.state)
    }

    fn finish_retirement(&mut self, id: usize) -> bool {
        let done = self.slots[id]
            .as_ref()
            .is_some_and(|slot| slot.rows.iter().all(|row| row.state == FontRowState::Idle));
        if done {
            self.slots[id] = None;
        }
        done
    }

    fn slot(&self, lease: FontProducerLease) -> Result<&ProducerSlot, FontProducerError> {
        self.slots
            .get(lease.producer_id as usize)
            .and_then(Option::as_ref)
            .filter(|slot| slot.lease.generation == lease.generation)
            .ok_or(FontProducerError::InvalidLease)
    }
    fn slot_mut(
        &mut self,
        lease: FontProducerLease,
    ) -> Result<&mut ProducerSlot, FontProducerError> {
        self.slots
            .get_mut(lease.producer_id as usize)
            .and_then(Option::as_mut)
            .filter(|slot| slot.lease.generation == lease.generation)
            .ok_or(FontProducerError::InvalidLease)
    }
    fn row(&self, token: FontRowToken) -> Result<&RowSlot, FontProducerError> {
        self.slots
            .get(token.producer_id as usize)
            .and_then(Option::as_ref)
            .filter(|slot| slot.lease.generation == token.generation)
            .and_then(|slot| slot.rows.get(token.row_index as usize))
            .filter(|row| row.sequence == token.sequence)
            .ok_or(FontProducerError::InvalidToken)
    }
    fn row_mut(&mut self, token: FontRowToken) -> Result<&mut RowSlot, FontProducerError> {
        self.slots
            .get_mut(token.producer_id as usize)
            .and_then(Option::as_mut)
            .filter(|slot| slot.lease.generation == token.generation)
            .and_then(|slot| slot.rows.get_mut(token.row_index as usize))
            .filter(|row| row.sequence == token.sequence)
            .ok_or(FontProducerError::InvalidToken)
    }
}

impl Default for FontProducerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

static FONT_PRODUCERS: Mutex<Option<FontProducerRegistry>> = Mutex::new(None);

fn with_registry<T>(operation: impl FnOnce(&mut FontProducerRegistry) -> T) -> T {
    let mut registry = FONT_PRODUCERS.lock();
    let registry = registry.get_or_insert_with(FontProducerRegistry::new);
    operation(registry)
}

pub fn register_producer(
    registration: FontProducerRegistration,
) -> Result<FontProducerLease, FontProducerError> {
    with_registry(|registry| registry.register(registration))
}

pub fn release_producer(lease: FontProducerLease) -> Result<bool, FontProducerError> {
    with_registry(|registry| registry.release(lease))
}

pub fn producer_status(lease: FontProducerLease) -> Result<FontProducerStatus, FontProducerError> {
    with_registry(|registry| registry.status(lease))
}

pub fn producer_row_state(token: FontRowToken) -> Result<FontRowState, FontProducerError> {
    with_registry(|registry| registry.row_state(token))
}

pub fn reserve_producer_row(
    lease: FontProducerLease,
    char_count: usize,
) -> Result<FontRowToken, FontProducerError> {
    with_registry(|registry| registry.reserve_row(lease, char_count))
}

pub fn reserve_specific_producer_row(
    lease: FontProducerLease,
    row_index: usize,
    char_count: usize,
) -> Result<FontRowToken, FontProducerError> {
    with_registry(|registry| registry.reserve_specific_row(lease, row_index, char_count))
}

pub fn cancel_reserved_producer_row(token: FontRowToken) -> Result<(), FontProducerError> {
    with_registry(|registry| registry.cancel_reserved(token))
}

pub fn quarantine_producer_row(token: FontRowToken) -> Result<(), FontProducerError> {
    with_registry(|registry| registry.quarantine(token))
}

pub fn abandon_producer_row(token: FontRowToken) -> Result<(), FontProducerError> {
    with_registry(|registry| registry.abandon(token))
}

pub fn mark_producer_row_gpu_complete(
    token: FontRowToken,
    completion: FontRowCompletion,
) -> Result<(), FontProducerError> {
    with_registry(|registry| registry.gpu_produced(token, completion))
}

pub fn mark_producer_row_surflive(
    token: FontRowToken,
    expected: FontRowCompletion,
) -> Result<FontRowCompletion, FontProducerError> {
    with_registry(|registry| registry.publish_surflive(token, expected))
}

pub fn acknowledge_producer_row(token: FontRowToken) -> Result<(), FontProducerError> {
    with_registry(|registry| registry.acknowledge(token))
}

pub fn acknowledge_unpresented_producer_row(
    token: FontRowToken,
    expected: FontRowCompletion,
) -> Result<(), FontProducerError> {
    with_registry(|registry| registry.acknowledge_unpresented(token, expected))
}

pub fn acknowledge_retired_producer_row(
    token: FontRowToken,
    expected: FontRowCompletion,
) -> Result<(), FontProducerError> {
    with_registry(|registry| registry.acknowledge_retired(token, expected))
}

fn validate_registration(registration: FontProducerRegistration) -> Result<(), FontProducerError> {
    if registration.face == 0 {
        return Err(FontProducerError::InvalidRegistration("face"));
    }
    if registration.tier == 0 || registration.font_pixels_milli == 0 {
        return Err(FontProducerError::InvalidRegistration("font tier/pixels"));
    }
    if registration.row_width_px == 0 || registration.row_width_px > FONT_PRODUCER_MAX_ROW_WIDTH {
        return Err(FontProducerError::InvalidRegistration("row width"));
    }
    if registration.row_height_px == 0 || registration.row_height_px > FONT_PRODUCER_MAX_ROW_HEIGHT
    {
        return Err(FontProducerError::InvalidRegistration("row height"));
    }
    if registration.max_chars == 0 || registration.max_chars > FONT_PRODUCER_MAX_CHARS {
        return Err(FontProducerError::InvalidRegistration("max chars"));
    }
    if registration.row_ring_depth == 0
        || registration.row_ring_depth > FONT_PRODUCER_MAX_ROW_RING_DEPTH
    {
        return Err(FontProducerError::InvalidRegistration("ring depth"));
    }
    let row_bytes = (registration.row_width_px as usize)
        .checked_mul(core::mem::size_of::<u32>())
        .and_then(|bytes| bytes.checked_add(63))
        .map(|bytes| bytes & !63)
        .ok_or(FontProducerError::InvalidRegistration("retained bytes"))?;
    let surface_bytes = row_bytes
        .checked_mul(registration.row_height_px as usize)
        .and_then(|bytes| bytes.checked_add(4_095))
        .map(|bytes| bytes & !4_095)
        .ok_or(FontProducerError::InvalidRegistration("retained bytes"))?;
    let retained_bytes = surface_bytes
        .checked_mul(registration.row_ring_depth)
        .ok_or(FontProducerError::InvalidRegistration("retained bytes"))?;
    if retained_bytes > FONT_PRODUCER_MAX_RETAINED_BYTES {
        return Err(FontProducerError::InvalidRegistration("retained bytes"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn registration(depth: usize) -> FontProducerRegistration {
        FontProducerRegistration {
            face: 1,
            tier: 1,
            font_pixels_milli: 16_000,
            row_width_px: 256,
            row_height_px: 24,
            format: FontProducerFormat::Rgba8Premultiplied,
            max_chars: 8,
            row_ring_depth: depth,
        }
    }
    fn complete(registry: &mut FontProducerRegistry, token: FontRowToken) {
        registry
            .gpu_produced(
                token,
                FontRowCompletion {
                    release_fence: 7,
                    metadata: 9,
                },
            )
            .unwrap();
        registry
            .publish_surflive(
                token,
                FontRowCompletion {
                    release_fence: 7,
                    metadata: 9,
                },
            )
            .unwrap();
    }

    #[test]
    fn registration_validation_and_cap() {
        let mut r = FontProducerRegistry::new();
        let mut bad = registration(1);
        bad.row_width_px = 0;
        assert_eq!(r.register(bad), Err(FontProducerError::InvalidRegistration("row width")));
        for _ in 0..FONT_PRODUCER_MAX_SLOTS {
            r.register(registration(1)).unwrap();
        }
        assert_eq!(r.register(registration(1)), Err(FontProducerError::RegistryFull));
    }
    #[test]
    fn full_credits_backpressure_and_ack_reuse() {
        let mut r = FontProducerRegistry::new();
        let lease = r.register(registration(2)).unwrap();
        let a = r.submit_row(lease, b"a", 1).unwrap();
        let b = r.submit_row(lease, b"bc", 2).unwrap();
        assert_eq!(r.submit_row(lease, b"d", 1), Err(FontProducerError::NoCredits));
        complete(&mut r, a);
        r.acknowledge(a).unwrap();
        let c = r.submit_row(lease, b"e", 1).unwrap();
        assert_ne!(c.sequence(), a.sequence());
        complete(&mut r, b);
        complete(&mut r, c);
    }
    #[test]
    fn stale_duplicate_and_mismatched_ack_rejected() {
        let mut r = FontProducerRegistry::new();
        let lease = r.register(registration(1)).unwrap();
        let t = r.submit_row(lease, b"x", 1).unwrap();
        assert_eq!(r.acknowledge(t), Err(FontProducerError::RowNotSurfLive));
        r.gpu_produced(
            t,
            FontRowCompletion {
                release_fence: 7,
                metadata: 9,
            },
        )
        .unwrap();
        assert_eq!(
            r.publish_surflive(
                t,
                FontRowCompletion {
                    release_fence: 8,
                    metadata: 9,
                }
            ),
            Err(FontProducerError::CompletionMismatch)
        );
        assert_eq!(r.row_state(t), Ok(FontRowState::Produced));
        r.publish_surflive(
            t,
            FontRowCompletion {
                release_fence: 7,
                metadata: 9,
            },
        )
        .unwrap();
        r.acknowledge(t).unwrap();
        assert_eq!(r.acknowledge(t), Err(FontProducerError::RowNotSurfLive));
        let stale = FontRowToken {
            sequence: t.sequence().wrapping_add(1),
            ..t
        };
        assert_eq!(r.row_state(stale), Err(FontProducerError::InvalidToken));
    }

    #[test]
    fn exact_unpresented_reacquire_restores_credit_without_surflive() {
        let mut r = FontProducerRegistry::new();
        let lease = r.register(registration(1)).unwrap();
        let token = r.submit_row(lease, b"x", 1).unwrap();
        let completion = FontRowCompletion {
            release_fence: 7,
            metadata: 9,
        };
        r.gpu_produced(token, completion).unwrap();
        assert_eq!(r.row_state(token), Ok(FontRowState::Produced));
        r.acknowledge_unpresented(token, completion).unwrap();
        let replacement = r.submit_row(lease, b"y", 1).unwrap();
        assert_ne!(replacement.sequence(), token.sequence());
    }
    #[test]
    fn release_in_flight_waits_for_ack_and_rejects_new_rows() {
        let mut r = FontProducerRegistry::new();
        let lease = r.register(registration(1)).unwrap();
        let t = r.submit_row(lease, b"x", 1).unwrap();
        assert!(!r.release(lease).unwrap());
        assert_eq!(r.reserve_row(lease, 1), Err(FontProducerError::AlreadyReleased));
        complete(&mut r, t);
        r.acknowledge(t).unwrap();
        assert_eq!(r.status(lease), Err(FontProducerError::InvalidLease));
    }

    #[test]
    fn destroyed_ui4_ring_retires_produced_or_surflive_rows_exactly() {
        let mut r = FontProducerRegistry::new();
        let lease = r.register(registration(2)).unwrap();
        let produced = r.submit_row(lease, b"x", 1).unwrap();
        let surflive = r.submit_row(lease, b"y", 1).unwrap();
        let completion = FontRowCompletion {
            release_fence: 7,
            metadata: 9,
        };
        r.gpu_produced(produced, completion).unwrap();
        r.gpu_produced(surflive, completion).unwrap();
        r.publish_surflive(surflive, completion).unwrap();
        assert_eq!(
            r.acknowledge_retired(
                produced,
                FontRowCompletion {
                    release_fence: 8,
                    metadata: 9,
                },
            ),
            Err(FontProducerError::CompletionMismatch)
        );
        r.acknowledge_retired(produced, completion).unwrap();
        r.acknowledge_retired(surflive, completion).unwrap();
        assert_eq!(r.status(lease).unwrap().credits, 2);
    }

    #[test]
    fn reversible_cancel_restores_credit_but_quarantine_does_not() {
        let mut r = FontProducerRegistry::new();
        let lease = r.register(registration(1)).unwrap();
        let cancelled = r.reserve_row(lease, 1).unwrap();
        r.cancel_reserved(cancelled).unwrap();
        assert_eq!(r.status(lease).unwrap().credits, 1);

        let quarantined = r.reserve_row(lease, 1).unwrap();
        r.quarantine(quarantined).unwrap();
        assert_eq!(r.status(lease).unwrap().credits, 0);
        assert!(!r.release(lease).unwrap());
    }

    #[test]
    fn retained_byte_cap_accounts_for_pitch_and_page_alignment() {
        let mut r = FontProducerRegistry::new();
        let mut oversized = registration(FONT_PRODUCER_MAX_ROW_RING_DEPTH);
        oversized.row_width_px = FONT_PRODUCER_MAX_ROW_WIDTH;
        oversized.row_height_px = FONT_PRODUCER_MAX_ROW_HEIGHT;
        assert_eq!(
            r.register(oversized),
            Err(FontProducerError::InvalidRegistration("retained bytes"))
        );
    }
}
