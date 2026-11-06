#!/usr/bin/env python3
"""Run the production USB event-pump task with the real task allocator on a host.

Only controller events and timers are replaced. The production task attribute,
controller limit, event dispatch and per-controller sequence counters are used.
"""

import os
import json
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def function_item(source, signature):
    start = source.index(signature)
    opening = source.index("{", start)
    end, depth = opening + 1, 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


def snapshot_library():
    source = (ROOT / "src/usb3/lib.rs").read_text()
    start = source.rfind("#[derive", 0, source.index("pub struct UsbControllerInfo"))
    end = source.index("pub fn observe_probed_devices(")
    observed_start = source.index("static OBSERVED_USB_DEVICES:")
    observed_end = source.index("\n\n", observed_start)
    return ("mod lib {\nuse alloc::{string::String, vec::Vec};\nuse spin::Mutex;\nuse crate::crabusb;\n"
            + source[observed_start:observed_end] + "\n" + source[start:end] + "\n"
            + function_item(source, "fn xhci_port_speed_name(") + "\n"
            + function_item(source, "pub(crate) fn reconcile_disconnected_roots(")
            + "\npub(crate) fn fixture_observe(index: usize, port: u8) { OBSERVED_USB_DEVICES[index].lock().push(TlbUsbDevice {root_port_id: port, ..Default::default()}); }\n}")


def production_items():
    source = (ROOT / "src/usb3/mod.rs").read_text()
    limit = re.search(r"const MAX_CRABUSB_CONTROLLERS: usize = [^;]+;", source).group()
    signature = "pub async fn usb_event_pump_task("
    start = source.rfind("#[trueos_executor::task", 0, source.index(signature))
    opening = source.index("{", source.index(signature))
    end, depth = opening + 1, 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    helpers = []
    for signature in ("async fn await_probe_without_cancel<", "async fn await_controller_operation<"):
        helper_start = source.index(signature)
        helper_opening = source.index("{", helper_start)
        helper_end, depth = helper_opening + 1, 1
        while depth:
            depth += (source[helper_end] == "{") - (source[helper_end] == "}")
            helper_end += 1
        helpers.append(source[helper_start:helper_end])
    watchdog = re.search(r"const PROBE_WATCHDOG_MS: u64 = [^;]+;", source).group()
    return limit + "\n" + watchdog + "\n" + source[start:end] + "\n" + "\n".join(helpers)


HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
use std::sync::atomic::AtomicU32;
#[cfg(test)]
use std::sync::atomic::Ordering;

#[macro_export]
macro_rules! log { ($($tokens:tt)*) => {{ let _ = format_args!($($tokens)*); }}; }
#[macro_export]
macro_rules! log_trace {
    (target: $target:literal; $($tokens:tt)*) => {{ let _ = format_args!($($tokens)*); }};
}
#[macro_export]
macro_rules! log_important {
    (target: $target:literal; $($tokens:tt)*) => {{ let _ = format_args!($($tokens)*); }};
}
mod log_os { pub mod flags { pub const USB_MASS_UAS_TRACE_LOGS: bool = false; } }

@RUNTIME_MODULE@
@LAB_MODULE@
@SNAPSHOT_LIBRARY@

mod disc { pub mod block {
    #[derive(Debug)] pub enum Error { NotReady }
    pub type Result<T> = core::result::Result<T, Error>;
} }
mod pci {
    #[derive(Clone, Copy)]
    pub struct PciDevice { pub bus:u8, pub slot:u8, pub function:u8, pub vendor_id:u16, pub device_id:u16, pub class:u8, pub subclass:u8, pub prog_if:u8 }
    pub fn with_devices<T>(f: impl FnOnce(&[PciDevice]) -> T) -> T {
        f(&[
            PciDevice { bus:0, slot:0x0d, function:0, vendor_id:0x8086, device_id:0x9a13, class:0x0c, subclass:3, prog_if:0x30 },
            PciDevice { bus:0, slot:0x14, function:0, vendor_id:0x8086, device_id:0xa0ed, class:0x0c, subclass:3, prog_if:0x30 },
        ])
    }
}

