#![allow(dead_code, private_interfaces)]
extern crate alloc;
extern crate self as spin;
extern crate self as v;
extern crate self as trueos_vm;
use std::sync::{MutexGuard, TryLockError};
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> MutexGuard<'_, T> { self.0.lock().unwrap() }
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        match self.0.try_lock() { Ok(g) => Some(g), Err(TryLockError::WouldBlock) => None, Err(e) => panic!("{e}") }
    }
}
pub mod bp_abi { @ABI@ }
pub mod vmcall {
    pub const STATUS_OK: u32 = 0;
    pub const OP_BP_UI4_HULL_IMAGE_REQUEST_V1: u32 = 0x239;
    pub const OP_BP_UI4_HULL_IMAGE_STATUS_V1: u32 = 0x23A;
    pub const OP_BP_UI4_HULL_IMAGE_RELEASE_V1: u32 = 0x23B;
    pub const OP_BP_UI4_HULL_IMAGE_RESET_V1: u32 = 0x23C;
    pub fn call_with_payload(_: u32, _: u64, _: u64, _: &[u8], _: &mut [u8]) -> (u32, u64) { (1, 0) }
}
pub mod hv { pub fn current_hull_guest_context_vm_id() -> Option<u8> { None } }
pub mod r { pub mod services { pub mod hull_image_service {
    pub fn request(_: u32, _: u32, _: &str, _: &str, _: &str, _: u32, _: u32, _: u64) -> i32 { 27 }
    pub fn status(_: u32, _: u32, _: u32) -> Result<crate::bp_abi::TrueosUi4HullImageInfoV1, i32> { Err(-3) }
    pub fn release(_: u32, _: u32, _: u32) -> i32 { 0 }
    pub fn reset(_: u32, _: u32, _: u64) -> i32 { 0 }
} } }
pub mod intel {
    pub const WARM_ALIGN: usize = 4096;
    pub fn align_up(n: usize, align: usize) -> Option<usize> { n.checked_add(align - 1).map(|x| x & !(align - 1)) }
    pub fn dma_flush(_: *mut u8, _: usize) {}
}
pub mod dma {
    pub fn alloc_ppgtt(bytes: usize, align: usize) -> Option<(u64, *mut u8)> {
        let layout = std::alloc::Layout::from_size_align(bytes, align).ok()?;
        let pointer = unsafe { std::alloc::alloc(layout) };
        (!pointer.is_null()).then_some((pointer as u64, pointer))
    }
    pub fn dealloc(pointer: *mut u8, bytes: usize) {
        unsafe { std::alloc::dealloc(pointer, std::alloc::Layout::from_size_align(bytes, crate::intel::WARM_ALIGN).unwrap()); }
    }
}
mod fixture {
    use crate::Mutex;
    use alloc::vec::Vec;
    pub const MAX_SURFACES: usize = 32;
    pub const ERROR_INVALID: i32 = -1;
    pub const ERROR_CONTEXT: i32 = -2;
    pub const ERROR_NOT_FOUND: i32 = -3;
    pub const ERROR_UI4: i32 = -6;
    pub const ERROR_BUSY: i32 = -7;
    pub const UI4_SCENE_SPRITE_GPU: u64 = 0x3800_0000;
    pub const UI4_SCENE_SOLID_SOURCE_BYTES: usize = 4096;
    pub const UI4_SCENE_SPRITE_MAX_BYTES: usize = 128 * 1024 * 1024;
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum WindowOwner { Kernel, Vm(u8) }
    #[derive(Clone, Copy)]
    pub struct WindowId(pub u32);
    impl WindowId { pub fn raw(self) -> u32 { self.0 } }
    #[derive(Clone, Copy)]
    pub struct GpgpuRgba8Surface { pub phys: u64, pub gpu: u64, pub bytes: usize, pub width: u32, pub height: u32, pub pitch_bytes: u32 }
    impl GpgpuRgba8Surface {
        pub fn new(phys: u64, gpu: u64, bytes: usize, width: u32, height: u32, pitch_bytes: u32) -> Option<Self> {
            Some(Self { phys, gpu, bytes, width, height, pitch_bytes })
        }
    }
    pub struct NonzeroAlphaRun;
    pub struct OwnedRgba8Surface {
        pub hull_image_budget_bytes: usize, pub opaque: bool, pub premultiplied: bool,
        pub nonzero_alpha_runs: *const NonzeroAlphaRun, pub nonzero_alpha_run_count: usize,
        pub surface: GpgpuRgba8Surface, pub virt: *mut u8, pub bytes: usize,
    }
    unsafe impl Send for OwnedRgba8Surface {}
    pub enum BlueprintSpriteSource { Uploaded(OwnedRgba8Surface) }
    impl BlueprintSpriteSource {
        fn surface(&self) -> GpgpuRgba8Surface { let Self::Uploaded(o) = self; o.surface }
        fn bytes(&self) -> usize { let Self::Uploaded(o) = self; o.bytes }
    }
    pub struct Rgba8Upload { pub owned: OwnedRgba8Surface }
    pub struct BlueprintSceneSurface {
        pub owner: WindowOwner, pub render_target: u32, pub window: WindowId,
        pub hull_images: hull_image_api::ImageResources,
        pub sprites: Vec<(u32, BlueprintSpriteSource)>, pub sprite_upload: Option<Rgba8Upload>,
        pub gpu_submission_unretired: bool,
    }
    pub static SURFACES: Mutex<Vec<BlueprintSceneSurface>> = Mutex::new(Vec::new());
    pub static SPRITE_MUTATION: Mutex<()> = Mutex::new(());
    pub fn blueprint_owner() -> Option<WindowOwner> { Some(WindowOwner::Vm(7)) }
    pub fn surface_mut(surfaces: &mut [BlueprintSceneSurface], owner: WindowOwner, window: u32) -> Option<&mut BlueprintSceneSurface> {
        surfaces.iter_mut().find(|s| s.owner == owner && s.render_target == window)
    }
    pub fn guest_status(_: u32, _: u64, _: u64, _: &[u8]) -> i32 { -6 }
    @ALLOCATE@
    @DESTROY@
    #[path = @BRIDGE_PATH@]
    pub mod hull_image_api;
}
