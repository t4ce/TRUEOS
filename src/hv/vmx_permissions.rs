//! Operation tags for the two Blueprint entry paths into the kernel.

use super::vmcall;
use trueos_crypto::VmxPermission;

// The same geometry operation must carry the same tag through either entry.
// All other requests, including new calls, take the High default.
macro_rules! tag_geometry_operations {
    ($($opcode:ident, $symbol:literal => $permission:ident;)*) => {
        pub(super) const fn for_vmcall(op: u32) -> VmxPermission {
            match op {
                $(vmcall::$opcode => VmxPermission::$permission,)*
                _ => VmxPermission::Other,
            }
        }

        pub(super) fn for_import(name: &str) -> VmxPermission {
            match name {
                $($symbol => VmxPermission::$permission,)*
                _ => VmxPermission::Other,
            }
        }
    };
}

tag_geometry_operations! {
    OP_BP_UI4_SCENE_FRAME_SET_POSITION, "trueos_cabi_ui4_scene_frame_set_position" => MoveWindow;
    OP_BP_UI4_SCENE_FRAME_RESIZE, "trueos_cabi_ui4_scene_frame_resize" => ResizeWindow;
}
