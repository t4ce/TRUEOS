//! Owned native TCP sockets before a transport is connected or listening.
use super::*;

pub(crate) unsafe fn mio_tcp_socket_create_host(family: u32, out: *mut u32) -> i32 {
    if out.is_null() || !matches!(family, 4 | 6) {
        return STATUS_INVALID_INPUT;
    }
    let owner = current_owner_vm();
    with_compat(|compat| {
        if !compat.socket_capacity_available(owner) {
            return STATUS_RESOURCE_EXHAUSTED;
        }
        let id = compat.alloc_socket_id();
        let local = if family == 4 {
            CompatAddr::V4 {
                addr: [0; 4],
                port: 0,
            }
        } else {
            CompatAddr::V6 {
                addr: [0; 16],
                port: 0,
            }
        };
        compat.sockets.push(MioSocketState {
            id,
            owner_vm: owner,
            actor: capture_network_actor(owner),
            progress_generation: owner.and_then(crate::hv::vm_run_generation).unwrap_or(0),
            kind: MioSocketKind::TcpSocket,
            setup: Some(TcpSocketSetup {
                family,
                bound: false,
                backlog: 1,
                nodelay: false,
            }),
            handle: None,
            listen_handles: Vec::new(),
            local: Some(local),
            peer: None,
            listen_port: None,
            connected: false,
            closed: false,
            read_shutdown: false,
            write_shutdown: false,
            peer_eof: false,
            error: STATUS_OK,
            tx_in_flight: 0,
            rx_stream: VecDeque::new(),
            rx_dgrams: VecDeque::new(),
            accept_queue: VecDeque::new(),
        });
        unsafe { out.write(id) };
        STATUS_OK
    })
}

fn reserve(compat: &mut MioCompat, id: u32, local: CompatAddr) -> i32 {
    let Some(socket) = compat.socket_for_owner(id, current_owner_vm()) else {
        return STATUS_NOT_FOUND;
    };
    if socket.kind != MioSocketKind::TcpSocket || socket.closed {
        return STATUS_INVALID_INPUT;
    }
    let setup = socket.setup.unwrap();
    let actor = socket.actor;
    if setup.bound || u32::from(local.to_raw().family) != setup.family {
        return STATUS_INVALID_INPUT;
    }
    let mut raw = local.to_raw();
    if raw.port == 0 {
        let Some(port) = compat.alloc_tcp_listen_port() else {
            return STATUS_RESOURCE_EXHAUSTED;
        };
        raw.port = port;
    }
    let local = CompatAddr::from_raw(raw).unwrap();
    if crate::access_key_db::authorize_tcp_listen(actor, local.socket_addr()).is_err() {
        return STATUS_IO;
    }
    if compat.tcp_listener_address_in_use(local) {
        return STATUS_IO;
    }
    let socket = compat.socket_mut(id).unwrap();
    socket.local = Some(local);
    socket.setup.as_mut().unwrap().bound = true;
    STATUS_OK
}

pub(crate) fn mio_tcp_socket_bind_host(id: u32, raw: TrueosMioSocketAddr) -> i32 {
    let Some(local) = CompatAddr::from_raw(raw) else {
        return STATUS_INVALID_INPUT;
    };
    with_compat(|compat| reserve(compat, id, local))
}

fn prepare(compat: &mut MioCompat, id: u32) -> Result<CompatAddr, i32> {
    let socket = compat
        .socket_for_owner(id, current_owner_vm())
        .ok_or(STATUS_NOT_FOUND)?;
    if socket.kind != MioSocketKind::TcpSocket || socket.closed {
        return Err(STATUS_INVALID_INPUT);
    }
    let local = socket.local.unwrap();
    if !socket.setup.unwrap().bound {
        let status = reserve(compat, id, local);
        if status != STATUS_OK {
            return Err(status);
        }
    }
    Ok(compat.socket(id).unwrap().local.unwrap())
}

