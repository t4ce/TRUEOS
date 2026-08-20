//! Kernel Clipboard authority: the push-only castle at the center of Code Land.
//!
//! This module intentionally has only three directions of travel:
//!
//! 1. a principal may publish one typed clip and immediately relinquishes all
//!    control over the accepted value;
//! 2. a principal may place an exact-type delivery gate on a live Matrix
//!    `§slot§` lifetime; and
//! 3. trusted kernel input routing may dispatch a paste intent, causing this
//!    authority to push the newest accepted clip only when its exact type fits
//!    that gate.
//!
//! There is deliberately no API to read, request, enumerate, peek, consume,
//! delete, retain, or subscribe to Clipboard contents. Paste never mutates the
//! ten-entry history. Publishers receive no ownership token, callback, expiry
//! notification, or deletion facility after acceptance.
//!
//! The public integration seam is event-shaped: a kernel-owned sink receives
//! `on_paste`. A sink should enqueue into a mediated Blueprint transport; it
//! must never be a raw function pointer into guest memory. Persistent and
//! one-shot gates both derive their identity from a generation-bearing Matrix
//! slot lease, and freeing that slot closes the gate.
//!
//! The authority is deliberately synchronously linearized by one kernel mutex.
//! That gives publish rejection and Matrix teardown a single commit point. A
//! future BSP executor task may marshal calls into this same core, but making a
//! queue the authority now would weaken immediate errors and add reply handles
//! without changing Clipboard semantics.

#![allow(
    dead_code,
    reason = "the central Clipboard contract intentionally precedes ABI and Blueprint adapters"
)]

use alloc::string::String as AllocString;
use alloc::sync::Arc;
use core::fmt;
use core::num::NonZeroU32;

use heapless::{Deque, String, Vec};
use spin::Mutex;
use zeroize::Zeroizing;

use crate::shell2::{
    MatrixSlotAttachment, MatrixSlotAttachmentError, MatrixSlotAttachmentId, MatrixSlotLease,
    attach_matrix_slot_resource, detach_matrix_slot_resource, matrix_slot_is_live,
};

pub(crate) const CLIPBOARD_HISTORY_CAPACITY: usize = 10;
pub(crate) const CLIPBOARD_TEXT_MAX_UTF8_BYTES: usize = 512;
pub(crate) const CLIPBOARD_PUBLISH_INTERVAL_MS: u64 = 100;

const CLIPBOARD_PRINCIPAL_CAPACITY: usize = 64;
const CLIPBOARD_GATE_CAPACITY: usize = 64;

/// This policy is kernel-owned and cannot be weakened by a publisher, gate,
/// Blueprint, or paste request. `PasswordOnly` exists to make the documented
/// policy choice closed and reviewable, rather than an accidental boolean.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardPasteAuthPolicy {
    AuthenticatedTwoFactorForEveryClip,
    AuthenticatedTwoFactorForPasswords,
}

