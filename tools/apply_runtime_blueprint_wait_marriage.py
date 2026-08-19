from pathlib import Path


def rep(path, old, new):
    p = Path(path)
    s = p.read_text()
    n = s.count(old)
    if n != 1:
        raise SystemExit(f"{path}: anchor count {n}, expected 1")
    p.write_text(s.replace(old, new, 1))


def ins(path, anchor, text):
    rep(path, anchor, anchor + text)


def before(path, anchor, text):
    rep(path, anchor, text + anchor)


rep("src/wait.rs", '''#[inline]
pub async fn platform_wait_after_for_vm_async(
    vm_id: u8,
    key: u64,
    observed: u32,
    timeout_ms: u64,
) -> bool {
    platform_wait_queue(platform_wait_vm_scope(vm_id), key)
        .wait_after_timeout(observed, timeout_ms)
        .await
}

#[inline]
pub fn platform_wake_one_for_vm(vm_id: u8, key: u64) -> bool {
    platform_wait_queue(platform_wait_vm_scope(vm_id), key).notify_one()
}

#[inline]
pub fn platform_wake_all_for_vm(vm_id: u8, key: u64) -> usize {
    platform_wait_queue(platform_wait_vm_scope(vm_id), key).notify_all()
}
''', '''#[inline]
pub async fn platform_wait_after_for_vm_async(
    vm_id: u8,
    key: u64,
    observed: u32,
    timeout_ms: u64,
) -> bool {
    let queue = platform_wait_queue(platform_wait_vm_scope(vm_id), key);
    if timeout_ms == u64::MAX {
        queue.wait_after(observed).await;
        true
    } else {
        queue.wait_after_timeout(observed, timeout_ms).await
    }
}

#[inline]
pub fn platform_wake_one_for_vm(vm_id: u8, key: u64) -> bool {
    platform_wait_queue(platform_wait_vm_scope(vm_id), key).notify_one()
}

#[inline]
pub fn platform_wake_all_for_vm(vm_id: u8, key: u64) -> usize {
    platform_wait_queue(platform_wait_vm_scope(vm_id), key).notify_all()
}

/// Advance every keyed wait generation owned by one Blueprint VM.
///
/// Lifecycle control uses this when the VM may be outside VMX in an executor
/// wait. These are deliberately spurious notifications: each platform waiter
/// owns its protected state and must recheck it after returning.
pub fn platform_wake_vm_scope(vm_id: u8) -> usize {
    let scope = platform_wait_vm_scope(vm_id);
    let queues = {
        let queues = PLATFORM_WAIT_QUEUES.lock();
        queues
            .iter()
            .filter_map(|(&(queue_scope, _), queue)| (queue_scope == scope).then_some(*queue))
            .collect::<Vec<_>>()
    };
    let count = queues.len();
    for queue in queues {
        queue.notify_all();
    }
    count
}
''')

