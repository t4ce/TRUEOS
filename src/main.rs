#![no_std]
#![no_main]

use core::fmt::{self, Write};
use core::panic::PanicInfo;
use core::ptr::read_volatile;

extern crate alloc;

mod limine;
mod pci;
mod allocators;
mod xhci;
mod dma;

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
    if long_mode_active() { debugcon_write_str("64bit"); }

    // log_limine_markers();
    dma::init_from_limine();
    dma::alloc_test_once();

    pci::enumerate_once();
    // pci::log_devices_once();
    // log_memmap_once();
    // allocators::alloc_demo();

    if let Some((dev, bar0)) = pci::first_xhci() {
        debugconf!("xhci candidate {:02X}:{:02X}.{} bar0=0x{:X}\n", dev.bus, dev.slot, dev.function, bar0);

        let (cmd_before, status_before) = pci::read_command_status(dev.bus, dev.slot, dev.function);
        debugconf!(
            "xhci pci cmd(before)=0x{:04X} status=0x{:04X} mem={} bm={}\n",
            cmd_before,
            status_before,
            pci::command_has_mem_space(cmd_before) as u8,
            pci::command_has_bus_master(cmd_before) as u8,
        );

        pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);

        let (cmd_after, status_after) = pci::read_command_status(dev.bus, dev.slot, dev.function);
        debugconf!(
            "xhci pci cmd(after )=0x{:04X} status=0x{:04X} mem={} bm={}\n",
            cmd_after,
            status_after,
            pci::command_has_mem_space(cmd_after) as u8,
            pci::command_has_bus_master(cmd_after) as u8,
        );

        let (bar0_lo, bar0_hi) = pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
        match bar0_hi {
            Some(hi) => debugconf!("xhci bar0 raw lo=0x{:08X} hi=0x{:08X}\n", bar0_lo, hi),
            None => debugconf!("xhci bar0 raw lo=0x{:08X}\n", bar0_lo),
        }

        // Minimal, read-only MMIO probe of xHCI capability header.
        // Reads: CAPLENGTH + HCIVERSION (dword 0), HCSPARAMS1 (dword 1).
        if let Some(hhdm) = limine::hhdm_offset() {
            let mmio = (bar0 as usize).wrapping_add(hhdm as usize) as *const u32;
            unsafe {
                let cap0 = read_volatile(mmio.add(0));
                let hcs1 = read_volatile(mmio.add(1));

                let caplength = (cap0 & 0xFF) as u8;
                let hci_version = ((cap0 >> 16) & 0xFFFF) as u16;
                let max_slots = (hcs1 & 0xFF) as u8;
                let max_ports = ((hcs1 >> 24) & 0xFF) as u8;

                debugconf!(
                    "xhci mmio caplen=0x{:02X} hci_ver=0x{:04X} slots={} ports={}\n",
                    caplength,
                    hci_version,
                    max_slots,
                    max_ports
                );
            }
        } else {
            debugconf!("xhci mmio probe skipped (no HHDM)\n");
        }
    } 
    
    //start_aps();

    let bsp_executor = unsafe { init_bsp_executor() };
    let spawner = bsp_executor.spawner();

    let mut counter: u64 = 0;
    loop {
        counter = counter.wrapping_add(1);
        if counter % 100_000_000 == 0 {
            debugcon_write_byte(b'0');
        }

        if counter % 10_000_000 == 0 {
            unsafe { bsp_executor.poll() };
        }
    }
}

fn log_limine_markers() {
    match limine::hhdm_offset() {
        Some(off) => debugconf!("LIMINE HHDM OK offset=0x{:X}\n", off),
        None => debugconf!("LIMINE HHDM MISSING\n"),
    }

    let req_ptr = &limine::LIMINE_MEMMAP_REQUEST as *const _ as usize;
    let resp_ptr = unsafe { limine::LIMINE_MEMMAP_REQUEST.response as usize };
    if let Some(entries) = limine::memmap_entries() {
        debugconf!(
            "LIMINE MEMMAP OK entries={} req=0x{:X} resp=0x{:X}\n",
            entries.len(),
            req_ptr,
            resp_ptr
        );
    } else {
        debugconf!("LIMINE MEMMAP MISSING req=0x{:X} resp=0x{:X}\n", req_ptr, resp_ptr);
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
    let resp_ptr = LIMINE_SMP_REQUEST.response;
    let resp = unsafe { &*resp_ptr };
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

pub(crate) struct DebugCon;

fn log_memmap_once() {
    let req_ptr = &limine::LIMINE_MEMMAP_REQUEST as *const _ as usize;
    let resp_ptr = unsafe { limine::LIMINE_MEMMAP_REQUEST.response as usize };
    debugconf!("memmap req=0x{:X} resp=0x{:X}\n", req_ptr, resp_ptr);

    if let Some(entries) = limine::memmap_entries() {
        debugconf!("memmap entries={}\n", entries.len());
        let mut shown = 0;
        for &ptr in entries {
            if shown >= 16 { break; }
            if ptr.is_null() { continue; }
            let e = unsafe { &*ptr };
            debugconf!("memmap {:016X}-{:016X} type={}\n", e.base, e.base + e.length, e.typ);
            shown += 1;
        }
    } else {
        debugconf!("memmap unavailable\n");
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

#[inline(always)]
fn long_mode_active() -> bool {
    const EFER_MSR: u32 = 0xC000_0080;
    const EFER_LMA_BIT: u64 = 1 << 10; // Long Mode Active

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
