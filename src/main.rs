#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(f16)]
#![allow(unsafe_op_in_unsafe_fn)]

const _: f16 = 0.0_f16;

#[macro_use]
pub extern crate alloc;

// Modules
mod allcaps;
mod allocators;
pub mod allports;
mod app_db;
mod aud;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[path = "hv/blueprint/blueprint_net_broker.rs"]
mod blueprint_net_broker;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[path = "hv/blueprint/blueprint_net_wire.rs"]
mod blueprint_net_wire;
mod blueprint_shims;
#[path = "Chronos.rs"]
mod chronos;
mod cpu;
mod crypt;
mod disc;
pub mod dma;
mod efi;
mod efi_img;
mod exceptions;
mod executor_cache;
mod executor_task_profile;
mod gpu;
#[path = "../crates/trueos-graphics/mod.rs"]
mod graphics;
mod hv;
mod intel;
#[path = "intel/sound/intel_hda_audio_demo.rs"]
mod intel_hda_audio_demo;
mod iso9660;
mod limine;
mod live_update;
mod locale;
mod log_os;
#[cfg(feature = "trueos_lumen")]
mod lumen;
mod microcode;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod mio_compat;
mod mio_probe;
mod net;
mod pci;
mod percpu;
mod phys;
mod pmu;
mod portio;
mod power;
mod r;
mod ram_probe;
mod ram_usage;
mod release_count;
mod remote_work_wake;
mod runtime;
mod shell2;
mod smp;
mod spirit;
mod std_abi_shim;
mod surfer;
mod turbo;
#[allow(non_snake_case)]
mod tyche;
mod uart1_com1;
mod ui4;
mod unix_abi_shim;
mod unix_compat;
mod unix_fd_probe;
#[path = "usb3/mod.rs"]
pub(crate) mod usb3;
mod user_input_record;
mod virtio_gpu_logo;
mod wait;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod wls;
mod workers;
mod x2apic;
mod z7;

// Re-exports
pub(crate) use crate::intel::hda;
pub(crate) use portio::{inb, inl, inw, outb, outl, outw};
pub use r::pat as pattern;
pub use r::time;
pub use r::{io, path};
pub(crate) use usb3 as usb2;

// Imports
use trueos_executor::{Spawner, raw::Executor};

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

