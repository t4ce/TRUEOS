use alloc::string::String as AllocString;
use alloc::vec::Vec as AllocVec;
use core::fmt::Write;
use core::sync::atomic::{AtomicU32, Ordering};

use heapless::{String, Vec as HVec};
use spin::Mutex;

use super::{
    HV_LOG_LINE, TRUEOS_VM_ID_LIMIT, current_guest_execution_context_vm_id,
    current_hull_guest_context_vm_id, current_vm_id, hvlogf,
};

const APP_WINDOW_DEFERRED_ID_FLAG: u32 = 0x8000_0000;
const APP_WINDOW_DEFERRED_VM_SHIFT: u32 = 24;
const APP_WINDOW_DEFERRED_SEQ_MASK: u32 = 0x00FF_FFFF;
const APP_WINDOW_DEFERRED_CAP: usize = 16;
const APP_WINDOW_PENDING_CLOSE_CAP: usize = 32;
const APP_WINDOW_TITLE_CAP: usize = 96;
const APP_WINDOW_DEFERRED_KIND_PLAIN: u8 = 1;
const APP_WINDOW_DEFERRED_KIND_SURFACE: u8 = 2;
const APP_WINDOW_VMCALL_CREATE_HEADER: usize = 40;
const APP_WINDOW_VMCALL_OP_HEADER: usize = 16;
const APP_WINDOW_VMCALL_OP_REQUEST_REPAINT: u32 = 1;
const APP_WINDOW_VMCALL_OP_CLOSE: u32 = 2;
const APP_WINDOW_VMCALL_OP_TITLE: u32 = 3;
const APP_WINDOW_VMCALL_OP_POSITION: u32 = 4;
const APP_WINDOW_VMCALL_OP_SIZE: u32 = 5;
const APP_WINDOW_VMCALL_OP_U32: u32 = 6;
const APP_WINDOW_VMCALL_OP_BOOL: u32 = 7;
const APP_WINDOW_VMCALL_OP_BUTTON_VISIBLE: u32 = 8;

static APP_WINDOW_DEFERRED_NEXT_ID_BY_VM: [AtomicU32; TRUEOS_VM_ID_LIMIT] =
    [const { AtomicU32::new(0) }; TRUEOS_VM_ID_LIMIT];
static APP_WINDOW_SESSIONS: [Mutex<Option<AppWindowSession>>; TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; TRUEOS_VM_ID_LIMIT];
static APP_WINDOW_DEFERRED_RECORDS: [Mutex<[DeferredAppWindowRecord; APP_WINDOW_DEFERRED_CAP]>;
    TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new([const { DeferredAppWindowRecord::empty() }; APP_WINDOW_DEFERRED_CAP]) };
        TRUEOS_VM_ID_LIMIT];
static APP_WINDOW_PENDING_CLOSES: [Mutex<HVec<u32, APP_WINDOW_PENDING_CLOSE_CAP>>;
    TRUEOS_VM_ID_LIMIT] = [const { Mutex::new(HVec::new()) }; TRUEOS_VM_ID_LIMIT];

#[derive(Clone)]
struct AppWindowSession {
    archive: AllocString,
    window_ids: AllocVec<u32>,
}

#[derive(Clone)]
struct DeferredAppWindowRecord {
    active: bool,
    materialized_window_id: u32,
    deferred_id: u32,
    kind: u8,
    title: String<APP_WINDOW_TITLE_CAP>,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    z: i32,
    alpha: u32,
    tex_id: u32,
    blend_enabled: bool,
    icon_id: Option<u32>,
    decoration_mode: Option<u32>,
    titlebar_visible: Option<bool>,
    bottom_bar_visible: Option<bool>,
    title_icon_visible: Option<bool>,
    decoration_button_visible: [Option<bool>; crate::r::ui2::Ui2WindowDecorationButton::COUNT],
    resize_button_visible: Option<bool>,
    hit_test_visible: Option<bool>,
    vertical_scrollbar_visible: Option<bool>,
    horizontal_scrollbar_visible: Option<bool>,
    vertical_scrollbar_side: Option<u32>,
    horizontal_scrollbar_side: Option<u32>,
    rotate_buttons_visible: Option<bool>,
    content_rotation_quadrants: Option<u32>,
    resize_maintain_aspect: Option<bool>,
    content_preserve_scale: Option<bool>,
    resize_mode: Option<u32>,
    repaint_requested: bool,
    close_requested: bool,
    cached_info: Option<crate::r::ui2::TrueosUi2WindowInfo>,
}

impl DeferredAppWindowRecord {
    const MATERIALIZING: u32 = u32::MAX;

    const fn empty() -> Self {
        Self {
            active: false,
            materialized_window_id: 0,
            deferred_id: 0,
            kind: 0,
            title: String::new(),
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            z: 0,
            alpha: 0,
            tex_id: 0,
            blend_enabled: false,
            icon_id: None,
            decoration_mode: None,
            titlebar_visible: None,
            bottom_bar_visible: None,
            title_icon_visible: None,
            decoration_button_visible: [None; crate::r::ui2::Ui2WindowDecorationButton::COUNT],
            resize_button_visible: None,
            hit_test_visible: None,
            vertical_scrollbar_visible: None,
            horizontal_scrollbar_visible: None,
            vertical_scrollbar_side: None,
            horizontal_scrollbar_side: None,
            rotate_buttons_visible: None,
            content_rotation_quadrants: None,
            resize_maintain_aspect: None,
            content_preserve_scale: None,
            resize_mode: None,
            repaint_requested: false,
            close_requested: false,
            cached_info: None,
        }
    }
}

fn app_window_broker_log(args: core::fmt::Arguments<'_>) {
    let mut line: String<HV_LOG_LINE> = String::new();
    let _ = line.write_fmt(args);
    if line.is_empty() {
        return;
    }

    hvlogf(format_args!("{}", line.as_str()));
}

pub fn log_blueprint_app_window_event(args: core::fmt::Arguments<'_>) {
    app_window_broker_log(args);
}