#[unsafe(export_name = "__pender")]
fn pender(_: *mut ()) {}

mod trueos_time {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    #[derive(Clone, Copy)]
    pub struct Duration(u64);
    impl Duration {
        pub fn from_micros(micros: u64) -> Self { Self(micros / 1000) }
        pub fn from_millis(millis: u64) -> Self { Self(millis) }
    }
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Instant(u64);
    impl Instant {
        pub fn now() -> Self { Self(CLOCK.load(Ordering::Relaxed)) }
        pub fn as_millis(&self) -> u64 { self.0 }
    }
    impl core::ops::Add<Duration> for Instant {
        type Output = Self;
        fn add(self, duration: Duration) -> Self { Self(self.0 + duration.0) }
    }
    pub async fn with_timeout<F: core::future::Future>(duration: Duration, future: F) -> Result<F::Output, ()> {
        let mut future = core::pin::pin!(future);
        core::future::poll_fn(move |cx| match future.as_mut().poll(cx) {
            core::task::Poll::Ready(value) => core::task::Poll::Ready(Ok(value)),
            core::task::Poll::Pending => {
                CLOCK.fetch_add(duration.0, Ordering::Relaxed);
                core::task::Poll::Ready(Err(()))
            }
        }).await
    }
    pub struct Timer;
    impl Timer {
        pub async fn after(_: Duration) {
            let mut yielded = false;
            core::future::poll_fn(move |cx| {
                if yielded {
                    core::task::Poll::Ready(())
                } else {
                    yielded = true;
                    cx.waker().wake_by_ref();
                    core::task::Poll::Pending
                }
            }).await
        }
    }
}

mod crabusb {
    use std::{collections::VecDeque, sync::{Arc, Mutex}};
    pub enum Event {
        Nothing,
        PortChange { port: u8 },
        TransferActivity { count: u64 },
        Stopped,
    }
    #[derive(Clone, Default)]
    pub struct EventHandler(Arc<Mutex<VecDeque<Event>>>);
    impl EventHandler {
        pub fn push(&self, event: Event) { self.0.lock().unwrap().push_back(event); }
        pub fn handle_event(&self) -> Event {
            self.0.lock().unwrap().pop_front().unwrap_or(Event::Nothing)
        }
    }
    pub mod diag {
        #[derive(Clone, Debug)] pub struct XhciPortSnapshot { pub port_id: u8, pub portsc: u32 }
        #[derive(Clone, Debug)] pub struct XhciControllerSnapshot { pub marker: u64, pub ports: Vec<XhciPortSnapshot> }
        pub enum XhciDirectRequest { Snapshot }
        #[derive(Debug)] pub enum XhciDirectResponse { Snapshot(XhciControllerSnapshot), Unexpected }
    }
    pub struct USBHost { pub result: Result<diag::XhciControllerSnapshot, String> }
    impl USBHost {
        pub async fn xhci_direct(&mut self, _: diag::XhciDirectRequest) -> Result<diag::XhciDirectResponse, String> {
            self.result.clone().map(diag::XhciDirectResponse::Snapshot)
        }
    }
}

@PRODUCTION_ITEMS@

static USB_PORT_CHANGE_SEQ: [AtomicU32; MAX_CRABUSB_CONTROLLERS] =
    [const { AtomicU32::new(0) }; MAX_CRABUSB_CONTROLLERS];

