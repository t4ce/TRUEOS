use crate::hv::vmcall::DispatchOutcome;

pub(crate) fn prepare(_vm_id: u8) -> Result<(), &'static str> {
    Err("wc3 launcher artifact was not supplied")
}

pub(crate) fn handle_vmcall(_vm_id: u8) -> DispatchOutcome {
    DispatchOutcome::Stop
}