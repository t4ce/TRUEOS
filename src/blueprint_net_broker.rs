use core::sync::atomic::{AtomicBool, Ordering};

use v::vnet as api;

use crate::r::net::VNet;

static VMX_GUEST_NET_BACKEND: AtomicBool = AtomicBool::new(false);

pub(crate) struct BlueprintNetBroker {
    backend: BrokerBackend,
}

enum BrokerBackend {
    #[cfg(feature = "intel-hv")]
    Vmx(VmxBroker),
    LocalVnet(VNet),
}

impl BlueprintNetBroker {
    pub(crate) fn open_primary() -> Option<Self> {
        #[cfg(feature = "intel-hv")]
        if VMX_GUEST_NET_BACKEND.load(Ordering::Acquire) {
            if let Some(vmx) = VmxBroker::open_primary() {
                return Some(Self {
                    backend: BrokerBackend::Vmx(vmx),
                });
            }
        }

        let vnet = VNet::open_primary()?;
        Some(Self {
            backend: BrokerBackend::LocalVnet(vnet),
        })
    }

    pub(crate) fn submit(&self, command: api::Command) -> Result<(), ()> {
        match &self.backend {
            #[cfg(feature = "intel-hv")]
            BrokerBackend::Vmx(vmx) => vmx.submit(command),
            BrokerBackend::LocalVnet(vnet) => vnet.submit(command),
        }
    }

    pub(crate) fn pop_event(&self) -> Option<api::Event> {
        match &self.backend {
            #[cfg(feature = "intel-hv")]
            BrokerBackend::Vmx(vmx) => vmx.pop_event(),
            BrokerBackend::LocalVnet(vnet) => vnet.pop_event(),
        }
    }
}

pub(crate) fn set_vmx_guest_net_backend(enabled: bool) {
    VMX_GUEST_NET_BACKEND.store(enabled, Ordering::Release);
}

#[cfg(feature = "intel-hv")]
struct VmxBroker {
    session_id: u32,
}

#[cfg(feature = "intel-hv")]
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
