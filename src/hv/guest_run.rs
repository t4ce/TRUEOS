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
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use core::mem::ManuallyDrop;

use trueos_executor::raw::Executor as RawExecutor;
use trueos_vm::vmcall;

use crate::shell2::{ShellBackend2, ShellIo2};

fn attached_write(bytes: &[u8]) {
    let mut written = 0usize;
    while written < bytes.len() {
        let end = core::cmp::min(written + trueos_vm::vmcall::PAYLOAD_CAP, bytes.len());
        let (status, count) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_SHELL_ATTACHED_WRITE,
            0,
            0,
            &bytes[written..end],
            &mut [],
        );
        if status != trueos_vm::vmcall::STATUS_OK || count == 0 {
            break;
        }
        written = written.saturating_add(count as usize);
    }
}

fn attached_write_str(text: &str) {
    attached_write(text.as_bytes());
}

fn attached_write_line(text: &str) {
    attached_write(text.as_bytes());
    attached_write(b"\r\n");
}

fn attached_read_byte() -> Option<u8> {
    let (status, data) =
        trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_SHELL_ATTACHED_READ_BYTE, 0, 0);
    if status == trueos_vm::vmcall::STATUS_OK && data != u64::MAX {
        Some(data as u8)
    } else {
        None
    }
}

fn guest_text_vmcall(op: u32, request: &[u8]) -> Option<String> {
    let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
    let (status, len) = trueos_vm::vmcall::call_with_payload(op, 0, 0, request, &mut bytes);
    if status != trueos_vm::vmcall::STATUS_OK {
        return None;
    }
    let got = core::cmp::min(len as usize, bytes.len());
    core::str::from_utf8(&bytes[..got]).ok().map(String::from)
}

fn container_shell_prompt() {
    attached_write_str("vmx> ");
}

fn container_shell_help() {
    attached_write_line("commands: env smp help stop pause snapshot preserve");
    attached_write_line("  stop     stop without writing a checkpoint");
    attached_write_line("  pause    preserve-pause; resume by vmid from F2 pause");
    attached_write_line("  snapshot Blueprint Ready checkpoint; warm and resumable");
    attached_write_line("  preserve preserve-stop; checkpoint first, then tear down");
}

fn container_shell_read_line(line: &mut Vec<u8>) {
    line.clear();
    loop {
        if let Some(byte) = attached_read_byte() {
            match byte {
                b'\r' | b'\n' => {
                    attached_write(b"\r\n");
                    return;
                }
                0x03 => {
                    line.clear();
                    attached_write(b"^C\r\n");
                    container_shell_prompt();
                }
                0x08 | 0x7f => {
                    if line.pop().is_some() {
                        attached_write(b"\x08 \x08");
                    }
                }
                byte if byte.is_ascii_graphic() || byte == b' ' => {
                    if line.len() < 512 {
                        line.push(byte);
                        attached_write(&[byte]);
                    }
                }
                _ => {}
            }
        } else {
            trueos_vm::vmcall::sleep_ms(10);
        }
    }
}

