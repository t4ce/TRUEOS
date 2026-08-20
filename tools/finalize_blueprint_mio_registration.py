from pathlib import Path


def read(path):
    return Path(path).read_text()


def write(path, source):
    Path(path).write_text(source)


def rep(path, old, new):
    source = read(path)
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"{path}: anchor count {count}, expected 1: {old[:100]!r}")
    write(path, source.replace(old, new, 1))


def replace_between(path, start, end, replacement=""):
    source = read(path)
    a = source.find(start)
    if a < 0:
        raise SystemExit(f"{path}: missing start {start[:80]!r}")
    b = source.find(end, a + len(start))
    if b < 0:
        raise SystemExit(f"{path}: missing end {end[:80]!r}")
    write(path, source[:a] + replacement + source[b:])


# Timed waits must not retain dead task wakers.
rep(
    "src/wait.rs",
    '''            match timeout.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(false),
                Poll::Pending => Poll::Pending,
            }
''',
    '''            match timeout.as_mut().poll(cx) {
                Poll::Ready(()) => {
                    let mut wakers = self.wakers.lock();
                    if let Some(index) = wakers
                        .iter()
                        .position(|registered| registered.will_wake(cx.waker()))
                    {
                        wakers.swap_remove(index);
                    }
                    Poll::Ready(false)
                }
                Poll::Pending => Poll::Pending,
            }
''',
)

# Crossterm makes fd0 nonblocking before registering it with Mio.
rep(
    "src/unix_abi_shim.rs",
    '''            let nonblocking = c_int::from_ne_bytes([value[0], value[1], value[2], value[3]]) != 0;
            match crate::std_abi_shim::socket_set_nonblocking(fd, nonblocking) {
''',
    '''            let nonblocking = c_int::from_ne_bytes([value[0], value[1], value[2], value[3]]) != 0;
            if (0..=2).contains(&fd) {
                let current = unsafe { crate::std_abi_shim::fcntl(fd, 3, 0) };
                if current < 0 {
                    return -1;
                }
                let flags = if nonblocking {
                    current | 0o4000
                } else {
                    current & !0o4000
                };
                return unsafe { crate::std_abi_shim::fcntl(fd, 4, flags) };
            }
            match crate::std_abi_shim::socket_set_nonblocking(fd, nonblocking) {
''',
)