#[test]
fn both_controllers_keep_independent_pumps_and_release_their_slots() {
    let executor = Box::leak(Box::new(trueos_executor::raw::Executor::new(core::ptr::null_mut())));
    let handlers: Vec<_> = (0..MAX_CRABUSB_CONTROLLERS)
        .map(|_| std::sync::Arc::new(crabusb::EventHandler::default())).collect();
    for (index, handler) in handlers.iter().enumerate() {
        let token = usb_event_pump_task(index, handler.clone())
            .unwrap_or_else(|error| panic!("controller {index} has no event pump: {error}"));
        executor.spawner().spawn(token);
    }
    let rejected = std::sync::Arc::new(crabusb::EventHandler::default());
    assert!(matches!(
        usb_event_pump_task(0, rejected.clone()),
        Err(trueos_executor::SpawnError::Busy)
    ));
    assert_eq!(std::sync::Arc::strong_count(&rejected), 1, "owner retains DMA ring on allocation failure");

    handlers[0].push(crabusb::Event::PortChange { port: 1 });
    handlers[1].push(crabusb::Event::TransferActivity { count: 7 });
    unsafe { executor.poll() };
    assert_eq!(USB_PORT_CHANGE_SEQ[0].load(Ordering::Acquire), 1);
    assert_eq!(USB_PORT_CHANGE_SEQ[1].load(Ordering::Acquire), 0);
    assert!(runtime::snapshot(0).event_ready);
    assert!(runtime::snapshot(1).event_ready);
    assert!(runtime::snapshot(0).root_port_change_seen);
    assert!(!runtime::snapshot(1).root_port_change_seen);

    handlers[1].push(crabusb::Event::PortChange { port: 4 });
    unsafe { executor.poll() };
    assert_eq!(USB_PORT_CHANGE_SEQ[0].load(Ordering::Acquire), 1);
    assert_eq!(USB_PORT_CHANGE_SEQ[1].load(Ordering::Acquire), 1);

    for handler in &handlers { handler.push(crabusb::Event::Stopped); }
    unsafe { executor.poll() };
    assert!(!runtime::snapshot(0).event_ready);
    assert!(!runtime::snapshot(1).event_ready);
    assert_eq!(std::sync::Arc::strong_count(&handlers[0]), 1, "owner retains DMA ring after pump stops");
    for index in 0..MAX_CRABUSB_CONTROLLERS {
        let handler = std::sync::Arc::new(crabusb::EventHandler::default());
        handler.push(crabusb::Event::Stopped);
        executor.spawner().spawn(usb_event_pump_task(index, handler).expect("pump slot is reusable"));
    }
    unsafe { executor.poll() };
    watchdog_retains_the_same_pending_future();
    snapshots_and_quarantine_preserve_controller_ownership();
}

#[cfg(test)]
fn watchdog_retains_the_same_pending_future() {
    use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize}};
    struct SlowProbe { polls: Arc<AtomicUsize>, completed: bool, premature_drop: Arc<AtomicBool> }
    impl core::future::Future for SlowProbe {
        type Output = u8;
        fn poll(mut self: core::pin::Pin<&mut Self>, _: &mut core::task::Context<'_>) -> core::task::Poll<u8> {
            if self.polls.fetch_add(1, Ordering::Relaxed) == 3 {
                self.completed = true;
                core::task::Poll::Ready(42)
            } else { core::task::Poll::Pending }
        }
    }
    impl Drop for SlowProbe {
        fn drop(&mut self) { if !self.completed { self.premature_drop.store(true, Ordering::Relaxed); } }
    }
    let polls = Arc::new(AtomicUsize::new(0));
    let premature_drop = Arc::new(AtomicBool::new(false));
    let before = runtime::snapshot(0).probe_watchdog_timeouts;
    runtime::update(0, |runtime| runtime.probe_started("host-watchdog", 0));
    let operation = SlowProbe { polls: polls.clone(), completed: false, premature_drop: premature_drop.clone() };
    let mut future = core::pin::pin!(await_probe_without_cancel(0, operation));
    let result = core::future::Future::poll(future.as_mut(), &mut core::task::Context::from_waker(core::task::Waker::noop()));
    assert_eq!(result, core::task::Poll::Ready(42));
    assert_eq!(polls.load(Ordering::Relaxed), 4);
    assert!(!premature_drop.load(Ordering::Relaxed));
    assert_eq!(runtime::snapshot(0).probe_watchdog_timeouts - before, 3);
    assert_eq!(runtime::snapshot(0).last_probe_elapsed_ms, 6000);
    let polls = Arc::new(AtomicUsize::new(0));
    let operation = SlowProbe { polls: polls.clone(), completed: false, premature_drop: premature_drop.clone() };
    let mut future = core::pin::pin!(await_controller_operation(0, "initializing", operation));
    let result = core::future::Future::poll(future.as_mut(), &mut core::task::Context::from_waker(core::task::Waker::noop()));
    assert_eq!(result, core::task::Poll::Ready(42));
    assert_eq!(polls.load(Ordering::Relaxed), 4);
    assert!(!premature_drop.load(Ordering::Relaxed));
    let runtime = runtime::snapshot(0);
    assert!(!runtime.operation_requested);
    assert_eq!((runtime.operation_stage, runtime.operation_elapsed_ms, runtime.operation_watchdog_timeouts),
        ("initializing", 6000, 3));
}

