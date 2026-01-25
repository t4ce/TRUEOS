/* TRUE OS (§) ® 2026
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

Think of TRUE OS as the world’s fast-moving “entropy dividend”:
A constant influx of resources, money, and safety.
*/

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt, alloc_error_handler, f16, f128)]

pub extern crate alloc;

mod acpi;
mod allocators;
mod audio;
mod backtrace;
mod disc;
mod exceptions;
mod limine;
mod limstats;
mod net;
mod pci;
mod percpu;
mod phys;
mod portio;
mod rng;
mod serial;
mod power;
mod globalog;
mod matrix;
mod shell;
mod shellqjs;
mod install;
mod shellcube;
mod ecma48;
mod txtedt;
mod surface;
mod tga;
mod time;
mod turbo;
mod efi;
mod usb;
mod vga;
mod x2apic;

pub(crate) use portio::{inb, inl, inw, outb, outl, outw};
use crate::usb::usb_scout_service;
use crate::x2apic::{detect_x2apic_topology, X2ApicTopology};
use ::limine::mp::Cpu as LimineCpu;
use core::ffi::c_char;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use alloc::boxed::Box;
use alloc::vec::Vec;
use embassy_executor::{raw::Executor, Spawner};
use trueos_qjs as qjs;
pub use surface::pat as pattern;
pub use surface::{io, path, strings};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
use spin::Once;

fn trueos_math_smoke_test() {
    crate::log!("trueos-math: smoke test begin\n");

    let a = trueos_math::Complex::new(3.0, 4.0);
    let b = trueos_math::Complex::new(1.0, 2.0);

    let sum = a.add(b);
    if sum == trueos_math::Complex::new(4.0, 6.0) {
        crate::log!("trueos-math: add ok\n");
    } else {
        crate::log!("trueos-math: add FAIL {:?}\n", sum);
    }

    let sq = a.square();
    if sq == trueos_math::Complex::new(-7.0, 24.0) {
        crate::log!("trueos-math: square ok\n");
    } else {
        crate::log!("trueos-math: square FAIL {:?}\n", sq);
    }

    let mag2 = a.magnitude_squared();
    if mag2 == 25.0 {
        crate::log!("trueos-math: magnitude_squared ok\n");
    } else {
        crate::log!("trueos-math: magnitude_squared FAIL {}\n", mag2);
    }

    crate::log!("trueos-math: smoke test end\n");
}

static TOTAL_SLOTS: AtomicUsize = AtomicUsize::new(0);
static CPU_SLOT_TABLE: AtomicPtr<CpuSlot> = AtomicPtr::new(core::ptr::null_mut());
static CPU_SLOT_LEN: AtomicUsize = AtomicUsize::new(0);

static RENDER_MANDELBROT_ONCE: Once<()> = Once::new();
static LOG_CPU_TOPOLOGY_ONCE: Once<()> = Once::new();

const MANDELBROT_W: usize = 256;
const MANDELBROT_H: usize = 256;

#[repr(C)]
#[derive(Copy, Clone)]
struct CpuSlot {
    lapic_id: u32,
    slot: u32,
}

#[inline]
fn cpu_slot_table() -> &'static [CpuSlot] {
    let len = CPU_SLOT_LEN.load(Ordering::Acquire);
    let ptr = CPU_SLOT_TABLE.load(Ordering::Acquire);
    if ptr.is_null() || len == 0 {
        return &[];
    }
    unsafe { core::slice::from_raw_parts(ptr, len) }
}

#[inline]
fn slot_for_lapic_id(lapic_id: u32, total: usize) -> usize {
    let slots = cpu_slot_table();
    if !slots.is_empty() {
        for entry in slots {
            if entry.lapic_id == lapic_id {
                return entry.slot as usize;
            }
        }
    }
    if total == 0 {
        0
    } else {
        (lapic_id as usize) % total
    }
}

fn slot_for_lapic_id_in_slots(lapic_id: u32, slots: &[CpuSlot]) -> u32 {
    for entry in slots {
        if entry.lapic_id == lapic_id {
            return entry.slot;
        }
    }
    0
}

fn install_cpu_slot_table_owned(slots: Vec<CpuSlot>) {
    let len = slots.len();
    let mut boxed = slots.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    core::mem::forget(boxed);
    CPU_SLOT_TABLE.store(ptr, Ordering::Release);
    CPU_SLOT_LEN.store(len, Ordering::Release);
    TOTAL_SLOTS.store(len, Ordering::Release);
}

