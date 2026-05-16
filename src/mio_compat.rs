use alloc::{collections::VecDeque, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use spin::Mutex;
use v::vnet as api;

use crate::blueprint_net_broker::VNetBridge;

const STATUS_OK: i32 = 0;
const STATUS_UNSUPPORTED: i32 = -1;
const STATUS_WOULD_BLOCK: i32 = -2;
const STATUS_NOT_CONNECTED: i32 = -3;
const STATUS_INVALID_INPUT: i32 = -4;
const STATUS_NOT_FOUND: i32 = -5;
const STATUS_IO: i32 = -6;
const STATUS_NO_DEVICE: i32 = -8;

const READY_READABLE: u8 = 0b0000_0001;
const READY_WRITABLE: u8 = 0b0000_0010;
const READY_ERROR: u8 = 0b0000_0100;
const READY_READ_CLOSED: u8 = 0b0000_1000;
const READY_WRITE_CLOSED: u8 = 0b0001_0000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TrueosMioSocketAddr {
    pub family: u8,
    pub port: u16,
    pub addr: [u8; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TrueosMioReadyEvent {
    pub token: usize,
    pub readiness: u8,
}

#[derive(Clone, Copy)]
enum CompatAddr {
    V4 { addr: [u8; 4], port: u16 },
    V6 { addr: [u8; 16], port: u16 },
}

impl CompatAddr {
    fn from_raw(raw: TrueosMioSocketAddr) -> Option<Self> {
        match raw.family {
            4 => Some(Self::V4 {
                addr: [raw.addr[0], raw.addr[1], raw.addr[2], raw.addr[3]],
                port: raw.port,
            }),
            6 => Some(Self::V6 {
                addr: raw.addr,
                port: raw.port,
            }),
            _ => None,
        }
    }

    fn to_raw(self) -> TrueosMioSocketAddr {
        match self {
            Self::V4 { addr, port } => {
                let mut raw = TrueosMioSocketAddr {
                    family: 4,
                    port,
                    addr: [0; 16],
                };
                raw.addr[..4].copy_from_slice(&addr);
                raw
            }
            Self::V6 { addr, port } => TrueosMioSocketAddr {
                family: 6,
                port,
                addr,
            },
        }
    }

    fn from_vnet(peer: Option<api::EndpointV4>, peer6: Option<api::EndpointV6>) -> Option<Self> {
        if let Some(peer) = peer {
            return Some(Self::V4 {
                addr: peer.addr,
                port: peer.port,
            });
        }
        peer6.map(|peer| Self::V6 {
            addr: peer.addr,
            port: peer.port,
        })
    }

    fn unspecified_same_family(self) -> Self {
        match self {
            Self::V4 { .. } => Self::V4 {
                addr: [0; 4],
                port: 0,
            },
            Self::V6 { .. } => Self::V6 {
                addr: [0; 16],
                port: 0,
            },
        }
    }
}

fn log_tcp_endpoint(prefix: &str, socket_id: u32, handle_id: u32, peer: CompatAddr) {
    match peer {
        CompatAddr::V4 { addr, port } => crate::log!(
            "{} socket={} handle={} peer={}.{}.{}.{}:{}\n",
            prefix,
            socket_id,
            handle_id,
            addr[0],
            addr[1],
            addr[2],
            addr[3],
            port
        ),
        CompatAddr::V6 { addr, port } => crate::log!(
            "{} socket={} handle={} peer={:02x}{:02x}:{:02x}{:02x}:...:{}\n",
            prefix,
            socket_id,
            handle_id,
            addr[0],
            addr[1],
            addr[2],
            addr[3],
            port
        ),
    }
}

fn compat_addr_port(addr: Option<CompatAddr>) -> Option<u16> {
    match addr {
        Some(CompatAddr::V4 { port, .. }) | Some(CompatAddr::V6 { port, .. }) => Some(port),
        None => None,
    }
}

fn probe_tcp_socket(socket: &MioSocketState) -> bool {
    socket.kind == MioSocketKind::TcpStream && matches!(compat_addr_port(socket.local), Some(4 | 5))
}

fn should_log_selector_probe(readiness: u8) -> bool {
    let interesting =
        readiness & (READY_READABLE | READY_ERROR | READY_READ_CLOSED | READY_WRITE_CLOSED);
    if interesting != 0 {
        return true;
    }

    static PURE_WRITABLE_PROBE_COUNT: core::sync::atomic::AtomicU32 =
        core::sync::atomic::AtomicU32::new(0);
    static PURE_WRITABLE_PROBE_LAST_LOG_NS: AtomicU64 = AtomicU64::new(0);
    let count = PURE_WRITABLE_PROBE_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1;
    count <= 4 || once_per_second(&PURE_WRITABLE_PROBE_LAST_LOG_NS)
}

fn once_per_second(last: &AtomicU64) -> bool {
    const ONE_SECOND_NS: u64 = 1_000_000_000;
    let now = crate::chronos::monotonic_nanos();
    let prev = last.load(AtomicOrdering::Relaxed);
    if prev != 0 && now.saturating_sub(prev) < ONE_SECOND_NS {
        return false;
    }
    last.compare_exchange(prev, now, AtomicOrdering::Relaxed, AtomicOrdering::Relaxed)
        .is_ok()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MioSocketKind {
    TcpStream,
    TcpListener,
    Udp,
}

struct MioSocketState {
    id: u32,
    owner_vm: Option<u8>,
    kind: MioSocketKind,
    handle: Option<api::NetHandle>,
    local: Option<CompatAddr>,
    peer: Option<CompatAddr>,
    listen_port: Option<u16>,
    connected: bool,
    closed: bool,
    error: i32,
    rx_stream: VecDeque<u8>,
    rx_dgrams: VecDeque<(CompatAddr, Vec<u8>)>,
    accept_queue: VecDeque<u32>,
}

struct PendingOpen {
    socket_id: u32,
    kind: api::SocketKind,
}

struct SelectorRegistration {
    selector_id: usize,
    socket_id: u32,
    owner_vm: Option<u8>,
    token: usize,
    interests: u8,
}

struct MioCompat {
    net: Option<VNetBridge>,
    sockets: Vec<MioSocketState>,
    pending_opens: VecDeque<PendingOpen>,
    registrations: Vec<SelectorRegistration>,
    next_socket_id: u32,
    udp_next_ephemeral: u16,
    tcp_listen_next_ephemeral: u16,
}

static MIO_COMPAT: Mutex<Option<MioCompat>> = Mutex::new(None);

fn with_compat<R>(f: impl FnOnce(&mut MioCompat) -> R) -> R {
    let mut guard = MIO_COMPAT.lock();
    let compat = guard.get_or_insert_with(MioCompat::new);
    f(compat)
}

const MIO_ADDR_BYTES: usize = core::mem::size_of::<TrueosMioSocketAddr>();
const MIO_READY_EVENT_BYTES: usize = core::mem::size_of::<TrueosMioReadyEvent>();

#[inline]
fn vm_guest_vmcall_active() -> bool {
    // Only the actual VM hull stack may execute the vmcall instruction.
    // Host-carried guest workers keep VM identity but use host broker state.
    crate::hv::current_hull_guest_context_vm_id().is_some()
}

#[inline]
fn current_owner_vm() -> Option<u8> {
    crate::hv::current_guest_execution_context_vm_id()
}

#[inline]
fn vmcall_i32(data: u64) -> i32 {
    (data as i64) as i32
}

#[inline]
fn vmcall_isize(data: u64) -> isize {
    (data as i64) as isize
}

#[inline]
fn addr_bytes(addr: &TrueosMioSocketAddr) -> &[u8] {
    unsafe { core::slice::from_raw_parts(addr as *const _ as *const u8, MIO_ADDR_BYTES) }
}

fn read_addr(bytes: &[u8]) -> Option<TrueosMioSocketAddr> {
    if bytes.len() < MIO_ADDR_BYTES {
        return None;
    }
    Some(unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const TrueosMioSocketAddr) })
}

fn guest_stack_host_ptr(vm_id: u8, guest_ptr: *const u8, len: usize) -> Option<*mut u8> {
    let guest_va = guest_ptr as usize as u64;
    let offset = guest_va.checked_sub(crate::hv::memory::GUEST_STACK_VA_BASE)? as usize;
    let stack = crate::hv::memory::guest_stack_slice_for_vm(vm_id)?;
    let end = offset.checked_add(len)?;
    if end > stack.len() {
        return None;
    }
    let base = crate::hv::memory::guest_stack_mut_ptr_for_vm(vm_id)?;
    Some(unsafe { base.add(offset) })
}

fn active_guest_host_ptr(vm_id: u8, guest_ptr: *const u8, len: usize) -> Option<*mut u8> {
    if len == 0 {
        return Some(guest_ptr.cast_mut());
    }
    if guest_ptr.is_null() {
        return None;
    }
    if let Some(host_ptr) = guest_stack_host_ptr(vm_id, guest_ptr, len) {
        return Some(host_ptr);
    }

    let guest_va = guest_ptr as usize;
    let heap = crate::allocators::hv_guest_heap_stats(vm_id);
    if heap.initialized
        && guest_va >= heap.heap_start
        && guest_va
            .checked_add(len)
            .is_some_and(|end| end <= heap.heap_end)
    {
        return Some(guest_ptr.cast_mut());
    }

    let high_half = (guest_va as u64) >= 0xffff_8000_0000_0000;
    if high_half {
        return Some(guest_ptr.cast_mut());
    }

    None
}

fn copy_from_active_guest(vm_id: u8, src: *const u8, len: usize) -> Option<Vec<u8>> {
    let host_ptr = active_guest_host_ptr(vm_id, src, len)?;
    let bytes = unsafe { core::slice::from_raw_parts(host_ptr.cast_const(), len) };
    Some(Vec::from(bytes))
}

fn copy_to_active_guest(vm_id: u8, dst: *mut u8, bytes: &[u8]) -> bool {
    let Some(dst_ptr) = active_guest_host_ptr(vm_id, dst.cast_const(), bytes.len()) else {
        return false;
    };
    if !bytes.is_empty() {
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst_ptr, bytes.len()) };
    }
    true
}

