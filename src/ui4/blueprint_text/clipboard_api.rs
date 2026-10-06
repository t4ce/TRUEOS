//! Typed clipboard adapter. Polling drains only previously authorized push events.
use super::*;
use crate::r::services::clipboard_service as c;
use alloc::sync::Arc;

struct Sink(Mutex<Option<Result<c::ClipboardDelivery, c::ClipboardError>>>);
impl c::ClipboardDeliverySink for Sink {
    fn on_paste(&self, event: c::ClipboardPasteEvent) -> Result<(), c::ClipboardSinkError> {
        let mut pending = self.0.lock();
        if pending.is_some() {
            return Err(c::ClipboardSinkError::Busy);
        }
        *pending = Some(Ok(event.into_delivery()));
        Ok(())
    }
}
struct Gate {
    owner: WindowOwner,
    window: WindowId,
    lease: crate::shell2::MatrixSlotLease,
    kind: c::ClipboardKind,
    sink: Arc<Sink>,
}
static GATES: Mutex<Vec<Gate>> = Mutex::new(Vec::new());

pub(crate) fn release_owner(owner: WindowOwner) {
    release_matching(owner, None);
}
pub(super) fn release_window(owner: WindowOwner, window: WindowId) {
    release_matching(owner, Some(window));
}
fn release_matching(owner: WindowOwner, window: Option<WindowId>) {
    let removed = {
        let mut gates = GATES.lock();
        let mut removed = Vec::new();
        let mut i = 0;
        while i < gates.len() {
            if gates[i].owner == owner && window.is_none_or(|w| gates[i].window == w) {
                removed.push(gates.remove(i));
            } else {
                i += 1;
            }
        }
        removed
    };
    if let WindowOwner::Vm(vm) = owner {
        for gate in removed {
            c::close_delivery_gate(&gate.lease, c::ClipboardPrincipal::Blueprint(vm));
        }
    }
}

pub(crate) fn trusted_paste(owner: WindowOwner, window: WindowId) {
    let WindowOwner::Vm(vm) = owner else {
        return;
    };
    let selected = {
        let gates = GATES.lock();
        gates
            .iter()
            .find(|g| g.owner == owner && g.window == window)
            .map(|g| (g.lease.clone(), Arc::clone(&g.sink)))
    };
    let Some((lease, sink)) = selected else {
        return;
    };
    // UI4 keyboard input is local; the guest cannot supply this auth scope.
    if let Err(error) =
        c::dispatch_trusted_paste(c::TrustedClipboardPasteIntent::from_kernel_input(
            lease,
            c::ClipboardPrincipal::Blueprint(vm),
            crate::shell2::TRANSPORT_LOCAL_SCOPE,
        ))
    {
        let mut pending = sink.0.lock();
        if pending.is_none() {
            *pending = Some(Err(error));
        }
    }
}

/// Commands: 1 publish, 2 focus typed gate (kind 0 closes), 3 take delivered event.
/// Kinds: 1 plain text, 2 password. No read or paste-request command exists.
pub unsafe extern "C" fn trueos_cabi_clipboard_command_v1(
    window_id: u32,
    command: u32,
    kind: u32,
    input: *const u8,
    input_len: usize,
    output: *mut u8,
    output_cap: usize,
) -> i32 {
    if input_len > 512
        || output_cap > 512
        || (input_len != 0 && input.is_null())
        || (output_cap != 0 && output.is_null())
    {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let request = if input_len == 0 {
            &[][..]
        } else {
            unsafe { core::slice::from_raw_parts(input, input_len) }
        };
        let mut response = zeroize::Zeroizing::new([0u8; 512]);
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_CLIPBOARD_COMMAND_V1,
            window_id as u64,
            command as u64 | ((kind as u64) << 32),
            request,
            &mut response[..output_cap],
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return ERROR_UI4;
        }
        let rc = data as i64 as i32;
        if rc > 0 {
            if rc as usize > output_cap {
                return ERROR_INVALID;
            }
            unsafe {
                core::ptr::copy_nonoverlapping(response.as_ptr(), output, rc as usize);
            }
        }
        return rc;
    }
    let Some(owner @ WindowOwner::Vm(vm)) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let window = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        surface.window
    };
    let principal = c::ClipboardPrincipal::Blueprint(vm);
    let Some(target) = crate::hv::blueprint_console_target(vm) else {
        return c::ClipboardError::MatrixSlotExpired.code();
    };
    let lease = crate::shell2::matrix_target_slot_lease(&target);
    let selected_kind = match kind {
        1 => c::ClipboardKind::Text,
        2 => c::ClipboardKind::Password,
        0 if command == 2 => {
            release_owner(owner);
            return 0;
        }
        _ => return ERROR_INVALID,
    };
    match command {
        1 => {
            let bytes = if input_len == 0 {
                &[][..]
            } else {
                unsafe { core::slice::from_raw_parts(input, input_len) }
            };
            let Ok(text) = core::str::from_utf8(bytes) else {
                return ERROR_INVALID;
            };
            let Some(scope) = crate::shell2::matrix_target_auth_scope(&target) else {
                return c::ClipboardError::AuthenticationRequired.code();
            };
            let published = if selected_kind == c::ClipboardKind::Password {
                c::ClipboardPublish::Password(text)
            } else {
                c::ClipboardPublish::Text(text)
            };
            c::publish_from_blueprint(principal, published, scope).map_or_else(|e| e.code(), |_| 0)
        }
        2 => {
            {
                let gates = GATES.lock();
                if gates.iter().any(|g| {
                    g.owner == owner
                        && g.window == window
                        && g.kind == selected_kind
                        && g.lease == lease
                }) {
                    return 0;
                }
            }
            release_owner(owner);
            let sink = Arc::new(Sink(Mutex::new(None)));
            if let Err(e) = c::place_delivery_gate(
                lease.clone(),
                principal,
                c::ClipboardGateSpec::new(selected_kind, c::ClipboardGateLifetime::Persistent),
                sink.clone(),
            ) {
                return e.code();
            }
            GATES.lock().push(Gate {
                owner,
                window,
                lease,
                kind: selected_kind,
                sink,
            });
            0
        }
        3 => {
            let gates = GATES.lock();
            let Some(gate) = gates.iter().find(|g| {
                g.owner == owner
                    && g.window == window
                    && g.kind == selected_kind
                    && g.lease == lease
            }) else {
                return c::ClipboardError::GateNotFound.code();
            };
            if !crate::shell2::matrix_slot_is_live(&gate.lease) {
                return c::ClipboardError::MatrixSlotExpired.code();
            }
            let mut pending = gate.sink.0.lock();
            let Some(event) = pending.as_ref() else {
                return 0;
            };
            let text = match event {
                Err(e) => {
                    let rc = e.code();
                    pending.take();
                    return rc;
                }
                Ok(c::ClipboardDelivery::Text(s)) => s.as_str(),
                Ok(c::ClipboardDelivery::Password(s)) => s.expose_secret(),
                _ => return ERROR_INVALID,
            };
            if text.len() > output_cap {
                return ERROR_INVALID;
            }
            let len = text.len();
            if len != 0 {
                unsafe {
                    core::ptr::copy_nonoverlapping(text.as_ptr(), output, len);
                }
            }
            pending.take();
            len as i32
        }
        _ => ERROR_INVALID,
    }
}