fn container_shell_command(raw: &str) -> bool {
    let trimmed = raw.trim();
    let cmd = trimmed.split_whitespace().next().unwrap_or("");
    match cmd {
        "" => {}
        "env" => match guest_text_vmcall(trueos_vm::vmcall::OP_BP_ENV_ALL, &[]) {
            Some(text) if !text.is_empty() => attached_write_str(text.as_str()),
            _ => attached_write_line("env: unavailable"),
        },
        "smp" => {
            let vm_id = crate::hv::current_vm_id().unwrap_or(0);
            let (status, vtid) =
                trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_THREAD_CURRENT_ID, 0, 0);
            let mut line = String::new();
            if status == trueos_vm::vmcall::STATUS_OK {
                let _ = write!(line, "smp: vm={} vthread={} async_jobs=not-wired", vm_id, vtid);
            } else {
                let _ = write!(line, "smp: vm={} vthread=unavailable async_jobs=not-wired", vm_id);
            }
            attached_write_line(line.as_str());
        }
        "help" | "?" => container_shell_help(),
        "stop" => {
            attached_write_line("vmx-shell: requesting stop without checkpoint");
            let (status, _) = trueos_vm::vmcall::call_with_payload(
                trueos_vm::vmcall::OP_BP_SHUTDOWN,
                0,
                0,
                b"vmx mini-shell stop",
                &mut [],
            );
            attached_write_line(
                alloc::format!("vmx-shell: stop returned unexpectedly status={}", status).as_str(),
            );
        }
        "pause" => {
            attached_write_line(
                "vmx-shell: requesting Blueprint PreparePause; checkpoint waits for Blueprint Ready",
            );
            let (status, _) = trueos_vm::vmcall::call(trueos_vm::vmcall::OP_LIFECYCLE_PAUSE, 0, 0);
            if status != trueos_vm::vmcall::STATUS_OK {
                attached_write_line(
                    "vmx-shell: replicatable pause unavailable; use preserve for a raw checkpoint",
                );
            } else {
                attached_write_line("vmx-shell: PreparePause requested");
            }
        }
        "snapshot" | "snap" => {
            attached_write_line("vmx-shell: requesting Blueprint PreparePause for warm snapshot");
            let (status, _) =
                trueos_vm::vmcall::call(trueos_vm::vmcall::OP_LIFECYCLE_SNAPSHOT, 0, 0);
            if status != trueos_vm::vmcall::STATUS_OK {
                attached_write_line(
                    "vmx-shell: replicatable snapshot unavailable; use preserve for a raw checkpoint",
                );
            } else {
                attached_write_line("vmx-shell: snapshot PreparePause requested");
            }
        }
        "preserve" => {
            attached_write_line("vmx-shell: requesting raw checkpoint-and-stop");
            trueos_vm::vmcall::preserve();
            attached_write_line("vmx-shell: resumed from raw checkpoint");
        }
        _ => attached_write_line("unknown vmx command"),
    }
    true
}

fn create_blueprint_dir_all_async(path: &str) -> Result<(), alloc::string::String> {
    // Bootstrap runs before the Blueprint environment stack exists. Calling
    // the generic async filesystem CABI here would resolve `path` through that
    // environment first, which can make Hull guest code touch the host-owned
    // environment map. The launch state already contains the resolved app
    // root, so submit it directly through the asynchronous VM-call protocol.
    if path.is_empty() || path.len() > trueos_vm::vmcall::PAYLOAD_CAP {
        return Err(alloc::format!("BadPath(len={})", path.len()));
    }
    let (status, operation) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_ASYNC_FS_CREATE_DIR_ALL_START,
        0,
        0,
        path.as_bytes(),
        &mut [],
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return Err(alloc::format!("AsyncStartStatus({})", status));
    }
    let operation = (operation as i64) as i32;
    if operation <= 0 {
        return Err(alloc::format!("AsyncStartRc({})", operation));
    }
    let operation = operation as u32;

    loop {
        let (status, value) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_ASYNC_FS_STATUS, operation as u64, 0);
        if status != trueos_vm::vmcall::STATUS_OK {
            return Err(alloc::format!("AsyncStatusTransport({})", status));
        }
        match (value as i64) as i32 {
            0 => {
                let _ = trueos_vm::vmcall::call(trueos_vm::vmcall::OP_YIELD, 0, 0);
            }
            1 => {
                let (status, discard) = trueos_vm::vmcall::call(
                    trueos_vm::vmcall::OP_BP_ASYNC_FS_DISCARD,
                    operation as u64,
                    0,
                );
                if status != trueos_vm::vmcall::STATUS_OK {
                    return Err(alloc::format!("AsyncDiscardStatus({})", status));
                }
                let discard = (discard as i64) as i32;
                return if discard == 0 {
                    Ok(())
                } else {
                    Err(alloc::format!("AsyncDiscardRc({})", discard))
                };
            }
            rc => {
                let _ = trueos_vm::vmcall::call(
                    trueos_vm::vmcall::OP_BP_ASYNC_FS_DISCARD,
                    operation as u64,
                    0,
                );
                return Err(alloc::format!("AsyncStatusRc({})", rc));
            }
        }
    }
}