fn copy_u32_to_active_guest(vm_id: u8, dst: *mut u32, value: u32) -> bool {
    copy_to_active_guest(vm_id, dst.cast::<u8>(), &value.to_ne_bytes())
}

fn copy_mio_addr_to_active_guest(
    vm_id: u8,
    dst: *mut TrueosMioSocketAddr,
    value: TrueosMioSocketAddr,
) -> bool {
    let bytes = unsafe {
        core::slice::from_raw_parts(
            (&value as *const TrueosMioSocketAddr).cast::<u8>(),
            MIO_ADDR_BYTES,
        )
    };
    copy_to_active_guest(vm_id, dst.cast::<u8>(), bytes)
}

fn copy_from_guest_stack(vm_id: u8, src: *const u8, len: usize) -> Option<Vec<u8>> {
    if len == 0 {
        return Some(Vec::new());
    }
    let bytes = if let Some(host_ptr) = guest_stack_host_ptr(vm_id, src, len) {
        unsafe { core::slice::from_raw_parts(host_ptr.cast_const(), len) }
    } else {
        unsafe { core::slice::from_raw_parts(src, len) }
    };
    Some(Vec::from(bytes))
}

fn copy_to_guest_stack(vm_id: u8, dst: *mut u8, bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    let dst_ptr = guest_stack_host_ptr(vm_id, dst.cast_const(), bytes.len()).unwrap_or(dst);
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst_ptr, bytes.len()) };
    true
}

fn copy_u32_to_guest_stack(vm_id: u8, dst: *mut u32, value: u32) -> bool {
    copy_to_guest_stack(vm_id, dst.cast::<u8>(), &value.to_ne_bytes())
}

fn copy_mio_addr_to_guest_stack(
    vm_id: u8,
    dst: *mut TrueosMioSocketAddr,
    value: TrueosMioSocketAddr,
) -> bool {
    let bytes = unsafe {
        core::slice::from_raw_parts(
            (&value as *const TrueosMioSocketAddr).cast::<u8>(),
            MIO_ADDR_BYTES,
        )
    };
    copy_to_guest_stack(vm_id, dst.cast::<u8>(), bytes)
}

fn guest_mio_socket_out(op: u32, addr: TrueosMioSocketAddr, out_socket_id: *mut u32) -> i32 {
    if out_socket_id.is_null() {
        return STATUS_INVALID_INPUT;
    }
    let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() else {
        return STATUS_INVALID_INPUT;
    };
    let mut out = [0u8; 1];
    let (status, data) =
        trueos_vm::vmcall::call_with_payload(op, 0, 0, addr_bytes(&addr), &mut out);
    if status != trueos_vm::vmcall::STATUS_OK {
        return STATUS_INVALID_INPUT;
    }
    let rc = vmcall_i32(data);
    if rc > 0 {
        if copy_u32_to_active_guest(vm_id, out_socket_id, rc as u32) {
            STATUS_OK
        } else {
            STATUS_INVALID_INPUT
        }
    } else {
        rc
    }
}

fn guest_mio_addr_out(op: u32, socket_id: u32, out_addr: *mut TrueosMioSocketAddr) -> i32 {
    if out_addr.is_null() {
        return STATUS_INVALID_INPUT;
    }
    let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() else {
        return STATUS_INVALID_INPUT;
    };
    let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
    let (status, data) =
        trueos_vm::vmcall::call_with_payload(op, socket_id as u64, 0, &[], &mut bytes);
    if status != trueos_vm::vmcall::STATUS_OK {
        return STATUS_INVALID_INPUT;
    }
    let rc = vmcall_i32(data);
    if rc != STATUS_OK {
        return rc;
    }
    let Some(addr) = read_addr(&bytes[..MIO_ADDR_BYTES]) else {
        return STATUS_INVALID_INPUT;
    };
    if !copy_mio_addr_to_active_guest(vm_id, out_addr, addr) {
        return STATUS_INVALID_INPUT;
    }
    STATUS_OK
}

fn guest_mio_signed_payload(op: u32, arg0: u64, arg1: u64, req: &[u8]) -> isize {
    let mut out = [0u8; 1];
    let (status, data) = trueos_vm::vmcall::call_with_payload(op, arg0, arg1, req, &mut out);
    if status == trueos_vm::vmcall::STATUS_OK {
        vmcall_isize(data)
    } else {
        STATUS_INVALID_INPUT as isize
    }
}

