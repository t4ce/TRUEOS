// Source-only placeholder for tools/font-tessellation/bake_font_patch.py.
// A matched native capture replaces this file; unavailable artifacts must fail closed.
use super::*;

pub(crate) const CONTRACT_VERSION: u32 = 1;
pub(crate) const AVAILABLE: bool = false;
pub(crate) const COMPILED_DEVICE_ID: u16 = 0;
pub(crate) static VERTEX: [u32; 0] = [];
pub(crate) static TESS_CONTROL: [u32; 0] = [];
pub(crate) static TESS_EVAL: [u32; 0] = [];
pub(crate) static FRAGMENT: [u32; 0] = [];
pub(crate) const HS_PACKET: [u32; 9] = [0x781b0007, 0, 0, 0, 0, 0, 0, 0, 0];
pub(crate) const TE_PACKET: [u32; 5] = [0x781c0003, 0, 0, 0, 0];
pub(crate) const DS_PACKET: [u32; 11] = [0x781d0009, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];



pub(crate) fn matches(_pipeline: &TrianglePipeline) -> bool {
    false
}
