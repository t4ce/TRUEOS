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
            debugcon_write_byte(b'-');
        }
    }
}

extern "C" fn ap_entry(cpu: *mut LimineSmpCpu) {
    if !cpu.is_null() {
        let cpu = unsafe { &*cpu };
        debugcon_write_byte(b'A');
        debugcon_write_hex_u8(cpu.lapic_id as u8);
    }
    let mut counter: u64 = 0;
    loop {
        counter = counter.wrapping_add(1);
        if counter % 100_000_000 == 0 {
            debugcon_write_byte(b'1');
        }
    }
}

fn start_aps() {
    let resp = LIMINE_SMP_REQUEST.response;
    if resp.is_null() {
        return;
    }
    let resp = unsafe { &*resp };
    debugcon_write_byte(b'B');
    debugcon_write_hex_u8(resp.bsp_lapic_id as u8);
    let count = resp.cpu_count as usize;
    let cpus = resp.cpus;
    if cpus.is_null() {
        return;
    }
    for idx in 0..count {
        let cpu_ptr = unsafe { *cpus.add(idx) };
        if cpu_ptr.is_null() {
            continue;
        }
        let cpu = unsafe { &mut *cpu_ptr };
        if cpu.lapic_id == resp.bsp_lapic_id {
            continue;
        }
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
fn debugcon_write_hex_u8(val: u8) {
    debugcon_write_byte(nibble_to_hex(val >> 4));
    debugcon_write_byte(nibble_to_hex(val & 0x0F));
}

#[inline(always)]
fn nibble_to_hex(val: u8) -> u8 {
    match val & 0x0F {
        0..=9 => b'0' + (val & 0x0F),
        _ => b'A' + ((val & 0x0F) - 10),
    }
}

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
fn long_mode_active() -> bool { unsafe { let mut lo:u32=0; let mut hi:u32=0; core::arch::asm!("rdmsr", in("ecx")0xC000_0080u32, out("eax")lo, out("edx")hi, options(nomem, nostack, preserves_flags)); (((hi as u64)<<32)|lo as u64) & (1<<10) != 0 } }
