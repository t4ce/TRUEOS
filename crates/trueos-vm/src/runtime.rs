use core::ffi::c_char;

use crate::vmcall;
use crate::vpanic;

const BOOT_FILENAME: &[u8] = b"<trueos-vm-qjs-boot>\0";
const BOOT_SOURCE: &[u8] = b"globalThis.__trueosVmBoot = 'ok';";

pub fn start() {
    vpanic::set_stage(0x2000);
    net_line("VMRUNTIME: qjs wrapper begin");

    unsafe {
        let Some(vm) = trueos_qjs::vm::QjsVm::new_node() else {
            vpanic::set_stage(0x2001);
            net_line("VMRUNTIME: qjs runtime init failed");
            return;
        };

        let rt = vm.rt_ptr();
        let ctx = vm.ctx_ptr();

        vpanic::set_stage(0x2002);
        trueos_qjs::node::install_globals(ctx);

        vpanic::set_stage(0x2003);
        let boot = trueos_qjs::js_eval_bytes(
            ctx,
            BOOT_SOURCE,
            BOOT_FILENAME.as_ptr() as *const c_char,
            trueos_qjs::JS_EVAL_TYPE_GLOBAL,
        );
        if boot.is_exception() {
            vpanic::set_stage(0x2004);
            trueos_qjs::qjs_diag::dump_last_exception(ctx, "trueos-vm qjs boot");
            trueos_qjs::js_free_value(ctx, boot);
            net_line("VMRUNTIME: qjs boot eval exception");
            return;
        }
        trueos_qjs::js_free_value(ctx, boot);

        // Drain one turn so immediate startup jobs/promises can run before preserve.
        let _ = trueos_qjs::vm::pump_runtime_once(rt, ctx, "trueos-vm-qjs-wrapper");
    }

    vpanic::set_stage(0x2005);
    net_line("VMRUNTIME: qjs wrapper ready");

    #[cfg(feature = "legacy-demo")]
    {
        crate::demo::start();
    }
}

fn net_line(text: &str) {
    let _ = vmcall::net_tcp_write(text.as_bytes());
    let _ = vmcall::net_tcp_write(b"\r\n");
}