impl MioCompat {
    fn new() -> Self {
        Self {
            net: None,
            sockets: Vec::new(),
            pending_opens: VecDeque::new(),
            registrations: Vec::new(),
            next_socket_id: 1,
            udp_next_ephemeral: 49_152,
            tcp_listen_next_ephemeral: 50_000,
        }
    }

    fn ensure_net(&mut self) -> Result<(), i32> {
        if self.net.is_none() {
            self.net = VNetBridge::open_primary();
        }
        if self.net.is_some() {
            Ok(())
        } else {
            Err(STATUS_NO_DEVICE)
        }
    }

    fn alloc_socket_id(&mut self) -> u32 {
        let id = self.next_socket_id;
        self.next_socket_id = self.next_socket_id.wrapping_add(1).max(1);
        id
    }

    fn alloc_udp_port(&mut self) -> u16 {
        let port = self.udp_next_ephemeral;
        self.udp_next_ephemeral = self.udp_next_ephemeral.wrapping_add(1).max(49_152);
        port
    }

    fn alloc_tcp_listen_port(&mut self) -> u16 {
        let port = self.tcp_listen_next_ephemeral;
        self.tcp_listen_next_ephemeral = self.tcp_listen_next_ephemeral.wrapping_add(1).max(50_000);
        port
    }

    fn socket(&self, socket_id: u32) -> Option<&MioSocketState> {
        self.sockets.iter().find(|socket| socket.id == socket_id)
    }

    fn socket_mut(&mut self, socket_id: u32) -> Option<&mut MioSocketState> {
        self.sockets
            .iter_mut()
            .find(|socket| socket.id == socket_id)
    }

    fn socket_for_owner(&self, socket_id: u32, owner_vm: Option<u8>) -> Option<&MioSocketState> {
        self.socket(socket_id)
            .filter(|socket| socket.owner_vm == owner_vm)
    }

    fn socket_mut_for_owner(
        &mut self,
        socket_id: u32,
        owner_vm: Option<u8>,
    ) -> Option<&mut MioSocketState> {
        self.socket_mut(socket_id)
            .filter(|socket| socket.owner_vm == owner_vm)
    }

    fn socket_by_handle_mut(&mut self, handle: api::NetHandle) -> Option<&mut MioSocketState> {
        self.sockets
            .iter_mut()
            .find(|socket| socket.handle == Some(handle))
    }

    fn socket_by_handle_id(&self, handle: api::NetHandle, kind: MioSocketKind) -> Option<u32> {
        self.sockets
            .iter()
            .find(|socket| socket.handle == Some(handle) && socket.kind == kind)
            .map(|socket| socket.id)
    }

    fn drop_pending_open(&mut self, socket_id: u32) {
        self.pending_opens
            .retain(|pending| pending.socket_id != socket_id);
    }

    fn submit(&mut self, cmd: api::Command) -> Result<(), i32> {
        self.ensure_net()?;
        let result = self
            .net
            .as_ref()
            .unwrap()
            .submit(cmd)
            .map_err(|_| STATUS_WOULD_BLOCK);

        if result.is_ok() {
            self.kick_net();
        }

        result
    }

    fn kick_net(&mut self) {
        for _ in 0..4 {
            if !crate::net::adapter::service_tick_primary_once() {
                break;
            }
        }
        self.pump();
    }