rep("src/hv/vmcall.rs", '''pub const OP_BP_TERMINAL_SURFACE_SNAPSHOT_V1: u32 = 0x137; // active terminal surface generation + geometry record/error
pub const OP_BP_LOG_RECORD_V1: u32 = 0x138; // arg0 level,arg1 target bytes,payload target || message -> host LogOs
''', '''pub const OP_BP_TERMINAL_SURFACE_SNAPSHOT_V1: u32 = 0x137; // active terminal surface generation + geometry record/error
pub const OP_BP_LOG_RECORD_V1: u32 = 0x138; // arg0 level,arg1 target bytes,payload target || message -> host LogOs
pub const OP_BP_PLATFORM_WAIT_OBSERVE_V1: u32 = 0x139; // arg0 VM-local key -> generation
pub const OP_BP_PLATFORM_WAIT_AFTER_V1: u32 = 0x13A; // arg0 key,arg1 observed,payload timeout_ms -> notified
pub const OP_BP_PLATFORM_WAKE_ONE_V1: u32 = 0x13B; // arg0 VM-local key -> woke bool
pub const OP_BP_PLATFORM_WAKE_ALL_V1: u32 = 0x13C; // arg0 VM-local key -> wake count
''')
rep("src/hv/vmcall.rs", '''pub const OP_BP_SERVICE_LANE_SUBMIT: u32 = 0x62; // arg0/arg1 boxed service-lane job raw parts
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub const OP_BP_TOKIO_BLOCKING_SPAWN: u32 = OP_BP_SERVICE_LANE_SUBMIT; // compatibility alias
pub const OP_BP_PLATFORM_WAKE_ONE: u32 = 0x63; // arg0 VM-local wait key -> woke bool
pub const OP_BP_PLATFORM_WAKE_ALL: u32 = 0x64; // arg0 VM-local wait key -> wake count
pub const OP_BP_INPUT_CURSOR_POS: u32 = 0x68; // arg0 cursor id -> packed x/y
''', '''pub const OP_BP_SERVICE_LANE_SUBMIT: u32 = 0x62; // arg0/arg1 boxed service-lane job raw parts
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub const OP_BP_TOKIO_BLOCKING_SPAWN: u32 = OP_BP_SERVICE_LANE_SUBMIT; // compatibility alias
pub const OP_BP_INPUT_CURSOR_POS: u32 = 0x68; // arg0 cursor id -> packed x/y
''')
rep("src/hv/vmcall.rs", '''    SleepMs(u64),
    /// Keep the current VMCALL pending, sleep in the host, then dispatch the
''', '''    SleepMs(u64),
    /// Keep a synchronous guest wait pending while its VM lane awaits on the host executor.
    PlatformWait {
        seq: u32,
        key: u64,
        observed: u32,
        timeout_ms: u64,
    },
    /// Keep the current VMCALL pending, sleep in the host, then dispatch the
''')

ins("src/hv/vmcall.rs", '''fn write_response(vm_id: u8, seq: u32, status: u32, data: u64, len: u32) {
    let Some(p) = host_ptr(vm_id) else {
        return;
    };
    unsafe {
        core::ptr::write_volatile(&mut (*p).response_status, status);
        core::ptr::write_volatile(&mut (*p).response_data, data);
        core::ptr::write_volatile(&mut (*p).response_len, len);
        // seq written last — guest may poll this as a completion flag
        core::ptr::write_volatile(&mut (*p).response_seq, seq);
    }
}
''', '''

pub(crate) fn complete_platform_wait(vm_id: u8, seq: u32, notified: bool) -> bool {
    let Some((op, current_seq, _, _, _)) = read_request(vm_id) else {
        return false;
    };
    if op != OP_BP_PLATFORM_WAIT_AFTER_V1 || current_seq != seq {
        return false;
    }
    write_response(vm_id, seq, STATUS_OK, u64::from(notified), 0);
    true
}
''')

rep("src/hv/vmcall.rs", '''pub fn guest_call(op: u32, arg0: u64, arg1: u64) -> (u32, u64) {
    let p = comm_page_guest_va() as *mut CommPage;
''', '''fn guest_call_with_payload(op: u32, arg0: u64, arg1: u64, payload: &[u8]) -> (u32, u64) {
    if payload.len() > PAYLOAD_CAP {
        return (STATUS_BAD_ARG, 0);
    }
    let p = comm_page_guest_va() as *mut CommPage;
''')
rep("src/hv/vmcall.rs", '''    unsafe {
        core::ptr::write_volatile(&mut (*p).request_arg0, arg0);
        core::ptr::write_volatile(&mut (*p).request_arg1, arg1);
        core::ptr::write_volatile(&mut (*p).request_len, 0);
''', '''    unsafe {
        if !payload.is_empty() {
            core::ptr::copy_nonoverlapping(payload.as_ptr(), (*p).payload.as_mut_ptr(), payload.len());
        }
        core::ptr::write_volatile(&mut (*p).request_arg0, arg0);
        core::ptr::write_volatile(&mut (*p).request_arg1, arg1);
        core::ptr::write_volatile(&mut (*p).request_len, payload.len() as u32);
''')
rep("src/hv/vmcall.rs", '''        (status, data)
    }
}

pub fn guest_yield() {
''', '''        (status, data)
    }
}

pub fn guest_call(op: u32, arg0: u64, arg1: u64) -> (u32, u64) {
    guest_call_with_payload(op, arg0, arg1, &[])
}

pub fn guest_platform_wait_observe(key: u64) -> u32 {
    let (status, value) = guest_call(OP_BP_PLATFORM_WAIT_OBSERVE_V1, key, 0);
    if status == STATUS_OK { value as u32 } else { 0 }
}

pub fn guest_platform_wait_after(key: u64, observed: u32, timeout_ms: u64) -> bool {
    let timeout = timeout_ms.to_le_bytes();
    let (status, value) = guest_call_with_payload(
        OP_BP_PLATFORM_WAIT_AFTER_V1,
        key,
        u64::from(observed),
        &timeout,
    );
    status == STATUS_OK && value != 0
}

pub fn guest_yield() {
''')

