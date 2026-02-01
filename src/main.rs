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
mod power;
mod globalog;
mod shell;
pub(crate) use shell::ecma48;
pub(crate) use shell::matrix;
mod tst;
mod surface;
mod tga;
mod time;
mod smp;
mod turbo;
mod efi;
mod usb;
mod v;
mod vga;
mod x2apic;

pub(crate) use portio::{inb, inl, inw, outb, outl, outw};
use crate::usb::usb_scout_service;
use crate::x2apic::{detect_x2apic_topology, X2ApicTopology};
use ::limine::mp::Cpu as LimineCpu;
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

static TOTAL_SLOTS: AtomicUsize = AtomicUsize::new(0);
static CPU_SLOT_TABLE: AtomicPtr<CpuSlot> = AtomicPtr::new(core::ptr::null_mut());
static CPU_SLOT_LEN: AtomicUsize = AtomicUsize::new(0);
static LOG_CPU_TOPOLOGY_ONCE: Once<()> = Once::new();

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
    // Important invariant for per-CPU mailboxes and other "slot indexed" data:
    // BSP must always be slot 0.
    let bsp_lapic_id = percpu::this_cpu().lapic_id();

    let mut items: Vec<(u32, (u32, u32, u32))> = Vec::new();
    for cpu in resp.cpus() {
        let lapic_id = cpu.lapic_id as u32;
        if lapic_id == bsp_lapic_id {
            continue;
        }
        items.push((lapic_id, topo.decode(lapic_id)));
    }

    items.sort_by(|a, b| {
        let (a_id, (a_pkg, a_core, a_smt)) = *a;
        let (b_id, (b_pkg, b_core, b_smt)) = *b;
        (a_pkg, a_core, a_smt, a_id).cmp(&(b_pkg, b_core, b_smt, b_id))
    });

    let mut slots: Vec<CpuSlot> = Vec::with_capacity(items.len() + 1);
    slots.push(CpuSlot {
        lapic_id: bsp_lapic_id,
        slot: 0,
    });

    for (lapic_id, _) in items {
        if slots.iter().any(|s| s.lapic_id == lapic_id) {
            continue;
        }
        let slot = slots.len() as u32;
        slots.push(CpuSlot { lapic_id, slot });
    }

    slots
}

// Bootloader-provided stacks can be very small; debug builds can need a lot more
// stack than expected very early (before heap/logging is fully online).
// Provide a known-good BSP stack and switch to it immediately in `_start`.
const BSP_BOOT_STACK_BYTES: usize = 8 * 1024 * 1024;

#[repr(align(16))]
struct BootStack {
    _bytes: [u8; BSP_BOOT_STACK_BYTES],
}