# Remove the duplicated kernel Mio registry. Socket objects and readiness
# probes stay; fd/token/interest ownership belongs to the Blueprint's Mio.
rep("src/mio_compat.rs", "use crate::wait::WaitQueue;\n", "")
rep(
    "src/mio_compat.rs",
    '''const CONNECT_COMPAT_WAIT_NS: u64 = 2_000_000_000;
const CONNECT_IO_FASTPATH_NS: u64 = 1_000_000_000;
const SELECTOR_PARK_SLICE_NS: u64 = 10_000_000;

static MIO_SELECTOR_WAIT: WaitQueue = WaitQueue::new();
''',
    '''const CONNECT_COMPAT_WAIT_NS: u64 = 2_000_000_000;
const CONNECT_IO_FASTPATH_NS: u64 = 1_000_000_000;
''',
)
rep(
    "src/mio_compat.rs",
    '''#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TrueosMioReadyEvent {
    pub token: usize,
    pub readiness: u8,
}

''',
    "",
)
rep(
    "src/mio_compat.rs",
    '''pub(crate) fn notify_net_event() {
    MIO_SELECTOR_WAIT.notify_all();
    let _ = crate::wait::platform_wake_all_blueprint_io_waiters();
}

pub(crate) unsafe fn mio_selector_wake_host(_selector_id: usize) -> i32 {
    MIO_SELECTOR_WAIT.notify_all();
    if let Some(vm_id) = current_owner_vm() {
        let _ = crate::wait::platform_wake_blueprint_io_for_vm(vm_id);
    } else {
        let _ = crate::wait::platform_wake_all_blueprint_io_waiters();
    }
    STATUS_OK
}
''',
    '''pub(crate) fn notify_net_event() {
    let _ = crate::wait::platform_wake_all_blueprint_io_waiters();
}
''',
)
rep(
    "src/mio_compat.rs",
    '''struct SelectorRegistration {
    selector_id: usize,
    socket_id: u32,
    owner_vm: Option<u8>,
    token: usize,
    interests: u8,
}

''',
    "",
)
rep("src/mio_compat.rs", "    registrations: Vec<SelectorRegistration>,\n", "")
rep("src/mio_compat.rs", "            registrations: Vec::new(),\n", "")
rep(
    "src/mio_compat.rs",
    "const MIO_READY_EVENT_BYTES: usize = core::mem::size_of::<TrueosMioReadyEvent>();\n",
    "",
)
rep(
    "src/mio_compat.rs",
    '''fn socket_owner_vm(socket_id: u32) -> Option<u8> {
    with_compat(|compat| compat.socket(socket_id).and_then(|socket| socket.owner_vm))
}

''',
    "",
)
source = read("src/mio_compat.rs").replace(
    "MIO_SELECTOR_WAIT.notify_all();",
    "let _ = crate::wait::platform_wake_all_blueprint_io_waiters();",
)
write("src/mio_compat.rs", source)
rep(
    "src/mio_compat.rs",
    '''        compat.drop_pending_open(socket_id);
        compat
            .registrations
            .retain(|reg| !(owner_matches(reg.owner_vm, owner_vm) && reg.socket_id == socket_id));
        for handle in handles {
''',
    '''        compat.drop_pending_open(socket_id);
        for handle in handles {
''',
)
rep(
    "src/mio_compat.rs",
    '''        compat
            .registrations
            .retain(|reg| reg.owner_vm != Some(vm_id) && !socket_ids.contains(&reg.socket_id));
        for handle in handles {
''',
    '''        for handle in handles {
''',
)
source = read("src/mio_compat.rs")
a = source.find("    fn selector_poll_ready_once(")
marker = "\n    }\n}\n\npub(crate) fn mio_socket_poll_ready_host"
b = source.find(marker, a)
if a < 0 or b < 0:
    raise SystemExit("src/mio_compat.rs: selector scan boundaries missing")
source = source[:a] + "}\n\npub(crate) fn mio_socket_poll_ready_host" + source[b + len(marker):]
write("src/mio_compat.rs", source)
source = read("src/mio_compat.rs")
a = source.find("pub(crate) unsafe fn mio_selector_register_socket_host(")
if a < 0:
    raise SystemExit("src/mio_compat.rs: selector API tail missing")
write("src/mio_compat.rs", source[:a].rstrip() + "\n")

# Delete the matching VMCall protocol and dynamic imports.
for path in ("src/hv/vmcall.rs", "crates/trueos-vm/src/vmcall.rs"):
    source = read(path)
    for line in (
        'pub const OP_BP_MIO_SELECTOR_REGISTER_SOCKET: u32 = 0x5D; // selector/socket/token/interests\n',
        'pub const OP_BP_MIO_SELECTOR_DEREGISTER_SOCKET: u32 = 0x5E; // selector/socket\n',
        'pub const OP_BP_MIO_SELECTOR_POLL: u32 = 0x5F; // selector/cap/timeout -> ready events\n',
        'pub const OP_BP_MIO_SELECTOR_WAKE: u32 = 0x80; // selector -> wake parked pollers\n',
        'pub const OP_BP_MIO_SELECTOR_REGISTER_SOCKET: u32 = 0x5D;\n',
        'pub const OP_BP_MIO_SELECTOR_DEREGISTER_SOCKET: u32 = 0x5E;\n',
        'pub const OP_BP_MIO_SELECTOR_POLL: u32 = 0x5F;\n',
        'pub const OP_BP_MIO_SELECTOR_WAKE: u32 = 0x80;\n',
    ):
        source = source.replace(line, "")
    write(path, source)
