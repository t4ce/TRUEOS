#![no_std]
#![no_main]

use core::{fmt::{self, Write}, panic::PanicInfo};

extern crate alloc;

mod allocators;
mod dma;
mod gdt;
mod limine;
mod mmio;
mod pci;
mod xhci;
mod osal;
mod usb;
mod time;


use embassy_executor::{raw::Executor, Spawner};
use ::limine::mp::Cpu as LimineCpu;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
use spin::Once;

use crate::usb::usb_scout;

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

    log_limine_markers();

    dma::init_from_limine();
    dma::alloc_test_once();

    pci::enumerate_once();
    //pci::log_devices_once();
    xhci::init_once();

    //log_memmap_once();

    allocators::alloc_demo();

    start_aps();

    let bsp_executor = unsafe { init_bsp_executor() };
    let spawner = bsp_executor.spawner();

    Once::new().call_once(|| {
        if let Some(info) = xhci::controller_info() {
            spawner.spawn(usb_scout(info));
        }
    });

    if let Some(info) = xhci::controller_info() {
        let _ = spawner.spawn(xhci::controller_poll_task(info));
    } else {
        debugconf!("xhci: poll task skipped (no controller info)\n");
    }

    let mut counter: u64 = 0;
    loop {
        if counter % 10000 == 0 {
            time::poll();
            unsafe { bsp_executor.poll() };
        }
        counter = counter.wrapping_add(1);
        if counter % 10_000_000 == 0 {
            debugcon_write_byte(b'0');
            //log_hpet_counter_once();
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

fn log_hpet_counter_once() {
    const HPET_BASE: u64 = 0xFED0_0000;
    const HPET_CFG_OFFSET: usize = 0x10;
    const HPET_MAIN_COUNTER_OFFSET: usize = 0xF0;

    let region = match mmio::map_mmio_region(HPET_BASE, 0x1000) {
        Ok(r) => r,
        Err(e) => {
            debugconf!("HPET map failed: {:?}\n", e);
            return;
        }
    };

    unsafe {
        let base = region.as_ptr();
        let cfg = base.add(HPET_CFG_OFFSET) as *mut u64;
        let counter = base.add(HPET_MAIN_COUNTER_OFFSET) as *const u64;

        let mut current_cfg = cfg.read_volatile();
        current_cfg |= 1; // enable main counter
        cfg.write_volatile(current_cfg);

        let ticks = counter.read_volatile();
        debugconf!("HPET counter=0x{:016X}\n", ticks);
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
