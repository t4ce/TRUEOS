extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};
use core::slice;

use spin::Mutex;
use v::vnet as api;

use crate::blueprint_net_broker::VNetBridge;

const EBADF: i32 = 9;
const EAGAIN: i32 = 11;
const EPIPE: i32 = 32;
const EIO: i32 = 5;
const EINVAL: i32 = 22;
const EMSGSIZE: i32 = 90;
const EAFNOSUPPORT: i32 = 97;
const EPROTONOSUPPORT: i32 = 93;
const EADDRNOTAVAIL: i32 = 99;
const ENETUNREACH: i32 = 101;
const ECONNRESET: i32 = 104;
const ENOBUFS: i32 = 105;
const EISCONN: i32 = 106;
const ENOTCONN: i32 = 107;
const ETIMEDOUT: i32 = 110;
const ECONNREFUSED: i32 = 111;
const EINPROGRESS: i32 = 115;

const AF_INET: i32 = 2;
const AF_INET6: i32 = 10;
const SOCK_STREAM: i32 = 1;
const IPPROTO_TCP: i32 = 6;
const MSG_PEEK: i32 = 2;

static SOCKET_SEQ: Mutex<u32> = Mutex::new(1);
static SOCKETS: Mutex<BTreeMap<u32, SocketState>> = Mutex::new(BTreeMap::new());

#[derive(Clone, Copy)]
enum RemoteAddr {
    V4([u8; 4], u16),
    V6([u8; 16], u16),
}

struct SocketState {
    owner_vm: Option<u8>,
    domain: i32,
    socket_type: i32,
    protocol: i32,
    vnet: Option<VNetBridge>,
    handle: Option<api::NetHandle>,
    remote: Option<RemoteAddr>,
    local_v4: Option<([u8; 4], u16)>,
    local_v6: Option<([u8; 16], u16)>,
    connected: bool,
    connect_submitted: bool,
    closed: bool,
    last_error: i32,
    recv_buf: VecDeque<u8>,
}

impl SocketState {
    fn new(owner_vm: Option<u8>, domain: i32, socket_type: i32, protocol: i32) -> Self {
        Self {
            owner_vm,
            domain,
            socket_type,
            protocol,
            vnet: None,
            handle: None,
            remote: None,
            local_v4: None,
            local_v6: None,
            connected: false,
            connect_submitted: false,
            closed: false,
            last_error: 0,
            recv_buf: VecDeque::new(),
        }
    }

    fn ensure_vnet(&mut self) -> Result<(), i32> {
        if self.vnet.is_some() {
            return Ok(());
        }
        if crate::net::device_count() == 0 {
            return Err(-ENETUNREACH);
        }
        let Some(vnet) = VNetBridge::open_primary() else {
            return Err(-ENETUNREACH);
        };
        self.vnet = Some(vnet);
        Ok(())
    }
}

fn next_socket_id() -> u32 {
    let mut seq = SOCKET_SEQ.lock();
    let next = *seq;
    *seq = seq.saturating_add(1).max(1);
    next
}

fn with_socket_mut<T>(
    socket_id: u32,
    f: impl FnOnce(&mut SocketState) -> Result<T, i32>,
) -> Result<T, i32> {
    let owner_vm = crate::hv::current_guest_execution_context_vm_id();
    let mut sockets = SOCKETS.lock();
    let Some(socket) = sockets.get_mut(&socket_id) else {
        return Err(-EBADF);
    };
    if socket.owner_vm != owner_vm {
        return Err(-EBADF);
    }
    f(socket)
}