// ── VmcallShellBackend ────────────────────────────────────────────────────────

pub(crate) struct VmcallShellBackend;

pub(crate) static VMCALL_SHELL: VmcallShellBackend = VmcallShellBackend;

impl ShellIo2 for VmcallShellBackend {
    fn raw_write_str(&self, s: &str) {
        vmcall::net_tcp_write(s.as_bytes());
    }

    fn raw_write_fmt(&self, args: core::fmt::Arguments<'_>) {
        struct W;
        impl core::fmt::Write for W {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                vmcall::net_tcp_write(s.as_bytes());
                Ok(())
            }
        }
        let _ = core::fmt::Write::write_fmt(&mut W, args);
    }

    fn raw_write_char(&self, ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        vmcall::net_tcp_write(s.as_bytes());
    }

    fn raw_write_byte(&self, b: u8) {
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

    let spawner = executor.spawner();

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
pub extern "C" fn trueos_hv_guest_container_shell_run() -> ! {
    attached_write_line("vmx-shell: ready");
    container_shell_help();
    let mut line = Vec::new();
    loop {
        container_shell_prompt();
        container_shell_read_line(&mut line);
        let Ok(text) = core::str::from_utf8(line.as_slice()) else {
            attached_write_line("input: invalid utf8");
            continue;
        };
        let _ = container_shell_command(text);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_hv_guest_blueprint_launch_active() -> bool {
    let vm_id = crate::hv::current_vm_id().unwrap_or(0);
    crate::hv::blueprint_launch_active(vm_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_hv_guest_blueprint_run() -> bool {
    let vm_id = crate::hv::current_vm_id().unwrap_or(0);
    let Some(state) = crate::hv::take_blueprint_launch(vm_id) else {
        return false;
    };
    // The Hull's private RW image contains a shallow copy of the host-owned
    // launch state. Taking that copy is required to consume the guest-visible
    // launch slot, but dropping it would free the same guest-heap buffers that
    // host teardown still owns. Keep this guest view borrowed-by-convention;
    // the host removes and drops the authoritative state after VM exit.
    let state = ManuallyDrop::new(state);

    let log = |line: &str| crate::hv::hvlogf(format_args!("{}", line));
    let warn = |line: &str| crate::hv::hvwarnf(format_args!("{}", line));

    crate::hv::hvlogf(format_args!(
        "run: guest blueprint launch archive={}",
        state.archive.as_str()
    ));

    let module = match crate::hv::blueprint::parse_blueprint(state.module_bytes.as_slice()) {
        Ok(module) => module,
        Err(err) => {
            warn(alloc::format!("run: guest blueprint parse failed: {}", err).as_str());
            return false;
        }
    };

    let unpacked = state.unpacked_bytes.as_slice();

    if !unpacked.starts_with(b"\x7fELF")
        || !matches!(crate::hv::blueprint::elf_type_name(unpacked), Some("REL"))
    {
        warn("run: guest blueprint rejected non-REL payload");
        return false;
    }

    if crate::hv::current_hull_guest_context_vm_id().is_none() {
        match crate::hv::blueprint::elf_imports(unpacked) {
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
    }

    // `img kernel:*` consumes a kernel-owned virtual image source through the
    // image-source ABI.  It neither names nor needs TRUEOSFS, so keep this
    // launch mode usable during the early warm-boot window instead of paying
    // the generic app-root/common directory rendezvous.  Ordinary `img` file
    // paths retain the normal filesystem bootstrap below.
    let virtual_image_launch = state.archive.eq_ignore_ascii_case("img.bp")
        && state
            .app_args
            .first()
            .is_some_and(|source| source.starts_with("kernel:"));
    // The process ABI still carries a logical app-root string even for a
    // filesystem-free invocation. Constructing that string performs no I/O.
    let Some(app_fs_root) =
        crate::allocators::with_hv_guest_alloc_domain(vm_id, || state.app_fs_root.clone())
    else {
        log("run: guest app fs root path failed: guest heap domain unavailable");
        return false;
    };
    if module.is_filesystem_independent() || virtual_image_launch {
        log(if virtual_image_launch {
            "run: guest app fs bootstrap skipped contract=kernel-virtual-image"
        } else {
            "run: guest app fs bootstrap skipped contract=filesystem-independent"
        });
    } else {
        crate::hv::hvlogf(format_args!("run: guest app fs path alloc begin vm={}", vm_id));
        let Some(app_fs_common) = crate::allocators::with_hv_guest_alloc_domain(vm_id, || {
            crate::hv::blueprint::app_fs_common_root()
        }) else {
            log("run: guest app fs paths failed: guest heap domain unavailable");
            return false;
        };
        crate::hv::hvlogf(format_args!(
            "run: guest app fs path alloc ok root={} common={}",
            app_fs_root.as_str(),
            app_fs_common.as_str()
        ));

        match create_blueprint_dir_all_async(app_fs_root.as_str()) {
            Ok(()) => {
                log(alloc::format!("run: guest app fs root ready path={}", app_fs_root.as_str())
                    .as_str())
            }
            Err(err) => log(alloc::format!(
                "run: guest app fs root create failed path={} err={:?}",
                app_fs_root.as_str(),
                err
            )
            .as_str()),
        }

        match create_blueprint_dir_all_async(app_fs_common.as_str()) {
            Ok(()) => log(alloc::format!(
                "run: guest app fs common ready path={}",
                app_fs_common.as_str()
            )
            .as_str()),
            Err(err) => log(alloc::format!(
                "run: guest app fs common create failed path={} err={:?}",
                app_fs_common.as_str(),
                err
            )
            .as_str()),
        }
        if crate::hv::current_hull_guest_context_vm_id().is_none() {
            log(alloc::format!(
                "run: guest app fs root prepared path={} common={}",
                app_fs_root.as_str(),
                app_fs_common.as_str()
            )
            .as_str());
        }
    }

    crate::blueprint_net_broker::set_vmx_guest_net_backend(true);
    crate::hv::hvlogf(format_args!("run: guest invoke alloc begin vm={}", vm_id));
    let invoke_result = crate::allocators::with_hv_guest_alloc_domain(vm_id, || {
        let process_args = crate::hv::blueprint::build_process_args(
            state.archive.as_str(),
            state.app_args.as_slice(),
        );
        let process_env = crate::hv::blueprint::build_process_env(
            state.archive.as_str(),
            Some(app_fs_root.as_str()),
            Some(&state.identity),
            state.launch_script.as_deref(),
        );
        crate::hv::blueprint::invoke_host_rel(
            unpacked,
            module.entry,
            module.flags,
            process_args,
            process_env,
            None,
            Some(app_fs_root),
        )
    })
    .unwrap_or_else(|| Err(alloc::format!("guest heap domain unavailable vm={}", vm_id)));
    crate::blueprint_net_broker::set_vmx_guest_net_backend(false);

    // Rich terminal ownership is scoped to the application invocation. Once
    // it returns, release the borrowed view back to the outer shell2 before
    // the hull stops.
    let (handoff_status, _) = trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_RETURN_TO_CLI, 0, 0);
    if handoff_status != trueos_vm::vmcall::STATUS_OK {
        warn("run: guest blueprint terminal->shell2 handoff failed");
    }

    match invoke_result {
        Ok(()) => {
            log("run: guest blueprint returned");
        }
        Err(err) => {
            log(alloc::format!("run: guest REL invoke failed: {}", err).as_str());
        }
    }

    false
}
