#![no_std]
#![no_main]
#![feature(abi_x86_interrupt, f16)]
#![allow(unsafe_op_in_unsafe_fn)]

const _: f16 = 0.0_f16;

#[macro_use]
pub extern crate alloc;

mod allocators;
mod cpu;
mod disc;
pub mod dma;
mod efi;
mod exceptions;
mod gfx;
mod globalog;
mod host_api;
mod hv;
#[cfg(feature = "hvv")]
pub mod hvv;
#[cfg(feature = "gfx_intel")]
mod intel;
mod iso9660;
mod limine;
mod logflag;
mod net;
mod pci;
mod percpu;
mod phys;
mod portal;
mod portio;
mod power;
mod r;
mod rng;
mod runtime;
mod shell2;
mod smp;
mod tga;
#[path = "tst/gfx_tetris.rs"]
mod tst_gfx_tetris;
#[path = "tst/html_shack.rs"]
mod tst_html_shack;
#[path = "tst/http_trueosfs.rs"]
mod tst_http_trueosfs;
#[path = "tst/net_tcp_shell.rs"]
mod tst_net_tcp_shell;
#[path = "tst/smtp_smoke.rs"]
mod tst_smtp_smoke;
#[path = "tst/ui2_mandelbrot_demo.rs"]
mod tst_ui2_mandelbrot_demo;
#[path = "tst/ui2_triangle_demo.rs"]
mod tst_ui2_triangle_demo;
#[path = "tst/ws_time.rs"]
mod tst_ws_time;
mod turbo;
mod usb2;
//mod vga;
mod wait;
mod x2apic;
mod z7;

use embassy_executor::{Spawner, raw::Executor};
pub(crate) use portio::{inb, inl, inw, outb, outl, outw};
pub use r::pat as pattern;
pub use r::time;
pub use r::{io, path};

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
        "call {dispatch}",
        "ud2",
        stack = sym BSP_BOOT_STACK,
        stack_size = const BSP_BOOT_STACK_BYTES,
        dispatch = sym start_dispatch,
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn start_dispatch() -> ! {
    if crate::hv::guest_boot_take() {
        unsafe { crate::hv::guest::entry() }
    } else {
        kmain()
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn kmain() -> ! {
    unsafe {
        cpu::enable_sse();
    }
    exceptions::init();
    //vga::init(limine::framebuffer_response());
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
    cpu::init_profiles(percpu::total_slots());
    percpu::init_bsp();
    dma::init_from_limine();
    pci::enumerate_impl();

    #[cfg(feature = "gfx_intel")]
    intel::init_once();

    //vga::cube::tick();
    trueos_qjs::set_font_atlas_small_provider(qjs_font_atlas_small_provider);
    trueos_qjs::set_font_atlas_large_provider(qjs_font_atlas_large_provider);
    trueos_qjs::host_api_hook::set_context_init_hook(host_api::install);

    pci::vrng::init_once();
    pci::vrng::smoke_test_once();
    crate::rng::init();

    disc::probe_once();
    efi::acpi::ensure_tables();
    efi::acpi::hpet::ensure();
    power::init();
    smp::init(percpu::total_slots());
    smp::mark_online();

    let executor = percpu::init_executor();
    let spawner = executor.spawner();

    let _ = cpu::register_current_worker_spawner(spawner);
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

    if let Err(e) = _spawner.spawn(crate::r::spawn_service::spawn_service_task(_spawner)) {
        crate::log!("spawn-svc: spawn failed: {:?}\n", e);
    }

    let mut counter: u64 = 0;
    loop {
        time::poll();
        unsafe { executor.poll() };
        if counter.is_multiple_of(5_000) {
            //vga::cube::tick();
        }
        if counter.is_multiple_of(10_000_000) {
            globalog::debugcon_write_byte_raw(b'0');
        }
        counter = counter.wrapping_add(1);
        power::idle_hint();
    }
}