#[cfg(test)]
fn ready<F: core::future::Future>(future: F) -> F::Output {
    match core::future::Future::poll(core::pin::pin!(future), &mut core::task::Context::from_waker(core::task::Waker::noop())) {
        core::task::Poll::Ready(result) => result,
        core::task::Poll::Pending => panic!("fixture operation unexpectedly pending"),
    }
}

#[cfg(test)]
fn snapshots_and_quarantine_preserve_controller_ownership() {
    let mut first = crabusb::USBHost { result: Ok(crabusb::diag::XhciControllerSnapshot { marker: 100,
        ports: vec![crabusb::diag::XhciPortSnapshot { port_id: 1, portsc: 1 }] }) };
    let mut second = crabusb::USBHost { result: Ok(crabusb::diag::XhciControllerSnapshot { marker: 200,
        ports: vec![crabusb::diag::XhciPortSnapshot { port_id: 3, portsc: 1 }, crabusb::diag::XhciPortSnapshot { port_id: 4, portsc: 0 }] }) };
    ready(lab::refresh_snapshot(0, &mut first)).unwrap();
    ready(lab::refresh_snapshot(1, &mut second)).unwrap();
    assert_eq!(lab::latest_snapshot(0).unwrap().marker, 100);
    assert_eq!(lab::latest_snapshot(1).unwrap().marker, 200);
    assert_eq!(runtime::snapshot(0).snapshot_sequence, 1);
    assert_eq!(runtime::snapshot(1).snapshot_sequence, 1);
    second.result = Err(String::from("MMIO observation failed"));
    assert!(ready(lab::refresh_snapshot(1, &mut second)).is_err());
    assert_eq!(lab::latest_snapshot(1).unwrap().marker, 200);
    assert!(runtime::snapshot(1).snapshot_error.as_deref().unwrap().contains("MMIO observation failed"));
    assert!(runtime::snapshot(0).snapshot_error.is_none());
    assert_eq!(runtime::snapshot(1).snapshot_sequence, 1);
    assert!(lab::latest_snapshot(MAX_CRABUSB_CONTROLLERS).is_none());
    let topology = lib::tlb_usb_snapshot().topology;
    assert_eq!(topology.iter().map(|node| (node.controller_index, node.root_port_id)).collect::<Vec<_>>(), vec![(0,1), (1,3), (1,4)]);
    runtime::update(0, |state| state.stage("init-failed", "failed"));
    runtime::update(1, |state| { state.stage("maintenance", "active"); state.event_ready = true; });
    let controllers = lib::pci_usb_controllers();
    assert_eq!((controllers[0].controller_phase, controllers[0].root_hub_lifecycle, controllers[0].event_ready), ("init-failed", "failed", false));
    assert_eq!((controllers[1].controller_phase, controllers[1].root_hub_lifecycle, controllers[1].event_ready), ("maintenance", "active", true));
    lib::fixture_observe(1, 4);
    assert_eq!(lib::tlb_usb_snapshot().devices.len(), 1);
    lib::reconcile_disconnected_roots(1, &lab::latest_snapshot(1).unwrap());
    assert!(lib::tlb_usb_snapshot().devices.is_empty());

    let inflight = ready(lab::begin_uas_io_when_available());
    {
        let mut quarantine = Box::pin(lab::enter_controller_quarantine());
        let poll = core::future::Future::poll(quarantine.as_mut(), &mut core::task::Context::from_waker(core::task::Waker::noop()));
        assert!(poll.is_pending());
        // Dropping this pending request must release admission immediately.
    }
    let after_cancel = ready(lab::begin_uas_io_when_available());
    drop(after_cancel);
    drop(inflight);
    let quarantine = ready(lab::enter_controller_quarantine()).unwrap();
    {
        let mut admission = Box::pin(lab::begin_uas_io_when_available());
        assert!(core::future::Future::poll(admission.as_mut(), &mut core::task::Context::from_waker(core::task::Waker::noop())).is_pending());
    }
    drop(quarantine);
    drop(ready(lab::begin_uas_io_when_available()));
}

