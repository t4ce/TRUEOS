use core::sync::atomic::{AtomicBool, Ordering};

use v::vnet as api;

use crate::r::net::VNet;

static VMX_GUEST_NET_BACKEND: AtomicBool = AtomicBool::new(false);

// One VNet bridge for compatibility code: local kernel clients use `VNet` directly,
// VM guests cross the blueprint-net wire protocol and land back on host `VNet`.
pub(crate) struct VNetBridge {
    backend: VNetBridgeBackend,
}

enum VNetBridgeBackend {
    Vmx(VmxBroker),
    LocalVnet(VNet),
}

impl VNetBridge {
    pub(crate) fn open_primary() -> Option<Self> {
        if VMX_GUEST_NET_BACKEND.load(Ordering::Acquire)
            && crate::hv::current_hull_guest_context_vm_id().is_some()
        {
            if let Some(vmx) = VmxBroker::open_primary() {
                return Some(Self {
                    backend: VNetBridgeBackend::Vmx(vmx),
                });
            }
        }

        // Host-carried VM work is already running in VMX root. It keeps VM
        // vthread identity for ownership, but must use the host VNet broker
        // directly instead of issuing a guest vmcall.
        let vnet = VNet::open_primary()?;
        Some(Self {
            backend: VNetBridgeBackend::LocalVnet(vnet),
        })
    }

    pub(crate) fn submit(&self, command: api::Command) -> Result<(), ()> {
        match &self.backend {
            VNetBridgeBackend::Vmx(vmx) => vmx.submit(command),
            VNetBridgeBackend::LocalVnet(vnet) => vnet.submit(command),
        }
    }

    pub(crate) fn pop_event(&self) -> Option<api::Event> {
        match &self.backend {
            VNetBridgeBackend::Vmx(vmx) => vmx.pop_event(),
            VNetBridgeBackend::LocalVnet(vnet) => vnet.pop_event(),
        }
    }
}

pub(crate) fn set_vmx_guest_net_backend(enabled: bool) {
    VMX_GUEST_NET_BACKEND.store(enabled, Ordering::Release);
}

struct VmxBroker {
    session_id: u32,
}

impl VmxBroker {
    fn open_primary() -> Option<Self> {
        let (status, session_id) = trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_NET_OPEN, 0, 0);
        if status != trueos_vm::vmcall::STATUS_OK || session_id == 0 {
            return None;
        }
        Some(Self {
            session_id: session_id as u32,
        })
    }

    fn submit(&self, command: api::Command) -> Result<(), ()> {
        let mut request = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        let len =
            crate::blueprint_net_wire::encode_command(command, &mut request).map_err(|_| ())?;

        // The bridge is created by the Hull and therefore owns a VMX session,
        // but Tokio may later poll it from a host-carried worker. Such a worker
        // retains the Blueprint VM identity while running in VMX root and must
        // enter the same host session directly; executing `vmcall` there is
        // invalid. Keeping the original session also preserves socket/event
        // ordering across Hull and worker execution realms.
        if crate::hv::current_hull_guest_context_vm_id().is_none() {
            let vm_id = crate::hv::current_guest_execution_context_vm_id().ok_or(())?;
            return crate::hv::blueprint_net::submit(vm_id, self.session_id, &request[..len]);
        }

        let mut response = [0u8; 1];
        let (status, _) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_NET_SUBMIT,
            self.session_id as u64,
            0,
            &request[..len],
            &mut response,
        );
        if status == trueos_vm::vmcall::STATUS_OK {
            Ok(())
        } else {
            Err(())
        }
    }

    fn pop_event(&self) -> Option<api::Event> {
        let mut response = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];

        if crate::hv::current_hull_guest_context_vm_id().is_none() {
            let vm_id = crate::hv::current_guest_execution_context_vm_id()?;
            let len = crate::hv::blueprint_net::poll_event(vm_id, self.session_id, &mut response)
                .ok()??;
            return crate::blueprint_net_wire::decode_event(&response[..len]).ok();
        }

        let (status, has_event) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_NET_POLL,
            self.session_id as u64,
            0,
            &[],
            &mut response,
        );
        if status != trueos_vm::vmcall::STATUS_OK || has_event == 0 {
            return None;
        }
        crate::blueprint_net_wire::decode_event(&response).ok()
    }
}
