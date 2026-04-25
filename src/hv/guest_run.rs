//! Guest-side shell2 instance driven over the vmcall I/O bridge.
//!
//! The guest kernel shares physical memory with the host via an identity EPT
//! (guest PA == host PA for all of 4 GB), so the heap, time driver, and all
//! kernel statics are already live when `trueos_hv_guest_shell_run` is called.
//! We only need a fresh Embassy executor and the thin `VmcallShellBackend`.
//!
//! I/O path:
//!   nc <host>:4245  <->  NET_SHELL_STATE  <->  vmcall bridge  <->  VmcallShellBackend
//!
//! Caveat: the host's net-tcp shell2 task and the guest's shell2 task both
//! route through the same `NET_SHELL_STATE` queues.  Bytes will be stolen by
//! whichever side polls first.  This tension is intentional – we are
//! rediscovering the original network/architecture block by running it live.

use alloc::boxed::Box;

use embassy_executor::raw::Executor as RawExecutor;
use trueos_vm::vmcall;

use crate::blueprint;
use crate::shell2::{ShellBackend2, ShellIo2};

// ── VmcallShellBackend ────────────────────────────────────────────────────────

pub(crate) struct VmcallShellBackend;

pub(crate) static VMCALL_SHELL: VmcallShellBackend = VmcallShellBackend;

impl ShellIo2 for VmcallShellBackend {
    fn write_str(&self, s: &str) {
        vmcall::net_tcp_write(s.as_bytes());
    }

    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        struct W;
        impl core::fmt::Write for W {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                vmcall::net_tcp_write(s.as_bytes());
                Ok(())
            }
        }
        let _ = core::fmt::Write::write_fmt(&mut W, args);
    }

    fn write_char(&self, ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        vmcall::net_tcp_write(s.as_bytes());
    }

    fn write_byte(&self, b: u8) {
        vmcall::net_tcp_write(&[b]);
    }
}

impl ShellBackend2 for VmcallShellBackend {
    fn init(&self) {}

    fn read_byte(&self) -> Option<u8> {
        let mut b = [0u8; 1];
        if vmcall::net_tcp_read(&mut b) > 0 {
            Some(b[0])
        } else {
            None
        }
    }
}

// ── guest shell entry ─────────────────────────────────────────────────────────

/// Called from `trueos_vm_guest_idle` in the guest binary.
///
/// The host's `kmain()` already ran before the VM was launched, so:
///   – global heap allocator is live (shared via identity EPT)
///   – Embassy time driver is calibrated (TSC-based; driven by `time::poll()`)
///   – all kernel statics are initialised
///
/// We create a standalone Embassy executor (not registered with percpu) and
/// run the real shell2 task over the vmcall I/O bridge.
#[unsafe(no_mangle)]
pub extern "C" fn trueos_hv_guest_shell_run() -> ! {
    vmcall::net_tcp_write(b"guest-shell: launching shell2 over vmcall bridge\r\n");

    // Allocate a fresh executor from the (already-initialised) host heap.
    // `null_mut()` pender: we busy-poll below, no signal needed.
    let executor: &'static mut RawExecutor =
        Box::leak(Box::new(RawExecutor::new(core::ptr::null_mut())));

    let spawner = unsafe { executor.spawner() };

    match crate::shell2::task(spawner, &VMCALL_SHELL) {
        Ok(token) => {
            spawner.spawn(token);
            vmcall::net_tcp_write(b"guest-shell: shell2 task spawned\r\n");
        }
        Err(_) => {
            vmcall::net_tcp_write(
                b"guest-shell: spawn failed - shell2 task pool exhausted (increase pool_size)\r\n",
            );
            loop {
                core::hint::spin_loop();
            }
        }
    }

    // Poll loop: `time::poll()` fires TSC-based timer wakers so
    // `Timer::after(5ms)` in the shell2 idle branch resolves correctly.
    loop {
        crate::time::poll();
        unsafe { executor.poll() };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_hv_guest_blueprint_launch_active() -> bool {
    crate::hv::blueprint_launch_active()
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_hv_guest_blueprint_run() -> bool {
    let Some(state) = crate::hv::take_blueprint_launch() else {
        return false;
    };

    let log = |line: &str| crate::hv::hvlogf(format_args!("{}", line));

    log(alloc::format!("run: guest blueprint launch archive={}", state.archive.as_str()).as_str());

    let module = match blueprint::parse_blueprint(state.module_bytes.as_slice()) {
        Ok(module) => module,
        Err(err) => {
            log(alloc::format!("run: guest blueprint parse failed: {}", err).as_str());
            return false;
        }
    };

    let unpacked = match blueprint::unpack_blueprint(&module) {
        Ok(bytes) => bytes,
        Err(err) => {
            log(alloc::format!("run: guest blueprint unpack failed: {}", err).as_str());
            return false;
        }
    };

    if !unpacked.starts_with(b"\x7fELF")
        || !matches!(blueprint::elf_type_name(unpacked.as_slice()), Some("REL"))
    {
        log("run: guest blueprint rejected non-REL payload");
        return false;
    }

    match blueprint::elf_imports(unpacked.as_slice()) {
        Ok(imports) => {
            let unresolved = imports
                .iter()
                .filter(|import| import.resolved_addr.is_none())
                .count();
            log(alloc::format!(
                "run: guest ELF imports={} unresolved={}",
                imports.len(),
                unresolved
            )
            .as_str());
            for import in imports
                .iter()
                .filter(|import| import.resolved_addr.is_none())
                .take(16)
            {
                log(alloc::format!("run: guest unresolved import {}", import.name).as_str());
            }
        }
        Err(err) => {
            log(alloc::format!("run: guest ELF import scan failed: {}", err).as_str());
        }
    }

    let process_args =
        blueprint::build_process_args(state.archive.as_str(), state.app_args.as_slice());
    let process_env = blueprint::build_process_env(state.archive.as_str());
    crate::hv::begin_blueprint_app_window_session(state.archive.as_str());
    crate::blueprint_net_broker::set_vmx_guest_net_backend(true);
    let invoke_result = blueprint::invoke_host_rel(
        unpacked.as_slice(),
        module.entry,
        process_args,
        process_env,
        state.console_target.clone(),
    );
    crate::blueprint_net_broker::set_vmx_guest_net_backend(false);
    match invoke_result {
        Ok(()) => {
            crate::hv::finish_blueprint_app_window_session(true);
            log("run: guest blueprint returned");
        }
        Err(err) => {
            crate::hv::finish_blueprint_app_window_session(true);
            log(alloc::format!("run: guest REL load failed: {}", err).as_str());
        }
    }

    false
}