fn pump_socket(socket: &mut SocketState) {
    let Some(vnet) = socket.vnet.as_ref() else {
        return;
    };

    while let Some(event) = vnet.pop_event() {
        match event {
            api::Event::Opened { handle, kind } if kind == api::SocketKind::Tcp => {
                socket.handle = Some(handle);
                socket.connected = true;
                socket.connect_submitted = false;
                socket.closed = false;
                socket.last_error = 0;
            }
            api::Event::TcpEstablished { handle, .. } => {
                socket.handle = Some(handle);
                socket.connected = true;
                socket.connect_submitted = false;
                socket.closed = false;
                socket.last_error = 0;
            }
            api::Event::TcpData { handle, data } => {
                if socket.handle == Some(handle) {
                    socket.recv_buf.extend(data.as_slice().iter().copied());
                }
            }
            api::Event::Closed { handle } => {
                if socket.handle == Some(handle) {
                    socket.handle = None;
                    socket.connected = false;
                    socket.connect_submitted = false;
                    socket.closed = true;
                }
            }
            api::Event::Error { msg } => {
                socket.last_error = map_net_error(msg);
                socket.connect_submitted = false;
            }
            _ => {}
        }
    }
}

fn map_net_error(msg: &str) -> i32 {
    if msg.contains("refused") || msg.contains("REFUSED") {
        ECONNREFUSED
    } else if msg.contains("timeout") || msg.contains("TIMEOUT") {
        ETIMEDOUT
    } else if msg.contains("reset") || msg.contains("RESET") {
        ECONNRESET
    } else if msg.contains("unreach") || msg.contains("UNREACH") || msg.contains("route") {
        ENETUNREACH
    } else if msg.contains("buffer") || msg.contains("queue") {
        ENOBUFS
    } else {
        EIO
    }
}

fn wait_until(timeout_ms: Option<u64>, mut condition: impl FnMut() -> bool) -> bool {
    match timeout_ms {
        Some(timeout_ms) => crate::wait::spin_until_timeout(timeout_ms, condition),
        None => loop {
            if condition() {
                return true;
            }
            crate::wait::spin_step();
        },
    }
}

fn finish_connect(socket_id: u32, timeout_ms: Option<u64>) -> Result<(), i32> {
    let mut result = Err(-ETIMEDOUT);
    let ready = wait_until(timeout_ms, || {
        match with_socket_mut(socket_id, |socket| {
            pump_socket(socket);
            if socket.connected {
                return Ok(Some(Ok(())));
            }
            if socket.last_error != 0 {
                return Ok(Some(Err(-socket.last_error)));
            }
            if socket.closed {
                return Ok(Some(Err(-ENOTCONN)));
            }
            Ok(None)
        }) {
            Ok(Some(state)) => {
                result = state;
                true
            }
            Ok(None) => false,
            Err(err) => {
                result = Err(err);
                true
            }
        }
    });

    if ready { result } else { Err(-ETIMEDOUT) }
}