rep("src/hv/vmcall.rs", '''        OP_BP_PLATFORM_WAKE_ONE => {
            let woke = crate::wait::platform_wake_one_for_vm(vm_id, arg0);
            write_response(vm_id, seq, STATUS_OK, u64::from(woke), 0);
            DispatchOutcome::Resume
        }
        OP_BP_PLATFORM_WAKE_ALL => {
            let count = crate::wait::platform_wake_all_for_vm(vm_id, arg0);
            write_response(vm_id, seq, STATUS_OK, count as u64, 0);
            DispatchOutcome::Resume
        }
''', '''        OP_BP_PLATFORM_WAIT_OBSERVE_V1 => {
            if arg1 != 0 || req_len != 0 {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }
            let observed = crate::wait::platform_wait_observe_for_vm(vm_id, arg0);
            write_response(vm_id, seq, STATUS_OK, u64::from(observed), 0);
            DispatchOutcome::Resume
        }
        OP_BP_PLATFORM_WAIT_AFTER_V1 => {
            if arg1 > u64::from(u32::MAX) {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            }
            let Some(payload) = request_payload(vm_id, req_len)
                .filter(|payload| payload.len() == core::mem::size_of::<u64>())
            else {
                write_response(vm_id, seq, STATUS_BAD_ARG, 0, 0);
                return DispatchOutcome::Resume;
            };
            let timeout_ms = u64::from_le_bytes([
                payload[0], payload[1], payload[2], payload[3],
                payload[4], payload[5], payload[6], payload[7],
            ]);
            let observed = arg1 as u32;
            let current = crate::wait::platform_wait_observe_for_vm(vm_id, arg0);
            if current != observed || timeout_ms == 0 {
                write_response(vm_id, seq, STATUS_OK, u64::from(current != observed), 0);
                DispatchOutcome::Resume
            } else {
                DispatchOutcome::PlatformWait { seq, key: arg0, observed, timeout_ms }
            }
        }
        OP_BP_PLATFORM_WAKE_ONE_V1 => {
            let woke = crate::wait::platform_wake_one_for_vm(vm_id, arg0);
            write_response(vm_id, seq, STATUS_OK, u64::from(woke), 0);
            DispatchOutcome::Resume
        }
        OP_BP_PLATFORM_WAKE_ALL_V1 => {
            let count = crate::wait::platform_wake_all_for_vm(vm_id, arg0);
            write_response(vm_id, seq, STATUS_OK, count as u64, 0);
            DispatchOutcome::Resume
        }
''')

rep("src/r/platform.rs", '''    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        // Hull waits are cooperative sleeps below, so they do not consume a
        // host wait-queue sequence. Hull wake operations still VM-call into
        // the host so they can wake service-lane workers.
        return 0;
    }
''', '''    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return crate::hv::vmcall::guest_platform_wait_observe(key);
    }
''')
rep("src/r/platform.rs", '''    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        if timeout_ms == 0 {
            crate::hv::vmcall::guest_yield();
        } else {
            crate::hv::vmcall::guest_sleep_ms(timeout_ms);
        }
        return false;
    }
''', '''    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return crate::hv::vmcall::guest_platform_wait_after(key, observed, timeout_ms);
    }
''')
rep("src/r/platform.rs", "OP_BP_PLATFORM_WAKE_ONE, key, 0", "OP_BP_PLATFORM_WAKE_ONE_V1, key, 0")
rep("src/r/platform.rs", "OP_BP_PLATFORM_WAKE_ALL, key, 0", "OP_BP_PLATFORM_WAKE_ALL_V1, key, 0")