pub(crate) fn mio_tcp_socket_connect_host(id: u32, raw: TrueosMioSocketAddr) -> i32 {
    let Some(peer) = CompatAddr::from_raw(raw) else {
        return STATUS_INVALID_INPUT;
    };
    if raw.port == 0 {
        return STATUS_INVALID_INPUT;
    }
    with_compat(|compat| {
        let Some(socket) = compat.socket_for_owner(id, current_owner_vm()) else {
            return STATUS_NOT_FOUND;
        };
        if socket.kind != MioSocketKind::TcpSocket
            || socket.setup.unwrap().family != u32::from(raw.family)
        {
            return STATUS_INVALID_INPUT;
        }
        let actor = socket.actor;
        let local = match prepare(compat, id) {
            Ok(local) => local,
            Err(status) => return status,
        };
        if let Err(status) = compat.ensure_net() {
            return status;
        }
        let socket = compat.socket_mut(id).unwrap();
        socket.kind = MioSocketKind::TcpStream;
        socket.peer = Some(peer);
        compat.pending_opens.push_back(PendingOpen {
            socket_id: id,
            kind: api::SocketKind::Tcp,
        });
        let as_addr = |addr: CompatAddr| match addr {
            CompatAddr::V4 { addr, port } => {
                core::net::SocketAddr::new(core::net::Ipv4Addr::from(addr).into(), port)
            }
            CompatAddr::V6 { addr, port } => {
                core::net::SocketAddr::new(core::net::Ipv6Addr::from(addr).into(), port)
            }
        };
        if compat
            .net
            .as_ref()
            .unwrap()
            .submit_tcp_connect_from_for_actor(as_addr(local), as_addr(peer), actor)
            .is_err()
        {
            compat.pending_opens.pop_back();
            let socket = compat.socket_mut(id).unwrap();
            socket.kind = MioSocketKind::TcpSocket;
            socket.peer = None;
            return STATUS_WOULD_BLOCK;
        }
        STATUS_OK
    })
}

pub(crate) fn mio_tcp_socket_listen_host(id: u32, backlog: u32) -> i32 {
    // The pool is a real bounded resource. Larger backlogs require a different
    // transport budget; do not silently claim to provide one.
    if backlog > TCP_LISTENER_PREPOST as u32 {
        return STATUS_INVALID_INPUT;
    }
    with_compat(|compat| {
        let local = match prepare(compat, id) {
            Ok(local) => local,
            Err(status) => return status,
        };
        let port = local.to_raw().port;
        let socket = compat.socket_mut(id).unwrap();
        socket.kind = MioSocketKind::TcpListener;
        socket.listen_port = Some(port);
        socket.setup.as_mut().unwrap().backlog = backlog.max(1) as usize;
        if let Err(status) = compat.post_tcp_listener_open(id, port) {
            let socket = compat.socket_mut(id).unwrap();
            socket.kind = MioSocketKind::TcpSocket;
            socket.listen_port = None;
            return status;
        }
        compat.ensure_tcp_listener_depth(id);
        STATUS_OK
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_mio_tcp_socket_create(family: u32, out: *mut u32) -> i32 {
    if out.is_null() || !matches!(family, 4 | 6) {
        return STATUS_INVALID_INPUT;
    }
    if vm_guest_vmcall_active() {
        return guest_mio_socket_out(
            trueos_vm::vmcall::OP_BP_MIO_TCP_SOCKET_CREATE,
            TrueosMioSocketAddr {
                family: family as u8,
                ..Default::default()
            },
            out,
        );
    }
    if let Some(owner) = current_owner_vm() {
        let mut id = 0;
        let status = unsafe { mio_tcp_socket_create_host(family, &mut id) };
        if status == STATUS_OK && !copy_u32_to_guest_stack(owner, out, id) {
            unsafe { mio_socket_close_host(id) };
            return STATUS_INVALID_INPUT;
        }
        return status;
    }
    unsafe { mio_tcp_socket_create_host(family, out) }
}

fn address_call(op: u32, id: u32, addr: TrueosMioSocketAddr) -> i32 {
    guest_mio_signed_payload(op, u64::from(id), 0, addr_bytes(&addr)) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_mio_tcp_socket_bind(id: u32, addr: TrueosMioSocketAddr) -> i32 {
    if vm_guest_vmcall_active() {
        return address_call(trueos_vm::vmcall::OP_BP_MIO_TCP_SOCKET_BIND, id, addr);
    }
    mio_tcp_socket_bind_host(id, addr)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_mio_tcp_socket_connect(id: u32, addr: TrueosMioSocketAddr) -> i32 {
    if vm_guest_vmcall_active() {
        return address_call(trueos_vm::vmcall::OP_BP_MIO_TCP_SOCKET_CONNECT, id, addr);
    }
    mio_tcp_socket_connect_host(id, addr)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_mio_tcp_socket_listen(id: u32, backlog: u32) -> i32 {
    if vm_guest_vmcall_active() {
        return guest_mio_signed_payload(
            trueos_vm::vmcall::OP_BP_MIO_TCP_SOCKET_LISTEN,
            u64::from(id),
            u64::from(backlog),
            &[],
        ) as i32;
    }
    mio_tcp_socket_listen_host(id, backlog)
}