    fn pump(&mut self) {
        while let Some(event) = self.net.as_ref().and_then(|net| net.pop_event()) {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: api::Event) {
        match event {
            api::Event::Opened { handle, kind } => {
                if let Some(index) = self
                    .pending_opens
                    .iter()
                    .position(|pending| pending.kind == kind)
                    && let Some(pending) = self.pending_opens.remove(index)
                    && let Some(socket) = self.socket_mut(pending.socket_id)
                {
                    socket.handle = Some(handle);
                    match socket.kind {
                        MioSocketKind::Udp => {
                            socket.connected = true;
                            crate::log!(
                                "mio_compat: udp opened socket={} handle={}\n",
                                socket.id,
                                handle.0
                            );
                        }
                        MioSocketKind::TcpStream => {
                            if let Some(peer) = socket.peer {
                                log_tcp_endpoint(
                                    "mio_compat: tcp opened",
                                    socket.id,
                                    handle.0,
                                    peer,
                                );
                            }
                        }
                        MioSocketKind::TcpListener => {}
                    }
                }
            }
            api::Event::TcpEstablished {
                handle,
                peer,
                peer6,
            } => {
                if let Some(listener_id) =
                    self.socket_by_handle_id(handle, MioSocketKind::TcpListener)
                {
                    let child_id = self.alloc_socket_id();
                    let event_peer = CompatAddr::from_vnet(peer, peer6);
                    let (owner_vm, local, fallback_peer, port, inherited_rx) = {
                        let listener = self.socket_mut(listener_id).unwrap();
                        let mut inherited_rx = VecDeque::new();
                        core::mem::swap(&mut inherited_rx, &mut listener.rx_stream);
                        if !inherited_rx.is_empty() {
                            crate::log!(
                                "mio_compat: tcp established inherited listener bytes listener={} child={} handle={} bytes={}\n",
                                listener_id,
                                child_id,
                                handle.0,
                                inherited_rx.len()
                            );
                        }
                        (
                            listener.owner_vm,
                            listener.local,
                            listener.local.map(CompatAddr::unspecified_same_family),
                            listener.listen_port,
                            inherited_rx,
                        )
                    };
                    let peer = event_peer.or(fallback_peer);

                    self.sockets.push(MioSocketState {
                        id: child_id,
                        owner_vm,
                        kind: MioSocketKind::TcpStream,
                        handle: Some(handle),
                        local,
                        peer,
                        listen_port: None,
                        connected: true,
                        closed: false,
                        error: STATUS_OK,
                        rx_stream: inherited_rx,
                        rx_dgrams: VecDeque::new(),
                        accept_queue: VecDeque::new(),
                    });

                    if let Some(listener) = self.socket_mut(listener_id) {
                        listener.handle = None;
                        listener.accept_queue.push_back(child_id);
                    }

                    if let Some(port) = port {
                        self.pending_opens.push_back(PendingOpen {
                            socket_id: listener_id,
                            kind: api::SocketKind::Tcp,
                        });
                        if self.submit(api::Command::OpenTcpListen { port }).is_err() {
                            self.drop_pending_open(listener_id);
                        }
                    }
                } else if let Some(socket) = self.socket_by_handle_mut(handle) {
                    socket.connected = true;
                    if socket.kind == MioSocketKind::TcpStream
                        && let Some(peer) = socket.peer
                    {
                        log_tcp_endpoint("mio_compat: tcp established", socket.id, handle.0, peer);
                    }
                }
            }
            api::Event::TcpData { handle, data } => {
                if let Some(socket) = self.socket_by_handle_mut(handle) {
                    if socket.kind == MioSocketKind::TcpListener {
                        crate::log!(
                            "mio_compat: tcp data queued on listener socket={} handle={} bytes={} queued_before={}\n",
                            socket.id,
                            handle.0,
                            data.as_slice().len(),
                            socket.rx_stream.len()
                        );
                    } else if probe_tcp_socket(socket) {
                        crate::log!(
                            "mio_compat: tcp data socket={} handle={} bytes={} queued_before={}\n",
                            socket.id,
                            handle.0,
                            data.as_slice().len(),
                            socket.rx_stream.len()
                        );
                    }
                    socket.rx_stream.extend(data.as_slice().iter().copied());
                    if probe_tcp_socket(socket) {
                        crate::log!(
                            "mio_compat: tcp data queued socket={} handle={} queued_after={}\n",
                            socket.id,
                            handle.0,
                            socket.rx_stream.len()
                        );
                    }
                } else {
                    crate::log!(
                        "mio_compat: tcp data orphan handle={} bytes={}\n",
                        handle.0,
                        data.as_slice().len()
                    );
                }
            }
            api::Event::TcpSent { handle, .. } => {
                if let Some(socket) = self.socket_by_handle_mut(handle) {
                    if socket.kind == MioSocketKind::TcpStream {
                        socket.connected = true;
                    }
                }
            }
            api::Event::UdpPacket { handle, from, data } => {
                if let Some(socket) = self.socket_by_handle_mut(handle) {
                    socket.rx_dgrams.push_back((
                        CompatAddr::V4 {
                            addr: from.addr,
                            port: from.port,
                        },
                        Vec::from(data.as_slice()),
                    ));
                }
            }
            api::Event::UdpPacketV6 { handle, from, data } => {
                if let Some(socket) = self.socket_by_handle_mut(handle) {
                    socket.rx_dgrams.push_back((
                        CompatAddr::V6 {
                            addr: from.addr,
                            port: from.port,
                        },
                        Vec::from(data.as_slice()),
                    ));
                }
            }
            api::Event::Closed { handle } => {
                if let Some(socket) = self.socket_by_handle_mut(handle) {
                    socket.handle = None;
                    socket.closed = true;
                }
            }
            api::Event::Error { .. } => {
                if let Some(pending) = self.pending_opens.pop_front()
                    && let Some(socket) = self.socket_mut(pending.socket_id)
                {
                    socket.error = STATUS_IO;
                }
            }
            api::Event::IcmpReply { .. } => {}
            api::Event::IcmpReplyV6 { .. } => {}
        }
    }

    fn ready_mask(&self, socket: &MioSocketState, interests: u8) -> u8 {
        let want_read = (interests & READY_READABLE) != 0;
        let want_write = (interests & READY_WRITABLE) != 0;
        let mut ready = 0u8;

        if socket.error != STATUS_OK {
            ready |= READY_ERROR;
        }

        match socket.kind {
            MioSocketKind::TcpStream => {
                if want_read && (!socket.rx_stream.is_empty() || socket.closed) {
                    ready |= READY_READABLE;
                }
                if want_write && socket.connected {
                    ready |= READY_WRITABLE;
                }
                if socket.closed {
                    ready |= READY_READ_CLOSED | READY_WRITE_CLOSED;
                }
            }
            MioSocketKind::TcpListener => {
                if want_read && !socket.accept_queue.is_empty() {
                    ready |= READY_READABLE;
                }
            }
            MioSocketKind::Udp => {
                if want_read && !socket.rx_dgrams.is_empty() {
                    ready |= READY_READABLE;
                }
                if want_write && socket.handle.is_some() {
                    ready |= READY_WRITABLE;
                }
            }
        }

        ready
    }

    fn selector_poll(
        &mut self,
        owner_vm: Option<u8>,
        selector_id: usize,
        out_events: *mut TrueosMioReadyEvent,
        out_cap: usize,
        timeout_nanos: u64,
    ) -> usize {
        let block_forever = timeout_nanos == u64::MAX;
        let mut spins = 0u64;

        loop {
            self.kick_net();
            self.pump();

            let mut written = 0usize;
            for reg in self
                .registrations
                .iter()
                .filter(|reg| reg.owner_vm == owner_vm && reg.selector_id == selector_id)
            {
                if written >= out_cap {
                    break;
                }

                let Some(socket) = self.socket(reg.socket_id) else {
                    continue;
                };

                let readiness = self.ready_mask(socket, reg.interests);
                if readiness == 0 {
                    continue;
                }

                if probe_tcp_socket(socket) && should_log_selector_probe(readiness) {
                    crate::log!(
                        "mio_compat: tcp selector-ready selector={} socket={} token={} interests=0x{:02x} readiness=0x{:02x} rx={} closed={}\n",
                        selector_id,
                        socket.id,
                        reg.token,
                        reg.interests,
                        readiness,
                        socket.rx_stream.len(),
                        socket.closed as u8
                    );
                }
                if crate::logflag::NET_LOG_TCP_FLOW
                    && socket.kind == MioSocketKind::Udp
                    && (readiness & READY_WRITABLE) != 0
                {
                    static UDP_WRITABLE_FLOW_LAST_LOG_NS: AtomicU64 = AtomicU64::new(0);
                    if once_per_second(&UDP_WRITABLE_FLOW_LAST_LOG_NS) {
                        crate::log!(
                            "mio_compat: udp selector-ready selector={} socket={} token={} readiness=0x{:02x}\n",
                            selector_id,
                            socket.id,
                            reg.token,
                            readiness
                        );
                    }
                }
                if crate::logflag::NET_LOG_TCP_FLOW
                    && socket.kind == MioSocketKind::TcpStream
                    && (readiness & READY_WRITABLE) != 0
                {
                    static TCP_WRITABLE_FLOW_LAST_LOG_NS: AtomicU64 = AtomicU64::new(0);
                    if once_per_second(&TCP_WRITABLE_FLOW_LAST_LOG_NS) {
                        crate::log!(
                            "mio_compat: tcp selector-ready selector={} socket={} token={} readiness=0x{:02x}\n",
                            selector_id,
                            socket.id,
                            reg.token,
                            readiness
                        );
                    }
                }

                unsafe {
                    out_events.add(written).write(TrueosMioReadyEvent {
                        token: reg.token,
                        readiness,
                    });
                }
                written += 1;
            }

            if written != 0 || timeout_nanos == 0 {
                return written;
            }

            if !block_forever && spins >= timeout_nanos.saturating_div(1_000_000).saturating_add(1)
            {
                return 0;
            }

            spins = spins.saturating_add(1);
            crate::wait::spin_step();
        }
    }
}

pub(crate) unsafe fn mio_tcp_listener_bind_host(
    addr: TrueosMioSocketAddr,
    out_socket_id: *mut u32,
) -> i32 {
    if out_socket_id.is_null() {
        return STATUS_INVALID_INPUT;
    }
    let Some(local) = CompatAddr::from_raw(addr) else {
        return STATUS_INVALID_INPUT;
    };

    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        let local = match local {
            CompatAddr::V4 { addr, port } => CompatAddr::V4 {
                addr,
                port: if port == 0 {
                    compat.alloc_tcp_listen_port()
                } else {
                    port
                },
            },
            CompatAddr::V6 { addr, port } => CompatAddr::V6 {
                addr,
                port: if port == 0 {
                    compat.alloc_tcp_listen_port()
                } else {
                    port
                },
            },
        };
        let port = match local {
            CompatAddr::V4 { port, .. } | CompatAddr::V6 { port, .. } => port,
        };

        let socket_id = compat.alloc_socket_id();
        compat.sockets.push(MioSocketState {
            id: socket_id,
            owner_vm,
            kind: MioSocketKind::TcpListener,
            handle: None,
            local: Some(local),
            peer: None,
            listen_port: Some(port),
            connected: false,
            closed: false,
            error: STATUS_OK,
            rx_stream: VecDeque::new(),
            rx_dgrams: VecDeque::new(),
            accept_queue: VecDeque::new(),
        });

        compat.pending_opens.push_back(PendingOpen {
            socket_id,
            kind: api::SocketKind::Tcp,
        });

        let status = match compat.submit(api::Command::OpenTcpListen { port }) {
            Ok(()) => STATUS_OK,
            Err(status) => {
                compat.drop_pending_open(socket_id);
                status
            }
        };

        if status == STATUS_OK {
            unsafe { *out_socket_id = socket_id };
        }

        status
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_tcp_listener_bind(
    addr: TrueosMioSocketAddr,
    out_socket_id: *mut u32,
) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_mio_socket_out(
            trueos_vm::vmcall::OP_BP_MIO_TCP_LISTENER_BIND,
            addr,
            out_socket_id,
        );
    }
    if let Some(vm_id) = current_owner_vm() {
        let mut socket_id = 0u32;
        let rc = mio_tcp_listener_bind_host(addr, &mut socket_id);
        if rc == STATUS_OK && !copy_u32_to_guest_stack(vm_id, out_socket_id, socket_id) {
            return STATUS_INVALID_INPUT;
        }
        return rc;
    }
    mio_tcp_listener_bind_host(addr, out_socket_id)
}

