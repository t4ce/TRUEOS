#![no_std]
#![no_main]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]
#![feature(f16)]
#![allow(unsafe_op_in_unsafe_fn)]

const _: f16 = 0.0_f16;

#[macro_use]
pub extern crate alloc;

mod allcaps;
mod allocators;
pub mod allports;
mod aud;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[path = "hv/blueprint/blueprint_net_broker.rs"]
mod blueprint_net_broker;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[path = "hv/blueprint/blueprint_net_wire.rs"]
mod blueprint_net_wire;
#[path = "Chronos.rs"]
mod chronos;
mod cpu;
mod disc;
pub mod dma;
mod efi;
mod efi_img;
#[cfg(target_arch = "x86_64")]
mod exceptions;
#[cfg(not(target_arch = "x86_64"))]
#[path = "exceptions_disabled.rs"]
mod exceptions;
#[path = "tst/ui2/gboi.rs"]
pub(crate) mod gboi;
mod gfx;
mod globalog;
#[cfg(target_arch = "x86_64")]
mod hv;
#[cfg(not(target_arch = "x86_64"))]
#[path = "hv_disabled.rs"]
mod hv;
mod hyper_probe;
mod intel;
#[path = "intel/intel_hda_audio_demo.rs"]
mod intel_hda_audio_demo;
mod iso9660;
mod limine;
mod locale;
mod logflag;
mod lumen;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod mio_compat;
mod mio_probe;
mod net;
mod pci;
mod percpu;
mod phys;
mod portio;
#[cfg(target_arch = "x86_64")]
mod power;
#[cfg(not(target_arch = "x86_64"))]
#[path = "power/power_disabled.rs"]
mod power;
mod r;
mod runtime;
mod shell2;
mod smp;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod stackkeeper;
mod std_abi_shim;
mod surfer;
mod t;
mod tga;
mod tst;
#[path = "tst/boot_factory_ram_probe.rs"]
mod tst_boot_factory_ram_probe;
#[path = "tst/http_trueosfs.rs"]
mod tst_http_trueosfs;
#[path = "intel/gpgpu/mandelbrot_gpu_sidequest.rs"]
mod tst_mandelbrot_gpu_sidequest;
#[path = "tst/net_tcp_shell.rs"]
mod tst_net_tcp_shell;
#[path = "tst/ws_time.rs"]
mod tst_ws_time;
mod turbo;
#[allow(non_snake_case)]
mod tyche;
#[path = "USB3/mod.rs"]
pub(crate) mod usb3;
pub(crate) use usb3 as usb2;
mod wait;
mod workers;
mod x2apic;
mod z7;

pub(crate) use crate::intel::hda;

use embassy_executor::{Spawner, raw::Executor};
pub(crate) use portio::{inb, inl, inw, outb, outl, outw};
pub use r::pat as pattern;
pub use r::time;
pub use r::{io, path};

// Provide a known-good BSP stack and switch to it immediately in `_start` for bigger stack
const BSP_BOOT_STACK_BYTES: usize = crate::allcaps::boot::BSP_BOOT_STACK_BYTES;

#[repr(align(16))]
struct BootStack {
    _bytes: [u8; BSP_BOOT_STACK_BYTES],
}

#[unsafe(link_section = ".bss")]
static mut BSP_BOOT_STACK: BootStack = BootStack {
    _bytes: [0; BSP_BOOT_STACK_BYTES],
};

// only the person that deeply understands the root complex, is allowed to touch this fn
#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "lea rsp, [rip + {stack} + {stack_size}]",
        // 16-byte align RSP for SysV ABI.
        "and rsp, -16",
        // Use `call` (not `jmp`) so the callee sees the expected stack
        // alignment (RSP % 16 == 8 at function entry). Some Rust/C code
        // assumes this and will fault on unaligned `movaps` spills.
        "call {main}",
        "ud2",
        stack = sym BSP_BOOT_STACK,
        stack_size = const BSP_BOOT_STACK_BYTES,
        main = sym kmain,
    );
}