pub fn app_window_session_archive(vm_id: u8) -> Option<AllocString> {
    let session_lock = APP_WINDOW_SESSIONS.get(vm_id as usize)?;
    session_lock
        .lock()
        .as_ref()
        .map(|session| session.archive.clone())
}

fn close_or_defer_app_window(vm_id: u8, window_id: u32, reason: &'static str) {
    if current_guest_execution_context_vm_id().is_none() {
        let _ = crate::r::ui2::close_window(window_id);
        return;
    }

    let Some(pending_lock) = APP_WINDOW_PENDING_CLOSES.get(vm_id as usize) else {
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} close defer failed window={} reason={} status=unsupported-vm",
            vm_id, window_id, reason
        ));
        return;
    };

    let mut pending = pending_lock.lock();
    if pending.contains(&window_id) {
        return;
    }
    if pending.push(window_id).is_ok() {
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} deferred close window={} reason={} status=queued",
            vm_id, window_id, reason
        ));
    } else {
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} deferred close window={} reason={} status=full cap={}",
            vm_id, window_id, reason, APP_WINDOW_PENDING_CLOSE_CAP
        ));
    }
}

fn drain_deferred_app_window_closes(vm_id: u8) -> usize {
    let Some(pending_lock) = APP_WINDOW_PENDING_CLOSES.get(vm_id as usize) else {
        return 0;
    };
    let mut pending_windows: HVec<u32, APP_WINDOW_PENDING_CLOSE_CAP> = HVec::new();
    {
        let mut pending = pending_lock.lock();
        for window_id in pending.iter().copied() {
            let _ = pending_windows.push(window_id);
        }
        pending.clear();
    }

    let mut closed = 0usize;
    for window_id in pending_windows {
        let _ = crate::r::ui2::close_window(window_id);
        closed += 1;
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} deferred close window={} status=closed",
            vm_id, window_id
        ));
    }
    closed
}

fn deferred_app_window_kind_name(kind: u8) -> &'static str {
    match kind {
        APP_WINDOW_DEFERRED_KIND_PLAIN => "plain",
        APP_WINDOW_DEFERRED_KIND_SURFACE => "surface",
        _ => "unknown",
    }
}

fn deferred_app_window_kind(kind: &str) -> u8 {
    match kind {
        "plain" => APP_WINDOW_DEFERRED_KIND_PLAIN,
        "surface" => APP_WINDOW_DEFERRED_KIND_SURFACE,
        _ => 0,
    }
}

fn seed_deferred_surface_texture(tex_id: u32, host_tex_id: u32, width: u32, height: u32) -> bool {
    if tex_id == 0 || width == 0 || height == 0 {
        return false;
    }
    let upload_tex_id = if host_tex_id != 0 { host_tex_id } else { tex_id };
    if host_tex_id != 0 && crate::r::io::cabi::host_texture_has_image(host_tex_id) {
        return true;
    }
    let Some(pixel_count) = (width as usize).checked_mul(height as usize) else {
        return false;
    };
    let Some(byte_len) = pixel_count.checked_mul(4) else {
        return false;
    };
    let mut pixels = AllocVec::with_capacity(byte_len);
    for _ in 0..pixel_count {
        pixels.extend_from_slice(&[0x08, 0x0C, 0x12, 0xFF]);
    }
    if host_tex_id != 0 {
        crate::r::io::cabi::queue_host_texture_rgba_image_upload_owned(
            upload_tex_id,
            width,
            height,
            pixels,
            0,
            "vm-surface-init",
        )
    } else {
        crate::r::io::cabi::queue_texture_rgba_image_upload_owned(
            upload_tex_id,
            width,
            height,
            pixels,
            0,
            "vm-surface-init",
        )
    }
}

fn truncate_deferred_app_window_title(title: &str) -> String<APP_WINDOW_TITLE_CAP> {
    let mut out = String::new();
    for ch in title.chars() {
        if out.push(ch).is_err() {
            break;
        }
    }
    out
}