fn build_cpu_slots(resp: &::limine::response::MpResponse, topo: X2ApicTopology) -> Vec<CpuSlot> {
    let mut items: Vec<(u32, (u32, u32, u32))> = Vec::new();
    let bsp_lapic_id = percpu::this_cpu().lapic_id();
    items.push((bsp_lapic_id, topo.decode(bsp_lapic_id)));

    for cpu in resp.cpus() {
        let lapic_id = cpu.lapic_id as u32;
        items.push((lapic_id, topo.decode(lapic_id)));
    }

    items.sort_by(|a, b| {
        let (a_id, (a_pkg, a_core, a_smt)) = *a;
        let (b_id, (b_pkg, b_core, b_smt)) = *b;
        (a_pkg, a_core, a_smt, a_id).cmp(&(b_pkg, b_core, b_smt, b_id))
    });

    let mut slots: Vec<CpuSlot> = Vec::with_capacity(items.len());
    for (lapic_id, _) in items {
        if slots.iter().any(|s| s.lapic_id == lapic_id) {
            continue;
        }
        let slot = slots.len() as u32;
        slots.push(CpuSlot { lapic_id, slot });
    }

    slots
}

#[link_section = ".bss"]
static mut MANDELBROT_PIXELS: [u32; MANDELBROT_W * MANDELBROT_H] = [0; MANDELBROT_W * MANDELBROT_H];

// Bootloader-provided stacks can be very small; debug builds can need a lot more
// stack than expected very early (before heap/logging is fully online).
// Provide a known-good BSP stack and switch to it immediately in `_start`.
const BSP_BOOT_STACK_BYTES: usize = 8 * 1024 * 1024;

#[repr(align(16))]
struct BootStack([u8; BSP_BOOT_STACK_BYTES]);

#[link_section = ".bss"]
static mut BSP_BOOT_STACK: BootStack = BootStack([0; BSP_BOOT_STACK_BYTES]);
/*
ConPink 	FF_55_FF 
ConBlue 	08_18_30
ConWhite 	FF_FF_FF
*/

#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "lea rsp, [rip + {stack} + {stack_size}]",
        // 16-byte align RSP for SysV ABI.
        "and rsp, -16",
        // Use `call` (not `jmp`) so the callee sees the expected stack
        // alignment (RSP % 16 == 8 at function entry). Some Rust/C code
        // assumes this and will fault on unaligned `movaps` spills.
        "call {kmain}",
        "ud2",
        stack = sym BSP_BOOT_STACK,
        stack_size = const BSP_BOOT_STACK_BYTES,
        kmain = sym kmain,
    );
}

