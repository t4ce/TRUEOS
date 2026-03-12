#![no_std]
#![no_main]
#![feature(abi_x86_interrupt, alloc_error_handler, f16, f128)]
// Rust 2024: `unsafe fn` bodies are safe-by-default; allow legacy style for now.
#![allow(unsafe_op_in_unsafe_fn)]

#[macro_use]
pub extern crate alloc;

mod allocators;
mod audio;
mod cpu;
mod disc;
mod efi;
mod exceptions;
mod gfx;
mod globalog;
mod hv;
mod iso9660;
mod limine;
mod net;
mod pci;
mod percpu;
mod phys;
mod portio;
mod power;
mod rng;
mod runtime;
mod shell;
mod smp;
mod surface;
mod tga;
mod time;
#[path = "tst/html.rs"]
mod tst_html;
#[path = "tst/http_trueosfs.rs"]
mod tst_http_trueosfs;
mod turbo;
mod usb;
#[path = "tst/usvg_smoke.rs"]
mod usvg_smoke;
mod v;
mod vga;
mod wait;
mod x2apic;
mod z7;

use embassy_executor::{Spawner, raw::Executor};
pub(crate) use portio::{inb, inl, inw, outb, outl, outw};
pub(crate) use shell::ecma48;
pub(crate) use shell::matrix;
pub use surface::pat as pattern;
pub use surface::{io, path};

fn qjs_font_atlas_small_provider() -> trueos_qjs::FontAtlasView<'static> {
    let atlas = crate::gfx::text::font_atlas_small_view();
    trueos_qjs::FontAtlasView {
        alpha: atlas.alpha,
        index: atlas.index,
        widths: atlas.widths,
        width: atlas.width,
        height: atlas.height,
        cell_w: atlas.cell_w,
        cell_h: atlas.cell_h,
        grid_w: atlas.grid_w,
        grid_h: atlas.grid_h,
    }
}

fn qjs_font_atlas_large_provider() -> trueos_qjs::FontAtlasView<'static> {
    let atlas = crate::gfx::text::font_atlas_large_view();
    trueos_qjs::FontAtlasView {
        alpha: atlas.alpha,
        index: atlas.index,
        widths: atlas.widths,
        width: atlas.width,
        height: atlas.height,
        cell_w: atlas.cell_w,
        cell_h: atlas.cell_h,
        grid_w: atlas.grid_w,
        grid_h: atlas.grid_h,
    }
}

// Provide a known-good BSP stack and switch to it immediately in `_start` for bigger stack
const BSP_BOOT_STACK_BYTES: usize = 8 * 1024 * 1024;

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
        "call {kmain}",
        "ud2",
        stack = sym BSP_BOOT_STACK,
        stack_size = const BSP_BOOT_STACK_BYTES,
        kmain = sym kmain,
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn kmain() -> ! {
    unsafe {
        cpu::enable_sse();
    }
    exceptions::init();
    crate::log!("long_mode_active: {}\n", cpu::long_mode_active());
    phys::register_memory_metadata();
    phys::init_pmm_from_limine();

    if !phys::try_install_heap_arena_candidates(allocators::install_heap_arena) {
        crate::log!("heap: failed to reserve/install any heap arena\n");
    }

    if let Some(perf) = limine::bootloader_performance() {
        crate::log!(
            "Boot Performance: reset={}_usec init={}_usec exec={}_usec\n",
            perf.reset_usec(),
            perf.init_usec(),
            perf.exec_usec()
        );
    }
    let smp_resp = limine::smp_response().unwrap();
    let lapic_ids: alloc::vec::Vec<u32> = smp_resp.cpus().iter().map(|c| c.lapic_id).collect();
    percpu::install_cpu_slot_lapic_order_owned(lapic_ids);
    percpu::init_bsp();
    pci::dma::init_from_limine();
    pci::enumerate_impl();
    #[cfg(feature = "gfx_intel")]
    gfx::intel::init_once();
    vga::init(limine::framebuffer_response());
    vga::cube::tick();
    trueos_qjs::set_font_atlas_small_provider(qjs_font_atlas_small_provider);
    trueos_qjs::set_font_atlas_large_provider(qjs_font_atlas_large_provider);

    usb::xhci::init_once();
    usb::truekey::init();
    pci::vrng::init_once();
    pci::vrng::smoke_test_once();
    crate::rng::init();
    usvg_smoke::run();
    disc::probe_once();
    efi::acpi::ensure_tables();
    efi::acpi::hpet::ensure();
    power::init();
    smp::init(percpu::total_slots());
    smp::mark_online();

    let executor = percpu::init_executor();
    let spawner = executor.spawner();

    // Register BSP spawner for affinity-first worker placement.
    trueos_qjs::workers::register_core_spawner(
        percpu::this_cpu().cpu_index(),
        cpu::intel_core_kind_hint(),
        spawner,
    );
    if let Err(e) = spawner.spawn(crate::wait::job_runner_task()) {
        crate::log!("wait: job_runner_task spawn failed: {:?}\n", e);
    }

    if trueos_qjs::async_fs::ensure_service_started(&spawner) {
        crate::v::readiness::set(crate::v::readiness::QJS_ASYNC_FS_READY);
    }

    // Worker spawners for APs are registered in `cpu::ap_start` once each AP brings up its executor.
    tga::init_once();
    net::init();

    #[cfg(feature = "dma_nic_fpga")]
    {
        match pci::nic_fpga_dma::init_default_once() {
            Ok(region) => {
                crate::log!(
                    "dma_nic_fpga: region phys=0x{:X} virt=0x{:X} size=0x{:X}\n",
                    region.phys_base,
                    region.virt_base,
                    region.size
                );
            }
            Err(e) => crate::log!("dma_nic_fpga: init failed: {:?}\n", e),
        }
    }
    _loop(executor, spawner, smp_resp)
}

fn _loop(
    executor: &'static Executor,
    _spawner: Spawner,
    resp: &'static ::limine::response::MpResponse,
) -> ! {
    resp.cpus()
        .iter()
        .filter(|c| c.lapic_id != percpu::this_cpu().lapic_id())
        .for_each(|c| c.goto_address.write(cpu::ap_start));

    if let Err(e) = _spawner.spawn(crate::v::spawn_service::spawn_service_task(_spawner)) {
        crate::log!("spawn-svc: spawn failed: {:?}\n", e);
    }

    let mut counter: u64 = 0;
    loop {
        if counter.is_multiple_of(10_000) {
            time::poll();
            unsafe { executor.poll() };
        }
        if counter.is_multiple_of(250_000) {
            vga::cube::tick();
        }
        if counter.is_multiple_of(10_000_000) {
            globalog::debugcon_write_byte_raw(b'0');
        }
        counter = counter.wrapping_add(1);
        power::idle_hint();
    }
}
