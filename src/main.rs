#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[repr(C)]
pub struct LimineSmpRequest {
    _id: [u64; 4],
    _revision: u64,
    response: *const LimineSmpResponse,
    flags: u64,
}

unsafe impl Sync for LimineSmpRequest {}

#[repr(C)]
pub struct LimineSmpResponse {
    revision: u64,
    flags: u32,
    bsp_lapic_id: u32,
    cpu_count: u64,
    cpus: *const *mut LimineSmpCpu,
}

#[repr(C)]
pub struct LimineSmpCpu {
    processor_id: u32,
    lapic_id: u32,
    reserved: u64,
    goto_address: extern "C" fn(*mut LimineSmpCpu),
    extra_argument: u64,
}

#[used]
#[link_section = ".limine_requests"]
static LIMINE_BASE_REVISION: [u64; 3] = [
    0xf9562b2d5c95a6c8,
    0x6a7b384944536bdc,
    0,
];

#[used]
#[link_section = ".limine_requests"]
static LIMINE_SMP_REQUEST: LimineSmpRequest = LimineSmpRequest {
    _id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x95a67b819a1b857e,
        0xa0b61b723b6a73e0,
    ],
    _revision: 0,
    response: core::ptr::null(),
    flags: 0,
};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    if long_mode_active() { debugcon_write_str("64bit"); } else { debugcon_write_str("32bit"); }
    start_aps();
    let mut counter: u64 = 0;
    loop {
        counter = counter.wrapping_add(1);
        if counter % 100_000_000 == 0 {
            debugcon_write_byte(b'0');
        }
    }
}

extern "C" fn ap_entry(cpu: *mut LimineSmpCpu) {
    if !cpu.is_null() {
        let cpu = unsafe { &*cpu };
        let mut counter: u64 = 0;
        loop {
            counter = counter.wrapping_add(1);
            if counter % 100_000_000 == 0 {
                debugcon_write_byte(b'0' + cpu.lapic_id as u8);
            }
        }
    }
}

fn start_aps() {
    let resp = unsafe { &*LIMINE_SMP_REQUEST.response };
    let count: usize = resp.cpu_count as usize;
    let cpus = resp.cpus;
    for idx in 0..count {
        let cpu_ptr = unsafe { *cpus.add(idx) };
        let cpu = unsafe { &mut *cpu_ptr };
        cpu.goto_address = ap_entry;
    }
}

#[inline(always)]
fn debugcon_write_str(s: &str) {
    for &b in s.as_bytes() {
        unsafe { outb(0xE9, b) };
    }
}

#[inline(always)]
fn debugcon_write_byte(b: u8) {
    unsafe { outb(0xE9, b) };
}

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
fn long_mode_active() -> bool { unsafe { let mut lo:u32=0; let mut hi:u32=0; core::arch::asm!("rdmsr", in("ecx")0xC000_0080u32, out("eax")lo, out("edx")hi, options(nomem, nostack, preserves_flags)); (((hi as u64)<<32)|lo as u64) & (1<<10) != 0 } }