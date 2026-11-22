use super::{Owner, ROUTES, Registry, Route};
use crate::ui4::blueprint_ui4::TrueosUi4ContextMenuEvent;
use crate::ui4::{ContextMenuEntry, ContextMenuRequest, ContextMenuResult, WindowId};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

static NEXT_CONTEXT: AtomicU64 = AtomicU64::new(1);

pub(super) struct Registration {
    owner: Owner,
    context: u64,
    entries: Vec<ContextMenuEntry>,
    pending: Option<TrueosUi4ContextMenuEvent>,
}

pub(crate) fn register(vm: u8, entries: Vec<ContextMenuEntry>) -> Result<(), ()> {
    let run = crate::hv::vm_run_generation(vm).ok_or(())?;
    let owner = Owner { vm, run };
    let mut routes = ROUTES.lock();
    let route = routes
        .routes
        .iter_mut()
        .find(|route| route.owner == Some(owner))
        .ok_or(())?;
    route.context_menu = (!entries.is_empty()).then(|| Registration {
        owner,
        context: NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed),
        entries,
        pending: None,
    });
    Ok(())
}

pub(crate) fn take(vm: u8) -> Result<Option<TrueosUi4ContextMenuEvent>, ()> {
    let run = crate::hv::vm_run_generation(vm).ok_or(())?;
    let owner = Owner { vm, run };
    let mut routes = ROUTES.lock();
    let route = routes
        .routes
        .iter_mut()
        .find(|route| route.owner == Some(owner))
        .ok_or(())?;
    Ok(route
        .context_menu
        .as_mut()
        .filter(|menu| menu.owner == owner)
        .and_then(|menu| menu.pending.take()))
}

fn selected_on_window(routes: &Registry, window: WindowId) -> Option<usize> {
    let frontend = routes.ui4_windows.iter().find(|entry| entry.1 == window)?.0;
    routes
        .routes
        .iter()
        .position(|route| route.selected && route.frontend == Some(frontend))
}

fn live_registration(route: &Route) -> Option<&Registration> {
    let owner = route.owner?;
    let menu = route.context_menu.as_ref()?;
    (menu.owner == owner && crate::hv::vm_run_generation(owner.vm) == Some(owner.run))
        .then_some(menu)
}

pub(crate) fn request(window: WindowId) -> Option<ContextMenuRequest> {
    let routes = ROUTES.lock();
    let route = &routes.routes[selected_on_window(&routes, window)?];
    let menu = live_registration(route)?;
    Some(ContextMenuRequest {
        dynamic: false,
        entries: menu.entries.clone(),
        context: menu.context,
        callback: complete,
    })
}

fn complete(result: ContextMenuResult) {
    let Some(action_id) = result.selected_action else {
        return;
    };
    let mut routes = ROUTES.lock();
    let Some(index) = selected_on_window(&routes, result.window) else {
        return;
    };
    let route = &mut routes.routes[index];
    if live_registration(route).is_some_and(|menu| menu.context == result.context)
        && let Some(menu) = &mut route.context_menu
    {
        menu.pending = Some(TrueosUi4ContextMenuEvent {
            context: result.context,
            action_id,
            selected: 1,
            reason: 0,
        });
    }
}
