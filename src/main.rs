#![no_std]
#![no_main]

use core::panic::PanicInfo;

extern crate alloc;

mod limine;
mod pci;
mod allocators;
mod usb;

use embassy_executor::raw::Executor;
use limine::{LimineSmpCpu, LIMINE_SMP_REQUEST};

const BSP_EXECUTOR_SIZE: usize = core::mem::size_of::<Executor>();

#[repr(C, align(64))]
struct ExecutorStorage([u8; BSP_EXECUTOR_SIZE]);

#[link_section = ".data"]
static mut BSP_EXECUTOR_STORAGE: ExecutorStorage = ExecutorStorage([0xA5; BSP_EXECUTOR_SIZE]);

#[inline(always)]
unsafe fn init_bsp_executor() -> &'static Executor {
    let storage_ptr = core::ptr::addr_of_mut!(BSP_EXECUTOR_STORAGE);
    let bsp_executor_ptr = (*storage_ptr).0.as_mut_ptr() as *mut Executor;
    core::ptr::write(bsp_executor_ptr, Executor::new(core::ptr::null_mut()));
    &*bsp_executor_ptr
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Keep interrupts masked until we have handlers; xHCI may trigger MSI/INTx early.
    unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)); }
    unsafe { enable_sse(); }
    if long_mode_active() { debugcon_write_str("64bit"); } else { debugcon_write_str("32bit"); }
    start_aps();
    let (heap_ptr, heap_len) = allocators::fallback_heap_span();
    allocators::init_linked_list_heap(heap_ptr as usize, heap_len);

    let bsp_executor = unsafe { init_bsp_executor() };
    let spawner = bsp_executor.spawner();
    spawner.must_spawn(pci::pci_enumerate_task());
    spawner.must_spawn(usb::usb_poll_task());
    unsafe { bsp_executor.poll() };
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
pub(crate) fn debugcon_write_str(s: &str) {
    for &b in s.as_bytes() {
        unsafe { outb(0xE9, b) };
    }
}

#[inline(always)]
pub(crate) fn debugcon_write_byte(b: u8) {
    unsafe { outb(0xE9, b) };
}

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
fn long_mode_active() -> bool { unsafe { let mut lo:u32=0; let mut hi:u32=0; core::arch::asm!("rdmsr", in("ecx")0xC000_0080u32, out("eax")lo, out("edx")hi, options(nomem, nostack, preserves_flags)); (((hi as u64)<<32)|lo as u64) & (1<<10) != 0 } }

#[inline(always)]
unsafe fn enable_sse() {
    // Enable SSE/FXSR so Rust-generated SIMD code executes without #UD.
    const CR0_MP: u64 = 1 << 1;
    const CR0_EM: u64 = 1 << 2;
    const CR0_TS: u64 = 1 << 3;
    const CR0_NE: u64 = 1 << 5;
    const CR4_OSFXSR: u64 = 1 << 9;
    const CR4_OSXMMEXCPT: u64 = 1 << 10;

    let mut cr0: u64;
    core::arch::asm!("mov {0}, cr0", out(reg) cr0, options(nomem, preserves_flags));
    cr0 |= CR0_MP | CR0_NE;
    cr0 &= !(CR0_EM | CR0_TS);
    core::arch::asm!("mov cr0, {0}", in(reg) cr0, options(nomem, preserves_flags));

    let mut cr4: u64;
    core::arch::asm!("mov {0}, cr4", out(reg) cr4, options(nomem, preserves_flags));
    cr4 |= CR4_OSFXSR | CR4_OSXMMEXCPT;
    core::arch::asm!("mov cr4, {0}", in(reg) cr4, options(nomem, preserves_flags));
}
