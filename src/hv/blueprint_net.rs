use spin::Mutex;
use v::vnet as api;

use crate::r::net::VNet;

struct HostBlueprintNetSession {
    id: u32,
    net: VNet,
}

static SESSION: Mutex<Option<HostBlueprintNetSession>> = Mutex::new(None);

fn pump_host_net() {
    for _ in 0..8 {
        crate::time::poll();
        crate::runtime::poll_local_executor();
        core::hint::spin_loop();
    }
}

pub(crate) fn open_primary() -> Option<u32> {
    pump_host_net();
    let net = VNet::open_primary()?;
    let mut session = SESSION.lock();
    let next_id = session
        .as_ref()
        .map(|session| session.id.wrapping_add(1).max(1))
        .unwrap_or(1);
    *session = Some(HostBlueprintNetSession { id: next_id, net });
    Some(next_id)
}

pub(crate) fn submit(session_id: u32, command_bytes: &[u8]) -> Result<(), ()> {
    let command = crate::blueprint_net_wire::decode_command(command_bytes).map_err(|_| ())?;
    match command {
        api::Command::OpenTcpConnect { remote } => {
            crate::hv::hvlogf(format_args!(
                "hv: blueprint-net submit tcp-connect {}.{}.{}.{}:{}",
                remote.addr[0],
                remote.addr[1],
                remote.addr[2],
                remote.addr[3],
                remote.port
            ));
        }
        api::Command::OpenTcpListen { port } => {
            crate::hv::hvlogf(format_args!("hv: blueprint-net submit tcp-listen port={}", port));
        }
        _ => {}
    }

    let result = {
        let mut guard = SESSION.lock();
        let Some(session) = guard.as_mut() else {
            return Err(());
        };
        if session.id != session_id {
            return Err(());
        }
        session.net.submit(command)
    };
    pump_host_net();
    result
}

pub(crate) fn poll_event(session_id: u32, out: &mut [u8]) -> Result<Option<usize>, ()> {
    pump_host_net();
    let mut session = SESSION.lock();
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
        api::Event::TcpEstablished { handle } => {
            crate::hv::hvlogf(format_args!(
                "hv: blueprint-net event tcp-established handle={}",
                handle.0
            ));
        }
        api::Event::Error { .. } => {
            crate::hv::hvlogf(format_args!("hv: blueprint-net event error"));
        }
        _ => {}
    }
    crate::blueprint_net_wire::encode_event(event, out)
        .map(Some)
        .map_err(|_| ())
}
