#![no_std]

extern crate alloc;

#[cfg(target_arch = "x86_64")]
pub mod demo;
#[cfg(target_arch = "x86_64")]
pub mod guest;
pub mod stream;
pub mod v;
#[cfg(target_arch = "x86_64")]
mod vfetch_job;
#[cfg(target_arch = "x86_64")]
pub mod vmcall;
#[cfg(target_arch = "x86_64")]
pub mod vpanic;

#[cfg(not(target_arch = "x86_64"))]
pub mod guest {
    #[derive(Copy, Clone, Debug)]
    pub struct HullImageLayout {
        pub text_start: u64,
        pub text_end: u64,
        pub rodata_start: u64,
        pub rodata_end: u64,
        pub data_start: u64,
        pub data_end: u64,
        pub vmcall_bss_start: u64,
        pub vmcall_bss_end: u64,
        pub vpanic_bss_start: u64,
        pub vpanic_bss_end: u64,
        pub demo_bss_start: u64,
        pub demo_bss_end: u64,
        pub bss_start: u64,
        pub bss_end: u64,
    }

    pub fn hull_image_layout() -> HullImageLayout {
        HullImageLayout {
            text_start: 0,
            text_end: 0,
            rodata_start: 0,
            rodata_end: 0,
            data_start: 0,
            data_end: 0,
            vmcall_bss_start: 0,
            vmcall_bss_end: 0,
            vpanic_bss_start: 0,
            vpanic_bss_end: 0,
            demo_bss_start: 0,
            demo_bss_end: 0,
            bss_start: 0,
            bss_end: 0,
        }
    }

    pub fn hull_image_bounds() -> (u64, u64) {
        (0, 0)
    }

