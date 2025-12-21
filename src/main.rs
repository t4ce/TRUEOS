#![no_std]
#![no_main]

use core::panic::PanicInfo;

mod limine;

use embassy_executor::raw::Executor;
use limine::{LimineSmpCpu, LIMINE_SMP_REQUEST};

const BSP_EXECUTOR_SIZE: usize = core::mem::size_of::<Executor>();

#[repr(C, align(64))]
struct ExecutorStorage([u8; BSP_EXECUTOR_SIZE]);

// Keep this in a file-backed section so it is mapped even if the loader fails
// to allocate/map the PT_LOAD memsz>filesz (".bss") tail early on.
#[link_section = ".data"]
static mut BSP_EXECUTOR_STORAGE: ExecutorStorage = ExecutorStorage([0xA5; BSP_EXECUTOR_SIZE]);

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    if long_mode_active() { debugcon_write_str("64bit"); } else { debugcon_write_str("32bit"); }
    start_aps();

    let storage_ptr = core::ptr::addr_of_mut!(BSP_EXECUTOR_STORAGE);
    let bsp_executor_ptr = unsafe { (*storage_ptr).0.as_mut_ptr() as *mut Executor };
    unsafe { core::ptr::write(bsp_executor_ptr, Executor::new(core::ptr::null_mut())) };
    let bsp_executor = unsafe { &*bsp_executor_ptr };
    
    let mut counter: u64 = 0;
    loop {
        counter = counter.wrapping_add(1);
        if counter % 100_000_000 == 0 {
            debugcon_write_byte(b'0');
            unsafe { bsp_executor.poll() };
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