fn app_window_payload_put_u32(payload: &mut [u8], offset: usize, value: u32) {
    payload[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn app_window_payload_put_i32(payload: &mut [u8], offset: usize, value: i32) {
    payload[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn app_window_payload_get_u32(payload: &[u8], offset: usize) -> Option<u32> {
    payload
        .get(offset..offset + 4)
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn app_window_payload_get_i32(payload: &[u8], offset: usize) -> Option<i32> {
    payload
        .get(offset..offset + 4)
        .map(|bytes| i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn app_window_guest_create_vmcall(
    vm_id: u8,
    kind_id: u8,
    title: &str,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    z: i32,
    alpha: u32,
    tex_id: u32,
    blend_enabled: bool,
) -> u32 {
    let title = truncate_deferred_app_window_title(title);
    let title_bytes = title.as_bytes();
    let title_len = title_bytes
        .len()
        .min(trueos_vm::vmcall::PAYLOAD_CAP.saturating_sub(APP_WINDOW_VMCALL_CREATE_HEADER));
    let mut payload = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
    app_window_payload_put_u32(&mut payload, 0, kind_id as u32);
    app_window_payload_put_i32(&mut payload, 4, x);
    app_window_payload_put_i32(&mut payload, 8, y);
    app_window_payload_put_u32(&mut payload, 12, width);
    app_window_payload_put_u32(&mut payload, 16, height);
    app_window_payload_put_i32(&mut payload, 20, z);
    app_window_payload_put_u32(&mut payload, 24, alpha);
    app_window_payload_put_u32(&mut payload, 28, tex_id);
    app_window_payload_put_u32(&mut payload, 32, u32::from(blend_enabled));
    app_window_payload_put_u32(&mut payload, 36, title_len as u32);
    payload[APP_WINDOW_VMCALL_CREATE_HEADER..APP_WINDOW_VMCALL_CREATE_HEADER + title_len]
        .copy_from_slice(&title_bytes[..title_len]);

    let mut out = [0u8; 0];
    let (status, window_id) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI2_WINDOW_CREATE,
        vm_id as u64,
        0,
        &payload[..APP_WINDOW_VMCALL_CREATE_HEADER + title_len],
        &mut out,
    );
    if status == trueos_vm::vmcall::STATUS_OK {
        window_id as u32
    } else {
        0
    }
}

fn app_window_guest_op_vmcall(
    window_id: u32,
    op_code: u32,
    a: u32,
    b: u32,
    text: Option<&str>,
) -> bool {
    let Some(vm_id) = deferred_blueprint_app_window_vm_id(window_id) else {
        return false;
    };
    if current_hull_guest_context_vm_id() != Some(vm_id) {
        return false;
    }
    let text_bytes = text.unwrap_or("").as_bytes();
    let text_len = text_bytes
        .len()
        .min(trueos_vm::vmcall::PAYLOAD_CAP.saturating_sub(APP_WINDOW_VMCALL_OP_HEADER));
    let mut payload = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
    app_window_payload_put_u32(&mut payload, 0, op_code);
    app_window_payload_put_u32(&mut payload, 4, a);
    app_window_payload_put_u32(&mut payload, 8, b);
    app_window_payload_put_u32(&mut payload, 12, text_len as u32);
    payload[APP_WINDOW_VMCALL_OP_HEADER..APP_WINDOW_VMCALL_OP_HEADER + text_len]
        .copy_from_slice(&text_bytes[..text_len]);

    let mut out = [0u8; 0];
    let (status, rc) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI2_WINDOW_OP,
        window_id as u64,
        0,
        &payload[..APP_WINDOW_VMCALL_OP_HEADER + text_len],
        &mut out,
    );
    status == trueos_vm::vmcall::STATUS_OK && rc == 0
}

fn deferred_app_window_u32_op_name(op: u32) -> Option<&'static str> {
    match op {
        1 => Some("set-icon"),
        2 => Some("set-decorations"),
        3 => Some("set-vertical-scrollbar-side"),
        4 => Some("set-horizontal-scrollbar-side"),
        5 => Some("set-content-rotation"),
        6 => Some("set-resize-mode"),
        _ => None,
    }
}

fn deferred_app_window_bool_op_name(op: u32) -> Option<&'static str> {
    match op {
        1 => Some("set-titlebar-visible"),
        2 => Some("set-bottom-bar-visible"),
        3 => Some("set-title-icon-visible"),
        4 => Some("set-resize-button-visible"),
        5 => Some("set-hit-test-visible"),
        6 => Some("set-vertical-scrollbar-visible"),
        7 => Some("set-horizontal-scrollbar-visible"),
        8 => Some("set-rotate-buttons-visible"),
        9 => Some("set-resize-maintain-aspect"),
        10 => Some("set-content-preserve-scale"),
        _ => None,
    }
}

fn deferred_app_window_u32_op_code(op: &'static str) -> u32 {
    match op {
        "set-icon" => 1,
        "set-decorations" => 2,
        "set-vertical-scrollbar-side" => 3,
        "set-horizontal-scrollbar-side" => 4,
        "set-content-rotation" => 5,
        "set-resize-mode" => 6,
        _ => 0,
    }
}

fn deferred_app_window_bool_op_code(op: &'static str) -> u32 {
    match op {
        "set-titlebar-visible" => 1,
        "set-bottom-bar-visible" => 2,
        "set-title-icon-visible" => 3,
        "set-resize-button-visible" => 4,
        "set-hit-test-visible" => 5,
        "set-vertical-scrollbar-visible" => 6,
        "set-horizontal-scrollbar-visible" => 7,
        "set-rotate-buttons-visible" => 8,
        "set-resize-maintain-aspect" => 9,
        "set-content-preserve-scale" => 10,
        _ => 0,
    }
}

pub fn handle_ui2_window_create_vmcall(vm_id: u8, payload: &[u8]) -> Result<u32, ()> {
    if payload.len() < APP_WINDOW_VMCALL_CREATE_HEADER {
        return Err(());
    }
    let kind_id = app_window_payload_get_u32(payload, 0).ok_or(())? as u8;
    let x = app_window_payload_get_i32(payload, 4).ok_or(())?;
    let y = app_window_payload_get_i32(payload, 8).ok_or(())?;
    let width = app_window_payload_get_u32(payload, 12).ok_or(())?;
    let height = app_window_payload_get_u32(payload, 16).ok_or(())?;
    let z = app_window_payload_get_i32(payload, 20).ok_or(())?;
    let alpha = app_window_payload_get_u32(payload, 24).ok_or(())?;
    let tex_id = app_window_payload_get_u32(payload, 28).ok_or(())?;
    let blend_enabled = app_window_payload_get_u32(payload, 32).ok_or(())? != 0;
    let title_len = app_window_payload_get_u32(payload, 36).ok_or(())? as usize;
    let title_bytes = payload
        .get(APP_WINDOW_VMCALL_CREATE_HEADER..APP_WINDOW_VMCALL_CREATE_HEADER + title_len)
        .ok_or(())?;
    let title = core::str::from_utf8(title_bytes).map_err(|_| ())?;
    let kind = deferred_app_window_kind_name(kind_id);
    if kind == "unknown" {
        return Err(());
    }
    let id = defer_blueprint_app_window_create(
        kind,
        title,
        x,
        y,
        width,
        height,
        z,
        alpha,
        tex_id,
        blend_enabled,
    );
    if id == 0 {
        Err(())
    } else {
        let _ = materialize_deferred_blueprint_app_windows(vm_id);
        Ok(id)
    }
}

pub fn handle_ui2_window_op_vmcall(vm_id: u8, window_id: u32, payload: &[u8]) -> Result<i32, ()> {
    if payload.len() < APP_WINDOW_VMCALL_OP_HEADER {
        return Err(());
    }
    if deferred_blueprint_app_window_vm_id(window_id) != Some(vm_id) {
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} deferred vmcall op rejected window={} status=vm-mismatch",
            vm_id, window_id
        ));
        return Err(());
    }
    let op_code = app_window_payload_get_u32(payload, 0).ok_or(())?;
    let a = app_window_payload_get_u32(payload, 4).ok_or(())?;
    let b = app_window_payload_get_u32(payload, 8).ok_or(())?;
    let text_len = app_window_payload_get_u32(payload, 12).ok_or(())? as usize;
    let text = if text_len == 0 {
        ""
    } else {
        let text_bytes = payload
            .get(APP_WINDOW_VMCALL_OP_HEADER..APP_WINDOW_VMCALL_OP_HEADER + text_len)
            .ok_or(())?;
        core::str::from_utf8(text_bytes).map_err(|_| ())?
    };
    let ok = match op_code {
        APP_WINDOW_VMCALL_OP_REQUEST_REPAINT => {
            note_deferred_blueprint_app_window_op(window_id, "request-repaint")
        }
        APP_WINDOW_VMCALL_OP_CLOSE => note_deferred_blueprint_app_window_op(window_id, "close"),
        APP_WINDOW_VMCALL_OP_TITLE => note_deferred_blueprint_app_window_title(window_id, text),
        APP_WINDOW_VMCALL_OP_POSITION => {
            note_deferred_blueprint_app_window_position(window_id, a as i32, b as i32)
        }
        APP_WINDOW_VMCALL_OP_SIZE => note_deferred_blueprint_app_window_size(window_id, a, b),
        APP_WINDOW_VMCALL_OP_U32 => deferred_app_window_u32_op_name(a)
            .map(|op| note_deferred_blueprint_app_window_u32(window_id, op, b))
            .unwrap_or(false),
        APP_WINDOW_VMCALL_OP_BOOL => deferred_app_window_bool_op_name(a)
            .map(|op| note_deferred_blueprint_app_window_bool(window_id, op, b != 0))
            .unwrap_or(false),
        APP_WINDOW_VMCALL_OP_BUTTON_VISIBLE => {
            note_deferred_blueprint_app_window_button_visible(window_id, a, b != 0)
        }
        _ => false,
    };
    if ok {
        let _ = materialize_deferred_blueprint_app_windows(vm_id);
        Ok(0)
    } else {
        Ok(-1)
    }
}