#[no_mangle]
pub extern "C" fn kmain() -> ! {
    unsafe {enable_sse();}
    exceptions::init();

    vga::init(limine::framebuffer_response());

    limstats::log_limine_markers(); //limstats::log_memmap_once();

    phys::register_memory_metadata();
    phys::init_pmm_from_limine();


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

    vga::init_font_cache();

    percpu::init_bsp();

    io::smoke_test();
    strings::smoke_test();
    path::smoke_test();
    pattern::smoke_test();
    trueos_math_smoke_test();
    
    // If booted via UEFI, parse+log the EFI System Table once.
    let dumped_uefi_system_table = efi::log_system_table_once(); // its crashreboots on our baremetal testrig

    crate::log!(
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
    if !dumped_uefi_system_table {
        efi::tbl::log_once();
    }
    acpi::ssdt::log_once();
    acpi::bgrt::log_once();
    acpi::hpet::ensure();

    rng::log_rng_caps();
    power::init();

    usb::xhci::init_once();

    // Optional: initialize TrueKey (CDC-based ESP32 binding) before enumeration.
    usb::truekey::configure_target_serial("9C:13:9E:E4:25:B8");
    usb::truekey::init();
    usb::cdc_shell::init();

    let resp = limine::smp_response().unwrap();
    TOTAL_SLOTS.store(resp.cpus().len() + 1, Ordering::Release);
    log_cpu_topology_once(&resp);

    let executor = Box::leak(Box::new(Executor::new(core::ptr::null_mut())));
    let spawner = executor.spawner();

    time::init(executor);

    quickjs_smoke_test();

    net::init();
    let net_ready = net::mac_address().is_some();
    if net_ready {
        let _ = spawner.spawn(net::adapter::net_service_task());
        let _ = spawner.spawn(net::adapter::net_smoke_task());
        let _ = spawner.spawn(net::adapter::net_shell_task());
    } else {
        crate::log!("net: skipping net tasks (no NIC)\n");
    }

    if tga::is_online() {
        let _ = spawner.spawn(tga::blink_task());
    }

    for info in usb::xhci::xhc_list().iter().copied() {
        // reads from hardware into dma buffs
        let _ = spawner.spawn(usb::xhci::poll_task(info));

        // reads from our dma buffs into usb rings
        let _ = spawner.spawn(usb::poll_task(info));

        // Single long-lived scout per controller. Rescans are triggered via a flag.
        let _ = spawner.spawn(usb_scout_service(info));
    }

    let _ = spawner.spawn(usb::hid::input_logger());

    let _ = spawner.spawn(usb::uac::sine_task());

    // Continuously drains the TrueKey log cache to the ESP32 when bound.
    let _ = spawner.spawn(usb::truekey::drain_loop());

    let _ = spawner.spawn(disc::files::fatfs_usb_demo_task());

    let _ = spawner.spawn(shell::task(spawner, &shell::UART1_COM1_BACKEND));
    let _ = spawner.spawn(shell::task(spawner, &shell::USB_CDC_SHELL_BACKEND));
    if net_ready {
        let _ = spawner.spawn(shell::task(spawner, &shell::NET_TCP_SHELL_BACKEND));
    }
    
    let bsp_lapic_id = percpu::this_cpu().lapic_id();
    for cpu in resp.cpus() {
        if cpu.lapic_id as u32 == bsp_lapic_id {
            continue;
        }
        cpu.goto_address.write(ap_start);
    }

    crate::log!("main: entering executor loop\n");

    _loop(executor, spawner)
}

fn quickjs_smoke_test() {
    unsafe { qjs::trueos_smoke::run() }
}

fn log_cpu_topology_once(resp: &::limine::response::MpResponse) {
    LOG_CPU_TOPOLOGY_ONCE.call_once(|| {
        let topo = detect_x2apic_topology();
        let slots = build_cpu_slots(resp, topo);

        crate::log!(
            "cpu-topology: total={} bsp_lapic_id={} leaf={} smt_bits={} core_bits={}\n",
            TOTAL_SLOTS.load(Ordering::Acquire),
            percpu::this_cpu().lapic_id(),
            topo.leaf,
            topo.smt_bits,
            topo.core_bits
        );
        crate::log!(
            "cpu-topology: role  lapic_id  pkg  core  smt  slot\n"
        );

        let bsp_lapic_id = percpu::this_cpu().lapic_id();
        let (pkg, core, smt) = topo.decode(bsp_lapic_id);
        let bsp_slot = slot_for_lapic_id_in_slots(bsp_lapic_id, &slots);
        crate::log!(
            "cpu-topology: {:<4} {:>8} {:>4} {:>5} {:>4} {:>5}\n",
            "bsp", bsp_lapic_id, pkg, core, smt, bsp_slot
        );

        for cpu in resp.cpus() {
            let lapic_id = cpu.lapic_id as u32;
            let (pkg, core, smt) = topo.decode(lapic_id);
            let slot = slot_for_lapic_id_in_slots(lapic_id, &slots);
            crate::log!(
                "cpu-topology: {:<4} {:>8} {:>4} {:>5} {:>4} {:>5}\n",
                "ap", lapic_id, pkg, core, smt, slot
            );
        }

        install_cpu_slot_table_owned(slots);
    });
}

pub(crate) fn draw_mandelbrot() {
    let Some((fb_w, fb_h)) = vga::framebuffer_dimensions() else {
        return;
    };

    let fb_w = fb_w as usize;
    let fb_h = fb_h as usize;
    let w = MANDELBROT_W;
    let h = MANDELBROT_H;
    let expected = w * h;

    // Rendering is the expensive part; do it once.
    RENDER_MANDELBROT_ONCE.call_once(|| unsafe {
        trueos_math::render_mandelbrot_rgb32(&mut MANDELBROT_PIXELS[..expected], w, h, 64);
    });

    unsafe {
        let img = vga::Image {
            width: w,
            height: h,
            pixels: &MANDELBROT_PIXELS[..expected],
        };

        let (origin_x, origin_y) = match acpi::bgrt::last_logo_rect() {
            Some((logo_x, logo_y, logo_w, _logo_h)) => {
                let x = logo_x.saturating_add(logo_w).saturating_sub(w);
                let y = logo_y.saturating_sub(h);
                (x, y)
            }
            None => (fb_w, fb_h),
        };
        let _ = vga::blit_image(origin_x, origin_y, &img);
    }
}

fn _loop(executor: &'static Executor, spawner: Spawner) -> ! {
    let mut counter: u64 = 0;
    loop {
        if counter % 10_000 == 0 {
            time::poll();
            unsafe { executor.poll() };
        }

        if counter % 500_000 == 0 {
            vga::cube::tick();
        }

        if counter % 10_000_000 == 0 {
            globalog::debugcon_write_byte_raw(b'0');
        }

        counter = counter.wrapping_add(1);
        power::idle_hint();
    }
}

unsafe extern "C" fn ap_start(cpu: &LimineCpu) -> ! {
    enable_sse();
    let total = TOTAL_SLOTS.load(Ordering::Acquire);
    let slot = slot_for_lapic_id(cpu.lapic_id as u32, total);
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
            //globalog::debugcon_write_byte_raw(b'0' + lapic_id as u8);
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
            globalog::debugcon_write_byte_raw(b'!');
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