#[cfg(target_arch = "aarch64")]
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "adrp x0, {stack}",
        "add x0, x0, :lo12:{stack}",
        "movz x1, #{stack_size_0}",
        "movk x1, #{stack_size_16}, lsl #16",
        "movk x1, #{stack_size_32}, lsl #32",
        "add x0, x0, x1",
        "and x0, x0, #0xfffffffffffffff0",
        "mov sp, x0",
        "bl {main}",
        "brk #0",
        stack = sym BSP_BOOT_STACK,
        stack_size_0 = const (BSP_BOOT_STACK_BYTES & 0xffff),
        stack_size_16 = const ((BSP_BOOT_STACK_BYTES >> 16) & 0xffff),
        stack_size_32 = const ((BSP_BOOT_STACK_BYTES >> 32) & 0xffff),
        main = sym kmain,
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn kmain() -> ! {
    unsafe {
        cpu::enable_sse();
    }
    globalog::init_log_facade();
    exceptions::init();
    if crate::logflag::BOOT_INFO_LOGS {
        crate::log!("long_mode_active: {}\n", cpu::long_mode_active());
    }
    phys::register_memory_metadata();
    phys::init_pmm_from_limine();
    limine::prime_bootloader_caches();

    if !phys::try_install_heap_arena_candidates(allocators::install_heap_arena) {
        crate::log!("heap: failed to reserve/install any heap arena\n");
    }

    if crate::logflag::BOOT_INFO_LOGS
        && let Some(perf) = limine::bootloader_performance()
    {
        crate::log!(
            "Boot Performance: reset={}_usec init={}_usec exec={}_usec\n",
            perf.reset_usec,
            perf.init_usec,
            perf.exec_usec
        );
    }
    let smp_resp = limine::smp_response();
    let lapic_ids: alloc::vec::Vec<u32> = if let Some(smp_resp) = smp_resp {
        smp_resp
            .cpus()
            .iter()
            .map(|c| limine::mp_cpu_id(c))
            .collect()
    } else {
        alloc::vec![0]
    };
    percpu::install_cpu_slot_lapic_order_owned(lapic_ids);
    cpu::init_profiles(percpu::total_slots());
    percpu::init_bsp();
    dma::init_from_limine();
    pci::enumerate_impl();
    intel::init_once();
    if intel::has_claimed_device() {
        let _ = hda::boot_probe_once();
    }

    //vga::cube::tick();

    pci::vrng::init_once();
    //pci::vrng::smoke_test_once();
    crate::tyche::init();

    disc::probe_once();
    efi::acpi::ensure_tables();
    efi::log_reset_runtime_once();

    // Chronos awake hpet dependend
    efi::acpi::hpet::ensure();
    chronos::awake();
    // i hope fmt dont make this syntax 2 row

    power::init();
    smp::init(percpu::total_slots());
    smp::mark_online();

    let executor = percpu::init_executor();
    let spawner = executor.spawner();

    let _ = cpu::register_current_worker_spawner(spawner);
    // Worker spawners for APs are registered in `cpu::ap_start` once each AP brings up its executor.

    tga::init_once();
    net::init();

    if crate::allcaps::probes::TOKIO_BOOT_PROBE {
        t::tokio_probe::log_boot_probe();
    } else {
        t::tokio_probe::assume_ready_when_probe_disabled();
    }
    if crate::allcaps::probes::MIO_BOOT_PROBE {
        mio_probe::log_boot_probe();
    } else {
        mio_probe::assume_ready_when_probe_disabled();
    }
    if crate::allcaps::probes::HYPER_BOOT_PROBE {
        hyper_probe::log_boot_probe();
    }
    match pci::nic_fpga_dma::init_default_once() {
        Ok(region) => {
            crate::log_info!(
                target: "boot";
                "dma_nic_fpga: region phys=0x{:X} virt=0x{:X} size=0x{:X}\n",
                region.phys_base,
                region.virt_base,
                region.size
            );
        }
        Err(e) => crate::log_warn!(target: "boot"; "dma_nic_fpga: init failed: {:?}\n", e),
    }
    let simd = cpu::simd_status();
    crate::log_info!(
        target: "boot";
        "cpu-simd: avx-state={} reason={} avx2-fma={} reason={}\n",
        if simd.avx_state_enabled { "yes" } else { "no" },
        simd.avx_state_reason.as_str(),
        if simd.avx2_fma_ready { "yes" } else { "no" },
        simd.avx2_fma_reason.as_str()
    );
    match crate::turbo::avx2_fma_sse2_help::bf16_helper_boot_exercise_task() {
        Ok(token) => spawner.spawn(token),
        Err(e) => crate::log!("lumen-simd-help: bf16 helper boot exercise spawn failed: {:?}\n", e),
    }
    _loop(executor, spawner, smp_resp)
}

fn _loop(
    executor: &'static Executor,
    _spawner: Spawner,
    resp: Option<&'static crate::limine::MpResponse>,
) -> ! {
    if let Some(resp) = resp {
        resp.cpus()
            .iter()
            .filter(|c| limine::mp_cpu_id(c) != percpu::this_cpu().lapic_id())
            .for_each(|c| c.bootstrap(cpu::ap_start, 0));
    }

    match crate::r::spawn_service::spawn_service_task(_spawner) {
        Ok(token) => _spawner.spawn(token),
        Err(e) => crate::log!("spawn-svc: spawn failed: {:?}\n", e),
    }

    let mut counter: u64 = 0;
    loop {
        time::poll();
        unsafe { executor.poll() };
        if counter.is_multiple_of(5_000) {
            let _ = crate::tst::ui2::coreticks_demo::ui2_coreticks_tick_tile_index(0);
        }
        if counter.is_multiple_of(10_000_000) {
            globalog::debugcon_write_byte_raw(b'0');
        }
        counter = counter.wrapping_add(1);
        power::idle_hint();
    }
}