pub(crate) unsafe fn mio_tcp_stream_connect_host(
    addr: TrueosMioSocketAddr,
    out_socket_id: *mut u32,
) -> i32 {
    if out_socket_id.is_null() {
        return STATUS_INVALID_INPUT;
    }
    let Some(peer) = CompatAddr::from_raw(addr) else {
        return STATUS_INVALID_INPUT;
    };

    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        let socket_id = compat.alloc_socket_id();
        compat.sockets.push(MioSocketState {
            id: socket_id,
            owner_vm,
            kind: MioSocketKind::TcpStream,
            handle: None,
            local: None,
            peer: Some(peer),
            listen_port: None,
            connected: false,
            closed: false,
            error: STATUS_OK,
            rx_stream: VecDeque::new(),
            rx_dgrams: VecDeque::new(),
            accept_queue: VecDeque::new(),
        });

        log_tcp_endpoint("mio_compat: tcp connect", socket_id, 0, peer);

        compat.pending_opens.push_back(PendingOpen {
            socket_id,
            kind: api::SocketKind::Tcp,
        });

        let status = match peer {
            CompatAddr::V4 { addr, port } => compat.submit(api::Command::OpenTcpConnect {
                remote: api::EndpointV4 { addr, port },
            }),
            CompatAddr::V6 { addr, port } => compat.submit(api::Command::OpenTcpConnectV6 {
                remote: api::EndpointV6 { addr, port },
            }),
        };

        let status = match status {
            Ok(()) => STATUS_OK,
            Err(status) => {
                compat.drop_pending_open(socket_id);
                status
            }
        };

        if status == STATUS_OK {
            unsafe { *out_socket_id = socket_id };
        }

        status
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_tcp_stream_connect(
    addr: TrueosMioSocketAddr,
    out_socket_id: *mut u32,
) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_mio_socket_out(
            trueos_vm::vmcall::OP_BP_MIO_TCP_STREAM_CONNECT,
            addr,
            out_socket_id,
        );
    }
    if let Some(vm_id) = current_owner_vm() {
        let mut socket_id = 0u32;
        let rc = mio_tcp_stream_connect_host(addr, &mut socket_id);
        if rc == STATUS_OK && !copy_u32_to_guest_stack(vm_id, out_socket_id, socket_id) {
            return STATUS_INVALID_INPUT;
        }
        return rc;
    }
    mio_tcp_stream_connect_host(addr, out_socket_id)
}

pub(crate) unsafe fn mio_udp_socket_bind_host(
    addr: TrueosMioSocketAddr,
    out_socket_id: *mut u32,
) -> i32 {
    if out_socket_id.is_null() {
        return STATUS_INVALID_INPUT;
    }
    let Some(local) = CompatAddr::from_raw(addr) else {
        return STATUS_INVALID_INPUT;
    };

    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        let local = match local {
            CompatAddr::V4 { addr, port } => CompatAddr::V4 {
                addr,
                port: if port == 0 {
                    compat.alloc_udp_port()
                } else {
                    port
                },
            },
            CompatAddr::V6 { addr, port } => CompatAddr::V6 {
                addr,
                port: if port == 0 {
                    compat.alloc_udp_port()
                } else {
                    port
                },
            },
        };

        let socket_id = compat.alloc_socket_id();
        compat.sockets.push(MioSocketState {
            id: socket_id,
            owner_vm,
            kind: MioSocketKind::Udp,
            handle: None,
            local: Some(local),
            peer: None,
            listen_port: None,
            connected: true,
            closed: false,
            error: STATUS_OK,
            rx_stream: VecDeque::new(),
            rx_dgrams: VecDeque::new(),
            accept_queue: VecDeque::new(),
        });

        let port = match local {
            CompatAddr::V4 { port, .. } | CompatAddr::V6 { port, .. } => port,
        };

        compat.pending_opens.push_back(PendingOpen {
            socket_id,
            kind: api::SocketKind::Udp,
        });

        let status = match compat.submit(api::Command::OpenUdp { port }) {
            Ok(()) => STATUS_OK,
            Err(status) => {
                compat.drop_pending_open(socket_id);
                status
            }
        };

        if status == STATUS_OK {
            unsafe { *out_socket_id = socket_id };
        }

        status
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_udp_socket_bind(
    addr: TrueosMioSocketAddr,
    out_socket_id: *mut u32,
) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_mio_socket_out(
            trueos_vm::vmcall::OP_BP_MIO_UDP_SOCKET_BIND,
            addr,
            out_socket_id,
        );
    }
    if let Some(vm_id) = current_owner_vm() {
        let mut socket_id = 0u32;
        let rc = mio_udp_socket_bind_host(addr, &mut socket_id);
        if rc == STATUS_OK && !copy_u32_to_guest_stack(vm_id, out_socket_id, socket_id) {
            return STATUS_INVALID_INPUT;
        }
        return rc;
    }
    mio_udp_socket_bind_host(addr, out_socket_id)
}

pub(crate) unsafe fn mio_socket_close_host(socket_id: u32) -> i32 {
    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        compat.pump();
        let Some(socket) = compat.socket_mut_for_owner(socket_id, owner_vm) else {
            return STATUS_NOT_FOUND;
        };
        let handle = socket.handle.take();
        socket.closed = true;
        if let Some(handle) = handle {
            let _ = compat.submit(api::Command::Close { handle });
        }
        STATUS_OK
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_socket_close(socket_id: u32) -> i32 {
    if vm_guest_vmcall_active() {
        let (status, data) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_MIO_SOCKET_CLOSE, socket_id as u64, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            vmcall_i32(data)
        } else {
            STATUS_INVALID_INPUT
        };
    }
    mio_socket_close_host(socket_id)
}

