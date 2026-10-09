//! UI4 cursor-plane previews and opaque cross-frame drag payloads.
use alloc::vec::Vec;

pub const MAX_LABEL_BYTES: usize = 256;
pub const MAX_PAYLOAD_BYTES: usize = 3072;
/// Little-endian u32 byte lengths followed by UTF-8 paths, repeated to EOF.
pub const PATH_LIST_V1: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DragError(pub i32);

/// Kernel copies the label/payload once. The cursor plane paints the preview
/// independently of the app's frame, including outside the source window.
pub struct DragBuffer {
    token: i32,
}
impl DragBuffer {
    pub fn begin(window: u32, kind: u32, label: &str, payload: &[u8]) -> Result<Self, DragError> {
        if label.is_empty() || label.len() > MAX_LABEL_BYTES || payload.len() > MAX_PAYLOAD_BYTES {
            return Err(DragError(-1));
        }
        let token = unsafe {
            crate::bp_abi::trueos_cabi_ui4_drag_begin_v1(
                window,
                kind,
                label.as_ptr(),
                label.len(),
                payload.as_ptr(),
                payload.len(),
            )
        };
        if token > 0 {
            Ok(Self { token })
        } else {
            Err(DragError(token))
        }
    }
    /// Use the current app's Shell3 UI4 lease; SSH has no local cursor plane.
    pub fn begin_attached(kind: u32, label: &str, payload: &[u8]) -> Result<Self, DragError> {
        Self::begin(0, kind, label, payload)
    }
    /// Hide/cancel the preview. True means UI4 queued a cross-frame drop;
    /// the source must then skip its own local drop action.
    pub fn finish(mut self) -> Result<bool, DragError> {
        let rc = unsafe { crate::bp_abi::trueos_cabi_ui4_drag_cancel_v1(self.token) };
        self.token = 0;
        if rc >= 0 {
            Ok(rc != 0)
        } else {
            Err(DragError(rc))
        }
    }
}
impl Drop for DragBuffer {
    fn drop(&mut self) {
        if self.token > 0 {
            unsafe {
                crate::bp_abi::trueos_cabi_ui4_drag_cancel_v1(self.token);
            }
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct DropPayload {
    pub kind: u32,
    pub x: i32,
    pub y: i32,
    pub bytes: Vec<u8>,
}
/// Coordinates are frame-local pixels, or character cells for window zero
/// (the caller's currently leased Shell3 TUI).
pub fn take_drop(window: u32) -> Result<Option<DropPayload>, DragError> {
    let mut out = [0u8; MAX_PAYLOAD_BYTES + 16];
    let rc =
        unsafe { crate::bp_abi::trueos_cabi_ui4_drag_take_v1(window, out.as_mut_ptr(), out.len()) };
    if rc < 0 {
        return Err(DragError(rc));
    }
    if rc == 0 {
        return Ok(None);
    }
    if rc < 16 || rc as usize > out.len() {
        return Err(DragError(-1));
    }
    let word = |start: usize| u32::from_le_bytes(out[start..start + 4].try_into().unwrap());
    let (kind, x, y, len) = (word(0), word(4) as i32, word(8) as i32, word(12) as usize);
    if len + 16 != rc as usize {
        return Err(DragError(-1));
    }
    Ok(Some(DropPayload {
        kind,
        x,
        y,
        bytes: out[16..16 + len].to_vec(),
    }))
}
