//! Collapse registration shares the existing bounded, chunked image transport.
use super::*;
// Collapse uses the ordinary upload transport. Keep its transient ID in the
// allowed low-bit namespace used by ordinary Blueprint sprite uploads.
pub(super) const UPLOAD_ID: u32 = 0x7FFF_FFFF;

/// Copy a straight-alpha RGBA8 icon, enabling kernel-owned collapse/restore.
/// Zero extents, images larger than 256x256, and incomplete uploads are rejected.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_enable_collapse_v1(
    window_id: u32,
    width: u32,
    height: u32,
    rgba: *const u8,
    bytes: usize,
) -> i32 {
    if !(1..=crate::ui4::MAX_COLLAPSE_IMAGE_SIZE).contains(&width)
        || !(1..=crate::ui4::MAX_COLLAPSE_IMAGE_SIZE).contains(&height)
        || rgba.is_null()
        || bytes != width as usize * height as usize * 4
    {
        return ERROR_INVALID;
    }
    let pixels = unsafe { core::slice::from_raw_parts(rgba, bytes) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut extent = [0u8; 8];
        extent[..4].copy_from_slice(&width.to_le_bytes());
        extent[4..].copy_from_slice(&height.to_le_bytes());
        let rc = guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SPRITE_UPLOAD_BEGIN,
            window_id as u64,
            UPLOAD_ID as u64,
            &extent,
        );
        if rc != 0 {
            return rc;
        }
        let chunk_size = trueos_vm::vmcall::PAYLOAD_CAP & !3;
        for (index, chunk) in pixels.chunks(chunk_size).enumerate() {
            let rc = guest_status(
                trueos_vm::vmcall::OP_BP_UI4_SCENE_SPRITE_UPLOAD_CHUNK,
                window_id as u64,
                ((UPLOAD_ID as u64) << 32) | (index * chunk_size) as u64,
                chunk,
            );
            if rc != 0 {
                return rc;
            }
        }
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_ENABLE_COLLAPSE_V1,
            window_id as u64,
            0,
            &[],
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let rc = begin_sprite_rgba8_upload(owner, window_id, UPLOAD_ID, width, height);
    if rc != 0 {
        return rc;
    }
    let rc = write_sprite_rgba8_upload_chunk(owner, window_id, UPLOAD_ID, 0, pixels);
    if rc != 0 {
        return rc;
    }
    finish(owner, window_id)
}

pub(crate) fn finish(owner: WindowOwner, window_id: u32) -> i32 {
    if window_id == 0 {
        return finish_terminal(owner);
    }
    let (window, upload) = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        if surface.render_target != surface.window.raw() {
            return ERROR_INVALID;
        }
        (surface.window, surface.sprite_upload.take())
    };
    let Some(upload) = upload else {
        return ERROR_STATE;
    };
    let source = upload.owned.surface;
    let result = if upload.sprite_id != UPLOAD_ID
        || upload.written != upload.packed_len
        || source.width > crate::ui4::MAX_COLLAPSE_IMAGE_SIZE
        || source.height > crate::ui4::MAX_COLLAPSE_IMAGE_SIZE
    {
        ERROR_INVALID
    } else {
        let pixels = unsafe { core::slice::from_raw_parts(upload.owned.virt, upload.owned.bytes) };
        crate::ui4::enable_window_collapse(
            owner,
            window,
            source.width,
            source.height,
            source.pitch_bytes as usize,
            pixels,
        )
        .map(|()| 0)
        .unwrap_or_else(|error| match error {
            crate::ui4::CollapseError::InvalidImage => ERROR_INVALID,
            crate::ui4::CollapseError::NotFound => ERROR_NOT_FOUND,
            crate::ui4::CollapseError::Busy => ERROR_BUSY,
            crate::ui4::CollapseError::Ui4 => ERROR_UI4,
        })
    };
    destroy_rgba8_surface(upload.owned);
    result
}

// Terminal icons use the same bounded chunk transport without allocating a
// Blueprint scene surface. Bind each upload to the current leased window.
struct TerminalIcon {
    owner: WindowOwner,
    window: Option<super::super::WindowId>,
    ready: bool,
    width: u32,
    height: u32,
    pixels: alloc::vec::Vec<u8>,
    written: usize,
}
static TERMINAL_ICONS: spin::Mutex<alloc::vec::Vec<TerminalIcon>> =
    spin::Mutex::new(alloc::vec::Vec::new());

fn terminal_window(owner: WindowOwner) -> Result<super::super::WindowId, i32> {
    let WindowOwner::Vm(vm) = owner else {
        return Err(ERROR_CONTEXT);
    };
    crate::shell3::tui::window_for_vm(vm).ok_or(ERROR_NOT_FOUND)
}