pub(crate) unsafe fn mio_socket_local_addr_host(
    socket_id: u32,
    out_addr: *mut TrueosMioSocketAddr,
) -> i32 {
    if out_addr.is_null() {
        return STATUS_INVALID_INPUT;
    }
    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        compat.pump();
        let Some(socket) = compat.socket_for_owner(socket_id, owner_vm) else {
            return STATUS_NOT_FOUND;
        };
        let Some(addr) = socket.local else {
            return STATUS_UNSUPPORTED;
        };
        unsafe { *out_addr = addr.to_raw() };
        STATUS_OK
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_socket_local_addr(
    socket_id: u32,
    out_addr: *mut TrueosMioSocketAddr,
) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_mio_addr_out(
            trueos_vm::vmcall::OP_BP_MIO_SOCKET_LOCAL_ADDR,
            socket_id,
            out_addr,
        );
    }
    if let Some(vm_id) = current_owner_vm() {
        let mut addr = TrueosMioSocketAddr::default();
        let rc = mio_socket_local_addr_host(socket_id, &mut addr);
        if rc == STATUS_OK && !copy_mio_addr_to_guest_stack(vm_id, out_addr, addr) {
            return STATUS_INVALID_INPUT;
        }
        return rc;
    }
    mio_socket_local_addr_host(socket_id, out_addr)
}

pub(crate) unsafe fn mio_socket_peer_addr_host(
    socket_id: u32,
    out_addr: *mut TrueosMioSocketAddr,
) -> i32 {
    if out_addr.is_null() {
        return STATUS_INVALID_INPUT;
    }
    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        compat.pump();
        let Some(socket) = compat.socket_for_owner(socket_id, owner_vm) else {
            return STATUS_NOT_FOUND;
        };
        let Some(addr) = socket.peer else {
            return STATUS_UNSUPPORTED;
        };
        unsafe { *out_addr = addr.to_raw() };
        STATUS_OK
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_socket_peer_addr(
    socket_id: u32,
    out_addr: *mut TrueosMioSocketAddr,
) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_mio_addr_out(
            trueos_vm::vmcall::OP_BP_MIO_SOCKET_PEER_ADDR,
            socket_id,
            out_addr,
        );
    }
    if let Some(vm_id) = current_owner_vm() {
        let mut addr = TrueosMioSocketAddr::default();
        let rc = mio_socket_peer_addr_host(socket_id, &mut addr);
        if rc == STATUS_OK && !copy_mio_addr_to_guest_stack(vm_id, out_addr, addr) {
            return STATUS_INVALID_INPUT;
        }
        return rc;
    }
    mio_socket_peer_addr_host(socket_id, out_addr)
}

pub(crate) unsafe fn mio_socket_take_error_host(socket_id: u32) -> i32 {
    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        compat.pump();
        let Some(socket) = compat.socket_mut_for_owner(socket_id, owner_vm) else {
            return STATUS_NOT_FOUND;
        };
        let status = socket.error;
        socket.error = STATUS_OK;
        status
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_socket_take_error(socket_id: u32) -> i32 {
    if vm_guest_vmcall_active() {
        let (status, data) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_MIO_SOCKET_TAKE_ERROR,
            socket_id as u64,
            0,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            vmcall_i32(data)
        } else {
            STATUS_INVALID_INPUT
        };
    }
    mio_socket_take_error_host(socket_id)
}

pub(crate) unsafe fn mio_tcp_stream_read_host(
    socket_id: u32,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_ptr.is_null() && out_cap != 0 {
        return STATUS_INVALID_INPUT as isize;
    }
    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        compat.pump();
        let Some(socket) = compat.socket_mut_for_owner(socket_id, owner_vm) else {
            return STATUS_NOT_FOUND as isize;
        };
        if socket.kind != MioSocketKind::TcpStream {
            return STATUS_INVALID_INPUT as isize;
        }
        if socket.rx_stream.is_empty() {
            if probe_tcp_socket(socket) {
                crate::log!(
                    "mio_compat: tcp read would-block socket={} cap={} closed={}\n",
                    socket.id,
                    out_cap,
                    socket.closed as u8
                );
            }
            return if socket.closed {
                0
            } else {
                STATUS_WOULD_BLOCK as isize
            };
        }

        let len = out_cap.min(socket.rx_stream.len());
        for index in 0..len {
            unsafe {
                out_ptr
                    .add(index)
                    .write(socket.rx_stream.pop_front().unwrap());
            }
        }
        if probe_tcp_socket(socket) {
            crate::log!(
                "mio_compat: tcp read socket={} bytes={} remaining={}\n",
                socket.id,
                len,
                socket.rx_stream.len()
            );
        }
        len as isize
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_tcp_stream_read(
    socket_id: u32,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if vm_guest_vmcall_active() {
        let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() else {
            return STATUS_INVALID_INPUT as isize;
        };
        if out_ptr.is_null() && out_cap != 0 {
            return STATUS_INVALID_INPUT as isize;
        }
        let want = out_cap.min(trueos_vm::vmcall::PAYLOAD_CAP);
        let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_MIO_TCP_STREAM_READ,
            socket_id as u64,
            want as u64,
            &[],
            &mut bytes,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return STATUS_INVALID_INPUT as isize;
        }
        let rc = vmcall_isize(data);
        if rc <= 0 {
            return rc;
        }
        let got = (rc as usize).min(want);
        if !copy_to_active_guest(vm_id, out_ptr, &bytes[..got]) {
            return STATUS_INVALID_INPUT as isize;
        }
        return got as isize;
    }
    if let Some(vm_id) = current_owner_vm() {
        if out_ptr.is_null() && out_cap != 0 {
            return STATUS_INVALID_INPUT as isize;
        }
        let want = out_cap.min(trueos_vm::vmcall::PAYLOAD_CAP);
        if want == 0 {
            return 0;
        }
        let mut bytes = Vec::new();
        bytes.resize(want, 0);
        let rc = mio_tcp_stream_read_host(socket_id, bytes.as_mut_ptr(), want);
        if rc <= 0 {
            return rc;
        }
        let got = (rc as usize).min(want);
        if !copy_to_guest_stack(vm_id, out_ptr, &bytes[..got]) {
            return STATUS_INVALID_INPUT as isize;
        }
        return got as isize;
    }
    mio_tcp_stream_read_host(socket_id, out_ptr, out_cap)
}

