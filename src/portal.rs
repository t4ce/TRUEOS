extern crate alloc;

use alloc::vec::Vec;
use core::ffi::c_char;

use embassy_executor::{SendSpawner, SpawnError, Spawner};

#[derive(Clone, Copy)]
pub(crate) struct LinkedPortal {
    pub(crate) name: &'static str,
    pub(crate) entry_symbol: &'static str,
    pub(crate) artifact_kind: &'static str,
    entry: unsafe extern "C" fn(argc: usize, argv: *const *const c_char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LaunchError {
    MissingPortal,
    NoWorkerSpawner,
    SpawnFailed,
}

struct PortalLaunch {
    name: &'static str,
    entry: unsafe extern "C" fn(argc: usize, argv: *const *const c_char),
    args: Vec<Vec<u8>>,
}

const LINKED_PORTALS: &[LinkedPortal] = &[];

pub(crate) fn linked_portals() -> &'static [LinkedPortal] {
    LINKED_PORTALS
}

pub(crate) fn launch_linked_portal(
    spawner: &Spawner,
    portal_index: usize,
    args: &[&str],
) -> Result<LinkedPortal, LaunchError> {
    let Some(portal) = LINKED_PORTALS.get(portal_index).copied() else {
        return Err(LaunchError::MissingPortal);
    };

    let mut owned_args = Vec::with_capacity(args.len());
    for arg in args {
        let mut bytes = Vec::with_capacity(arg.len() + 1);
        bytes.extend_from_slice(arg.as_bytes());
        bytes.push(0);
        owned_args.push(bytes);
    }

    let launch = PortalLaunch {
        name: portal.name,
        entry: portal.entry,
        args: owned_args,
    };

    let _ = spawner;
    let worker_spawner = pick_portal_spawner().ok_or(LaunchError::NoWorkerSpawner)?;
    let token = linked_portal_task(launch).map_err(map_spawn_error)?;
    worker_spawner.spawn(token);

    Ok(portal)
}

#[inline]
fn pick_portal_spawner() -> Option<SendSpawner> {
    trueos_qjs::workers::pick_background_spawner()
}

#[inline]
fn map_spawn_error(_: SpawnError) -> LaunchError {
    LaunchError::SpawnFailed
}

#[embassy_executor::task(pool_size = 4)]
async fn linked_portal_task(launch: PortalLaunch) {
    crate::log!("portal: start name={} argc={}\n", launch.name, launch.args.len());

    let argv = launch
        .args
        .iter()
        .map(|arg| arg.as_ptr().cast::<c_char>())
        .collect::<Vec<_>>();
    let argv_ptr = if argv.is_empty() {
        core::ptr::null()
    } else {
        argv.as_ptr()
    };

    unsafe { (launch.entry)(argv.len(), argv_ptr) };

    crate::log!("portal: exit name={}\n", launch.name);
}
