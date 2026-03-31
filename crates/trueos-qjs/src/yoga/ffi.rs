#![cfg(feature = "trueos")]

use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;

type YGNodeRef = *mut c_void;
type YGConfigRef = *mut c_void;

unsafe extern "C" {
    fn YGConfigNew() -> YGConfigRef;
    fn YGConfigFree(config: YGConfigRef);
    fn YGConfigSetUseWebDefaults(config: YGConfigRef, enabled: bool);

    fn YGNodeNewWithConfig(config: YGConfigRef) -> YGNodeRef;
    fn YGNodeFreeRecursive(node: YGNodeRef);
    fn YGNodeInsertChild(node: YGNodeRef, child: YGNodeRef, index: u32);
    fn YGNodeGetChildCount(node: YGNodeRef) -> u32;
    fn YGNodeCalculateLayout(node: YGNodeRef, width: f32, height: f32, direction: i32);

    fn YGNodeStyleSetFlexDirection(node: YGNodeRef, value: i32);
    fn YGNodeStyleSetAlignItems(node: YGNodeRef, value: i32);
    fn YGNodeStyleSetAlignSelf(node: YGNodeRef, value: i32);
    fn YGNodeStyleSetJustifyContent(node: YGNodeRef, value: i32);
    fn YGNodeStyleSetFlexWrap(node: YGNodeRef, value: i32);
    fn YGNodeStyleSetFlexGrow(node: YGNodeRef, value: f32);
    fn YGNodeStyleSetFlexShrink(node: YGNodeRef, value: f32);
    fn YGNodeStyleSetPositionType(node: YGNodeRef, value: i32);

    fn YGNodeStyleSetWidth(node: YGNodeRef, value: f32);
    fn YGNodeStyleSetHeight(node: YGNodeRef, value: f32);
    fn YGNodeStyleSetMinWidth(node: YGNodeRef, value: f32);
    fn YGNodeStyleSetMinHeight(node: YGNodeRef, value: f32);
    fn YGNodeStyleSetPadding(node: YGNodeRef, edge: i32, value: f32);
    fn YGNodeStyleSetMargin(node: YGNodeRef, edge: i32, value: f32);
    fn YGNodeStyleSetPosition(node: YGNodeRef, edge: i32, value: f32);

    fn YGNodeLayoutGetLeft(node: YGNodeRef) -> f32;
    fn YGNodeLayoutGetTop(node: YGNodeRef) -> f32;
    fn YGNodeLayoutGetWidth(node: YGNodeRef) -> f32;
    fn YGNodeLayoutGetHeight(node: YGNodeRef) -> f32;
}

static CONFIGS: Mutex<Vec<Option<usize>>> = Mutex::new(Vec::new());
static NODES: Mutex<Vec<Option<usize>>> = Mutex::new(Vec::new());
static DEFAULT_CONFIG_HANDLE: AtomicU32 = AtomicU32::new(0);

fn push_handle<T>(v: &mut Vec<Option<T>>, ptr: T) -> u32 {
    for (i, slot) in v.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(ptr);
            return (i + 1) as u32;
        }
    }
    v.push(Some(ptr));
    v.len() as u32
}

fn with_config<R>(handle: u32, f: impl FnOnce(YGConfigRef) -> R) -> Option<R> {
    if handle == 0 {
        return None;
    }
    let idx = (handle - 1) as usize;
    let cfg = CONFIGS.lock().get(idx).and_then(|x| *x)? as YGConfigRef;
    Some(f(cfg))
}

fn with_node<R>(handle: u32, f: impl FnOnce(YGNodeRef) -> R) -> Option<R> {
    if handle == 0 {
        return None;
    }
    let idx = (handle - 1) as usize;
    let node = NODES.lock().get(idx).and_then(|x| *x)? as YGNodeRef;
    Some(f(node))
}

pub fn config_create() -> u32 {
    let cfg = unsafe { YGConfigNew() };
    if cfg.is_null() {
        return 0;
    }
    let mut v = CONFIGS.lock();
    push_handle(&mut v, cfg as usize)
}

pub fn config_set_use_web_defaults(handle: u32, enabled: bool) {
    let _ = with_config(handle, |cfg| unsafe { YGConfigSetUseWebDefaults(cfg, enabled) });
}

pub fn config_free(handle: u32) {
    if handle == 0 {
        return;
    }
    let idx = (handle - 1) as usize;
    let mut v = CONFIGS.lock();
    let Some(slot) = v.get_mut(idx) else {
        return;
    };
    let Some(cfg) = *slot else {
        return;
    };
    unsafe { YGConfigFree(cfg as YGConfigRef) };
    *slot = None;
}

