// trueos-vm: thin vmcall-only guest binary (no_std, no heap).
// Conceptually a QJS VM inside a VMX-style hypervisor.
// QJS lives entirely on the host side, communicated via vmcall CommPage ABI.
// Legacy demo path is silent by default; enable with feature = "legacy-demo".
// guest boots -> runtime::start() -> vmcall net_tcp_write("VMRUNTIME: ready") -> host QJS takes over.

use crate::vmcall;
use crate::vpanic;

pub fn start() {
    vpanic::set_stage(0x2000);
    net_line("VMRUNTIME: ready");

    #[cfg(feature = "legacy-demo")]
    {
        crate::demo::start();
    }
}

fn net_line(text: &str) {
    let _ = vmcall::net_tcp_write(text.as_bytes());
    let _ = vmcall::net_tcp_write(b"\r\n");
}