#[inline]
fn vm_guest_vmcall_active() -> bool {
    // Only the actual VM hull stack may execute the vmcall instruction.
    // Host-carried guest workers keep VM identity but use host broker state.
    crate::hv::current_hull_guest_context_vm_id().is_some()
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
fn pack_i32_pair(a: i32, b: i32) -> u64 {
    (a as u32 as u64) | ((b as u32 as u64) << 32)
}

#[inline]
fn pack_v4_port(addr_be: u32, port_be: u16) -> u64 {
    (addr_be as u64) | ((port_be as u64) << 32)
}

#[inline]
fn unpack_v4_port(bits: u64) -> (u32, u16) {
    (bits as u32, ((bits >> 32) & 0xFFFF) as u16)
}

#[inline]
fn pack_v4_port_nonblocking(addr_be: u32, port_be: u16, nonblocking: u32) -> u64 {
    pack_v4_port(addr_be, port_be) | (((nonblocking != 0) as u64) << 48)
}

#[inline]
fn unpack_v4_port_nonblocking(bits: u64) -> (u32, u16, u32) {
    let (addr, port) = unpack_v4_port(bits);
    (addr, port, ((bits >> 48) & 1) as u32)
}

#[inline]
fn pack_port_nonblocking(port_be: u16, nonblocking: u32) -> u64 {
    (port_be as u64) | (((nonblocking != 0) as u64) << 16)
}

#[inline]
fn unpack_port_nonblocking(bits: u64) -> (u16, u32) {
    (bits as u16, ((bits >> 16) & 1) as u32)
}

fn guest_call_i32(op: u32, arg0: u64, arg1: u64) -> i32 {
    let (status, data) = trueos_vm::vmcall::call(op, arg0, arg1);
    if status == trueos_vm::vmcall::STATUS_OK {
        vmcall_i32(data)
    } else {
        -EINVAL
    }
}

fn guest_call_i32_payload(op: u32, arg0: u64, arg1: u64, req: &[u8]) -> i32 {
    let mut out = [0u8; 1];
    let (status, data) = trueos_vm::vmcall::call_with_payload(op, arg0, arg1, req, &mut out);
    if status == trueos_vm::vmcall::STATUS_OK {
        vmcall_i32(data)
    } else {
        -EINVAL
    }
}

fn connect_inner(socket_id: u32, remote: RemoteAddr, nonblocking: bool) -> Result<(), i32> {
    with_socket_mut(socket_id, |socket| {
        if socket.connected {
            return Err(-EISCONN);
        }
        socket.ensure_vnet()?;
        socket.remote = Some(remote);

        if !socket.connect_submitted {
            let Some(vnet) = socket.vnet.as_ref() else {
                return Err(-ENETUNREACH);
            };

            let submit = match remote {
                RemoteAddr::V4(addr, port) => vnet.submit(api::Command::OpenTcpConnect {
                    remote: api::EndpointV4 { addr, port },
                }),
                RemoteAddr::V6(addr, port) => vnet.submit(api::Command::OpenTcpConnectV6 {
                    remote: api::EndpointV6 { addr, port },
                }),
            };

            if submit.is_err() {
                return Err(-EAGAIN);
            }
            socket.connect_submitted = true;
            socket.closed = false;
            socket.last_error = 0;
        }

        pump_socket(socket);
        if socket.connected {
            return Ok(());
        }
        if socket.last_error != 0 {
            return Err(-socket.last_error);
        }
        if nonblocking {
            return Err(-EINPROGRESS);
        }
        Ok(())
    })?;

    if nonblocking {
        Err(-EINPROGRESS)
    } else {
        finish_connect(socket_id, None)
    }
}

fn close_socket_state(socket: &mut SocketState) {
    if let (Some(vnet), Some(handle)) = (socket.vnet.as_ref(), socket.handle.take()) {
        let _ = vnet.submit(api::Command::Close { handle });
    }
    socket.connected = false;
    socket.connect_submitted = false;
    socket.closed = true;
}

fn recv_inner(socket_id: u32, out: &mut [u8], flags: i32) -> Result<Option<usize>, i32> {
    with_socket_mut(socket_id, |socket| {
        pump_socket(socket);

        if !socket.recv_buf.is_empty() {
            let count = out.len().min(socket.recv_buf.len());
            if flags & MSG_PEEK != 0 {
                for (slot, byte) in out.iter_mut().zip(socket.recv_buf.iter().take(count)) {
                    *slot = *byte;
                }
            } else {
                for slot in out.iter_mut().take(count) {
                    if let Some(byte) = socket.recv_buf.pop_front() {
                        *slot = byte;
                    }
                }
            }
            return Ok(Some(count));
        }

        if socket.last_error != 0 {
            return Err(-socket.last_error);
        }
        if socket.closed {
            return Ok(Some(0));
        }
        if !socket.connected {
            return Err(-ENOTCONN);
        }
        Ok(None)
    })
}

pub(crate) fn socket_tcp_open_host(domain: i32, socket_type: i32, protocol: i32) -> i32 {
    if !matches!(domain, AF_INET | AF_INET6) {
        return -EAFNOSUPPORT;
    }
    if socket_type != SOCK_STREAM {
        return -EPROTONOSUPPORT;
    }
    if protocol != 0 && protocol != IPPROTO_TCP {
        return -EPROTONOSUPPORT;
    }

    let socket_id = next_socket_id();
    let owner_vm = crate::hv::current_guest_execution_context_vm_id();
    SOCKETS
        .lock()
        .insert(socket_id, SocketState::new(owner_vm, domain, socket_type, protocol));
    socket_id as i32
}

pub(crate) fn socket_tcp_close_host(socket_id: u32) -> i32 {
    let owner_vm = crate::hv::current_guest_execution_context_vm_id();
    let mut sockets = SOCKETS.lock();
    let Some(socket) = sockets.get(&socket_id) else {
        return -EBADF;
    };
    if socket.owner_vm != owner_vm {
        return -EBADF;
    }
    let Some(mut socket) = sockets.remove(&socket_id) else {
        return -EBADF;
    };
    drop(sockets);
    close_socket_state(&mut socket);
    0
}

pub(crate) fn socket_tcp_set_nonblocking_host(socket_id: u32, nonblocking: u32) -> i32 {
    match with_socket_mut(socket_id, |socket| {
        let _ = nonblocking;
        let _ = socket.domain;
        Ok(())
    }) {
        Ok(()) => 0,
        Err(err) => err,
    }
}

pub(crate) fn socket_tcp_bind_v4_host(socket_id: u32, addr_be: u32, port_be: u16) -> i32 {
    let addr = addr_be.to_be_bytes();
    match with_socket_mut(socket_id, |socket| {
        if socket.domain != AF_INET {
            return Err(-EAFNOSUPPORT);
        }
        if addr != [0, 0, 0, 0] || port_be != 0 {
            return Err(-EADDRNOTAVAIL);
        }
        socket.local_v4 = Some((addr, 0));
        Ok(())
    }) {
        Ok(()) => 0,
        Err(err) => err,
    }
}

pub(crate) fn socket_tcp_bind_v6_host(socket_id: u32, addr: [u8; 16], port_be: u16) -> i32 {
    match with_socket_mut(socket_id, |socket| {
        if socket.domain != AF_INET6 {
            return Err(-EAFNOSUPPORT);
        }
        if addr != [0; 16] || port_be != 0 {
            return Err(-EADDRNOTAVAIL);
        }
        socket.local_v6 = Some((addr, 0));
        Ok(())
    }) {
        Ok(()) => 0,
        Err(err) => err,
    }
}

pub(crate) fn socket_tcp_connect_v4_host(
    socket_id: u32,
    addr_be: u32,
    port_be: u16,
    nonblocking: u32,
) -> i32 {
    match connect_inner(
        socket_id,
        RemoteAddr::V4(addr_be.to_be_bytes(), u16::from_be(port_be)),
        nonblocking != 0,
    ) {
        Ok(()) => 0,
        Err(err) => err,
    }
}

pub(crate) fn socket_tcp_connect_v6_host(
    socket_id: u32,
    addr: [u8; 16],
    port_be: u16,
    nonblocking: u32,
) -> i32 {
    match connect_inner(socket_id, RemoteAddr::V6(addr, u16::from_be(port_be)), nonblocking != 0) {
        Ok(()) => 0,
        Err(err) => err,
    }
}

pub(crate) fn socket_tcp_poll_connect_host(socket_id: u32, timeout_ms: u64) -> i32 {
    match finish_connect(socket_id, Some(timeout_ms)) {
        Ok(()) => 0,
        Err(err) => err,
    }
}

pub(crate) fn socket_tcp_send_host(socket_id: u32, data: &[u8]) -> isize {
    if data.is_empty() {
        return 0;
    }

    let mut sent = 0usize;
    while sent < data.len() {
        let end = (sent + api::MAX_MSG).min(data.len());
        let result = with_socket_mut(socket_id, |socket| {
            pump_socket(socket);
            if socket.last_error != 0 {
                return Err(-socket.last_error);
            }
            if socket.closed {
                return Err(-EPIPE);
            }
            if !socket.connected {
                return Err(if socket.connect_submitted {
                    -EINPROGRESS
                } else {
                    -ENOTCONN
                });
            }
            let Some(handle) = socket.handle else {
                return Err(-ENOTCONN);
            };
            let Some(vnet) = socket.vnet.as_ref() else {
                return Err(-ENETUNREACH);
            };
            vnet.submit(api::Command::SendTcp {
                handle,
                data: api::ByteBuf::from_slice_trunc(&data[sent..end]),
            })
            .map_err(|_| -EAGAIN)?;
            Ok(())
        });

        match result {
            Ok(()) => sent = end,
            Err(_) if sent != 0 => return sent as isize,
            Err(err) => return err as isize,
        }
    }

    sent as isize
}

pub(crate) fn socket_tcp_recv_host(
    socket_id: u32,
    out: &mut [u8],
    flags: i32,
    nonblocking: u32,
    timeout_ms: u64,
) -> isize {
    if out.is_empty() {
        return 0;
    }

    match recv_inner(socket_id, out, flags) {
        Ok(Some(count)) => return count as isize,
        Ok(None) => {}
        Err(err) => return err as isize,
    }

    if nonblocking != 0 {
        return -EAGAIN as isize;
    }

    let mut result = Err(-ETIMEDOUT);
    let timeout = if timeout_ms == u64::MAX {
        None
    } else {
        Some(timeout_ms)
    };
    let ready = wait_until(timeout, || match recv_inner(socket_id, out, flags) {
        Ok(Some(count)) => {
            result = Ok(count);
            true
        }
        Ok(None) => false,
        Err(err) => {
            result = Err(err);
            true
        }
    });

    if ready {
        match result {
            Ok(count) => count as isize,
            Err(err) => err as isize,
        }
    } else {
        -ETIMEDOUT as isize
    }
}

pub(crate) fn socket_tcp_shutdown_host(socket_id: u32, _how: u32) -> i32 {
    match with_socket_mut(socket_id, |socket| {
        close_socket_state(socket);
        Ok(())
    }) {
        Ok(()) => 0,
        Err(err) => err,
    }
}

pub(crate) fn socket_tcp_take_error_host(socket_id: u32) -> i32 {
    match with_socket_mut(socket_id, |socket| {
        pump_socket(socket);
        let err = socket.last_error;
        socket.last_error = 0;
        Ok(err)
    }) {
        Ok(err) => err,
        Err(err) => err,
    }
}

pub(crate) fn socket_tcp_peer_v4_host(socket_id: u32) -> Result<(u32, u16), i32> {
    with_socket_mut(socket_id, |socket| match socket.remote {
        Some(RemoteAddr::V4(addr, port)) => Ok((u32::from_be_bytes(addr), port.to_be())),
        Some(RemoteAddr::V6(_, _)) => Err(-EAFNOSUPPORT),
        None => Err(-ENOTCONN),
    })
}

pub(crate) fn socket_tcp_peer_v6_host(socket_id: u32) -> Result<([u8; 16], u16), i32> {
    with_socket_mut(socket_id, |socket| match socket.remote {
        Some(RemoteAddr::V6(addr, port)) => Ok((addr, port.to_be())),
        Some(RemoteAddr::V4(_, _)) => Err(-EAFNOSUPPORT),
        None => Err(-ENOTCONN),
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_open(domain: i32, socket_type: i32, protocol: i32) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_call_i32(
            trueos_vm::vmcall::OP_BP_SOCKET_TCP_OPEN,
            pack_i32_pair(domain, socket_type),
            protocol as u32 as u64,
        );
    }
    socket_tcp_open_host(domain, socket_type, protocol)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_close(socket_id: u32) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_call_i32(trueos_vm::vmcall::OP_BP_SOCKET_TCP_CLOSE, socket_id as u64, 0);
    }
    socket_tcp_close_host(socket_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_set_nonblocking(socket_id: u32, nonblocking: u32) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_call_i32(
            trueos_vm::vmcall::OP_BP_SOCKET_TCP_SET_NONBLOCKING,
            socket_id as u64,
            nonblocking as u64,
        );
    }
    socket_tcp_set_nonblocking_host(socket_id, nonblocking)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_bind_v4(
    socket_id: u32,
    addr_be: u32,
    port_be: u16,
) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_call_i32(
            trueos_vm::vmcall::OP_BP_SOCKET_TCP_BIND_V4,
            socket_id as u64,
            pack_v4_port(addr_be, port_be),
        );
    }
    socket_tcp_bind_v4_host(socket_id, addr_be, port_be)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_bind_v6(
    socket_id: u32,
    addr_ptr: *const u8,
    port_be: u16,
) -> i32 {
    if addr_ptr.is_null() {
        return -EINVAL;
    }

    let mut addr = [0u8; 16];
    // SAFETY: the caller provides a 16-byte IPv6 buffer.
    unsafe { addr.copy_from_slice(slice::from_raw_parts(addr_ptr, 16)) };

    if vm_guest_vmcall_active() {
        return guest_call_i32_payload(
            trueos_vm::vmcall::OP_BP_SOCKET_TCP_BIND_V6,
            socket_id as u64,
            port_be as u64,
            &addr,
        );
    }
    socket_tcp_bind_v6_host(socket_id, addr, port_be)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_connect_v4(
    socket_id: u32,
    addr_be: u32,
    port_be: u16,
    nonblocking: u32,
) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_call_i32(
            trueos_vm::vmcall::OP_BP_SOCKET_TCP_CONNECT_V4,
            socket_id as u64,
            pack_v4_port_nonblocking(addr_be, port_be, nonblocking),
        );
    }
    socket_tcp_connect_v4_host(socket_id, addr_be, port_be, nonblocking)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_connect_v6(
    socket_id: u32,
    addr_ptr: *const u8,
    port_be: u16,
    nonblocking: u32,
) -> i32 {
    if addr_ptr.is_null() {
        return -EINVAL;
    }

    let mut addr = [0u8; 16];
    // SAFETY: the caller provides a 16-byte IPv6 buffer.
    unsafe { addr.copy_from_slice(slice::from_raw_parts(addr_ptr, 16)) };

    if vm_guest_vmcall_active() {
        return guest_call_i32_payload(
            trueos_vm::vmcall::OP_BP_SOCKET_TCP_CONNECT_V6,
            socket_id as u64,
            pack_port_nonblocking(port_be, nonblocking),
            &addr,
        );
    }
    socket_tcp_connect_v6_host(socket_id, addr, port_be, nonblocking)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_poll_connect(socket_id: u32, timeout_ms: u64) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_call_i32(
            trueos_vm::vmcall::OP_BP_SOCKET_TCP_POLL_CONNECT,
            socket_id as u64,
            timeout_ms,
        );
    }
    socket_tcp_poll_connect_host(socket_id, timeout_ms)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_send(
    socket_id: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> isize {
    if data_len == 0 {
        return 0;
    }
    if data_ptr.is_null() {
        return -EINVAL as isize;
    }

    // SAFETY: the caller provides a valid pointer for `data_len` bytes.
    let data = unsafe { slice::from_raw_parts(data_ptr, data_len) };
    if vm_guest_vmcall_active() {
        let mut sent = 0usize;
        while sent < data.len() {
            let end = core::cmp::min(sent + trueos_vm::vmcall::PAYLOAD_CAP, data.len());
            let mut out = [0u8; 1];
            let (status, rc) = trueos_vm::vmcall::call_with_payload(
                trueos_vm::vmcall::OP_BP_SOCKET_TCP_SEND,
                socket_id as u64,
                0,
                &data[sent..end],
                &mut out,
            );
            if status != trueos_vm::vmcall::STATUS_OK {
                return if sent == 0 {
                    -EINVAL as isize
                } else {
                    sent as isize
                };
            }
            let rc = vmcall_isize(rc);
            if rc < 0 {
                return if sent == 0 { rc } else { sent as isize };
            }
            if rc == 0 {
                return sent as isize;
            }
            sent += core::cmp::min(rc as usize, end - sent);
        }
        return sent as isize;
    }
    socket_tcp_send_host(socket_id, data)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_recv(
    socket_id: u32,
    out_ptr: *mut u8,
    out_cap: usize,
    flags: i32,
    nonblocking: u32,
    timeout_ms: u64,
) -> isize {
    if out_cap == 0 {
        return 0;
    }
    if out_ptr.is_null() {
        return -EINVAL as isize;
    }
    if out_cap > isize::MAX as usize {
        return -EMSGSIZE as isize;
    }

    // SAFETY: the caller provides a valid output buffer for `out_cap` bytes.
    let out = unsafe { slice::from_raw_parts_mut(out_ptr, out_cap) };
    if vm_guest_vmcall_active() {
        let want = core::cmp::min(out.len(), trueos_vm::vmcall::PAYLOAD_CAP);
        let mut req = [0u8; 16];
        req[..4].copy_from_slice(&flags.to_le_bytes());
        req[4..8].copy_from_slice(&nonblocking.to_le_bytes());
        req[8..16].copy_from_slice(&timeout_ms.to_le_bytes());
        let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let (status, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_SOCKET_TCP_RECV,
            socket_id as u64,
            want as u64,
            &req,
            &mut bytes,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return -EINVAL as isize;
        }
        let rc = vmcall_isize(rc);
        if rc <= 0 {
            return rc;
        }
        let got = core::cmp::min(rc as usize, want);
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, got);
        }
        return got as isize;
    }
    socket_tcp_recv_host(socket_id, out, flags, nonblocking, timeout_ms)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_shutdown(socket_id: u32, _how: u32) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_call_i32(
            trueos_vm::vmcall::OP_BP_SOCKET_TCP_SHUTDOWN,
            socket_id as u64,
            _how as u64,
        );
    }
    socket_tcp_shutdown_host(socket_id, _how)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_take_error(socket_id: u32) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_call_i32(trueos_vm::vmcall::OP_BP_SOCKET_TCP_TAKE_ERROR, socket_id as u64, 0);
    }
    socket_tcp_take_error_host(socket_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_peer_v4(
    socket_id: u32,
    out_addr_be: *mut u32,
    out_port_be: *mut u16,
) -> i32 {
    if out_addr_be.is_null() || out_port_be.is_null() {
        return -EINVAL;
    }
    if vm_guest_vmcall_active() {
        let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let (status, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_SOCKET_TCP_PEER_V4,
            socket_id as u64,
            0,
            &[],
            &mut bytes,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return -EINVAL;
        }
        let rc = vmcall_i32(rc);
        if rc != 0 {
            return rc;
        }
        let mut addr = [0u8; 4];
        addr.copy_from_slice(&bytes[..4]);
        let mut port = [0u8; 2];
        port.copy_from_slice(&bytes[4..6]);
        // SAFETY: output pointers are validated above.
        unsafe {
            *out_addr_be = u32::from_le_bytes(addr);
            *out_port_be = u16::from_le_bytes(port);
        }
        return 0;
    }
    match socket_tcp_peer_v4_host(socket_id) {
        Ok((addr, port)) => {
            // SAFETY: output pointers are validated above.
            unsafe {
                *out_addr_be = addr;
                *out_port_be = port;
            }
            0
        }
        Err(err) => err,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_socket_tcp_peer_v6(
    socket_id: u32,
    out_addr_ptr: *mut u8,
    out_port_be: *mut u16,
) -> i32 {
    if out_addr_ptr.is_null() || out_port_be.is_null() {
        return -EINVAL;
    }
    if vm_guest_vmcall_active() {
        let mut bytes = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let (status, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_SOCKET_TCP_PEER_V6,
            socket_id as u64,
            0,
            &[],
            &mut bytes,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return -EINVAL;
        }
        let rc = vmcall_i32(rc);
        if rc != 0 {
            return rc;
        }
        let mut port = [0u8; 2];
        port.copy_from_slice(&bytes[16..18]);
        // SAFETY: output pointers are validated above.
        unsafe {
            slice::from_raw_parts_mut(out_addr_ptr, 16).copy_from_slice(&bytes[..16]);
            *out_port_be = u16::from_le_bytes(port);
        }
        return 0;
    }
    match socket_tcp_peer_v6_host(socket_id) {
        Ok((addr, port)) => {
            // SAFETY: output pointers are validated above.
            unsafe {
                slice::from_raw_parts_mut(out_addr_ptr, 16).copy_from_slice(&addr);
                *out_port_be = port;
            }
            0
        }
        Err(err) => err,
    }
}
