//! Window-scoped metadata for native font sprites; no bitmap readback.
use super::{blueprint_owner, SURFACES, ERROR_CONTEXT, ERROR_INVALID, ERROR_NOT_FOUND, ERROR_UI4};
pub use crate::graphics::font::FontMetricsV1 as TrueosUi4FontMetricsV1;

pub(crate) fn metrics(owner: crate::ui4::WindowOwner, window: u32, font: u32, pixels: f32,
    out: &mut TrueosUi4FontMetricsV1) -> i32 {
    if !pixels.is_finite() || !(4.0..=256.0).contains(&pixels) { return ERROR_INVALID; }
    if !SURFACES.lock().iter().any(|surface| surface.owner == owner && surface.render_target == window) {
        return ERROR_NOT_FOUND;
    }
    let Some(face) = crate::intel::gpu_font::GpuFontFace::from_id(font) else { return ERROR_INVALID; };
    let face = face.resolve_optional();
    match crate::graphics::font::terminal_font_metrics(face.registry_name(), face.id() as u32, pixels) {
        Ok(value) => { *out = value; 0 },
        Err(_) => ERROR_UI4,
    }
}

pub unsafe extern "C" fn trueos_cabi_ui4_scene_font_metrics_v1(
    window: u32, font: u32, pixels: f32, out: *mut TrueosUi4FontMetricsV1,
) -> i32 {
    if out.is_null() { return ERROR_INVALID; }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut response = [0u8; core::mem::size_of::<TrueosUi4FontMetricsV1>()];
        let (status, value) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_FONT_METRICS_V1, window as u64, font as u64,
            &pixels.to_le_bytes(), &mut response);
        if status != trueos_vm::vmcall::STATUS_OK { return ERROR_UI4; }
        let result = value as i64 as i32;
        if result == 0 { unsafe { out.write(core::ptr::read_unaligned(response.as_ptr().cast())) }; }
        return result;
    }
    let Some(owner) = blueprint_owner() else { return ERROR_CONTEXT; };
    let mut result = TrueosUi4FontMetricsV1::default();
    let rc = metrics(owner, window, font, pixels, &mut result);
    if rc == 0 { unsafe { out.write(result) }; }
    rc
}