    pub unsafe fn entry() -> ! {
        loop {
            core::hint::spin_loop();
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub mod vmcall {
    pub const OP_PRESERVE: u32 = 0x01;
    pub const OP_PING: u32 = 0x02;
    pub const OP_UNIX_TIME: u32 = 0x03;
    pub const OP_MONOTONIC_NANOS: u32 = 0x08;
    pub const OP_BP_CPU_COUNT: u32 = 0x07;
    pub const OP_BP_UI3_FRAME_CREATE: u32 = 0x82;
    pub const OP_BP_UI3_FRAME_CLOSE: u32 = 0x83;
    pub const OP_BP_UI3_FRAME_REQUEST_REPAINT: u32 = 0x84;
    pub const OP_BP_UI3_FRAME_SET_POSITION: u32 = 0x85;
    pub const OP_BP_UI3_FRAME_SET_SIZE: u32 = 0x86;
    pub const OP_BP_UI3_FRAME_BEGIN: u32 = 0x87;
    pub const OP_BP_UI3_FRAME_END: u32 = 0x88;
    pub const OP_BP_UI3_FRAME_SET_RENDER_TARGET: u32 = 0x89;
    pub const OP_BP_UI3_FRAME_DRAW_SOLID_BATCH: u32 = 0x8A;
    pub const OP_BP_UI3_FRAME_DRAW_SPRITE_BATCH: u32 = 0x8B;
    pub const OP_BP_UI3_TEXTURE_UPLOAD_BEGIN: u32 = 0x8C;
    pub const OP_BP_UI3_TEXTURE_UPLOAD_CHUNK: u32 = 0x8D;
    pub const OP_BP_UI3_TEXTURE_UPLOAD_FINISH: u32 = 0x8E;
    pub const OP_BP_UI3_TEXTURE_STATUS: u32 = 0x8F;
    pub const OP_BP_UI3_TEXTURE_DIMENSIONS: u32 = 0x90;
    pub const OP_NET_TCP_WRITE: u32 = 0x10;
    pub const OP_NET_TCP_READ: u32 = 0x11;
    pub const OP_BP_NET_OPEN: u32 = 0x20;
    pub const OP_BP_NET_SUBMIT: u32 = 0x21;
    pub const OP_BP_NET_POLL: u32 = 0x22;
    pub const OP_BP_FETCH_BYTES_START: u32 = 0x23;
    pub const OP_BP_FETCH_BYTES_RESULT_LEN: u32 = 0x24;
    pub const OP_BP_FETCH_BYTES_READ: u32 = 0x25;
    pub const OP_BP_FETCH_BYTES_DISCARD: u32 = 0x26;
    pub const OP_BP_THREAD_CURRENT_ID: u32 = 0x61;
    pub const OP_BP_TOKIO_BLOCKING_SPAWN: u32 = 0x62;
    pub const OP_BP_LEGACY_FRAME_CREATE: u32 = 0x63;
    pub const OP_BP_LEGACY_FRAME_OP: u32 = 0x64;
    pub const OP_BP_GFX_TEXTURE_UPLOAD_BEGIN: u32 = 0x65;
    pub const OP_BP_GFX_TEXTURE_UPLOAD_CHUNK: u32 = 0x66;
    pub const OP_BP_GFX_TEXTURE_UPLOAD_FINISH: u32 = 0x67;
    pub const OP_BP_GFX_TEXTURE_DIMENSIONS: u32 = 0x70;
    pub const OP_BP_GFX_QUEUE_RENDER_RGB: u32 = 0x71;
    pub const OP_BP_GFX_QUEUE_RENDER_TEX: u32 = 0x72;
    pub const OP_BP_GFX_QUEUE_RENDER_MANDELBROT: u32 = 0x73;
    pub const OP_BP_GFX_QUEUE_RENDER_BEGIN: u32 = 0x74;
    pub const OP_BP_GFX_QUEUE_RENDER_CHUNK: u32 = 0x75;
    pub const OP_BP_GFX_QUEUE_RENDER_FINISH: u32 = 0x76;
    pub const OP_BP_GFX_TEXTURE_STATUS: u32 = 0x77;
    pub const OP_BP_INPUT_CURSOR_POS: u32 = 0x68;
    pub const OP_BP_INPUT_CURSOR_BUTTONS: u32 = 0x69;
    pub const OP_BP_INPUT_CURSOR_EVENTS: u32 = 0x6A;
    pub const OP_BP_ENV_ALL: u32 = 0x6E;
    pub const OP_BP_FS_LIST_TREE: u32 = 0x6F;
    pub const OP_BP_FS_LIST_DIR: u32 = 0x81;

    pub const STATUS_OK: u32 = 0;
    pub const STATUS_BAD_ARG: u32 = 2;
    pub const PAYLOAD_CAP: usize = 4096 - 56;

    pub fn hull_bss_anchor() -> u64 {
        0
    }

    pub fn call(_op: u32, _arg0: u64, _arg1: u64) -> (u32, u64) {
        (STATUS_BAD_ARG, 0)
    }

    pub fn cpu_count() -> Option<usize> {
        None
    }

    pub fn call_with_payload(
        _op: u32,
        _arg0: u64,
        _arg1: u64,
        _req: &[u8],
        _out: &mut [u8],
    ) -> (u32, u64) {
        (STATUS_BAD_ARG, 0)
    }

    pub fn ping() -> bool {
        false
    }

    pub fn unix_time() -> u64 {
        0
    }

    pub fn monotonic_nanos() -> u64 {
        0
    }

    pub fn net_tcp_write(_bytes: &[u8]) -> usize {
        0
    }

    pub fn net_tcp_read(_out: &mut [u8]) -> usize {
        0
    }

    pub fn preserve() {}
}

#[cfg(not(target_arch = "x86_64"))]
pub mod vpanic {
    pub fn set_stage(_stage: u32) {}
    pub fn stage() -> u32 {
        0
    }
    pub fn hull_bss_anchor() -> u64 {
        0
    }
    pub fn note(_tag: &str) {}
    pub fn dump(_tag: &str) {}
}