pub(super) fn begin_terminal(owner: WindowOwner, width: u32, height: u32) -> i32 {
    if !(1..=crate::ui4::MAX_COLLAPSE_IMAGE_SIZE).contains(&width)
        || !(1..=crate::ui4::MAX_COLLAPSE_IMAGE_SIZE).contains(&height)
    {
        return ERROR_INVALID;
    }
    if !local_lease(owner) {
        return ERROR_NOT_FOUND;
    }
    let window = None;
    let mut icons = TERMINAL_ICONS.lock();
    if let Some(window) = icons
        .iter()
        .find(|icon| icon.owner == owner)
        .and_then(|icon| icon.window)
    {
        super::super::window_broker::clear_collapse(WindowOwner::SHELL3_SERVICE, window);
    }
    icons.retain(|icon| icon.owner != owner);
    icons.push(TerminalIcon {
        owner,
        window,
        ready: false,
        width,
        height,
        pixels: alloc::vec![0; width as usize * height as usize * 4],
        written: 0,
    });
    0
}

pub(super) fn write_terminal(owner: WindowOwner, offset: usize, bytes: &[u8]) -> i32 {
    let mut icons = TERMINAL_ICONS.lock();
    let Some(icon) = icons.iter_mut().find(|icon| icon.owner == owner) else {
        return ERROR_STATE;
    };
    if !local_lease(owner) {
        return ERROR_NOT_FOUND;
    }
    let Some(end) = offset.checked_add(bytes.len()) else {
        return ERROR_INVALID;
    };
    if bytes.is_empty() || offset != icon.written || bytes.len() & 3 != 0 || end > icon.pixels.len()
    {
        return ERROR_INVALID;
    }
    icon.pixels[offset..end].copy_from_slice(bytes);
    icon.written = end;
    0
}

fn finish_terminal(owner: WindowOwner) -> i32 {
    let mut icons = TERMINAL_ICONS.lock();
    let Some(icon) = icons.iter_mut().find(|icon| icon.owner == owner) else {
        return ERROR_STATE;
    };
    if !local_lease(owner) {
        return ERROR_NOT_FOUND;
    }
    if icon.pixels.is_empty() || icon.written != icon.pixels.len() {
        return ERROR_INVALID;
    }
    icon.ready = true;
    // Selection/presentation can lag the successful lease claim. Retain the
    // uploaded image until Shell3 binds the selected terminal to a UI4 frame.
    icon.window = None;
    install_terminal(icon)
}

fn local_lease(owner: WindowOwner) -> bool {
    let WindowOwner::Vm(vm) = owner else {
        return false;
    };
    crate::shell3::tui::has_local_lease(vm)
}

fn install_terminal(icon: &mut TerminalIcon) -> i32 {
    let Ok(window) = terminal_window(icon.owner) else {
        return 0;
    };
    if !icon.ready || icon.window == Some(window) {
        return 0;
    }
    let rc = crate::ui4::enable_window_collapse(
        WindowOwner::SHELL3_SERVICE,
        window,
        icon.width,
        icon.height,
        icon.width as usize * 4,
        &icon.pixels,
    )
    .map(|()| 0)
    .unwrap_or(ERROR_UI4);
    if rc == 0 {
        if let Some(old) = icon.window.replace(window) {
            super::super::window_broker::clear_collapse(WindowOwner::SHELL3_SERVICE, old);
        }
    }
    rc
}

/// Shell3 calls this after presenting its selected terminal, including a new
/// broker window following a surface replacement. No guest retry is required.
pub(crate) fn refresh_terminal(owner: WindowOwner) {
    let mut icons = TERMINAL_ICONS.lock();
    if let Some(icon) = icons.iter_mut().find(|icon| icon.owner == owner) {
        let _ = install_terminal(icon);
    }
}

pub(crate) fn release_terminal(owner: WindowOwner) {
    let windows = {
        let mut icons = TERMINAL_ICONS.lock();
        let windows: alloc::vec::Vec<_> = icons
            .iter()
            .filter(|icon| icon.owner == owner)
            .filter_map(|icon| icon.window)
            .collect();
        icons.retain(|icon| icon.owner != owner);
        windows
    };
    for window in windows {
        super::super::window_broker::clear_collapse(WindowOwner::SHELL3_SERVICE, window);
    }
}

/// Collapse an opted-in frame. Window zero means the caller's leased terminal.
/// This is idempotent, so repeated UI activation cannot accidentally restore it.
pub extern "C" fn trueos_cabi_ui4_scene_collapse_v1(window_id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_ENABLE_COLLAPSE_V1,
            window_id as u64,
            1,
            &[],
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    collapse(owner, window_id)
}

pub(crate) fn collapse(owner: WindowOwner, window_id: u32) -> i32 {
    let (owner, window) = if window_id == 0 {
        let window = match terminal_window(owner) {
            Ok(window) => window,
            Err(rc) => return rc,
        };
        // Only the terminal owner which registered this icon may activate it.
        if !TERMINAL_ICONS
            .lock()
            .iter()
            .any(|icon| icon.owner == owner && icon.window == Some(window))
        {
            return ERROR_STATE;
        }
        (WindowOwner::SHELL3_SERVICE, window)
    } else {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        if surface.render_target != surface.window.raw() {
            return ERROR_INVALID;
        }
        (owner, surface.window)
    };
    match super::super::window_broker::collapse_state(owner, window) {
        Some(true) => 0,
        Some(false) => super::super::window_broker::toggle_collapse(owner, window)
            .map(|()| 0)
            .unwrap_or(ERROR_UI4),
        None => ERROR_STATE,
    }
}
