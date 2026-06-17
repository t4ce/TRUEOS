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

use embassy_executor::raw::Executor as RawExecutor;
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

fn guest_env_var(key: &str) -> Option<String> {
    guest_text_vmcall(trueos_vm::vmcall::OP_BP_ENV_VAR, key.as_bytes())
}

fn container_shell_prompt() {
    attached_write_str("vmx> ");
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
    let mut words = trimmed.splitn(2, char::is_whitespace);
    let cmd = words.next().unwrap_or("");
    let rest = words.next().unwrap_or("").trim_start();
    match cmd {
        "" => {}
        "echo" => attached_write_line(rest),
        "hostname" => {
            let hostname = guest_env_var("HOSTNAME")
                .or_else(|| guest_env_var("TRUEOS_HOSTNAME"))
                .unwrap_or_else(|| String::from("TRUEOS"));
            attached_write_line(hostname.as_str());
        }
        "homedir" => {
            let home = guest_env_var("HOME").unwrap_or_else(|| String::from("/"));
            attached_write_line(home.as_str());
        }
        "env" => match guest_text_vmcall(trueos_vm::vmcall::OP_BP_ENV_ALL, &[]) {
            Some(text) if !text.is_empty() => attached_write_str(text.as_str()),
            _ => attached_write_line("env: unavailable"),
        },
        "file" => {
            let path = if rest.is_empty() {
                guest_env_var("HOME").unwrap_or_else(|| String::from("/"))
            } else {
                String::from(rest)
            };
            match guest_text_vmcall(trueos_vm::vmcall::OP_BP_FS_LIST_TREE, path.as_bytes()) {
                Some(text) if !text.is_empty() => attached_write_str(text.as_str()),
                _ => attached_write_line("file: unavailable"),
            }
        }
        "thread" => {
            let vm_id = crate::hv::current_vm_id().unwrap_or(0);
            let (status, vtid) =
                trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_THREAD_CURRENT_ID, 0, 0);
            let mut line = String::new();
            if status == trueos_vm::vmcall::STATUS_OK {
                let _ = write!(line, "thread: vm={} vthread={} async_jobs=not-wired", vm_id, vtid);
            } else {
                let _ =
                    write!(line, "thread: vm={} vthread=unavailable async_jobs=not-wired", vm_id);
            }
            attached_write_line(line.as_str());
        }
        "help" => attached_write_line("commands: echo hostname homedir env disc thread help exit"),
        "exit" => return false,
        _ => attached_write_line("unknown command; try `help`"),
    }
    true
}

fn create_blueprint_dir_all(path: &str) -> Result<(), alloc::string::String> {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        if path.len() > trueos_vm::vmcall::PAYLOAD_CAP {
            return Err(alloc::format!("TooLarge(len={})", path.len()));
        }

        let mut out = [0u8; 1];
        let (status, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_FS_CREATE_DIR_ALL,
            0,
            0,
            path.as_bytes(),
            &mut out,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return Err(alloc::format!("VmcallStatus({})", status));
        }

        let rc = (rc as i64) as i32;
        if rc == 0 {
            Ok(())
        } else {
            Err(alloc::format!("CabiRc({})", rc))
        }
    } else {
        crate::r::io::kfs::create_dir_all(path).map_err(|err| alloc::format!("{:?}", err))
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
    attached_write_line("commands: echo hostname homedir env disc thread help exit");
    let mut line = Vec::new();
    loop {
        container_shell_prompt();
        container_shell_read_line(&mut line);
        let Ok(text) = core::str::from_utf8(line.as_slice()) else {
            attached_write_line("input: invalid utf8");
            continue;
        };
        if !container_shell_command(text) {
            attached_write_line("vmx-shell: preserving hull");
            trueos_vm::vmcall::preserve();
            loop {
                core::hint::spin_loop();
            }
        }
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

    let log = |line: &str| crate::hv::hvlogf(format_args!("{}", line));

    crate::hv::hvlogf(format_args!(
        "run: guest blueprint launch archive={}",
        state.archive.as_str()
    ));

    let module = match crate::hv::blueprint::parse_blueprint(state.module_bytes.as_slice()) {
        Ok(module) => module,
        Err(err) => {
            log(alloc::format!("run: guest blueprint parse failed: {}", err).as_str());
            return false;
        }
    };

    let unpacked = state.unpacked_bytes.as_slice();

    if !unpacked.starts_with(b"\x7fELF")
        || !matches!(crate::hv::blueprint::elf_type_name(unpacked), Some("REL"))
    {
        log("run: guest blueprint rejected non-REL payload");
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

    crate::hv::hvlogf(format_args!("run: guest app fs path alloc begin vm={}", vm_id));
    let Some((app_fs_root, app_fs_common)) =
        crate::allocators::with_hv_guest_alloc_domain(vm_id, || {
            (
                crate::hv::blueprint::app_fs_root_for_archive(
                    state.archive.as_str(),
                    state.module_bytes.as_slice(),
                ),
                crate::hv::blueprint::app_fs_common_for_archive(state.archive.as_str()),
            )
        })
    else {
        log("run: guest app fs paths failed: guest heap domain unavailable");
        return false;
    };
    crate::hv::hvlogf(format_args!(
        "run: guest app fs path alloc ok root={} common={}",
        app_fs_root.as_str(),
        app_fs_common.as_str()
    ));

    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        if let Err(err) = create_blueprint_dir_all(app_fs_root.as_str()) {
            log(alloc::format!(
                "run: guest app fs root create failed path={} err={}",
                app_fs_root.as_str(),
                err
            )
            .as_str());
        }
    } else {
        match create_blueprint_dir_all(app_fs_root.as_str()) {
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
    }

    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        if let Err(err) = create_blueprint_dir_all(app_fs_common.as_str()) {
            log(alloc::format!(
                "run: guest app fs common create failed path={} err={}",
                app_fs_common.as_str(),
                err
            )
            .as_str());
        }
    } else {
        match create_blueprint_dir_all(app_fs_common.as_str()) {
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
    }
    if crate::hv::current_hull_guest_context_vm_id().is_none() {
        log(alloc::format!(
            "run: guest app fs root prepared path={} common={}",
            app_fs_root.as_str(),
            app_fs_common.as_str()
        )
        .as_str());
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
        );
        crate::hv::blueprint::invoke_host_rel(
            unpacked,
            module.entry,
            process_args,
            process_env,
            None,
            Some(app_fs_root),
        )
    })
    .unwrap_or_else(|| Err(alloc::format!("guest heap domain unavailable vm={}", vm_id)));
    crate::blueprint_net_broker::set_vmx_guest_net_backend(false);

    match invoke_result {
        Ok(()) => {
            log("run: guest blueprint returned");
        }
        Err(err) => {
            log(alloc::format!("run: guest REL load failed: {}", err).as_str());
        }
    }

    false
}
