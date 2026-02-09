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
mod runtime;
mod shell;
mod tst;
mod surface;
mod tga;
mod time;
mod wait;
mod smp;
mod turbo;
mod efi;
mod usb;
mod v;
mod vga;
mod x2apic;

pub(crate) use shell::ecma48;
pub(crate) use shell::matrix;
pub(crate) use portio::{inb, inl, inw, outb, outl, outw};
use crate::x2apic::{detect_x2apic_topology, X2ApicTopology};
use ::limine::mp::Cpu as LimineCpu;
use alloc::boxed::Box;
use alloc::vec::Vec;
use embassy_executor::{raw::Executor, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};
pub use surface::pat as pattern;
pub use surface::{io, path};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
use spin::Once;

static LOG_CPU_TOPOLOGY_ONCE: Once<()> = Once::new();
const AP_HEARTBEAT_TASK_POOL: usize = 256;

fn build_cpu_slot_lapic_order(
    resp: &::limine::response::MpResponse,
    topo: X2ApicTopology,
) -> Vec<u32> {
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

    let mut lapic_order: Vec<u32> = Vec::with_capacity(items.len() + 1);
    lapic_order.push(bsp_lapic_id);

    for (lapic_id, _) in items {
        if lapic_order.iter().any(|id| *id == lapic_id) {
            continue;
        }
        lapic_order.push(lapic_id);
    }

    lapic_order
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

    limstats::log_limine_markers(); 

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
    usb::truekey::init();

    let resp = limine::smp_response().unwrap();
    percpu::set_total_slots(resp.cpus().len() + 1);
    log_cpu_topology_once(&resp);
    smp::init(percpu::total_slots());
    smp::mark_online();

    let executor = Box::leak(Box::new(Executor::new(core::ptr::null_mut())));
    unsafe {
        (&mut *percpu::this_cpu_ptr()).set_executor_ptr(executor as *mut Executor);
    }
    let spawner = executor.spawner();

    if let Err(e) = crate::v::taskmon::spawn(&spawner, "job-runner", crate::wait::job_runner_task()) {
        crate::log!("wait: job_runner_task spawn failed: {:?}\n", e);
    }

    net::init();

    crate::v::mode::set_mode(crate::v::mode::SystemMode::Normal);

    // Spawn all Embassy tasks via the centralized v-layer spawn service.
    if let Err(e) = crate::v::taskmon::spawn(
        &spawner,
        "spawn-service",
        crate::v::spawn_service::spawn_service_task(spawner),
    ) {
        crate::log!("spawn-svc: spawn failed: {:?}\n", e);
    }

    // QuickJS smoke test (kept after net init so URL imports can work if used).
    // unsafe { qjs::trueos_smoke::run() };

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
        let lapic_order = build_cpu_slot_lapic_order(resp, topo);
        percpu::install_cpu_slot_lapic_order_owned(lapic_order);

        crate::log!(
            "cpu-topology: total={} bsp_lapic_id={} leaf={} smt_bits={} core_bits={}\n",
            percpu::total_slots(),
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
        let bsp_slot = percpu::slot_for_lapic_id(bsp_lapic_id);
        crate::log!(
            "cpu-topology: {:<4} {:>8} {:>4} {:>5} {:>4} {:>5}\n",
            "bsp", bsp_lapic_id, pkg, core, smt, bsp_slot
        );

        for cpu in resp.cpus() {
            let lapic_id = cpu.lapic_id as u32;
            let (pkg, core, smt) = topo.decode(lapic_id);
            let slot = percpu::slot_for_lapic_id(lapic_id);
            crate::log!(
                "cpu-topology: {:<4} {:>8} {:>4} {:>5} {:>4} {:>5}\n",
                "ap", lapic_id, pkg, core, smt, slot
            );
        }
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
    let slot = percpu::slot_for_lapic_id(cpu.lapic_id as u32);
    percpu::init_ap(cpu.lapic_id as u32, slot as u32);
    let ex = Box::leak(Box::new(Executor::new(core::ptr::null_mut())));
    unsafe {
        (&mut *percpu::this_cpu_ptr()).set_executor_ptr(ex as *mut Executor);
    }
    let spawner = ex.spawner();
    if percpu::this_cpu().cpu_index() == 1 {
        runtime::register_first_ap_spawner(spawner);
    }
    if let Err(e) = spawner.spawn(ap_heartbeat_task()) {
        crate::log!("ap: heartbeat task spawn failed: {:?}\n", e);
    }
    crate::smp::mark_online();
    exceptions::load_this_cpu();
    runtime::run_ap_forever()
}

#[embassy_executor::task(pool_size = AP_HEARTBEAT_TASK_POOL)]
async fn ap_heartbeat_task() {
    loop {
        Timer::after(EmbassyDuration::from_secs(5)).await;
        let slot = percpu::this_cpu().cpu_index() as u8;
        let mark = if slot < 10 {
            b'0' + slot
        } else {
            b'A' + ((slot - 10) % 26)
        };
        globalog::debugcon_write_byte_raw(mark);
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
