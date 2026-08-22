use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;
use v::vnet as api;

use crate::r::net::VNet;

struct HostBlueprintNetSession {
    id: u32,
    net: VNet,
    handles: Vec<api::NetHandle>,
}

static SESSIONS: [Mutex<Option<HostBlueprintNetSession>>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { Mutex::new(None) }; crate::allcaps::hv::VM_ID_LIMIT];
static NEXT_SESSION_ID: AtomicU32 = AtomicU32::new(1);

fn session_slot(vm_id: u8) -> Option<&'static Mutex<Option<HostBlueprintNetSession>>> {
    SESSIONS.get(vm_id as usize)
}

fn remember_event_handle(session: &mut HostBlueprintNetSession, event: &api::Event) {
    let handle = match event {
        api::Event::Opened { handle, .. }
        | api::Event::TcpEstablished { handle, .. }
        | api::Event::TcpData { handle, .. }
        | api::Event::TcpSent { handle, .. }
        | api::Event::UdpPacket { handle, .. }
        | api::Event::UdpPacketV6 { handle, .. }
        | api::Event::IpPacket { handle, .. } => Some(*handle),
        api::Event::Closed { handle } => {
            session.handles.retain(|known| known != handle);
            None
        }
        api::Event::Error { .. }
        | api::Event::IcmpReply { .. }
        | api::Event::IcmpReplyV6 { .. } => None,
    };
    if let Some(handle) = handle
        && !session.handles.contains(&handle)
    {
        session.handles.push(handle);
    }
}

fn pump_host_net(vm_id: u8) -> Result<(), ()> {
    for _ in 0..8 {
        if crate::hv::lifecycle_request_pending(vm_id) {
            return Err(());
        }
        crate::time::poll();
        crate::runtime::poll_local_executor();
        core::hint::spin_loop();
    }
    Ok(())
}

pub(crate) fn open_primary(vm_id: u8) -> Option<u32> {
    pump_host_net(vm_id).ok()?;
    let net = VNet::open_primary()?;
    let slot = session_slot(vm_id)?;
    let mut session = crate::hv::sync::lock(vm_id, slot).ok()?;
    let next_id = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed).max(1);
    *session = Some(HostBlueprintNetSession {
        id: next_id,
        net,
        handles: Vec::new(),
    });
    Some(next_id)
}

pub(crate) fn submit(vm_id: u8, session_id: u32, command_bytes: &[u8]) -> Result<(), ()> {
    let command = crate::blueprint_net_wire::decode_command(command_bytes).map_err(|_| ())?;
    match command {
        api::Command::OpenTcpConnect { remote } => {
            crate::hv::hvlogf(format_args!(
                "hv: blueprint-net submit tcp-connect {}.{}.{}.{}:{}",
                remote.addr[0], remote.addr[1], remote.addr[2], remote.addr[3], remote.port
            ));
        }
        api::Command::OpenTcpListen { port } => {
            crate::hv::hvlogf(format_args!("hv: blueprint-net submit tcp-listen port={}", port));
        }
        _ => {}
    }

    let result = {
        let slot = session_slot(vm_id).ok_or(())?;
        let mut guard = crate::hv::sync::lock(vm_id, slot).map_err(|_| ())?;
        let Some(session) = guard.as_mut() else {
            return Err(());
        };
        if session.id != session_id {
            return Err(());
        }
        session.net.submit(command)
    };
    pump_host_net(vm_id)?;
    result
}

pub(crate) fn poll_event(vm_id: u8, session_id: u32, out: &mut [u8]) -> Result<Option<usize>, ()> {
    pump_host_net(vm_id)?;
    let slot = session_slot(vm_id).ok_or(())?;
    let mut session = crate::hv::sync::lock(vm_id, slot).map_err(|_| ())?;
    let Some(session) = session.as_mut() else {
        return Err(());
    };
    if session.id != session_id {
        return Err(());
    }
    let Some(event) = session.net.pop_event() else {
        return Ok(None);
    };
    remember_event_handle(session, &event);
    match event {
        api::Event::Opened {
            kind: api::SocketKind::Tcp,
            handle,
        } => {
            crate::hv::hvlogf(format_args!(
                "hv: blueprint-net event tcp-opened handle={}",
                handle.0
            ));
        }
        api::Event::TcpEstablished { handle, .. } => {
            crate::hv::hvlogf(format_args!(
                "hv: blueprint-net event tcp-established handle={}",
                handle.0
            ));
        }
        api::Event::TcpData { handle, ref data } => {
            use core::sync::atomic::{AtomicUsize, Ordering};

            static TCP_DATA_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
            let count = TCP_DATA_TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
            if count < 32 || count.is_power_of_two() {
                crate::hv::hvlogf(format_args!(
                    "hv: blueprint-net event tcp-data handle={} bytes={} count={}",
                    handle.0,
                    data.as_slice().len(),
                    count
                ));
            }
        }
        api::Event::Error { .. } => {
            crate::hv::hverrorf(format_args!("hv: blueprint-net event error"));
        }
        _ => {}
    }
    crate::blueprint_net_wire::encode_event(event, out)
        .map(Some)
        .map_err(|_| ())
}

pub(crate) fn release_vm(vm_id: u8) -> usize {
    let Some(slot) = session_slot(vm_id) else {
        return 0;
    };
    let Some(mut session) = slot.lock().take() else {
        return 0;
    };

    while let Some(event) = session.net.pop_event() {
        remember_event_handle(&mut session, &event);
    }
    let handles = core::mem::take(&mut session.handles);
    for handle in handles.iter().copied() {
        let _ = session.net.submit(api::Command::Close { handle });
    }
    handles.len()
}