pub fn defer_blueprint_app_window_create(
    kind: &str,
    title: &str,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    z: i32,
    alpha: u32,
    tex_id: u32,
    blend_enabled: bool,
) -> u32 {
    let Some(vm_id) = current_guest_execution_context_vm_id() else {
        return 0;
    };
    let kind_id = deferred_app_window_kind(kind);
    if kind_id == 0 {
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} deferred create rejected kind={} title={}",
            vm_id, kind, title
        ));
        return 0;
    }
    if current_hull_guest_context_vm_id() == Some(vm_id) {
        return app_window_guest_create_vmcall(
            vm_id,
            kind_id,
            title,
            x,
            y,
            width,
            height,
            z,
            alpha,
            tex_id,
            blend_enabled,
        );
    }
    let Some(seq) = APP_WINDOW_DEFERRED_NEXT_ID_BY_VM
        .get(vm_id as usize)
        .map(|next| next.fetch_add(1, Ordering::Relaxed) & APP_WINDOW_DEFERRED_SEQ_MASK)
    else {
        return 0;
    };
    let id = APP_WINDOW_DEFERRED_ID_FLAG | ((vm_id as u32) << APP_WINDOW_DEFERRED_VM_SHIFT) | seq;
    let Some(records_lock) = APP_WINDOW_DEFERRED_RECORDS.get(vm_id as usize) else {
        return 0;
    };
    let mut records = records_lock.lock();
    let Some(slot) = records.iter_mut().find(|record| !record.active) else {
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} deferred create rejected kind={} title={} status=full cap={}",
            vm_id, kind, title, APP_WINDOW_DEFERRED_CAP
        ));
        return 0;
    };
    if tex_id != 0 && !crate::r::io::cabi::claim_vm_texture_id_for_vm(vm_id, tex_id, "vm-window") {
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} deferred create rejected kind={} title={} tex={}",
            vm_id, kind, title, tex_id
        ));
        return 0;
    }
    let host_tex_id = if tex_id == 0 {
        0
    } else {
        crate::r::io::cabi::host_texture_id_for_vm(vm_id, tex_id)
    };
    if tex_id != 0 && host_tex_id == 0 {
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} deferred create rejected kind={} title={} tex={}",
            vm_id, kind, title, tex_id
        ));
        return 0;
    }
    let width = width.max(1);
    let height = height.max(1);
    if kind_id == APP_WINDOW_DEFERRED_KIND_SURFACE
        && tex_id != 0
        && !seed_deferred_surface_texture(tex_id, host_tex_id, width, height)
    {
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} deferred surface texture init failed title={} tex={} host_tex={} size={}x{}",
            vm_id, title, tex_id, host_tex_id, width, height
        ));
        return 0;
    }

    *slot = DeferredAppWindowRecord {
        active: true,
        materialized_window_id: 0,
        deferred_id: id,
        kind: kind_id,
        title: truncate_deferred_app_window_title(title),
        x,
        y,
        width,
        height,
        z,
        alpha: alpha.min(255),
        tex_id: host_tex_id,
        blend_enabled,
        icon_id: None,
        decoration_mode: None,
        titlebar_visible: None,
        bottom_bar_visible: None,
        title_icon_visible: None,
        decoration_button_visible: [None; crate::r::ui2::Ui2WindowDecorationButton::COUNT],
        resize_button_visible: None,
        hit_test_visible: None,
        vertical_scrollbar_visible: None,
        horizontal_scrollbar_visible: None,
        vertical_scrollbar_side: None,
        horizontal_scrollbar_side: None,
        rotate_buttons_visible: None,
        content_rotation_quadrants: None,
        resize_maintain_aspect: None,
        content_preserve_scale: None,
        resize_mode: None,
        repaint_requested: false,
        close_requested: false,
        cached_info: None,
    };
    app_window_broker_log(format_args!(
        "app-window-broker: vm{} deferred create kind={} window={} seq={} title={} rect=({},{} {}x{}) tex={} host_tex={} materialize=host-service",
        vm_id, kind, id, seq, title, x, y, width, height, tex_id, host_tex_id
    ));
    id
}