#[test]
fn runtime_records_real_lifecycle_success_failure_and_pending_watchdog() {
    let mut state = runtime::UsbRuntimeDiag::new();
    assert_eq!((state.controller_phase, state.root_hub_lifecycle, state.event_ready, state.last_probe_state),
        ("discovered", "unclaimed", false, "never"));
    state.stage("discovering", "active");
    state.event_ready = true;
    state.probe_started("initial", 10);
    state.probe_watchdog(2010);
    assert!(state.probe_requested);
    assert_eq!((state.probe_attempts, state.probe_failures, state.probe_watchdog_timeouts), (1, 0, 1));
    state.probe_finished(Err(String::from("address completion failed")), 2020);
    assert!(!state.probe_requested);
    assert!(state.event_ready);
    assert_eq!(state.root_hub_lifecycle, "active");
    assert_eq!(state.last_error.as_deref(), Some("address completion failed"));
    assert_eq!((state.probe_fail_streak, state.probe_failures, state.last_probe_elapsed_ms), (1, 1, 2010));
    state.probe_started("retry", 3000);
    state.probe_finished(Ok(0), 3002);
    assert_eq!((state.empty_probe_streak, state.probe_fail_streak, state.probe_successes), (1, 0, 1));
    state.probe_started("rescan", 4000);
    state.probe_finished(Ok(2), 4010);
    assert_eq!((state.empty_probe_streak, state.last_probe_device_count, state.probe_attempts), (0, 2, 3));
    state.operation_started("handoff", 4020);
    assert!(state.operation_requested);
    state.operation_watchdog(6020);
    state.operation_finished(6030);
    assert_eq!((state.operation_requested, state.operation_elapsed_ms, state.controller_phase),
        (false, 2010, "maintenance"));
    state.pump_stopped();
    assert_eq!((state.event_ready, state.root_hub_lifecycle, state.last_error_stage), (false, "failed", "event-pump"));
}
'''


def run():
    with tempfile.TemporaryDirectory(prefix="trueos-usb-pumps-") as temporary:
        project = Path(temporary)
        (project / "src").mkdir()
        (project / "src/lib.rs").write_text(
            HARNESS.replace("@PRODUCTION_ITEMS@", production_items())
                .replace("@RUNTIME_MODULE@", f'#[path = {json.dumps(str(ROOT / "src/usb3/runtime.rs"))}] mod runtime;')
                .replace("@LAB_MODULE@", "mod lab {\n" + (ROOT / "src/usb3/lab.rs").read_text()
                    .replace("//!", "//").replace("use trueos_time::", "use crate::trueos_time::") + "\n}")
                .replace("@SNAPSHOT_LIBRARY@", snapshot_library())
        )
        (project / "Cargo.toml").write_text(
            '[package]\nname = "usb-pump-regression"\nversion = "0.0.0"\nedition = "2024"\n'
            '[workspace]\n[dependencies]\n'
            f'trueos-executor = {{ path = "{ROOT / "crates/trueos-executor"}" }}\n'
            'spin = "0.10"\n'
            '[patch.crates-io]\n'
            f'cordyceps = {{ path = "{ROOT / "vendor/cordyceps-0.3.5"}" }}\n'
        )
        channel = re.search(
            r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text()
        ).group(1)
        env = os.environ.copy()
        env.pop("RUSTFLAGS", None)
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        subprocess.run(
            ["cargo", f"+{channel}", "test", "--offline", "--manifest-path", str(project / "Cargo.toml")],
            cwd=project,
            env=env,
            check=True,
        )


if __name__ == "__main__":
    run()
