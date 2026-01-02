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
mod debugcon;
mod disc;
mod limine;
mod limlog;
mod pci;
mod percpu;
mod phys;
mod portio;
mod rng;
mod serial;
mod truelog;
mod surface;
mod tga;
mod time;
mod turbo;
mod uefi;
mod usb;
mod vga;
pub(crate) use portio::{inb, inl, inw, outb, outl, outw};

pub(crate) use crate::surface as std;

use crate::pci::mmio;
use crate::usb::usb_scout;
use ::acpi::sdt::hpet;
use ::limine::mp::Cpu as LimineCpu;
use core::{cell::UnsafeCell, mem::MaybeUninit};
use core::panic::PanicInfo;
use embassy_executor::{raw::Executor, Spawner};
use spin::Once;
pub use surface::pat as pattern;
pub use surface::{io, path, strings};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};

static SMP_RESP: Once<&'static ::limine::response::MpResponse> = Once::new();

#[repr(align(64))]
struct ExecStorage(UnsafeCell<MaybeUninit<Executor>>);

unsafe impl Sync for ExecStorage {}

static STORAGE: ExecStorage = ExecStorage(UnsafeCell::new(MaybeUninit::uninit()));

#[inline(always)]
unsafe fn init_bsp_executor() -> &'static Executor {
    let bsp_executor_ptr = (*STORAGE.0.get()).as_mut_ptr();
    bsp_executor_ptr.write(Executor::new(core::ptr::null_mut()));
    &*bsp_executor_ptr
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {enable_sse();}

    truelog::init_log_shim();

    vga::init(limine::framebuffer_response());

    limlog::log_limine_markers(); //limlog::log_memmap_once();

    phys::register_memory_metadata();
    phys::init_pmm_from_limine();

    // If booted via UEFI, parse+log the EFI System Table once.
    // uefi::log_system_table_once(); // its crashreboots on our baremetal testrig

    const HEAP_CANDIDATES: [usize; 7] = [
        1024 * 1024 * 1024,
        512 * 1024 * 1024,
        256 * 1024 * 1024,
        128 * 1024 * 1024,
        64 * 1024 * 1024,
        32 * 1024 * 1024,
        16 * 1024 * 1024,
    ];
    let mut heap_ready = false;
    for &size in HEAP_CANDIDATES.iter() {
        if let Some(arena) = phys::reserve_heap_arena(size, 2 * 1024 * 1024) {
            if allocators::install_heap_arena(arena) {
                heap_ready = true;
                break;
            }
        }
    }
    if !heap_ready {
        crate::debugconf!(
            "heap: fallback ({} KiB) active\n",
            allocators::FALLBACK_HEAP_SIZE / 1024
        );
    }

    percpu::init_bsp();

    crate::io::smoke_test();
    crate::strings::smoke_test();
    crate::path::smoke_test();
    crate::pattern::smoke_test();

    let desired_turbo = turbo::desired_state();
    let local_turbo = turbo::local_state();
    crate::debugconf!(
        "turbo: desired={:?} local={:?}\n",
        desired_turbo,
        local_turbo
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

    let resp = *SMP_RESP.call_once(|| limine::smp_response().expect("LIMINE SMP MISSING"));
    for cpu in resp.cpus() {
        cpu.goto_address.write(ap_entry);
    }

    let executor = unsafe { init_bsp_executor() };
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
            debugcon::debugcon_write_byte_raw(b'0');
            if let Some(info) = usb::xhci::xhc_info() {
                let _ = spawner.spawn(usb_scout(info));
            }
        }

        counter = counter.wrapping_add(1);
    }
}

unsafe extern "C" fn ap_entry(cpu: &LimineCpu) -> ! {
    // floating-point math (SSE) needs per core enabling
    enable_sse();

    let total_slots = SMP_RESP.get().expect("SMP response missing").cpus().len();

    let slot = (cpu.lapic_id as usize) % total_slots;

    percpu::init_ap(cpu.lapic_id as u32, slot as u32);

    let mut counter: u64 = 0;
    loop {
        if counter % 10_000_000 == 0 {
            vga::draw_header_square(
                total_slots,
                slot,
                vga::DEFAULT_SHADOW_COLOR,
                (counter % 360) as u32,
            );
        }
        if counter % 100_000_000 == 0 {
            debugcon::debugcon_write_byte_raw(b'0' + cpu.lapic_id as u8);
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
            debugcon::debugcon_write_byte(b'!');
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