#[unsafe(no_mangle)]
pub extern "C" fn kmain() -> ! {
    unsafe {
        cpu::enable_sse();
    }
    if live_update::warm_boot_active() {
        // First candidate-side proof: direct volatile CPU writes into the
        // preserved Limine framebuffer, before every runtime subsystem.
        let _ = virtio_gpu_logo::stamp_warm_candidate_entry_cross();
    }
    log_os::init_global_dispatch();
    // Blueprint modules may import Ring's prefixed native implementation
    // symbols directly. Keep the Rust crate linked; build.rs retains and
    // publishes the native routines through the runtime import resolver.
    core::hint::black_box(&ring::digest::SHA256);
    live_update::log_boot_mode();
    crate::log_info!(
        target: "global";
        "boot: stage=bsp-early log_config boot_level={:?} gfx_level={:?} gpgpu_level={:?} render_level={:?} helio_gfx_diag={} ui4_diag={}\n",
        crate::log_os::flags::BOOT_LOG_LEVEL,
        crate::log_os::flags::GFX_LOG_LEVEL,
        crate::log_os::flags::GPGPU_LOG_LEVEL,
        crate::log_os::flags::RENDER_LOG_LEVEL,
        crate::log_os::flags::HELIO_GFX_DIAG_PROFILE_ENABLED as u8,
        crate::log_os::flags::UI4_DIAG_PROFILE_ENABLED as u8,
    );
    exceptions::init();
    if crate::log_os::flags::BOOT_INFO_LOGS {
        crate::log!("long_mode_active: {}\n", cpu::long_mode_active());
    }
    phys::register_memory_metadata();
    phys::init_pmm_from_limine();
    limine::prime_bootloader_caches();
    locale::prime_bootloader_timezone();

    if !phys::try_install_heap_arena_candidates(allocators::install_heap_arena) {
        crate::log!("heap: failed to reserve/install any heap arena\n");
    }

    match app_db::init_bsp() {
        Ok(count) => crate::log_info!(
            target: "apps";
            "app.db: initialized storage=ram builtins={} persistence=none\n",
            count
        ),
        Err(err) => {
            crate::log_error!(target: "apps"; "app.db: initialization failed err={}\n", err)
        }
    }

    if crate::log_os::flags::BOOT_INFO_LOGS
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
    microcode::init_from_limine_bsp();
    dma::init_from_limine();
    pci::enumerate_impl();
    log_os::set_emulator_uart_logging(intel::is_emulator_environment());
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

    net::init();
    #[cfg(target_os = "trueos")]
    aud::alsa_trueos_backend::install();

    if crate::allcaps::probes::MIO_BOOT_PROBE {
        mio_probe::log_boot_probe();
    } else {
        mio_probe::assume_ready_when_probe_disabled();
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
    let sse42 = crate::r::pat::sse42_available();
    crate::log_info!(
        target: "boot";
        "cpu-string-search: sse4.2={} pcmpestri={} fallback=memmem\n",
        if sse42 { "yes" } else { "no" },
        if sse42 { "enabled" } else { "disabled" }
    );
    if live_update::warm_boot_active() {
        live_update::release_warm_aps();
    } else {
        boot_secondary_processors(smp_resp);
    }
    spawn_bsp_services(spawner);
    live_update::spawn_post_boot(spawner);
    _loop(executor)
}

fn boot_secondary_processors(resp: Option<&'static crate::limine::MpResponse>) {
    if let Some(resp) = resp {
        resp.cpus()
            .iter()
            .filter(|c| limine::mp_cpu_id(c) != percpu::this_cpu().lapic_id())
            .for_each(|c| c.bootstrap(cpu::ap_start, 0));
    }
}

fn spawn_bsp_services(spawner: Spawner) {
    if crate::allcaps::executor::BSP_TASK_PROFILE_ENABLED {
        match crate::executor_task_profile::bsp_task_profile_reporter_task(spawner) {
            Ok(token) => spawner.spawn(token),
            Err(e) => crate::log!("bsp-taskmon: reporter spawn failed err={:?}\n", e),
        }
    }
    match crate::r::spawn_service::spawn_service_task(spawner) {
        Ok(token) => spawner.spawn(token),
        Err(e) => crate::log!("spawn-svc: spawn failed: {:?}\n", e),
    }
}

fn _loop(executor: &'static Executor) -> ! {
    loop {
        time::poll();
        // Keep the per-CPU executor-poll guard authoritative for BSP tasks.
        // Do not call `executor.poll()` directly here: that hid the fact that a
        // BSP task was recursively polling its own executor through synchronous
        // kfs access. Blocking filesystem callers belong on leased AP service
        // lanes and cross into the BSP through the TRUEOSFS request broker.
        debug_assert!(core::ptr::eq(executor, unsafe { &*percpu::this_cpu().executor_ptr() },));
        runtime::poll_local_executor();
        service_pending_bsp_interrupts();
        //if counter.is_multiple_of(10_000_000) {
        //    log_os::debugcon_write_byte_raw(b'0');
        //}
        core::hint::spin_loop()
    }
}

/// Admit pending hardware interrupts at a controlled boundary in the BSP's
/// polling scheduler.
///
/// AP runtimes already open an interrupt window with `sti; hlt`, but the BSP
/// never halts and historically left IF clear for its entire executor loop.
/// Keep interrupts masked while Rust tasks and their locks are being polled;
/// the instruction after `sti` consumes the architectural interrupt shadow,
/// allowing a pending vector to enter before `cli` closes the window again.
#[inline(always)]
fn service_pending_bsp_interrupts() {
    unsafe {
        core::arch::asm!("sti", "nop", "cli", options(nomem, nostack));
    }
}
