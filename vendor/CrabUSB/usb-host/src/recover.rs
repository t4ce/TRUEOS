//! Typed semantic recovery operations for a quarantined xHCI controller.
//!
//! This module deliberately contains no JSON or AI policy. It is the compact
//! hardware contract consumed by an operating-system recovery service.

use crate::diag::XhciControllerSnapshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XhciDmaPolicy {
    FromCapability,
    Force32Bit,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum XhciRecoveryStage {
    Constructed,
    TransportPrepared,
    TransportProven,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XhciRecoveryRequest {
    Inspect,
    PrepareTransport { max_slots: u8 },
    ProveNoop { timeout_ms: u32 },
    PollEvents,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XhciRecoveryEvent {
    Nothing,
    PortChange { port: u8 },
    TransferActivity { count: usize },
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XhciRecoveryInspection {
    pub stage: XhciRecoveryStage,
    pub dma_bits: u8,
    pub max_slots_enabled: u8,
    pub snapshot: XhciControllerSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XhciTransportProof {
    pub stage: XhciRecoveryStage,
    pub dma_bits: u8,
    pub max_slots_enabled: u8,
    pub snapshot: XhciControllerSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XhciNoopProof {
    pub stage: XhciRecoveryStage,
    pub dma_bits: u8,
    pub waited_ms: u32,
    pub command_trb_pointer: u64,
    pub completion_slot_id: u8,
    pub snapshot: XhciControllerSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XhciRecoveryEventProof {
    pub event: XhciRecoveryEvent,
    pub snapshot: XhciControllerSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XhciRecoveryResponse {
    Inspection(XhciRecoveryInspection),
    TransportPrepared(XhciTransportProof),
    NoopProven(XhciNoopProof),
    Events(XhciRecoveryEventProof),
}
