use spin::Mutex;

use crate::r::net::VNet;

struct HostBlueprintNetSession {
    id: u32,
    net: VNet,
}

static SESSION: Mutex<Option<HostBlueprintNetSession>> = Mutex::new(None);

pub(crate) fn open_primary() -> Option<u32> {
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
    let mut session = SESSION.lock();
    let Some(session) = session.as_mut() else {
        return Err(());
    };
    if session.id != session_id {
        return Err(());
    }
    session.net.submit(command)
}

pub(crate) fn poll_event(session_id: u32, out: &mut [u8]) -> Result<Option<usize>, ()> {
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
    crate::blueprint_net_wire::encode_event(event, out)
        .map(Some)
        .map_err(|_| ())
}