pub fn note_deferred_blueprint_app_window_op(window_id: u32, op: &'static str) -> bool {
    let Some(vm_id) = deferred_blueprint_app_window_vm_id(window_id) else {
        return false;
    };
    if current_hull_guest_context_vm_id() == Some(vm_id) {
        let op_code = match op {
            "request-repaint" => APP_WINDOW_VMCALL_OP_REQUEST_REPAINT,
            "close" => APP_WINDOW_VMCALL_OP_CLOSE,
            _ => 0,
        };
        return op_code == 0 || app_window_guest_op_vmcall(window_id, op_code, 0, 0, None);
    }
    let Some(records_lock) = APP_WINDOW_DEFERRED_RECORDS.get(vm_id as usize) else {
        return false;
    };
    let mut records = records_lock.lock();
    let Some(record) = records
        .iter_mut()
        .find(|record| record.active && record.deferred_id == window_id)
    else {
        return false;
    };
    match op {
        "request-repaint" => record.repaint_requested = true,
        "close" => record.close_requested = true,
        _ => {}
    }
    true
}

pub fn note_deferred_blueprint_app_window_title(window_id: u32, title: &str) -> bool {
    if let Some(vm_id) = deferred_blueprint_app_window_vm_id(window_id)
        && current_hull_guest_context_vm_id() == Some(vm_id)
    {
        return app_window_guest_op_vmcall(
            window_id,
            APP_WINDOW_VMCALL_OP_TITLE,
            0,
            0,
            Some(title),
        );
    }
    with_deferred_blueprint_app_window_record(window_id, |record| {
        record.title = truncate_deferred_app_window_title(title);
        true
    })
    .unwrap_or(false)
}

pub fn note_deferred_blueprint_app_window_position(window_id: u32, x: i32, y: i32) -> bool {
    if let Some(vm_id) = deferred_blueprint_app_window_vm_id(window_id)
        && current_hull_guest_context_vm_id() == Some(vm_id)
    {
        return app_window_guest_op_vmcall(
            window_id,
            APP_WINDOW_VMCALL_OP_POSITION,
            x as u32,
            y as u32,
            None,
        );
    }
    with_deferred_blueprint_app_window_record(window_id, |record| {
        record.x = x;
        record.y = y;
        true
    })
    .unwrap_or(false)
}

pub fn note_deferred_blueprint_app_window_size(window_id: u32, width: u32, height: u32) -> bool {
    if let Some(vm_id) = deferred_blueprint_app_window_vm_id(window_id)
        && current_hull_guest_context_vm_id() == Some(vm_id)
    {
        return app_window_guest_op_vmcall(
            window_id,
            APP_WINDOW_VMCALL_OP_SIZE,
            width,
            height,
            None,
        );
    }
    with_deferred_blueprint_app_window_record(window_id, |record| {
        record.width = width.max(1);
        record.height = height.max(1);
        true
    })
    .unwrap_or(false)
}

pub fn note_deferred_blueprint_app_window_u32(
    window_id: u32,
    op: &'static str,
    value: u32,
) -> bool {
    if let Some(vm_id) = deferred_blueprint_app_window_vm_id(window_id)
        && current_hull_guest_context_vm_id() == Some(vm_id)
    {
        let op_code = deferred_app_window_u32_op_code(op);
        return op_code != 0
            && app_window_guest_op_vmcall(
                window_id,
                APP_WINDOW_VMCALL_OP_U32,
                op_code,
                value,
                None,
            );
    }
    with_deferred_blueprint_app_window_record(window_id, |record| {
        match op {
            "set-icon" => record.icon_id = Some(value),
            "set-decorations" => record.decoration_mode = Some(value),
            "set-vertical-scrollbar-side" => record.vertical_scrollbar_side = Some(value),
            "set-horizontal-scrollbar-side" => record.horizontal_scrollbar_side = Some(value),
            "set-content-rotation" => record.content_rotation_quadrants = Some(value % 4),
            "set-resize-mode" => record.resize_mode = Some(value),
            _ => {}
        }
        true
    })
    .unwrap_or(false)
}

pub fn note_deferred_blueprint_app_window_bool(
    window_id: u32,
    op: &'static str,
    value: bool,
) -> bool {
    if let Some(vm_id) = deferred_blueprint_app_window_vm_id(window_id)
        && current_hull_guest_context_vm_id() == Some(vm_id)
    {
        let op_code = deferred_app_window_bool_op_code(op);
        return op_code != 0
            && app_window_guest_op_vmcall(
                window_id,
                APP_WINDOW_VMCALL_OP_BOOL,
                op_code,
                u32::from(value),
                None,
            );
    }
    with_deferred_blueprint_app_window_record(window_id, |record| {
        match op {
            "set-titlebar-visible" => record.titlebar_visible = Some(value),
            "set-bottom-bar-visible" => record.bottom_bar_visible = Some(value),
            "set-title-icon-visible" => record.title_icon_visible = Some(value),
            "set-resize-button-visible" => record.resize_button_visible = Some(value),
            "set-hit-test-visible" => record.hit_test_visible = Some(value),
            "set-vertical-scrollbar-visible" => record.vertical_scrollbar_visible = Some(value),
            "set-horizontal-scrollbar-visible" => record.horizontal_scrollbar_visible = Some(value),
            "set-rotate-buttons-visible" => record.rotate_buttons_visible = Some(value),
            "set-resize-maintain-aspect" => record.resize_maintain_aspect = Some(value),
            "set-content-preserve-scale" => record.content_preserve_scale = Some(value),
            _ => {}
        }
        true
    })
    .unwrap_or(false)
}

