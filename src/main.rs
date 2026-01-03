/*
██████████████████████████████████████████████████████████████████████
██░        ░░       ░░░  ░░░░  ░░        ░░░░░░░░░      ░░░░      ░░██
██▒▒▒▒  ▒▒▒▒▒  ▒▒▒▒  ▒▒  ▒▒▒▒  ▒▒  ▒▒▒▒▒▒▒▒▒▒▒▒▒▒  ▒▒▒▒  ▒▒  ▒▒▒▒▒▒▒██
██▓▓▓▓  ▓▓▓▓▓       ▓▓▓  ▓▓▓▓  ▓▓      ▓▓▓▓▓▓▓▓▓▓  ▓▓▓▓  ▓▓▓      ▓▓██
██████  █████  ███  ███  ████  ██  ██████████████  ████  ████████  ███
██████  █████  ████  ███      ███        █████████      ████      ████
██████████████████████████████████████████████████████████████████████
A Rust Based 64 Bit Paged X84 Baremetal OS Targeted at Intel and GOWIN

Think of rust as the world’s quiet, slow-moving “entropy tax”:
A constant drain of resources, money, and safety.

Think of FalseOS as the world’s fast-moving “entropy dividend”:
A constant influx of resources, money, and safety.
*/

#![no_std]
#![no_main]
#![feature(alloc_error_handler, f16, f128)]

pub extern crate alloc;

mod acpi;
mod allocators;
mod backtrace;
mod disc;
mod limine;
mod limstats;
mod pci;
mod percpu;
mod phys;
mod portio;
mod rng;
mod serial;
mod globalog;
mod surface;
mod tga;
mod time;
mod turbo;
mod uefi;
mod usb;
mod vga;
pub(crate) use portio::{inb, inl, inw, outb, outl, outw};
use crate::usb::usb_scout;
use ::limine::mp::Cpu as LimineCpu;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};
use alloc::boxed::Box;
use embassy_executor::{raw::Executor, Spawner};
pub use surface::pat as pattern;
pub use surface::{io, path, strings};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};

static TOTAL_SLOTS: AtomicUsize = AtomicUsize::new(0);

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {enable_sse();}

    globalog::Globalog::init_log_shim();

    vga::init(limine::framebuffer_response());

    limstats::log_limine_markers(); //limstats::log_memmap_once();

    phys::register_memory_metadata();
    phys::init_pmm_from_limine();

    // If booted via UEFI, parse+log the EFI System Table once.
    // uefi::log_system_table_once(); // its crashreboots on our baremetal testrig

    const KIB: usize = 1024;
    const MIB: usize = 1024 * KIB;
    const GIB: usize = 1024 * MIB;
    const HEAP_ALIGN: usize = 2 * MIB;
    const HEAP_CANDIDATES: [usize; 7] = [GIB, 512 * MIB, 256 * MIB, 128 * MIB,64 * MIB, 32 * MIB, 16 * MIB];
    for &size in HEAP_CANDIDATES.iter() {
        if let Some(arena) = phys::reserve_heap_arena(size, HEAP_ALIGN) {
            if allocators::install_heap_arena(arena) {
                break;
            }
        }
    }

    percpu::init_bsp();

    io::smoke_test();
    strings::smoke_test();
    path::smoke_test();
    pattern::smoke_test();

    crate::debugconf!(
        "turbo: {:?}\n", turbo::local_state()
    );

    pci::dma::init_from_limine();
    pci::dma::alloc_test_once();
    pci::enumerate_once();
    pci::log_devices_once();
    disc::probe_once();
    tga::init_once();

    acpi::ensure_tables();
    acpi::facp::log_once();
    acpi::tpm2::log_once();
    acpi::dmar::log_once();
    acpi::fpdt::log_once();
    acpi::uefi_tbl::log_once();
    acpi::ssdt::log_once();
    acpi::bgrt::log_once();
    acpi::hpet::ensure();

    rng::log_rng_caps();

    usb::xhci::init_once();

    let resp = limine::smp_response().unwrap();

    TOTAL_SLOTS.store(resp.cpus().len(), Ordering::Release);
    for cpu in resp.cpus() {
        cpu.goto_address.write(ap_entry);
    }

    let executor = Box::leak(Box::new(Executor::new(core::ptr::null_mut())));
    let spawner = executor.spawner();

    if tga::is_online() {
        let _ = spawner.spawn(tga::blink_task());
    }

    // reads from hardware into dma buffs
    if let Some(info) = usb::xhci::xhc_info() {
        let _ = spawner.spawn(usb::xhci::poll_task(info));
    }

    // reads from our dma buffs into usb rings
    if let Some(info) = usb::xhci::xhc_info() {
        let _ = spawner.spawn(usb::poll_task(info));
    }

    // Enumerate USB devices once. Re-running this while poll tasks are active
    // reprograms the controller and can disrupt in-flight transfers.
    if let Some(info) = usb::xhci::xhc_info() {
        let _ = spawner.spawn(usb_scout(info));
    }

    let _ = spawner.spawn(usb::hid::input_logger());

    disc::files::create_demo_file(); //needs hardware qemu param i guess

    _loop(executor, spawner)
}

fn _loop(executor: &'static Executor, spawner: Spawner) -> ! {
    let mut counter: u64 = 0;
    loop {
        if counter % 10_000 == 0 {
            time::poll();
            unsafe { executor.poll() };
        }

        if counter % 1_000_000 == 0 {
            vga::cube::tick();
        }

        // Periodic rescan for hotplug. Safe because `usb_scout` is now init-once + rescan.
        if counter % 100_000_000 == 0 {
            globalog::Globalog::debugcon_write_byte_raw(b'0');
            if let Some(info) = usb::xhci::xhc_info() {
                let _ = spawner.spawn(usb_scout(info));
            }
        }

        counter = counter.wrapping_add(1);
    }
}

unsafe extern "C" fn ap_entry(cpu: &LimineCpu) -> ! {
    enable_sse();
    let total = TOTAL_SLOTS.load(Ordering::Acquire);
    let slot = (cpu.lapic_id as usize) % total;
    percpu::init_ap(cpu.lapic_id as u32, slot as u32);
    ap_loop(cpu.lapic_id as u32, total, slot)
}

fn ap_loop(lapic_id: u32, total: usize, slot: usize) -> ! {
    let mut counter: u64 = 0;
    loop {
        if counter % 10_000_000 == 0 {
            vga::draw_header_square(
                total,
                slot,
                vga::DEFAULT_SHADOW_COLOR,
                (counter % 360) as u32,
            );
        }
        if counter % 100_000_000 == 0 {
            globalog::Globalog::debugcon_write_byte_raw(b'0' + lapic_id as u8);
        }
        counter = counter.wrapping_add(1);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { core::arch::asm!("cli", options(nomem, nostack)) };
    backtrace::print(64);
    let mut counter: u64 = 0;
    loop {
        counter = counter.wrapping_add(1);
        if counter % 100_000_000 == 0 {
            globalog::Globalog::debugcon_write_byte_raw(b'!');
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
pub(crate) fn long_mode_active() -> bool {
    use x86_64::registers::model_specific::Msr;
    const IA32_EFER: u32 = 0xC000_0080;
    const EFER_LMA_BIT: u64 = 1 << 10;
    let efer = unsafe { Msr::new(IA32_EFER).read() };
    (efer & EFER_LMA_BIT) != 0
}