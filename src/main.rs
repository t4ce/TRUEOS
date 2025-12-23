#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::fmt::{self, Write};
use core::panic::PanicInfo;

extern crate alloc;

mod allocators;
mod dma;
mod gdt;
mod limine;
mod mmio;
mod pci;
mod xhci;
mod usb;
mod interrupts;
mod time;

use embassy_executor::raw::Executor;
use embassy_time::{Duration as EmbassyDuration, Timer};
use ::limine::mp::Cpu as LimineCpu;
use x86_64::instructions::interrupts as cpu_ints;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};

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
    unsafe { enable_sse(); }
    gdt::install();
    interrupts::install();
    cpu_ints::enable();

    log_limine_markers();

    dma::init_from_limine();
    dma::alloc_test_once();

    pci::enumerate_once();
    pci::log_devices_once();
    xhci::init_once();

    log_memmap_once();

    allocators::alloc_demo();

    start_aps();

    let bsp_executor = unsafe { init_bsp_executor() };
    let spawner = bsp_executor.spawner();

    usb::init_crab_controller(&spawner);
    spawner.spawn(usb_poll_task());
    
    let mut counter: u64 = 0;
    loop {
        if counter % 10000 == 0 {
            time::poll();
            unsafe { bsp_executor.poll() };
        }
        counter = counter.wrapping_add(1);
        if counter % 10_000_000 == 0 {
            debugcon_write_byte(b'0');
        }
    }
}

fn log_limine_markers() {
    if long_mode_active() {
        debugcon_write_str("64bit");
    }
    match limine::hhdm_offset() {
        Some(off) => debugconf!("LIMINE HHDM OK offset=0x{:X}\n", off),
        None => debugconf!("LIMINE HHDM MISSING\n"),
    }

    let req_ptr = &limine::MEMMAP_REQUEST as *const _ as usize;
    let resp_ptr = limine::MEMMAP_REQUEST
        .get_response()
        .map(|r| r as *const _ as usize)
        .unwrap_or(0);
    if let Some(entries) = limine::memmap_entries() {
        debugconf!(
            "LIMINE MEMMAP OK entries={} req=0x{:X} resp=0x{:X}\n",
            entries.len(),
            req_ptr,
            resp_ptr
        );
    } else {
        debugconf!(
            "LIMINE MEMMAP MISSING req=0x{:X} resp=0x{:X}\n",
            req_ptr,
            resp_ptr
        );
    }
}

unsafe extern "C" fn ap_entry(cpu: &LimineCpu) -> ! {
    let mut counter: u64 = 0;
    loop {
        counter = counter.wrapping_add(1);
        if counter % 100_000_000 == 0 {
            debugcon_write_byte(b'0' + cpu.lapic_id as u8);
        }
    }
}

fn start_aps() {
    let Some(resp) = limine::smp_response() else {
        debugconf!("smp response missing\n");
        return;
    };

    for cpu in resp.cpus() {
        cpu.goto_address.write(ap_entry);
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

pub(crate) struct DebugCon;

fn log_memmap_once() {
    let req_ptr = &limine::MEMMAP_REQUEST as *const _ as usize;
    let resp_ptr = limine::MEMMAP_REQUEST
        .get_response()
        .map(|r| r as *const _ as usize)
        .unwrap_or(0);
    if let Some(entries) = limine::memmap_entries() {
        for entry in entries {
            debugconf!(
                "memmap {:016X}-{:016X} len=0x{:X} type={}\n",
                entry.base,
                entry.base + entry.length,
                entry.length,
                limine::memmap_type_name(entry.entry_type)
            );
        }
    } 
}

impl Write for DebugCon {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        debugcon_write_str(s);
        Ok(())
    }
}

#[macro_export]
macro_rules! debugconf {
    ($($tt:tt)*) => {{
        let _ = core::fmt::write(&mut $crate::DebugCon, format_args!($($tt)*));
    }};
}

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

#[embassy_executor::task]
async fn usb_poll_task() {
    loop {
        let handled = usb::poll_crab_events_once();
        if handled {
            Timer::after(EmbassyDuration::from_micros(500)).await;
        } else {
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }
    }
}

unsafe fn enable_sse() {
    let mut cr0 = Cr0::read();
    cr0.remove(Cr0Flags::EMULATE_COPROCESSOR);
    cr0.insert(Cr0Flags::MONITOR_COPROCESSOR);
    Cr0::write(cr0);

    let mut cr4 = Cr4::read();
    cr4.insert(Cr4Flags::OSFXSR | Cr4Flags::OSXMMEXCPT_ENABLE);
    Cr4::write(cr4);
}

#[inline(always)]
fn long_mode_active() -> bool {
    const EFER_MSR: u32 = 0xC000_0080;
    const EFER_LMA_BIT: u64 = 1 << 10;

    unsafe {
        let mut lo: u32 = 0;
        let mut hi: u32 = 0;
        core::arch::asm!(
            "rdmsr",
            in("ecx") EFER_MSR,
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags)
        );
        let efer = ((hi as u64) << 32) | lo as u64;
        (efer & EFER_LMA_BIT) != 0
    }
}