pub fn note_deferred_blueprint_app_window_button_visible(
    window_id: u32,
    button: u32,
    value: bool,
) -> bool {
    if let Some(vm_id) = deferred_blueprint_app_window_vm_id(window_id)
        && current_hull_guest_context_vm_id() == Some(vm_id)
    {
        return app_window_guest_op_vmcall(
            window_id,
            APP_WINDOW_VMCALL_OP_BUTTON_VISIBLE,
            button,
            u32::from(value),
            None,
        );
    }
    with_deferred_blueprint_app_window_record(window_id, |record| {
        if let Some(slot) = record.decoration_button_visible.get_mut(button as usize) {
            *slot = Some(value);
        }
        true
    })
    .unwrap_or(false)
}

fn with_deferred_blueprint_app_window_record<R>(
    window_id: u32,
    f: impl FnOnce(&mut DeferredAppWindowRecord) -> R,
) -> Option<R> {
    let vm_id = deferred_blueprint_app_window_vm_id(window_id)?;
    let records_lock = APP_WINDOW_DEFERRED_RECORDS.get(vm_id as usize)?;
    let mut records = records_lock.lock();
    let record = records
        .iter_mut()
        .find(|record| record.active && record.deferred_id == window_id)?;
    Some(f(record))
}

pub fn deferred_blueprint_app_window_vm_id(window_id: u32) -> Option<u8> {
    if (window_id & APP_WINDOW_DEFERRED_ID_FLAG) == 0 {
        return None;
    }
    let vm_id = ((window_id & !APP_WINDOW_DEFERRED_ID_FLAG) >> APP_WINDOW_DEFERRED_VM_SHIFT) as u8;
    if (vm_id as usize) < TRUEOS_VM_ID_LIMIT {
        Some(vm_id)
    } else {
        None
    }
}

pub fn deferred_blueprint_app_window_current_vm(window_id: u32) -> Option<u8> {
    let vm_id = deferred_blueprint_app_window_vm_id(window_id)?;
    if current_guest_execution_context_vm_id() == Some(vm_id) {
        Some(vm_id)
    } else {
        None
    }
}

pub fn materialized_blueprint_app_window_id(window_id: u32) -> Option<u32> {
    let vm_id = deferred_blueprint_app_window_vm_id(window_id)?;
    let records_lock = APP_WINDOW_DEFERRED_RECORDS.get(vm_id as usize)?;
    records_lock
        .lock()
        .iter()
        .find(|record| {
            record.active
                && record.deferred_id == window_id
                && record.materialized_window_id != 0
                && record.materialized_window_id != DeferredAppWindowRecord::MATERIALIZING
        })
        .map(|record| record.materialized_window_id)
}

pub fn host_blueprint_app_window_id(window_id: u32) -> u32 {
    if let Some(host_window_id) = materialized_blueprint_app_window_id(window_id) {
        return host_window_id;
    }

    if current_guest_execution_context_vm_id().is_none()
        && let Some(vm_id) = deferred_blueprint_app_window_vm_id(window_id)
    {
        let _ = materialize_deferred_blueprint_app_windows(vm_id);
        if let Some(host_window_id) = materialized_blueprint_app_window_id(window_id) {
            return host_window_id;
        }
    }

    window_id
}

pub fn deferred_blueprint_app_window_info_current_vm(
    window_id: u32,
) -> Option<crate::r::ui2::TrueosUi2WindowInfo> {
    let vm_id = deferred_blueprint_app_window_current_vm(window_id)?;
    let records_lock = APP_WINDOW_DEFERRED_RECORDS.get(vm_id as usize)?;
    let mut info = records_lock
        .lock()
        .iter()
        .find(|record| record.active && record.deferred_id == window_id)
        .and_then(|record| record.cached_info)?;
    info.id = window_id;
    Some(info)
}

pub fn begin_blueprint_app_window_session(vm_id: u8, archive: &str) {
    let Some(session_lock) = APP_WINDOW_SESSIONS.get(vm_id as usize) else {
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} session begin rejected archive={}",
            vm_id, archive
        ));
        return;
    };
    if let Some(records_lock) = APP_WINDOW_DEFERRED_RECORDS.get(vm_id as usize) {
        records_lock
            .lock()
            .iter_mut()
            .for_each(|record| *record = DeferredAppWindowRecord::empty());
    }

    let previous = session_lock.lock().replace(AppWindowSession {
        archive: AllocString::from(archive),
        window_ids: AllocVec::with_capacity(4),
    });
    if let Some(previous) = previous {
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} session replaced archive={} windows={}",
            vm_id,
            previous.archive.as_str(),
            previous.window_ids.len()
        ));
        for window_id in previous.window_ids {
            close_or_defer_app_window(vm_id, window_id, "session-replaced");
        }
    }
    app_window_broker_log(format_args!(
        "app-window-broker: vm{} session begin archive={}",
        vm_id, archive
    ));
}

pub fn register_blueprint_app_window_for_vm(vm_id: u8, window_id: u32, kind: &str, title: &str) {
    let Some(session_lock) = APP_WINDOW_SESSIONS.get(vm_id as usize) else {
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} create with unsupported vm id kind={} window={} title={}",
            vm_id, kind, window_id, title
        ));
        return;
    };
    let mut sessions = session_lock.lock();
    let Some(session) = sessions.as_mut() else {
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} create without active session kind={} window={} title={}",
            vm_id, kind, window_id, title
        ));
        return;
    };

    if !session.window_ids.contains(&window_id) {
        session.window_ids.push(window_id);
    }

    app_window_broker_log(format_args!(
        "app-window-broker: vm{} created archive={} kind={} window={} title={}",
        vm_id,
        session.archive.as_str(),
        kind,
        window_id,
        title
    ));
}

pub fn register_blueprint_app_window(window_id: u32, kind: &str, title: &str) {
    let Some(vm_id) = current_vm_id() else {
        app_window_broker_log(format_args!(
            "app-window-broker: create without vm context kind={} window={} title={}",
            kind, window_id, title
        ));
        return;
    };
    register_blueprint_app_window_for_vm(vm_id, window_id, kind, title);
}

