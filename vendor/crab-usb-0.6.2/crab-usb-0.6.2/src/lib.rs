#![cfg_attr(target_os = "none", no_std)]
#![feature(iterator_try_collect)]

#[macro_use]
extern crate alloc;
#[macro_use]
extern crate log;
#[macro_use]
extern crate anyhow;

use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub use usb_if;

#[macro_use]
mod _macros;

pub(crate) mod backend;
pub mod device;
pub mod err;
mod host;

pub use crate::backend::ty::Event;
pub use crate::backend::ty::ep::{
    EndpointBulkIn, EndpointBulkOut, EndpointControl, EndpointInterruptIn, EndpointInterruptOut,
    EndpointIsoIn, EndpointIsoOut, EndpointKind,
};
pub use host::*;

#[allow(unused_imports)]
#[cfg(kmod)]
pub use crate::backend::kmod::*;

define_int_type!(BusAddr, u64);

pub type Mmio = NonNull<u8>;

#[derive(Clone, Copy, Debug, Default)]
pub struct DebugLastSubmit {
    pub dci: u8,
    pub direction: u8,
    pub len: u32,
    pub ptr: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DebugLastEvent {
    pub slot_id: u8,
    pub ep_id: u8,
    pub completion_code: u8,
    pub residual: u32,
    pub ptr: u64,
}

static DEBUG_LAST_SUBMIT_DCI: AtomicU32 = AtomicU32::new(0);
static DEBUG_LAST_SUBMIT_DIR: AtomicU32 = AtomicU32::new(0);
static DEBUG_LAST_SUBMIT_LEN: AtomicU32 = AtomicU32::new(0);
static DEBUG_LAST_SUBMIT_PTR: AtomicU64 = AtomicU64::new(0);

static DEBUG_LAST_EVENT_SLOT: AtomicU32 = AtomicU32::new(0);
static DEBUG_LAST_EVENT_EP: AtomicU32 = AtomicU32::new(0);
static DEBUG_LAST_EVENT_CC: AtomicU32 = AtomicU32::new(0);
static DEBUG_LAST_EVENT_RESIDUAL: AtomicU32 = AtomicU32::new(0);
static DEBUG_LAST_EVENT_PTR: AtomicU64 = AtomicU64::new(0);

pub fn debug_record_submit(dci: u8, direction: u8, len: u32, ptr: u64) {
    DEBUG_LAST_SUBMIT_DCI.store(dci as u32, Ordering::Release);
    DEBUG_LAST_SUBMIT_DIR.store(direction as u32, Ordering::Release);
    DEBUG_LAST_SUBMIT_LEN.store(len, Ordering::Release);
    DEBUG_LAST_SUBMIT_PTR.store(ptr, Ordering::Release);
}

pub fn debug_record_event(slot_id: u8, ep_id: u8, completion_code: u8, residual: u32, ptr: u64) {
    DEBUG_LAST_EVENT_SLOT.store(slot_id as u32, Ordering::Release);
    DEBUG_LAST_EVENT_EP.store(ep_id as u32, Ordering::Release);
    DEBUG_LAST_EVENT_CC.store(completion_code as u32, Ordering::Release);
    DEBUG_LAST_EVENT_RESIDUAL.store(residual, Ordering::Release);
    DEBUG_LAST_EVENT_PTR.store(ptr, Ordering::Release);
}

pub fn debug_last_submit() -> DebugLastSubmit {
    DebugLastSubmit {
        dci: DEBUG_LAST_SUBMIT_DCI.load(Ordering::Acquire) as u8,
        direction: DEBUG_LAST_SUBMIT_DIR.load(Ordering::Acquire) as u8,
        len: DEBUG_LAST_SUBMIT_LEN.load(Ordering::Acquire),
        ptr: DEBUG_LAST_SUBMIT_PTR.load(Ordering::Acquire),
    }
}

pub fn debug_last_event() -> DebugLastEvent {
    DebugLastEvent {
        slot_id: DEBUG_LAST_EVENT_SLOT.load(Ordering::Acquire) as u8,
        ep_id: DEBUG_LAST_EVENT_EP.load(Ordering::Acquire) as u8,
        completion_code: DEBUG_LAST_EVENT_CC.load(Ordering::Acquire) as u8,
        residual: DEBUG_LAST_EVENT_RESIDUAL.load(Ordering::Acquire),
        ptr: DEBUG_LAST_EVENT_PTR.load(Ordering::Acquire),
    }
}
