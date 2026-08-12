use spin::Mutex;
use v::vnet as api;

use crate::r::net::VNet;

struct HostBlueprintNetSession {
    id: u32,
    net: VNet,
}

static SESSION: Mutex<Option<HostBlueprintNetSession>> = Mutex::new(None);

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
    let mut session = crate::hv::sync::lock(vm_id, &SESSION).ok()?;
    let next_id = session
        .as_ref()
        .map(|session| session.id.wrapping_add(1).max(1))
        .unwrap_or(1);
    *session = Some(HostBlueprintNetSession { id: next_id, net });
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
        let mut guard = crate::hv::sync::lock(vm_id, &SESSION).map_err(|_| ())?;
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
    let mut session = crate::hv::sync::lock(vm_id, &SESSION).map_err(|_| ())?;
    let Some(session) = session.as_mut() else {
        return Err(());
    };
    if session.id != session_id {
        return Err(());
    }
    let Some(event) = session.net.pop_event() else {
        return Ok(None);
    };
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