pub fn materialize_deferred_blueprint_app_windows(vm_id: u8) -> usize {
    drain_deferred_app_window_closes(vm_id);

    let Some(records_lock) = APP_WINDOW_DEFERRED_RECORDS.get(vm_id as usize) else {
        return 0;
    };
    let mut materialized = 0usize;
    let mut pending: HVec<DeferredAppWindowRecord, APP_WINDOW_DEFERRED_CAP> = HVec::new();
    {
        let mut records = records_lock.lock();
        for record in records.iter_mut() {
            if record.active
                && record.materialized_window_id == 0
                && !record.close_requested
                && pending.push(record.clone()).is_ok()
            {
                record.materialized_window_id = DeferredAppWindowRecord::MATERIALIZING;
            }
        }
    }

    for record in pending {
        let kind = deferred_app_window_kind_name(record.kind);
        let rect = crate::r::ui2::Ui2Rect {
            x: record.x as f32,
            y: record.y as f32,
            w: record.width.max(1) as f32,
            h: record.height.max(1) as f32,
        };
        let z = record.z.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        let alpha = record.alpha.min(255) as u8;
        let window_id = match record.kind {
            APP_WINDOW_DEFERRED_KIND_PLAIN => {
                crate::r::ui2::create_window(record.title.as_str(), rect, z, alpha)
            }
            APP_WINDOW_DEFERRED_KIND_SURFACE => {
                crate::r::ui2::create_hosted_surface_content_window(
                    record.title.as_str(),
                    rect,
                    z,
                    alpha,
                    record.tex_id,
                    record.blend_enabled,
                )
            }
            _ => 0,
        };
        if window_id == 0 {
            let mut records = records_lock.lock();
            if let Some(slot) = records
                .iter_mut()
                .find(|slot| slot.active && slot.deferred_id == record.deferred_id)
            {
                slot.materialized_window_id = 0;
            }
            app_window_broker_log(format_args!(
                "app-window-broker: vm{} deferred materialize failed kind={} deferred={} title={}",
                vm_id,
                kind,
                record.deferred_id,
                record.title.as_str()
            ));
            continue;
        }

        let _ = crate::r::ui2::set_window_vm_origin(window_id, Some(vm_id));
        apply_deferred_app_window_properties(window_id, &record);
        let _ = crate::r::ui2::focus_window(window_id);
        if record.repaint_requested {
            let _ =
                crate::r::ui2::request_window_content_present(window_id, "vm-deferred-materialize");
        }
        register_blueprint_app_window_for_vm(vm_id, window_id, kind, record.title.as_str());
        materialized += 1;

        {
            let mut records = records_lock.lock();
            if let Some(slot) = records
                .iter_mut()
                .find(|slot| slot.active && slot.deferred_id == record.deferred_id)
            {
                slot.materialized_window_id = window_id;
                slot.cached_info = crate::r::ui2::window_info_by_id(window_id);
            }
        }
        app_window_broker_log(format_args!(
            "app-window-broker: vm{} deferred materialized kind={} deferred={} window={} title={}",
            vm_id,
            kind,
            record.deferred_id,
            window_id,
            record.title.as_str()
        ));
    }
    drain_deferred_app_window_closes(vm_id);
    materialized
}

fn sync_materialized_deferred_blueprint_app_window_updates(vm_id: u8) {
    let Some(records_lock) = APP_WINDOW_DEFERRED_RECORDS.get(vm_id as usize) else {
        return;
    };
    let mut pending: HVec<DeferredAppWindowRecord, APP_WINDOW_DEFERRED_CAP> = HVec::new();
    {
        let records = records_lock.lock();
        for record in records.iter() {
            if record.active
                && record.materialized_window_id != 0
                && record.materialized_window_id != DeferredAppWindowRecord::MATERIALIZING
                && !record.close_requested
            {
                let _ = pending.push(record.clone());
            }
        }
    }

    let mut clear_repaint: HVec<u32, APP_WINDOW_DEFERRED_CAP> = HVec::new();
    let mut info_updates: HVec<(u32, crate::r::ui2::TrueosUi2WindowInfo), APP_WINDOW_DEFERRED_CAP> =
        HVec::new();
    for record in pending {
        let window_id = record.materialized_window_id;
        let _ = crate::r::ui2::set_window_title(window_id, record.title.as_str());
        if let Some(info) = crate::r::ui2::window_info_by_id(window_id) {
            let _ = info_updates.push((record.deferred_id, info));
        }
        if record.repaint_requested
            && crate::r::ui2::request_window_content_present(
                window_id,
                "vm-deferred-request-repaint",
            )
        {
            let _ = clear_repaint.push(record.deferred_id);
        }
    }

    if !clear_repaint.is_empty() || !info_updates.is_empty() {
        let mut records = records_lock.lock();
        for (deferred_id, info) in info_updates {
            if let Some(record) = records
                .iter_mut()
                .find(|record| record.active && record.deferred_id == deferred_id)
            {
                record.cached_info = Some(info);
            }
        }
        for deferred_id in clear_repaint {
            if let Some(record) = records
                .iter_mut()
                .find(|record| record.active && record.deferred_id == deferred_id)
            {
                record.repaint_requested = false;
            }
        }
    }
}