/// TRUEOS currently chooses the strict interpretation: every paste requires
/// the active `crypt` two-factor session for the trusted input scope. Password
/// delivery is unconditionally authenticated under every policy variant.
pub(crate) const CLIPBOARD_PASTE_AUTH_POLICY: ClipboardPasteAuthPolicy =
    ClipboardPasteAuthPolicy::AuthenticatedTwoFactorForEveryClip;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardPrincipal {
    KernelService(u32),
    Blueprint(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardKind {
    Text,
    Json,
    Phone,
    Float,
    Password,
    Image,
    Asset,
}

/// Generation-checked kernel object reference. It is a typed descriptor, not
/// a pointer and not a guest-provided address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KernelResourceHandle {
    authority: NonZeroU32,
    index: u32,
    generation: NonZeroU32,
}

impl KernelResourceHandle {
    pub(crate) fn new(authority: u32, index: u32, generation: u32) -> Option<Self> {
        Some(Self {
            authority: NonZeroU32::new(authority)?,
            index,
            generation: NonZeroU32::new(generation)?,
        })
    }

    pub(crate) const fn authority(self) -> u32 {
        self.authority.get()
    }

    pub(crate) const fn index(self) -> u32 {
        self.index
    }

    pub(crate) const fn generation(self) -> u32 {
        self.generation.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardImageFormat {
    Rgba8Premultiplied,
    Rgba8Straight,
    Bgra8Premultiplied,
    EncodedPng,
    EncodedJpeg,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KernelImageDescriptor {
    handle: KernelResourceHandle,
    width: NonZeroU32,
    height: NonZeroU32,
    format: ClipboardImageFormat,
}

impl KernelImageDescriptor {
    pub(crate) fn new(
        handle: KernelResourceHandle,
        width: u32,
        height: u32,
        format: ClipboardImageFormat,
    ) -> Result<Self, ClipboardError> {
        Ok(Self {
            handle,
            width: NonZeroU32::new(width).ok_or(ClipboardError::InvalidResourceDescriptor)?,
            height: NonZeroU32::new(height).ok_or(ClipboardError::InvalidResourceDescriptor)?,
            format,
        })
    }

    pub(crate) const fn handle(self) -> KernelResourceHandle {
        self.handle
    }

    pub(crate) const fn width(self) -> u32 {
        self.width.get()
    }

    pub(crate) const fn height(self) -> u32 {
        self.height.get()
    }

    pub(crate) const fn format(self) -> ClipboardImageFormat {
        self.format
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardAssetClass {
    Document,
    Audio,
    Video,
    Font,
    Archive,
    Opaque(u16),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KernelAssetDescriptor {
    handle: KernelResourceHandle,
    class: ClipboardAssetClass,
    byte_len: u64,
}

impl KernelAssetDescriptor {
    pub(crate) const fn new(
        handle: KernelResourceHandle,
        class: ClipboardAssetClass,
        byte_len: u64,
    ) -> Self {
        Self {
            handle,
            class,
            byte_len,
        }
    }

    pub(crate) const fn handle(self) -> KernelResourceHandle {
        self.handle
    }

    pub(crate) const fn class(self) -> ClipboardAssetClass {
        self.class
    }

    pub(crate) const fn byte_len(self) -> u64 {
        self.byte_len
    }
}

/// Borrowed ingress accepted by [`publish_clip`]. Textual variants are
/// validated and copied into bounded kernel storage before the history lock is
/// changed. No borrowed publisher memory survives acceptance.
pub(crate) enum ClipboardPublish<'a> {
    Text(&'a str),
    Json(&'a str),
    Phone(&'a str),
    Float(f64),
    Password(&'a str),
    Image(KernelImageDescriptor),
    Asset(KernelAssetDescriptor),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardGateLifetime {
    OneShot,
    Persistent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ClipboardGateSpec {
    kind: ClipboardKind,
    lifetime: ClipboardGateLifetime,
}

impl ClipboardGateSpec {
    pub(crate) const fn new(kind: ClipboardKind, lifetime: ClipboardGateLifetime) -> Self {
        Self { kind, lifetime }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardSinkError {
    Closed,
    Busy,
    Rejected,
}

/// Kernel-owned push endpoint. Implementations should enqueue a typed event
/// into their own mediated transport and return promptly.
pub(crate) trait ClipboardDeliverySink: Send + Sync {
    fn on_paste(&self, event: ClipboardPasteEvent) -> Result<(), ClipboardSinkError>;
}

/// Sensitive UTF-8 delivered only to a password-compatible gate. Clones and
/// history eviction zero their backing allocations; debug formatting never
/// renders the value.
pub(crate) struct ClipboardPassword(Zeroizing<AllocString>);

impl ClipboardPassword {
    fn from_str(value: &str) -> Self {
        Self(Zeroizing::new(AllocString::from(value)))
    }

    /// Explicit exposure is limited to the selected password delivery event.
    pub(crate) fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl Clone for ClipboardPassword {
    fn clone(&self) -> Self {
        Self::from_str(self.0.as_str())
    }
}

impl fmt::Debug for ClipboardPassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClipboardPassword")
            .field("utf8_bytes", &self.0.len())
            .field("value", &"<redacted>")
            .finish()
    }
}

/// Owned payload pushed into a live gate. Possessing this value means the
/// Clipboard selected and delivered it; it is not a handle for later reads.
#[derive(Clone)]
pub(crate) enum ClipboardDelivery {
    Text(String<CLIPBOARD_TEXT_MAX_UTF8_BYTES>),
    Json(String<CLIPBOARD_TEXT_MAX_UTF8_BYTES>),
    Phone(String<CLIPBOARD_TEXT_MAX_UTF8_BYTES>),
    Float(f64),
    Password(ClipboardPassword),
    Image(KernelImageDescriptor),
    Asset(KernelAssetDescriptor),
}

impl ClipboardDelivery {
    pub(crate) const fn kind(&self) -> ClipboardKind {
        match self {
            Self::Text(_) => ClipboardKind::Text,
            Self::Json(_) => ClipboardKind::Json,
            Self::Phone(_) => ClipboardKind::Phone,
            Self::Float(_) => ClipboardKind::Float,
            Self::Password(_) => ClipboardKind::Password,
            Self::Image(_) => ClipboardKind::Image,
            Self::Asset(_) => ClipboardKind::Asset,
        }
    }

    fn payload_bytes(&self) -> Option<usize> {
        match self {
            Self::Text(value) | Self::Json(value) | Self::Phone(value) => Some(value.len()),
            Self::Password(value) => Some(value.0.len()),
            Self::Float(_) | Self::Image(_) | Self::Asset(_) => None,
        }
    }
}

impl fmt::Debug for ClipboardDelivery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Metadata-only formatting prevents accidental clipboard-content logs.
        formatter
            .debug_struct("ClipboardDelivery")
            .field("kind", &self.kind())
            .field("payload_bytes", &self.payload_bytes())
            .finish()
    }
}

pub(crate) struct ClipboardPasteEvent {
    delivery: ClipboardDelivery,
}

impl ClipboardPasteEvent {
    pub(crate) fn delivery(&self) -> &ClipboardDelivery {
        &self.delivery
    }

    pub(crate) fn into_delivery(self) -> ClipboardDelivery {
        self.delivery
    }
}

/// Token constructed only by trusted kernel input routing after a real paste
/// gesture. Blueprint memory must never be decoded directly into this type.
pub(crate) struct TrustedClipboardPasteIntent {
    target: MatrixSlotLease,
    recipient: ClipboardPrincipal,
    auth_scope: u8,
}

impl TrustedClipboardPasteIntent {
    pub(crate) fn from_kernel_input(
        target: MatrixSlotLease,
        recipient: ClipboardPrincipal,
        auth_scope: u8,
    ) -> Self {
        Self {
            target,
            recipient,
            auth_scope,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClipboardError {
    PayloadTooLarge {
        kind: ClipboardKind,
        actual_bytes: usize,
        maximum_bytes: usize,
    },
    MalformedJson,
    InvalidResourceDescriptor,
    RateLimited {
        retry_after_ticks: u64,
    },
    PrincipalCapacity,
    GateCapacity,
    GateAlreadyPlaced,
    GateNotFound,
    GatePrincipalMismatch,
    MatrixSlotExpired,
    MatrixAttachmentCapacity,
    AuthenticationRequired,
    ClipboardEmpty,
    NewestClipTypeMismatch {
        newest: ClipboardKind,
        gate: ClipboardKind,
    },
    DeliveryFailed(ClipboardSinkError),
}

impl ClipboardError {
    /// Stable, payload-free integration code suitable for the OS error log.
    /// The core itself performs no logging, so passwords and other clip data
    /// cannot accidentally cross that boundary.
    pub(crate) const fn code(self) -> i32 {
        match self {
            Self::PayloadTooLarge { .. } => -1,
            Self::MalformedJson => -2,
            Self::InvalidResourceDescriptor => -3,
            Self::RateLimited { .. } => -4,
            Self::PrincipalCapacity => -5,
            Self::GateCapacity => -6,
            Self::GateAlreadyPlaced => -7,
            Self::GateNotFound => -8,
            Self::GatePrincipalMismatch => -9,
            Self::MatrixSlotExpired => -10,
            Self::MatrixAttachmentCapacity => -11,
            Self::AuthenticationRequired => -12,
            Self::ClipboardEmpty => -13,
            Self::NewestClipTypeMismatch { .. } => -14,
            Self::DeliveryFailed(_) => -15,
        }
    }
}

struct PrincipalPublishClock {
    principal: ClipboardPrincipal,
    last_accepted_at_ticks: u64,
}

struct ClipboardGate {
    target: MatrixSlotLease,
    recipient: ClipboardPrincipal,
    spec: ClipboardGateSpec,
    sink: Arc<dyn ClipboardDeliverySink>,
    attachment_id: Option<MatrixSlotAttachmentId>,
}

struct ClipboardCore {
    history: Deque<ClipboardDelivery, CLIPBOARD_HISTORY_CAPACITY>,
    publish_clocks: Vec<PrincipalPublishClock, CLIPBOARD_PRINCIPAL_CAPACITY>,
    gates: Vec<ClipboardGate, CLIPBOARD_GATE_CAPACITY>,
}

impl ClipboardCore {
    const fn new() -> Self {
        Self {
            history: Deque::new(),
            publish_clocks: Vec::new(),
            gates: Vec::new(),
        }
    }

    fn publish(
        &mut self,
        principal: ClipboardPrincipal,
        delivery: ClipboardDelivery,
        now_ticks: u64,
    ) -> Result<(), ClipboardError> {
        let clock_index = self
            .publish_clocks
            .iter()
            .position(|clock| clock.principal == principal);
        if let Some(index) = clock_index {
            let earliest = self.publish_clocks[index]
                .last_accepted_at_ticks
                .saturating_add(publish_interval_ticks());
            if now_ticks < earliest {
                return Err(ClipboardError::RateLimited {
                    retry_after_ticks: earliest.saturating_sub(now_ticks),
                });
            }
        } else if self.publish_clocks.is_full() {
            return Err(ClipboardError::PrincipalCapacity);
        }

        // The lock makes eviction plus insertion one indivisible authority
        // transition. Accepted entries are never mutated afterward.
        if self.history.len() == CLIPBOARD_HISTORY_CAPACITY {
            let _ = self.history.pop_front();
        }
        let _ = self.history.push_back(delivery);

        if let Some(index) = clock_index {
            self.publish_clocks[index].last_accepted_at_ticks = now_ticks;
        } else {
            let _ = self.publish_clocks.push(PrincipalPublishClock {
                principal,
                last_accepted_at_ticks: now_ticks,
            });
        }
        Ok(())
    }

    fn reserve_gate(
        &mut self,
        target: MatrixSlotLease,
        recipient: ClipboardPrincipal,
        spec: ClipboardGateSpec,
        sink: Arc<dyn ClipboardDeliverySink>,
    ) -> Result<(), ClipboardError> {
        if self.gates.iter().any(|gate| gate.target == target) {
            return Err(ClipboardError::GateAlreadyPlaced);
        }
        self.gates
            .push(ClipboardGate {
                target,
                recipient,
                spec,
                sink,
                attachment_id: None,
            })
            .map_err(|_| ClipboardError::GateCapacity)
    }

    fn activate_gate(
        &mut self,
        target: &MatrixSlotLease,
        attachment_id: MatrixSlotAttachmentId,
    ) -> bool {
        let Some(gate) = self.gates.iter_mut().find(|gate| gate.target == *target) else {
            return false;
        };
        if gate.attachment_id.is_some() {
            return false;
        }
        gate.attachment_id = Some(attachment_id);
        true
    }

    fn retire_gate(&mut self, target: &MatrixSlotLease) -> bool {
        let Some(index) = self.gates.iter().position(|gate| gate.target == *target) else {
            return false;
        };
        let _ = self.gates.swap_remove(index);
        true
    }

    fn prepare_delivery(
        &self,
        intent: &TrustedClipboardPasteIntent,
        authenticated_two_factor: bool,
    ) -> Result<PreparedDelivery, ClipboardError> {
        let gate = self
            .gates
            .iter()
            .find(|gate| gate.target == intent.target && gate.attachment_id.is_some())
            .ok_or(ClipboardError::GateNotFound)?;
        if gate.recipient != intent.recipient {
            return Err(ClipboardError::GatePrincipalMismatch);
        }

        let delivery = self.newest_delivery_for(gate.spec.kind)?;
        if authorization_required(delivery.kind()) && !authenticated_two_factor {
            return Err(ClipboardError::AuthenticationRequired);
        }

        Ok(PreparedDelivery {
            target: gate.target.clone(),
            attachment_id: gate
                .attachment_id
                .expect("an active Clipboard gate always has a Matrix attachment"),
            one_shot: gate.spec.lifetime == ClipboardGateLifetime::OneShot,
            sink: Arc::clone(&gate.sink),
            event: ClipboardPasteEvent { delivery },
        })
    }

    /// Ordinary paste never searches history. Historical selection is a
    /// separate future authority surface; until then, an incompatible newest
    /// clip is a hard type mismatch even if an older entry would fit.
    fn newest_delivery_for(
        &self,
        gate_kind: ClipboardKind,
    ) -> Result<ClipboardDelivery, ClipboardError> {
        let newest = self.history.back().ok_or(ClipboardError::ClipboardEmpty)?;
        if newest.kind() != gate_kind {
            return Err(ClipboardError::NewestClipTypeMismatch {
                newest: newest.kind(),
                gate: gate_kind,
            });
        }
        Ok(newest.clone())
    }

    fn complete_one_shot(
        &mut self,
        target: &MatrixSlotLease,
        attachment_id: MatrixSlotAttachmentId,
    ) -> bool {
        let Some(index) = self
            .gates
            .iter()
            .position(|gate| gate.target == *target && gate.attachment_id == Some(attachment_id))
        else {
            return false;
        };
        let _ = self.gates.swap_remove(index);
        true
    }
}

struct PreparedDelivery {
    target: MatrixSlotLease,
    attachment_id: MatrixSlotAttachmentId,
    one_shot: bool,
    sink: Arc<dyn ClipboardDeliverySink>,
    event: ClipboardPasteEvent,
}

struct ClipboardMatrixAttachment;

impl MatrixSlotAttachment for ClipboardMatrixAttachment {
    fn on_matrix_slot_freed(&self, lease: &MatrixSlotLease) {
        let _ = CLIPBOARD.lock().retire_gate(lease);
    }
}

static CLIPBOARD: Mutex<ClipboardCore> = Mutex::new(ClipboardCore::new());

/// Publish a typed clip. A successful return is the publisher's final contact
/// with that value; it conveys no ownership or later control.
pub(crate) fn publish_clip(
    principal: ClipboardPrincipal,
    published: ClipboardPublish<'_>,
) -> Result<(), ClipboardError> {
    let delivery = validate_and_own(published)?;
    CLIPBOARD
        .lock()
        .publish(principal, delivery, embassy_time_driver::now())
}

/// Place one typed push gate on an existing Matrix lifetime. The gate is
/// pending (and cannot receive) until Matrix accepts its lifecycle attachment.
pub(crate) fn place_delivery_gate(
    target: MatrixSlotLease,
    recipient: ClipboardPrincipal,
    spec: ClipboardGateSpec,
    sink: Arc<dyn ClipboardDeliverySink>,
) -> Result<(), ClipboardError> {
    if !matrix_slot_is_live(&target) {
        return Err(ClipboardError::MatrixSlotExpired);
    }

    CLIPBOARD
        .lock()
        .reserve_gate(target.clone(), recipient, spec, sink)?;

    let attachment_id =
        match attach_matrix_slot_resource(&target, Arc::new(ClipboardMatrixAttachment)) {
            Ok(id) => id,
            Err(error) => {
                let _ = CLIPBOARD.lock().retire_gate(&target);
                return Err(match error {
                    MatrixSlotAttachmentError::TargetExpired => ClipboardError::MatrixSlotExpired,
                    MatrixSlotAttachmentError::Capacity => ClipboardError::MatrixAttachmentCapacity,
                });
            }
        };

    if CLIPBOARD.lock().activate_gate(&target, attachment_id) {
        Ok(())
    } else {
        let _ = detach_matrix_slot_resource(&target, attachment_id);
        Err(ClipboardError::MatrixSlotExpired)
    }
}

/// Inspect only the newest accepted clip and push it when its type exactly
/// matches the gate. This function never searches or removes history. A
/// one-shot gate is retired only after its sink accepts the event; failed
/// delivery leaves the gate live for a later trusted intent.
pub(crate) fn dispatch_trusted_paste(
    intent: TrustedClipboardPasteIntent,
) -> Result<(), ClipboardError> {
    if !matrix_slot_is_live(&intent.target) {
        let _ = CLIPBOARD.lock().retire_gate(&intent.target);
        return Err(ClipboardError::MatrixSlotExpired);
    }

    // Ask the existing crypt authority at delivery time. Clipboard stores no
    // authentication bit, session snapshot, credential, or bypass token.
    let authenticated_two_factor =
        crate::crypt::has_authenticated_two_factor_session(intent.auth_scope);
    let prepared = CLIPBOARD
        .lock()
        .prepare_delivery(&intent, authenticated_two_factor)?;

    // Do not hold the Clipboard lock while a delivery adapter enqueues work.
    // Slot free may retire the gate concurrently; this already-authorized push
    // is then merely the final in-flight event from the old lifetime.
    prepared
        .sink
        .on_paste(prepared.event)
        .map_err(ClipboardError::DeliveryFailed)?;

    if prepared.one_shot
        && CLIPBOARD
            .lock()
            .complete_one_shot(&prepared.target, prepared.attachment_id)
    {
        let _ = detach_matrix_slot_resource(&prepared.target, prepared.attachment_id);
    }
    Ok(())
}

fn authorization_required(kind: ClipboardKind) -> bool {
    kind == ClipboardKind::Password
        || match CLIPBOARD_PASTE_AUTH_POLICY {
            ClipboardPasteAuthPolicy::AuthenticatedTwoFactorForEveryClip => true,
            ClipboardPasteAuthPolicy::AuthenticatedTwoFactorForPasswords => false,
        }
}

fn publish_interval_ticks() -> u64 {
    embassy_time_driver::TICK_HZ
        .saturating_mul(CLIPBOARD_PUBLISH_INTERVAL_MS)
        .saturating_add(999)
        / 1_000
}

fn validate_and_own(published: ClipboardPublish<'_>) -> Result<ClipboardDelivery, ClipboardError> {
    match published {
        ClipboardPublish::Text(value) => {
            bounded_text(ClipboardKind::Text, value).map(ClipboardDelivery::Text)
        }
        ClipboardPublish::Json(value) => {
            let bounded = bounded_text(ClipboardKind::Json, value)?;
            serde_json::from_str::<serde_json::Value>(bounded.as_str())
                .map_err(|_| ClipboardError::MalformedJson)?;
            Ok(ClipboardDelivery::Json(bounded))
        }
        ClipboardPublish::Phone(value) => {
            bounded_text(ClipboardKind::Phone, value).map(ClipboardDelivery::Phone)
        }
        ClipboardPublish::Float(value) => Ok(ClipboardDelivery::Float(value)),
        ClipboardPublish::Password(value) => {
            reject_oversize(ClipboardKind::Password, value)?;
            Ok(ClipboardDelivery::Password(ClipboardPassword::from_str(value)))
        }
        ClipboardPublish::Image(value) => Ok(ClipboardDelivery::Image(value)),
        ClipboardPublish::Asset(value) => Ok(ClipboardDelivery::Asset(value)),
    }
}

fn bounded_text(
    kind: ClipboardKind,
    value: &str,
) -> Result<String<CLIPBOARD_TEXT_MAX_UTF8_BYTES>, ClipboardError> {
    reject_oversize(kind, value)?;
    let mut stored = String::new();
    stored
        .push_str(value)
        .map_err(|_| ClipboardError::PayloadTooLarge {
            kind,
            actual_bytes: value.len(),
            maximum_bytes: CLIPBOARD_TEXT_MAX_UTF8_BYTES,
        })?;
    Ok(stored)
}

fn reject_oversize(kind: ClipboardKind, value: &str) -> Result<(), ClipboardError> {
    if value.len() > CLIPBOARD_TEXT_MAX_UTF8_BYTES {
        Err(ClipboardError::PayloadTooLarge {
            kind,
            actual_bytes: value.len(),
            maximum_bytes: CLIPBOARD_TEXT_MAX_UTF8_BYTES,
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_history_is_a_ten_entry_overwriting_ring() {
        let mut clipboard = ClipboardCore::new();
        let principal = ClipboardPrincipal::KernelService(7);
        let interval = publish_interval_ticks().max(1);

        for value in 0..=CLIPBOARD_HISTORY_CAPACITY {
            clipboard
                .publish(principal, ClipboardDelivery::Float(value as f64), value as u64 * interval)
                .unwrap();
        }

        assert_eq!(clipboard.history.len(), CLIPBOARD_HISTORY_CAPACITY);
        assert!(matches!(
            clipboard.history.front(),
            Some(ClipboardDelivery::Float(value)) if *value == 1.0
        ));
    }

    #[test]
    fn ordinary_paste_never_falls_back_to_an_older_matching_type() {
        let mut clipboard = ClipboardCore::new();
        let principal = ClipboardPrincipal::KernelService(9);
        let interval = publish_interval_ticks().max(1);
        clipboard
            .publish(
                principal,
                ClipboardDelivery::Text(bounded_text(ClipboardKind::Text, "older").unwrap()),
                0,
            )
            .unwrap();
        clipboard
            .publish(principal, ClipboardDelivery::Float(42.0), interval)
            .unwrap();

        assert!(matches!(
            clipboard.newest_delivery_for(ClipboardKind::Text),
            Err(ClipboardError::NewestClipTypeMismatch {
                newest: ClipboardKind::Float,
                gate: ClipboardKind::Text,
            })
        ));
    }

    #[test]
    fn text_is_rejected_instead_of_truncated_at_513_utf8_bytes() {
        let oversized = "x".repeat(CLIPBOARD_TEXT_MAX_UTF8_BYTES + 1);
        assert!(matches!(
            validate_and_own(ClipboardPublish::Text(oversized.as_str())),
            Err(ClipboardError::PayloadTooLarge {
                actual_bytes: 513,
                maximum_bytes: 512,
                ..
            })
        ));
    }

    #[test]
    fn password_debug_never_contains_the_value() {
        let password = ClipboardPassword::from_str("correct horse battery staple");
        let rendered = alloc::format!("{password:?}");
        assert!(!rendered.contains("correct"));
        assert!(rendered.contains("redacted"));
    }
}