pub(crate) unsafe fn mio_tcp_stream_write_host(
    socket_id: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> isize {
    if data_ptr.is_null() && data_len != 0 {
        return STATUS_INVALID_INPUT as isize;
    }
    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        compat.pump();
        let Some(socket) = compat.socket_for_owner(socket_id, owner_vm) else {
            return STATUS_NOT_FOUND as isize;
        };
        if socket.kind != MioSocketKind::TcpStream {
            return STATUS_INVALID_INPUT as isize;
        }
        if !socket.connected {
            return if socket.error != STATUS_OK {
                socket.error as isize
            } else {
                STATUS_WOULD_BLOCK as isize
            };
        }
        let Some(handle) = socket.handle else {
            return STATUS_NOT_CONNECTED as isize;
        };

        let len = data_len.min(api::MAX_MSG);
        if probe_tcp_socket(socket) {
            crate::log!(
                "mio_compat: tcp write socket={} handle={} bytes={} requested={}\n",
                socket.id,
                handle.0,
                len,
                data_len
            );
        }
        let data = unsafe { core::slice::from_raw_parts(data_ptr, len) };
        match compat.submit(api::Command::SendTcp {
            handle,
            data: api::ByteBuf::from_slice_trunc(data),
        }) {
            Ok(()) => len as isize,
            Err(status) => status as isize,
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_tcp_stream_write(
    socket_id: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> isize {
    if vm_guest_vmcall_active() {
        let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() else {
            return STATUS_INVALID_INPUT as isize;
        };
        if data_ptr.is_null() && data_len != 0 {
            return STATUS_INVALID_INPUT as isize;
        }
        let Some(data) = copy_from_active_guest(vm_id, data_ptr, data_len) else {
            return STATUS_INVALID_INPUT as isize;
        };
        let mut sent = 0usize;
        while sent < data.len() {
            let end = (sent + trueos_vm::vmcall::PAYLOAD_CAP).min(data.len());
            let rc = guest_mio_signed_payload(
                trueos_vm::vmcall::OP_BP_MIO_TCP_STREAM_WRITE,
                socket_id as u64,
                0,
                &data[sent..end],
            );
            if rc < 0 {
                return if sent == 0 { rc } else { sent as isize };
            }
            if rc == 0 {
                return sent as isize;
            }
            sent += (rc as usize).min(end - sent);
        }
        return sent as isize;
    }
    if let Some(vm_id) = current_owner_vm() {
        if data_ptr.is_null() && data_len != 0 {
            return STATUS_INVALID_INPUT as isize;
        }
        let Some(data) = copy_from_guest_stack(vm_id, data_ptr, data_len) else {
            return STATUS_INVALID_INPUT as isize;
        };
        return mio_tcp_stream_write_host(socket_id, data.as_ptr(), data.len());
    }
    mio_tcp_stream_write_host(socket_id, data_ptr, data_len)
}

pub(crate) unsafe fn mio_udp_socket_connect_host(socket_id: u32, addr: TrueosMioSocketAddr) -> i32 {
    let Some(peer) = CompatAddr::from_raw(addr) else {
        return STATUS_INVALID_INPUT;
    };
    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        let Some(socket) = compat.socket_mut_for_owner(socket_id, owner_vm) else {
            return STATUS_NOT_FOUND;
        };
        if socket.kind != MioSocketKind::Udp {
            return STATUS_INVALID_INPUT;
        }
        socket.peer = Some(peer);
        STATUS_OK
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_udp_socket_connect(
    socket_id: u32,
    addr: TrueosMioSocketAddr,
) -> i32 {
    if vm_guest_vmcall_active() {
        let mut out = [0u8; 1];
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_MIO_UDP_SOCKET_CONNECT,
            socket_id as u64,
            0,
            addr_bytes(&addr),
            &mut out,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            vmcall_i32(data)
        } else {
            STATUS_INVALID_INPUT
        };
    }
    mio_udp_socket_connect_host(socket_id, addr)
}

pub(crate) unsafe fn mio_udp_socket_send_to_host(
    socket_id: u32,
    addr: TrueosMioSocketAddr,
    data_ptr: *const u8,
    data_len: usize,
) -> isize {
    if data_ptr.is_null() && data_len != 0 {
        return STATUS_INVALID_INPUT as isize;
    }
    let Some(peer) = CompatAddr::from_raw(addr) else {
        return STATUS_INVALID_INPUT as isize;
    };
    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        compat.pump();
        let Some(socket) = compat.socket_for_owner(socket_id, owner_vm) else {
            return STATUS_NOT_FOUND as isize;
        };
        if socket.kind != MioSocketKind::Udp {
            return STATUS_INVALID_INPUT as isize;
        }
        let Some(handle) = socket.handle else {
            return STATUS_NOT_CONNECTED as isize;
        };

        let len = data_len.min(api::MAX_MSG);
        let data = unsafe { core::slice::from_raw_parts(data_ptr, len) };
        let command = match peer {
            CompatAddr::V4 { addr, port } => api::Command::SendUdp {
                handle,
                remote: api::EndpointV4 { addr, port },
                data: api::ByteBuf::from_slice_trunc(data),
            },
            CompatAddr::V6 { addr, port } => api::Command::SendUdpV6 {
                handle,
                remote: api::EndpointV6 { addr, port },
                data: api::ByteBuf::from_slice_trunc(data),
            },
        };

        match compat.submit(command) {
            Ok(()) => len as isize,
            Err(status) => status as isize,
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_udp_socket_send_to(
    socket_id: u32,
    addr: TrueosMioSocketAddr,
    data_ptr: *const u8,
    data_len: usize,
) -> isize {
    if vm_guest_vmcall_active() {
        let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() else {
            return STATUS_INVALID_INPUT as isize;
        };
        if data_ptr.is_null() && data_len != 0 {
            return STATUS_INVALID_INPUT as isize;
        }
        let Some(data) = copy_from_active_guest(vm_id, data_ptr, data_len) else {
            return STATUS_INVALID_INPUT as isize;
        };
        let mut req = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        if MIO_ADDR_BYTES >= req.len() {
            return STATUS_INVALID_INPUT as isize;
        }
        req[..MIO_ADDR_BYTES].copy_from_slice(addr_bytes(&addr));
        let data_cap = req.len() - MIO_ADDR_BYTES;
        let n = data.len().min(data_cap);
        req[MIO_ADDR_BYTES..MIO_ADDR_BYTES + n].copy_from_slice(&data[..n]);
        return guest_mio_signed_payload(
            trueos_vm::vmcall::OP_BP_MIO_UDP_SOCKET_SEND_TO,
            socket_id as u64,
            n as u64,
            &req[..MIO_ADDR_BYTES + n],
        );
    }
    mio_udp_socket_send_to_host(socket_id, addr, data_ptr, data_len)
}

pub(crate) unsafe fn mio_udp_socket_recv_from_host(
    socket_id: u32,
    out_addr: *mut TrueosMioSocketAddr,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_addr.is_null() || (out_ptr.is_null() && out_cap != 0) {
        return STATUS_INVALID_INPUT as isize;
    }
    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        compat.pump();
        let Some(socket) = compat.socket_mut_for_owner(socket_id, owner_vm) else {
            return STATUS_NOT_FOUND as isize;
        };
        if socket.kind != MioSocketKind::Udp {
            return STATUS_INVALID_INPUT as isize;
        }
        let Some((from, data)) = socket.rx_dgrams.pop_front() else {
            return STATUS_WOULD_BLOCK as isize;
        };
        unsafe { *out_addr = from.to_raw() };
        let len = out_cap.min(data.len());
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), out_ptr, len);
        }
        len as isize
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_udp_socket_recv_from(
    socket_id: u32,
    out_addr: *mut TrueosMioSocketAddr,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if vm_guest_vmcall_active() {
        let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() else {
            return STATUS_INVALID_INPUT as isize;
        };
        if out_addr.is_null() || (out_ptr.is_null() && out_cap != 0) {
            return STATUS_INVALID_INPUT as isize;
        }
        let want = out_cap.min(trueos_vm::vmcall::PAYLOAD_CAP - MIO_ADDR_BYTES);
        let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_MIO_UDP_SOCKET_RECV_FROM,
            socket_id as u64,
            want as u64,
            &[],
            &mut bytes,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return STATUS_INVALID_INPUT as isize;
        }
        let rc = vmcall_isize(data);
        if rc <= 0 {
            return rc;
        }
        let Some(addr) = read_addr(&bytes[..MIO_ADDR_BYTES]) else {
            return STATUS_INVALID_INPUT as isize;
        };
        let got = (rc as usize).min(want);
        if !copy_mio_addr_to_active_guest(vm_id, out_addr, addr) {
            return STATUS_INVALID_INPUT as isize;
        }
        if !copy_to_active_guest(vm_id, out_ptr, &bytes[MIO_ADDR_BYTES..MIO_ADDR_BYTES + got]) {
            return STATUS_INVALID_INPUT as isize;
        }
        return got as isize;
    }
    mio_udp_socket_recv_from_host(socket_id, out_addr, out_ptr, out_cap)
}

pub(crate) unsafe fn mio_tcp_listener_accept_host(
    socket_id: u32,
    out_socket_id: *mut u32,
    out_addr: *mut TrueosMioSocketAddr,
) -> i32 {
    if out_socket_id.is_null() || out_addr.is_null() {
        return STATUS_INVALID_INPUT;
    }
    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        compat.pump();
        let Some(listener_index) = compat
            .sockets
            .iter()
            .position(|socket| socket.id == socket_id && socket.owner_vm == owner_vm)
        else {
            return STATUS_NOT_FOUND;
        };
        if compat.sockets[listener_index].kind != MioSocketKind::TcpListener {
            return STATUS_INVALID_INPUT;
        }
        let fallback_addr = compat.sockets[listener_index]
            .local
            .map(CompatAddr::unspecified_same_family)
            .unwrap_or(CompatAddr::V4 {
                addr: [0; 4],
                port: 0,
            });
        let Some(child_id) = compat.sockets[listener_index].accept_queue.pop_front() else {
            return STATUS_WOULD_BLOCK;
        };
        let peer_addr = compat
            .socket_for_owner(child_id, owner_vm)
            .and_then(|socket| socket.peer)
            .unwrap_or(fallback_addr);
        unsafe {
            *out_socket_id = child_id;
            *out_addr = peer_addr.to_raw();
        }
        STATUS_OK
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_tcp_listener_accept(
    socket_id: u32,
    out_socket_id: *mut u32,
    out_addr: *mut TrueosMioSocketAddr,
) -> i32 {
    if vm_guest_vmcall_active() {
        let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() else {
            return STATUS_INVALID_INPUT;
        };
        if out_socket_id.is_null() || out_addr.is_null() {
            return STATUS_INVALID_INPUT;
        }
        let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_MIO_TCP_LISTENER_ACCEPT,
            socket_id as u64,
            0,
            &[],
            &mut bytes,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return STATUS_INVALID_INPUT;
        }
        let child_id = vmcall_i32(data);
        if child_id <= 0 {
            return child_id;
        }
        let Some(addr) = read_addr(&bytes[..MIO_ADDR_BYTES]) else {
            return STATUS_INVALID_INPUT;
        };
        if !copy_u32_to_active_guest(vm_id, out_socket_id, child_id as u32)
            || !copy_mio_addr_to_active_guest(vm_id, out_addr, addr)
        {
            return STATUS_INVALID_INPUT;
        }
        return STATUS_OK;
    }
    mio_tcp_listener_accept_host(socket_id, out_socket_id, out_addr)
}

pub(crate) unsafe fn mio_selector_register_socket_host(
    selector_id: usize,
    socket_id: u32,
    token: usize,
    interests: u8,
) -> i32 {
    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        if compat.socket_for_owner(socket_id, owner_vm).is_none() {
            return STATUS_NOT_FOUND;
        }

        if let Some(reg) = compat.registrations.iter_mut().find(|reg| {
            reg.owner_vm == owner_vm && reg.selector_id == selector_id && reg.socket_id == socket_id
        }) {
            reg.token = token;
            reg.interests = interests;
            return STATUS_OK;
        }

        compat.registrations.push(SelectorRegistration {
            selector_id,
            socket_id,
            owner_vm,
            token,
            interests,
        });
        STATUS_OK
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_selector_register_socket(
    selector_id: usize,
    socket_id: u32,
    token: usize,
    interests: u8,
) -> i32 {
    if vm_guest_vmcall_active() {
        let arg0 = selector_id as u64;
        let arg1 = (socket_id as u64) | ((interests as u64) << 32);
        let mut req = [0u8; 8];
        req.copy_from_slice(&(token as u64).to_le_bytes());
        let mut out = [0u8; 1];
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_MIO_SELECTOR_REGISTER_SOCKET,
            arg0,
            arg1,
            &req,
            &mut out,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            vmcall_i32(data)
        } else {
            STATUS_INVALID_INPUT
        };
    }
    mio_selector_register_socket_host(selector_id, socket_id, token, interests)
}

pub(crate) unsafe fn mio_selector_deregister_socket_host(
    selector_id: usize,
    socket_id: u32,
) -> i32 {
    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        compat.registrations.retain(|reg| {
            !(reg.owner_vm == owner_vm
                && reg.selector_id == selector_id
                && reg.socket_id == socket_id)
        });
        STATUS_OK
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_selector_deregister_socket(
    selector_id: usize,
    socket_id: u32,
) -> i32 {
    if vm_guest_vmcall_active() {
        let (status, data) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_MIO_SELECTOR_DEREGISTER_SOCKET,
            selector_id as u64,
            socket_id as u64,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            vmcall_i32(data)
        } else {
            STATUS_INVALID_INPUT
        };
    }
    mio_selector_deregister_socket_host(selector_id, socket_id)
}

pub(crate) unsafe fn mio_selector_poll_host(
    selector_id: usize,
    out_events: *mut TrueosMioReadyEvent,
    out_cap: usize,
    timeout_nanos: u64,
) -> usize {
    if out_events.is_null() && out_cap != 0 {
        return 0;
    }
    let owner_vm = current_owner_vm();
    with_compat(|compat| {
        compat.selector_poll(owner_vm, selector_id, out_events, out_cap, timeout_nanos)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_selector_poll(
    selector_id: usize,
    out_events: *mut TrueosMioReadyEvent,
    out_cap: usize,
    timeout_nanos: u64,
) -> usize {
    if vm_guest_vmcall_active() {
        let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() else {
            return 0;
        };
        if out_events.is_null() && out_cap != 0 {
            return 0;
        }
        let max_events = (trueos_vm::vmcall::PAYLOAD_CAP / MIO_READY_EVENT_BYTES).min(out_cap);
        if max_events == 0 {
            return 0;
        }
        let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let timeout = timeout_nanos.to_le_bytes();
        let (status, count) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_MIO_SELECTOR_POLL,
            selector_id as u64,
            max_events as u64,
            &timeout,
            &mut bytes,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return 0;
        }
        let count = (count as usize).min(max_events);
        if count != 0 {
            if !copy_to_active_guest(
                vm_id,
                out_events.cast::<u8>(),
                &bytes[..count * MIO_READY_EVENT_BYTES],
            ) {
                return 0;
            }
        }
        return count;
    }
    if let Some(vm_id) = current_owner_vm() {
        if out_events.is_null() && out_cap != 0 {
            return 0;
        }
        let max_events = (trueos_vm::vmcall::PAYLOAD_CAP / MIO_READY_EVENT_BYTES).min(out_cap);
        if max_events == 0 {
            return 0;
        }
        let mut events = Vec::new();
        events.resize(max_events, TrueosMioReadyEvent::default());
        let count =
            mio_selector_poll_host(selector_id, events.as_mut_ptr(), max_events, timeout_nanos)
                .min(max_events);
        if count != 0 {
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    events.as_ptr().cast::<u8>(),
                    count * MIO_READY_EVENT_BYTES,
                )
            };
            if !copy_to_guest_stack(vm_id, out_events.cast::<u8>(), bytes) {
                return 0;
            }
        }
        return count;
    }
    mio_selector_poll_host(selector_id, out_events, out_cap, timeout_nanos)
}
