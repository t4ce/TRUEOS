#!/usr/bin/env python3
"""Run the production USB event-pump task with the real task allocator on a host.

Only controller events and timers are replaced. The production task attribute,
controller limit, event dispatch and per-controller sequence counters are used.
"""

import os
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


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
    return limit + "\n" + source[start:end]


HARNESS = r'''
use std::sync::atomic::AtomicU32;
#[cfg(test)]
use std::sync::atomic::Ordering;

#[macro_export]
macro_rules! log { ($($tokens:tt)*) => {{ let _ = format_args!($($tokens)*); }}; }
#[macro_export]
macro_rules! log_trace {
    (target: $target:literal; $($tokens:tt)*) => {{ let _ = format_args!($($tokens)*); }};
}
mod log_os { pub mod flags { pub const USB_MASS_UAS_TRACE_LOGS: bool = false; } }

#[unsafe(export_name = "__pender")]
fn pender(_: *mut ()) {}

mod trueos_time {
    pub struct Duration;
    impl Duration { pub fn from_micros(_: u64) -> Self { Self } }
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
}

@PRODUCTION_ITEMS@

static USB_PORT_CHANGE_SEQ: [AtomicU32; MAX_CRABUSB_CONTROLLERS] =
    [const { AtomicU32::new(0) }; MAX_CRABUSB_CONTROLLERS];

#[test]
fn both_controllers_keep_independent_pumps_and_release_their_slots() {
    let executor = Box::leak(Box::new(trueos_executor::raw::Executor::new(core::ptr::null_mut())));
    let handlers: Vec<_> = (0..MAX_CRABUSB_CONTROLLERS)
        .map(|_| crabusb::EventHandler::default()).collect();
    for (index, handler) in handlers.iter().enumerate() {
        let token = usb_event_pump_task(index, handler.clone())
            .unwrap_or_else(|error| panic!("controller {index} has no event pump: {error}"));
        executor.spawner().spawn(token);
    }
    assert!(matches!(
        usb_event_pump_task(0, crabusb::EventHandler::default()),
        Err(trueos_executor::SpawnError::Busy)
    ));

    handlers[0].push(crabusb::Event::PortChange { port: 1 });
    handlers[1].push(crabusb::Event::TransferActivity { count: 7 });
    unsafe { executor.poll() };
    assert_eq!(USB_PORT_CHANGE_SEQ[0].load(Ordering::Acquire), 1);
    assert_eq!(USB_PORT_CHANGE_SEQ[1].load(Ordering::Acquire), 0);

    handlers[1].push(crabusb::Event::PortChange { port: 4 });
    unsafe { executor.poll() };
    assert_eq!(USB_PORT_CHANGE_SEQ[0].load(Ordering::Acquire), 1);
    assert_eq!(USB_PORT_CHANGE_SEQ[1].load(Ordering::Acquire), 1);

    for handler in &handlers { handler.push(crabusb::Event::Stopped); }
    unsafe { executor.poll() };
    for index in 0..MAX_CRABUSB_CONTROLLERS {
        let handler = crabusb::EventHandler::default();
        handler.push(crabusb::Event::Stopped);
        executor.spawner().spawn(usb_event_pump_task(index, handler).expect("pump slot is reusable"));
    }
    unsafe { executor.poll() };
}
'''


def run():
    with tempfile.TemporaryDirectory(prefix="trueos-usb-pumps-") as temporary:
        project = Path(temporary)
        (project / "src").mkdir()
        (project / "src/lib.rs").write_text(
            HARNESS.replace("@PRODUCTION_ITEMS@", production_items())
        )
        (project / "Cargo.toml").write_text(
            '[package]\nname = "usb-pump-regression"\nversion = "0.0.0"\nedition = "2024"\n'
            '[workspace]\n[dependencies]\n'
            f'trueos-executor = {{ path = "{ROOT / "crates/trueos-executor"}" }}\n'
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