fn apply_deferred_app_window_properties(window_id: u32, record: &DeferredAppWindowRecord) {
    if let Some(icon_id) = record.icon_id {
        let _ = crate::r::ui2::set_window_icon(window_id, icon_id);
    }
    if let Some(mode) = record.decoration_mode.and_then(deferred_decoration_mode) {
        let _ = crate::r::ui2::set_window_decorations(window_id, mode);
    }
    if let Some(visible) = record.titlebar_visible {
        let _ = crate::r::ui2::set_window_titlebar_visible(window_id, visible);
    }
    if let Some(visible) = record.bottom_bar_visible {
        let _ = crate::r::ui2::set_window_bottom_bar_visible(window_id, visible);
    }
    if let Some(visible) = record.title_icon_visible {
        let _ = crate::r::ui2::set_window_title_icon_visible(window_id, visible);
    }
    for (idx, visible) in record.decoration_button_visible.iter().enumerate() {
        let Some(visible) = visible else {
            continue;
        };
        if let Some(button) = crate::r::ui2::Ui2WindowDecorationButton::from_u32(idx as u32) {
            let _ = crate::r::ui2::set_window_titlebar_button_visible(window_id, button, *visible);
        }
    }
    if let Some(visible) = record.resize_button_visible {
        let _ = crate::r::ui2::set_window_resize_button_visible(window_id, visible);
    }
    if let Some(visible) = record.hit_test_visible {
        let _ = crate::r::ui2::set_window_hit_test_visible(window_id, visible);
    }
    if let Some(visible) = record.vertical_scrollbar_visible {
        let _ = crate::r::ui2::set_window_left_scrollbar_visible(window_id, visible);
    }
    if let Some(visible) = record.horizontal_scrollbar_visible {
        let _ = crate::r::ui2::set_window_bottom_scrollbar_visible(window_id, visible);
    }
    if let Some(side) = record
        .vertical_scrollbar_side
        .and_then(deferred_vertical_scrollbar_side)
    {
        let _ = crate::r::ui2::set_window_vertical_scrollbar_side(window_id, side);
    }
    if let Some(side) = record
        .horizontal_scrollbar_side
        .and_then(deferred_horizontal_scrollbar_side)
    {
        let _ = crate::r::ui2::set_window_horizontal_scrollbar_side(window_id, side);
    }
    if let Some(visible) = record.rotate_buttons_visible {
        let _ = crate::r::ui2::set_window_rotate_buttons_visible(window_id, visible);
    }
    if let Some(quadrants) = record.content_rotation_quadrants {
        let _ =
            crate::r::ui2::set_window_content_rotation_quadrants(window_id, (quadrants % 4) as u8);
    }
    if let Some(maintain_aspect) = record.resize_maintain_aspect {
        let _ = crate::r::ui2::set_window_resize_maintain_aspect(window_id, maintain_aspect);
    }
    if let Some(preserve_scale) = record.content_preserve_scale {
        let _ = crate::r::ui2::set_window_content_preserve_scale(window_id, preserve_scale);
    }
    if let Some(mode) = record.resize_mode.and_then(deferred_resize_mode) {
        let _ = crate::r::ui2::set_window_resize_mode(window_id, mode);
    }
}

fn deferred_decoration_mode(value: u32) -> Option<crate::r::ui2::Ui2WindowDecorationMode> {
    match value {
        0 => Some(crate::r::ui2::Ui2WindowDecorationMode::System),
        1 => Some(crate::r::ui2::Ui2WindowDecorationMode::Client),
        2 => Some(crate::r::ui2::Ui2WindowDecorationMode::None),
        _ => None,
    }
}

fn deferred_resize_mode(value: u32) -> Option<crate::r::ui2::Ui2WindowResizeMode> {
    match value {
        0 => Some(crate::r::ui2::Ui2WindowResizeMode::Auto),
        1 => Some(crate::r::ui2::Ui2WindowResizeMode::Live),
        2 => Some(crate::r::ui2::Ui2WindowResizeMode::PreviewCommit),
        _ => None,
    }
}

fn deferred_vertical_scrollbar_side(
    value: u32,
) -> Option<crate::r::ui2::Ui2WindowVerticalScrollbarSide> {
    match value {
        0 => Some(crate::r::ui2::Ui2WindowVerticalScrollbarSide::Left),
        1 => Some(crate::r::ui2::Ui2WindowVerticalScrollbarSide::Right),
        _ => None,
    }
}

fn deferred_horizontal_scrollbar_side(
    value: u32,
) -> Option<crate::r::ui2::Ui2WindowHorizontalScrollbarSide> {
    match value {
        0 => Some(crate::r::ui2::Ui2WindowHorizontalScrollbarSide::Top),
        1 => Some(crate::r::ui2::Ui2WindowHorizontalScrollbarSide::Bottom),
        _ => None,
    }
}

pub fn materialize_pending_deferred_blueprint_app_windows() -> usize {
    let mut materialized = 0usize;
    for vm_id in 0..TRUEOS_VM_ID_LIMIT {
        materialized += materialize_deferred_blueprint_app_windows(vm_id as u8);
        sync_materialized_deferred_blueprint_app_window_updates(vm_id as u8);
    }
    materialized
}

pub fn request_deferred_blueprint_app_windows_for_host_texture(
    host_tex_id: u32,
    reason: &'static str,
) -> usize {
    if host_tex_id == 0 {
        return 0;
    }

    let mut window_ids: HVec<u32, APP_WINDOW_DEFERRED_CAP> = HVec::new();
    for records_lock in APP_WINDOW_DEFERRED_RECORDS.iter() {
        let records = records_lock.lock();
        for record in records.iter() {
            if record.active
                && record.tex_id == host_tex_id
                && record.materialized_window_id != 0
                && record.materialized_window_id != DeferredAppWindowRecord::MATERIALIZING
                && !record.close_requested
            {
                let _ = window_ids.push(record.materialized_window_id);
            }
        }
    }

    let mut requested = 0usize;
    for window_id in window_ids {
        if crate::r::ui2::request_window_content_present(window_id, reason) {
            requested += 1;
        }
    }
    requested
}

pub fn finish_blueprint_app_window_session(vm_id: u8, close_windows: bool) {
    let Some(session_lock) = APP_WINDOW_SESSIONS.get(vm_id as usize) else {
        return;
    };
    let Some(session) = session_lock.lock().take() else {
        return;
    };

    app_window_broker_log(format_args!(
        "app-window-broker: vm{} session end archive={} windows={} close_windows={}",
        vm_id,
        session.archive.as_str(),
        session.window_ids.len(),
        close_windows
    ));

    if close_windows {
        for window_id in session.window_ids {
            close_or_defer_app_window(vm_id, window_id, "session-end");
        }
    }
    if let Some(records_lock) = APP_WINDOW_DEFERRED_RECORDS.get(vm_id as usize) {
        records_lock
            .lock()
            .iter_mut()
            .for_each(|record| *record = DeferredAppWindowRecord::empty());
    }
}
