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
    pub const OP_NET_TCP_WRITE: u32 = 0x10;
    pub const OP_NET_TCP_READ: u32 = 0x11;
    pub const OP_BP_NET_OPEN: u32 = 0x20;
    pub const OP_BP_NET_SUBMIT: u32 = 0x21;
    pub const OP_BP_NET_POLL: u32 = 0x22;

    pub const STATUS_OK: u32 = 0;
    pub const STATUS_BAD_ARG: u32 = 2;
    pub const PAYLOAD_CAP: usize = 4096 - 56;

    pub fn hull_bss_anchor() -> u64 {
        0
    }

    pub fn call(_op: u32, _arg0: u64, _arg1: u64) -> (u32, u64) {
        (STATUS_BAD_ARG, 0)
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