#[link_section = ".bss"]
static mut BSP_BOOT_STACK: BootStack = BootStack {
    _bytes: [0; BSP_BOOT_STACK_BYTES],
};

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

    if !phys::try_install_heap_arena_candidates(allocators::install_heap_arena) {
        crate::log!("heap: failed to reserve/install any heap arena\n");
    }

    percpu::init_bsp();

    io::smoke_test();
    
    pci::dma::init_from_limine();
    pci::dma::alloc_test_once();
    pci::enumerate_once();
    pci::log_devices_once();
    pci::vrng::init_once();
    pci::vrng::smoke_test_once();

    // Seed the kernel CSPRNG once we have our PCI entropy sources (virtio-rng).
    #[cfg(target_arch = "x86_64")]
    {
        let ok = crate::rng::init();
        crate::log!("rng: init {}\n", if ok { "ok" } else { "failed" });
    }
    disc::probe_once();
    tga::init_once();

    efi::acpi::ensure_tables();
    efi::acpi::log_once();
    efi::log_once();
    efi::acpi::hpet::ensure();

    power::init();
    usb::xhci::init_once();
    usb::xhci::install_block_on_hook();
    usb::truekey::init();

    let resp = limine::smp_response().unwrap();
    TOTAL_SLOTS.store(resp.cpus().len() + 1, Ordering::Release);
    log_cpu_topology_once(&resp);
    smp::init(resp.cpus().len() + 1);
    smp::mark_online();

    let executor = Box::leak(Box::new(Executor::new(core::ptr::null_mut())));
    let spawner = executor.spawner();

    // Defer font cache build; early boot can proceed without it.
    // VGA text rendering will become active once the cache is ready.
    if let Err(e) = spawner.spawn(vga::init_font_cache_task()) {
        crate::log!("vga: spawn init_font_cache_task failed: {:?}\n", e);
    }

    // Runs only after being requested (e.g. when USBMS registers).
    let _ = spawner.spawn(crate::tst::smoke_fs::bsp_smoke_service_task());
    // Handles deferred TRUEOSFS probing/mount requests from hotplug drivers.
    let _ = spawner.spawn(crate::v::fs::trueosfs::mount_service_task());

	// Serves QuickJS Promise-based async filesystem operations.
	let _ = spawner.spawn(io::cabi::qjs_async_fs_service_task());

    time::init(executor);

    net::init();
    let net_ready = net::mac_address().is_some();
    if net_ready {
        let count = net::device_count();
        for idx in 0..count {
            if let Err(e) = spawner.spawn(net::adapter::net_poll_task(idx)) {
                crate::log!("net: spawn net_poll_task({}) failed: {:?}\n", idx, e);
            }
        }

        if let Err(e) = spawner.spawn(net::adapter::net_service_task()) {
            crate::log!("net: spawn net_service_task failed: {:?}\n", e);
        }

        if let Err(e) = spawner.spawn(net::tls_socket::tls_socket_service_task()) {
            crate::log!("tls-socket: spawn tls_socket_service_task failed: {:?}\n", e);
        }

        // NOTE: Boot-time plaintext HTTP smoke probe removed.

        crate::log!("net-shell: spawning tcp listener on 4245\n");
        if let Err(e) = spawner.spawn(net::adapter::net_shell_task()) {
            crate::log!("net-shell: spawn net_shell_task failed: {:?}\n", e);
        }

        if let Err(e) = spawner.spawn(crate::v::net::http_trueosfs_task()) {
            crate::log!("http-trueosfs: spawn http_trueosfs_task failed: {:?}\n", e);
        }
    } else {
        crate::log!("net: skipping net tasks (no NIC)\n");
    }

    // QuickJS smoke test (kept after net init so URL imports can work if used).
    unsafe { qjs::trueos_smoke::run() };

    let _ = spawner.spawn(tga::blink_task());

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

    // Virtual LED multiplexer/manager (best-effort; no-op when the controller is absent).
    let _ = spawner.spawn(v::leds::task());

    // Periodic LED color cycle (drives a virtual LED once per second).
    let _ = spawner.spawn(v::leds::color_cycle_task());

    // Continuously drains the TrueKey log cache when bound (requires truekey to be configured).
    let _ = spawner.spawn(usb::truekey::drain_loop());

	// Boot-time smoke test for the CDN fetch-to-file layer (prints rc + FS_ERR_*/NET_ERR_*).
	if net_ready {
         let _ = spawner.spawn(tst::boot_fetch_to_file_smoke_task());
        // NOTE: leave the heavier cheerio smoke test opt-in; it can add significant
        // DNS/TLS pressure during early boot and mask unrelated network issues.
        // let _ = spawner.spawn(tst::boot_cheerio_smoke_task());
        if let Err(e) = spawner.spawn(pci::pciids::boot_cache_pci_ids_task()) {
            crate::log!("pciids: spawn boot_cache_pci_ids_task failed: {:?}\n", e);
        }
	}

    if let Err(e) = spawner.spawn(shell::task(spawner, &shell::UART1_COM1_BACKEND)) {
        crate::log!("shell: spawn UART shell failed: {:?}\n", e);
    }
    if net_ready {
        if let Err(e) = spawner.spawn(shell::task(spawner, &shell::NET_TCP_SHELL_BACKEND)) {
            crate::log!("shell: spawn net TCP shell failed: {:?}\n", e);
        }
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

fn _loop(executor: &'static Executor, _spawner: Spawner) -> ! {
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
    crate::smp::mark_online();
    exceptions::load_this_cpu();
    ap_loop(cpu.lapic_id as u32, total, slot)
}

//     let executor = Box::leak(Box::new(Executor::new(core::ptr::null_mut())));
//     let spawner = executor.spawner();
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
            crate::smp::poll();
        }
        if counter % 100_000_000 == 0 {
            globalog::debugcon_write_byte_raw(b'0' + lapic_id as u8);
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