rep("src/hv/mod.rs", '''fn nudge_vm_control(
    vm_id: u8,
    action: crate::hv::control_kick::LifecycleKickAction,
    reason: &'static str,
) -> bool {
    let Some(vm) = vm_slot(vm_id) else {
        return false;
    };
''', '''fn nudge_vm_control(
    vm_id: u8,
    action: crate::hv::control_kick::LifecycleKickAction,
    reason: &'static str,
) -> bool {
    let Some(vm) = vm_slot(vm_id) else {
        return false;
    };
    // A Blueprint may be outside VMX while the surrounding lane awaits a
    // synchronous platform call. Advance its wait generations before trying
    // the resident-guest interrupt path so lifecycle control always makes the
    // executor task runnable.
    let _ = crate::wait::platform_wake_vm_scope(vm_id);
''')
before("src/hv/mod.rs", '''                        crate::hv::vmcall::DispatchOutcome::RetryAfterMs(ms) => {
''', '''                        crate::hv::vmcall::DispatchOutcome::PlatformWait {
                            seq,
                            key,
                            observed,
                            timeout_ms,
                        } => {
                            clear_current_vm_id();
                            let notified = crate::wait::platform_wait_after_for_vm_async(
                                vm_id, key, observed, timeout_ms,
                            )
                            .await;
                            set_current_vm_id(vm_id);
                            crate::smp::poll();
                            if vm
                                .map(|vm| vm.stop_req.load(Ordering::Acquire))
                                .unwrap_or(false)
                            {
                                hvlogf(format_args!(
                                    "hv: vm{} reporting: host stop request consumed during platform wait",
                                    vm_id
                                ));
                                break 'vmexit;
                            }
                            if !crate::hv::vmcall::complete_platform_wait(vm_id, seq, notified) {
                                return Err("platform wait completion identity lost");
                            }
                            break 'vmcall;
                        }
''')

rep("crates/trueos-vm/src/vmcall.rs", '''pub const OP_BP_TERMINAL_SURFACE_SNAPSHOT_V1: u32 = 0x137;
pub const OP_BP_LOG_RECORD_V1: u32 = 0x138;
''', '''pub const OP_BP_TERMINAL_SURFACE_SNAPSHOT_V1: u32 = 0x137;
pub const OP_BP_LOG_RECORD_V1: u32 = 0x138;
pub const OP_BP_PLATFORM_WAIT_OBSERVE_V1: u32 = 0x139;
pub const OP_BP_PLATFORM_WAIT_AFTER_V1: u32 = 0x13A;
pub const OP_BP_PLATFORM_WAKE_ONE_V1: u32 = 0x13B;
pub const OP_BP_PLATFORM_WAKE_ALL_V1: u32 = 0x13C;
''')

host_vmcall = Path("src/hv/vmcall.rs").read_text()
platform = Path("src/r/platform.rs").read_text()
if "pub const OP_BP_PLATFORM_WAKE_ONE: u32 = 0x63" in host_vmcall:
    raise SystemExit("legacy colliding platform wake-one op survived")
if "pub const OP_BP_PLATFORM_WAKE_ALL: u32 = 0x64" in host_vmcall:
    raise SystemExit("legacy colliding platform wake-all op survived")
if "guest_sleep_ms(timeout_ms)" in platform:
    raise SystemExit("Hull platform-wait sleep fallback survived")
if "Hull waits are cooperative sleeps below" in platform:
    raise SystemExit("Hull platform-wait observe fallback survived")

for path in ("src/wait.rs", "src/hv/vmcall.rs", "src/r/platform.rs", "src/hv/mod.rs", "crates/trueos-vm/src/vmcall.rs"):
    if not Path(path).read_text().endswith("\n"):
        raise SystemExit(f"{path}: newline lost")
print("runtime Blueprint wait marriage applied")