fn ensure_default_config() -> u32 {
    let cached = DEFAULT_CONFIG_HANDLE.load(Ordering::Acquire);
    if cached != 0 {
        return cached;
    }
    let h = config_create();
    if h != 0 {
        config_set_use_web_defaults(h, true);
        let _ = DEFAULT_CONFIG_HANDLE.compare_exchange(0, h, Ordering::AcqRel, Ordering::Acquire);
    }
    DEFAULT_CONFIG_HANDLE.load(Ordering::Acquire)
}

pub fn node_create(config_handle: u32) -> u32 {
    let cfg_h = if config_handle == 0 {
        ensure_default_config()
    } else {
        config_handle
    };
    let cfg = with_config(cfg_h, |c| c).unwrap_or(ptr::null_mut());
    if cfg.is_null() {
        return 0;
    }
    let node = unsafe { YGNodeNewWithConfig(cfg) };
    if node.is_null() {
        return 0;
    }
    let mut v = NODES.lock();
    push_handle(&mut v, node as usize)
}

pub fn node_free_recursive(handle: u32) {
    let node = with_node(handle, |n| n).unwrap_or(ptr::null_mut());
    if node.is_null() {
        return;
    }
    unsafe { YGNodeFreeRecursive(node) };
    // Nodes are usually built per-frame and freed from the root. Reset table.
    NODES.lock().clear();
}

pub fn node_insert_child(parent: u32, child: u32, index: u32) {
    let p = with_node(parent, |n| n).unwrap_or(ptr::null_mut());
    let c = with_node(child, |n| n).unwrap_or(ptr::null_mut());
    if p.is_null() || c.is_null() {
        return;
    }
    unsafe { YGNodeInsertChild(p, c, index) };
}

pub fn node_get_child_count(handle: u32) -> u32 {
    with_node(handle, |n| unsafe { YGNodeGetChildCount(n) }).unwrap_or(0)
}

pub fn node_calculate_layout(handle: u32, width: f32, height: f32, direction: i32) {
    let _ = with_node(handle, |n| unsafe { YGNodeCalculateLayout(n, width, height, direction) });
}

pub fn node_set_flex_direction(handle: u32, v: i32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetFlexDirection(n, v) });
}
pub fn node_set_align_items(handle: u32, v: i32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetAlignItems(n, v) });
}
pub fn node_set_align_self(handle: u32, v: i32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetAlignSelf(n, v) });
}
pub fn node_set_justify_content(handle: u32, v: i32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetJustifyContent(n, v) });
}
pub fn node_set_flex_wrap(handle: u32, v: i32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetFlexWrap(n, v) });
}
pub fn node_set_flex_grow(handle: u32, v: f32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetFlexGrow(n, v) });
}
pub fn node_set_flex_shrink(handle: u32, v: f32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetFlexShrink(n, v) });
}
pub fn node_set_position_type(handle: u32, v: i32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetPositionType(n, v) });
}
pub fn node_set_width(handle: u32, v: f32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetWidth(n, v) });
}
pub fn node_set_height(handle: u32, v: f32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetHeight(n, v) });
}
pub fn node_set_min_width(handle: u32, v: f32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetMinWidth(n, v) });
}
pub fn node_set_min_height(handle: u32, v: f32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetMinHeight(n, v) });
}
pub fn node_set_padding(handle: u32, edge: i32, v: f32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetPadding(n, edge, v) });
}
pub fn node_set_margin(handle: u32, edge: i32, v: f32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetMargin(n, edge, v) });
}
pub fn node_set_position(handle: u32, edge: i32, v: f32) {
    let _ = with_node(handle, |n| unsafe { YGNodeStyleSetPosition(n, edge, v) });
}

pub fn node_get_computed_left(handle: u32) -> f32 {
    with_node(handle, |n| unsafe { YGNodeLayoutGetLeft(n) }).unwrap_or(0.0)
}
pub fn node_get_computed_top(handle: u32) -> f32 {
    with_node(handle, |n| unsafe { YGNodeLayoutGetTop(n) }).unwrap_or(0.0)
}
pub fn node_get_computed_width(handle: u32) -> f32 {
    with_node(handle, |n| unsafe { YGNodeLayoutGetWidth(n) }).unwrap_or(0.0)
}
pub fn node_get_computed_height(handle: u32) -> f32 {
    with_node(handle, |n| unsafe { YGNodeLayoutGetHeight(n) }).unwrap_or(0.0)
}
