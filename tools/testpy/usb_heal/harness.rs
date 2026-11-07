#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;

use core::{future::Future, task::{Context, Poll, Waker}};
use std::sync::atomic::{AtomicU64, Ordering};
static NOW: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant(u64);
#[derive(Clone, Copy)]
pub struct Duration(u64);
impl Instant { pub fn now() -> Self { Self(NOW.load(Ordering::Relaxed)) } pub fn as_millis(self) -> u64 { self.0 } }
impl Duration { pub fn from_millis(ms: u64) -> Self { Self(ms) } }
impl core::ops::Add<Duration> for Instant {
    type Output = Self;
    fn add(self, rhs: Duration) -> Self { Self(self.0 + rhs.0) }
}
pub struct Timer;
impl Timer { pub async fn after(duration: Duration) { NOW.fetch_add(duration.0, Ordering::Relaxed); } }
pub async fn with_timeout<F: Future>(duration: Duration, future: F) -> Result<F::Output, ()> {
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => Ok(value),
        Poll::Pending => { NOW.fetch_add(duration.0, Ordering::Relaxed); Err(()) }
    }
}
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    for _ in 0..1_000 {
        if let Poll::Ready(value) = future.as_mut().poll(&mut Context::from_waker(Waker::noop())) { return value; }
    }
    panic!("unexpected unbounded owner wait")
}
#[macro_export] macro_rules! log { ($($tokens:tt)*) => {{ let _ = format_args!($($tokens)*); }}; }
mod wait {
    pub struct EmbassySpinRawMutex(spin::Mutex<()>);
    unsafe impl embassy_sync::blocking_mutex::raw::RawMutex for EmbassySpinRawMutex {
        const INIT: Self = Self(spin::Mutex::new(()));
        fn lock<R>(&self, f: impl FnOnce() -> R) -> R { let _lock = self.0.lock(); f() }
    }
}
mod pci {
    #[derive(Clone, Copy)]
    pub struct PciDevice { pub bus: u8, pub slot: u8, pub function: u8, pub vendor_id: u16, pub device_id: u16 }
    pub fn config_read_u8(_: u8, _: u8, _: u8, _: u16) -> u8 { 1 }
    pub fn config_read_u16(_: u8, _: u8, _: u8, _: u16) -> u16 { 0 }
    pub fn claim_device(_: &PciDevice, _: &str) -> Result<(), &'static str> { Ok(()) }
}
mod usb3 {
    pub const MAX_CRABUSB_CONTROLLERS: usize = 2;
    pub mod lab {
        use super::crabusb::diag::XhciControllerSnapshot;
        static LATEST: [spin::Mutex<Option<XhciControllerSnapshot>>; super::MAX_CRABUSB_CONTROLLERS] =
            [const { spin::Mutex::new(None) }; super::MAX_CRABUSB_CONTROLLERS];
        pub fn latest_snapshot(index: usize) -> Option<XhciControllerSnapshot> {
            LATEST.get(index)?.lock().clone()
        }
        pub fn publish(index: usize, snapshot: Option<XhciControllerSnapshot>) {
            *LATEST[index].lock() = snapshot;
        }
    }
    pub mod runtime {
        use std::sync::atomic::{AtomicBool, Ordering};
        pub static READY: AtomicBool = AtomicBool::new(true);
        pub static BUSY: AtomicBool = AtomicBool::new(false);
        pub static OPERATING: AtomicBool = AtomicBool::new(false);
        pub struct State {
            pub event_ready: bool, pub probe_requested: bool, pub operation_requested: bool,
            pub controller_phase: &'static str, pub root_hub_lifecycle: &'static str,
            pub operation_stage: &'static str, pub operation_elapsed_ms: u64,
            pub operation_watchdog_timeouts: u64, pub probe_watchdog_timeouts: u64,
            pub last_probe_state: &'static str, pub last_error: Option<String>,
            pub snapshot_sequence: u64, pub snapshot_at_ms: Option<u64>, pub snapshot_error: Option<String>,
        }
        pub fn snapshot(_: usize) -> State { State {
            event_ready: READY.load(Ordering::Relaxed), probe_requested: BUSY.load(Ordering::Relaxed),
            operation_requested: OPERATING.load(Ordering::Relaxed), controller_phase: "maintenance", root_hub_lifecycle: "running",
            operation_stage: "idle", operation_elapsed_ms: 0, operation_watchdog_timeouts: 0,
            probe_watchdog_timeouts: 0, last_probe_state: "ok", last_error: None,
            snapshot_sequence: 1, snapshot_at_ms: Some(0), snapshot_error: None,
        } }
    }
    #[path = "@SERVICE@"] pub mod heal_service;
    #[path = "@PROTOCOL@"] pub mod heal_protocol;
    pub mod crabusb {
        #[path = "@DIAG@"] pub mod diag;
        #[path = "@RECOVER@"] pub mod recover;
        use diag::*;
        use recover::*;
        pub struct Mmio;
        pub trait KernelOp {}
        pub struct XhciConstructionPolicy;
        impl XhciConstructionPolicy { pub fn quarantined() -> Self { Self } }
        pub struct USBHost {
            pub snapshot: XhciControllerSnapshot,
            pub controller_resets: usize,
            pub port_resets: usize,
            pub ever_probed: bool,
            pub reset_completes: bool,
        }
        impl USBHost {
            pub fn new_xhci_with_construction_policy_and_mmio_len(_: Mmio, _: usize, _: &'static dyn KernelOp, _: XhciConstructionPolicy) -> Result<Self, &'static str> { Err("not exercised") }
            pub async fn xhci_recover(&mut self, request: XhciRecoveryRequest) -> Result<XhciRecoveryResponse, &'static str> {
                match request {
                    XhciRecoveryRequest::Inspect => {
                        if self.reset_completes && self.snapshot.ports[0].portsc & (1 << 4) != 0 {
                            self.snapshot.ports[0].portsc &= !(1 << 4);
                            self.snapshot.ports[0].portsc |= 1 << 1;
                        }
                        Ok(XhciRecoveryResponse::Inspection(XhciRecoveryInspection {
                            stage: XhciRecoveryStage::TransportPrepared, dma_bits: 64, max_slots_enabled: 32, snapshot: self.snapshot.clone()
                        }))
                    }
                    XhciRecoveryRequest::PrepareTransport { max_slots } => {
                        self.controller_resets += 1;
                        Ok(XhciRecoveryResponse::TransportPrepared(XhciTransportProof {
                            stage: XhciRecoveryStage::TransportPrepared, dma_bits: 64, max_slots_enabled: max_slots, snapshot: self.snapshot.clone()
                        }))
                    }
                    XhciRecoveryRequest::ProveNoop { .. } => Ok(XhciRecoveryResponse::NoopProven(XhciNoopProof {
                        stage: XhciRecoveryStage::TransportProven, dma_bits: 64, waited_ms: 10,
                        command_trb_pointer: 4096, completion_slot_id: 0, snapshot: self.snapshot.clone()
                    })),
                    XhciRecoveryRequest::PollEvents => Ok(XhciRecoveryResponse::Events(XhciRecoveryEventProof {
                        event: XhciRecoveryEvent::Nothing, snapshot: self.snapshot.clone()
                    })),
                }
            }
            pub async fn request_root_port_reset(&mut self, port: u8) -> Result<(), &'static str> {
                let idx = usize::from(port.checked_sub(1).ok_or("zero port")?);
                let item = self.snapshot.ports.get_mut(idx).ok_or("invalid port")?;
                if let Some(reason) = explicit_reset_rejection(self.ever_probed, item.portsc) { return Err(reason); }
                self.port_resets += 1;
                item.portsc |= 1 << 4;
                Ok(())
            }
        }
        @RESET_GUARD@
    }
}
pub use usb3::crabusb::diag;
fn host() -> usb3::crabusb::USBHost {
    use usb3::crabusb::{self, diag::*};
    crabusb::USBHost {
        snapshot: XhciControllerSnapshot {
            enumeration: XhciEnumerationDiagnostics::default(),
            mmio_len: 4096, caplength: 32, hciversion: 0x110,
            hcsparams1: (1 << 24) | 32, hcsparams2: 0, hcsparams3: 0,
            hccparams1: 1 << 3, hccparams2: 0, dboff: 0, rtsoff: 0,
            usbcmd: 1, usbsts: 0, pagesize: 1, dnctrl: 0, crcr: 4096,
            dcbaap: 8192, config: 32, mfindex: 5, iman: 0, imod: 0,
            erstsz: 1, erstba: 12288, erdp: 12304,
            protocols: vec![XhciProtocolSnapshot { major: 2, minor: 0, name: 0, port_offset: 1, port_count: 1, slot_type: 0, psi_count: 0 }],
            ports: vec![XhciPortSnapshot { port_id: 1, portsc: 1 | (1 << 9), portpmsc: 0, portli: 0, porthlpmc: 0 }],
        },
        controller_resets: 0, port_resets: 0, ever_probed: false, reset_completes: true,
    }
}
fn dev(index: u8) -> pci::PciDevice { pci::PciDevice { bus: 0, slot: 10 + index, function: 0, vendor_id: 0x8086, device_id: 0x9a13 } }