rep(
    "src/hv/vmcall.rs",
    "const MIO_READY_EVENT_BYTES: usize = core::mem::size_of::<crate::mio_compat::TrueosMioReadyEvent>();\n",
    "",
)
replace_between(
    "src/hv/vmcall.rs",
    "        OP_BP_MIO_SELECTOR_REGISTER_SOCKET => {",
    "        _ => {",
)
source = read("src/hv/blueprint/blueprint.rs")
a = source.find('        "trueos_mio_selector_register_socket" => {')
b = source.find("        _ => None,", a)
if a < 0 or b < 0:
    raise SystemExit("src/hv/blueprint/blueprint.rs: selector resolver boundaries missing")
write("src/hv/blueprint/blueprint.rs", source[:a] + source[b:])

# A reconnect advances typed surface identity and must wake a parked TUI.
old = '''pub(crate) fn net_shell_begin_connection(handle: NetHandle) -> (bool, NetShellOwnershipSnapshot) {
    let mut st = NET_SHELL_STATE.lock();
    // `TcpData` is allowed to select `handle` before `TcpEstablished` is
    // drained, so transport identity needs its own established-handle field.
    // Clearing it on close also makes reuse of the same adapter handle a new
    // terminal surface incarnation.
    let is_new_connection = st.established_handle != Some(handle);
    st.handle = Some(handle);
    if is_new_connection {
        st.established_handle = Some(handle);
        st.rx.clear();
        st.tx.clear();
        // A command has not yet been admitted to this connection, so a reset
        // retained from an earlier surface is stale.  The TCP task observes
        // the direct owner below and queues one freshly tagged reset for this
        // connection after the state transition.
        st.direct_control_tx.clear();
        advance_surface_generation(&mut st);
    }
    (
        is_new_connection,
        NetShellOwnershipSnapshot {
            owner: NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire),
            epoch: st.handoff_epoch,
        },
    )
}
'''
new = '''pub(crate) fn net_shell_begin_connection(handle: NetHandle) -> (bool, NetShellOwnershipSnapshot) {
    let (is_new_connection, snapshot) = {
        let mut st = NET_SHELL_STATE.lock();
        let is_new_connection = st.established_handle != Some(handle);
        st.handle = Some(handle);
        if is_new_connection {
            st.established_handle = Some(handle);
            st.rx.clear();
            st.tx.clear();
            st.direct_control_tx.clear();
            advance_surface_generation(&mut st);
        }
        (
            is_new_connection,
            NetShellOwnershipSnapshot {
                owner: NET_SHELL_DIRECT_OWNER.load(Ordering::Acquire),
                epoch: st.handoff_epoch,
            },
        )
    };
    if is_new_connection
        && let Some(vm_id) = snapshot.blueprint_vm()
    {
        let _ = crate::wait::platform_wake_blueprint_io_for_vm(vm_id);
    }
    (is_new_connection, snapshot)
}
'''
rep("src/shell2/backends/net_tcp.rs", old, new)
rep(
    "src/shell2/backends/net_tcp_shell.rs",
    '''                    NetEvent::Closed { handle } => {
                        forget_tcp_handle(&mut pending_tcp_writes, handle);
''',
    '''                    NetEvent::Closed { handle } => {
                        forget_tcp_handle(&mut pending_tcp_writes, handle);
                        let _ = crate::wait::platform_wake_all_blueprint_io_waiters();
''',
)

for path in (
    "src/mio_compat.rs",
    "src/hv/vmcall.rs",
    "crates/trueos-vm/src/vmcall.rs",
    "src/hv/blueprint/blueprint.rs",
):
    source = read(path)
    for forbidden in (
        "SelectorRegistration",
        "MIO_SELECTOR_WAIT",
        "SELECTOR_PARK_SLICE_NS",
        "TrueosMioReadyEvent",
        "mio_selector_register_socket",
        "mio_selector_deregister_socket",
        "mio_selector_poll",
        "mio_selector_wake",
        "OP_BP_MIO_SELECTOR_",
        "trueos_mio_selector_",
    ):
        if forbidden in source:
            raise SystemExit(f"{path}: legacy selector residue {forbidden}")

print("Blueprint Mio selector ownership finalized")
