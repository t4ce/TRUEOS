# Commit b08792b1 contents

Commit: `b08792b15c1930fd46e4cf94b6691c92a27577c9`

Files in this commit:
- `src/allocators.rs`
- `src/main.rs`

## src/allocators.rs

```rust
use core::alloc::{GlobalAlloc, Layout};
use core::arch::asm;
use core::mem::{align_of, size_of};
use core::ptr::{NonNull, null_mut};
use spin::Mutex;

use crate::phys::{self, HeapArena};

#[repr(C)]
struct FreeBlock {
    size: usize,
    next: Option<NonNull<FreeBlock>>,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct AllocTag {
    block_start: usize,
    block_size: usize,
}

struct FreeList {
    head: Option<NonNull<FreeBlock>>,
    initialized: bool,
    heap_virt_start: usize,
    heap_len: usize,
    heap_phys_start: usize,
}

unsafe impl Send for FreeList {}

impl FreeList {
    const fn new() -> Self {
        Self {
            head: None,
            initialized: false,
            heap_virt_start: 0,
            heap_len: 0,
            heap_phys_start: 0,
        }
    }

    unsafe fn init_arena(&mut self) {
        let heap_start = self.heap_virt_start;
        let heap_len = self.heap_len;
        let heap_end = heap_start + heap_len;

        let block_start = align_up(heap_start, align_of::<FreeBlock>());
        if block_start >= heap_end {
            return;
        }

        let size = heap_end - block_start;
        if size < minimum_block_size() {
            return;
        }

        let block = block_start as *mut FreeBlock;
        block.write(FreeBlock { size, next: None });
        self.head = Some(NonNull::new_unchecked(block));
        self.initialized = true;
    }

    unsafe fn alloc(&mut self, layout: Layout) -> *mut u8 {
        if !self.initialized {
            noheap_halt();
        }

        let mut current = self.head;
        let mut prev: Option<NonNull<FreeBlock>> = None;

        while let Some(mut block_ptr) = current {
            let block = block_ptr.as_mut();

            let block_start = block as *mut FreeBlock as usize;

            let payload_start = match aligned_payload(block_start, layout) {
                Some(v) => v,
                None => {
                    prev = Some(block_ptr);
                    current = block.next;
                    continue;
                }
            };

            let total_used = match payload_start
                .checked_add(layout.size())
                .and_then(|end| end.checked_sub(block_start))
            {
                Some(v) => v,
                None => {
                    prev = Some(block_ptr);
                    current = block.next;
                    continue;
                }
            };

            // If we split, the next free-list node must be properly aligned for `FreeBlock`.
            // This padding is accounted to the allocated block size.
            let aligned_used = align_up(total_used, align_of::<FreeBlock>());

            if aligned_used > block.size {
                prev = Some(block_ptr);
                current = block.next;
                continue;
            }

            let mut remaining = block.size.saturating_sub(aligned_used);

            let next_block = if remaining >= minimum_block_size() {
                let next_start = block_start + aligned_used;
                let next_ptr = next_start as *mut FreeBlock;
                next_ptr.write(FreeBlock {
                    size: remaining,
                    next: block.next,
                });
                Some(NonNull::new_unchecked(next_ptr))
            } else {
                remaining = 0;
                block.next
            };
            let alloc_block_size = if remaining == 0 {
                block.size
            } else {
                aligned_used
            };
            block.size = alloc_block_size;

            match prev {
                Some(mut p) => p.as_mut().next = next_block,
                None => self.head = next_block,
            }

            let tag_ptr = payload_start - size_of::<AllocTag>();
            (tag_ptr as *mut AllocTag).write(AllocTag {
                block_start,
                block_size: alloc_block_size,
            });

            return payload_start as *mut u8;
        }

        null_mut()
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }

        let tag_ptr = ptr.sub(size_of::<AllocTag>()) as *mut AllocTag;
        let tag = *tag_ptr;
        let block_size = tag.block_size;
        let block_start = tag.block_start;
        let block_ptr = block_start as *mut FreeBlock;
        block_ptr.write(FreeBlock {
            size: block_size,
            next: None,
        });

        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut current = self.head;

        while let Some(node) = current {
            if (node.as_ptr() as usize) > block_start {
                break;
            }
            prev = current;
            current = node.as_ref().next;
        }

        let mut new_node = NonNull::new_unchecked(block_ptr);
        {
            let new_block = new_node.as_mut();
            new_block.next = current;
        }

        if let Some(mut p) = prev {
            p.as_mut().next = Some(new_node);
        } else {
            self.head = Some(new_node);
        }

        self.try_merge_with_next(new_node);

        if let Some(p) = prev {
            self.try_merge_with_next(p);
        }
    }

    unsafe fn install_heap(&mut self, virt_start: usize, phys_start: usize, len: usize) {
        self.heap_virt_start = virt_start;
        self.heap_len = len;
        self.heap_phys_start = phys_start;
        self.init_arena();
    }

    unsafe fn try_merge_with_next(&mut self, mut node: NonNull<FreeBlock>) {
        let node_size = node.as_ref().size;
        let node_end = (node.as_ptr() as usize).saturating_add(node_size);

        if let Some(next_ptr) = node.as_ref().next {
            let next_start = next_ptr.as_ptr() as usize;
            if node_end == next_start {
                let next_size = next_ptr.as_ref().size;
                let next_next = next_ptr.as_ref().next;
                let new_size = node_size + next_size;
                let node_mut = node.as_mut();
                node_mut.size = new_size;
                node_mut.next = next_next;
            }
        }
    }
}

struct Allocator;

static ALLOCATOR: Mutex<FreeList> = Mutex::new(FreeList::new());

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATOR.lock().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        ALLOCATOR.lock().dealloc(ptr)
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: Allocator = Allocator;

pub unsafe fn alloc_raw(layout: Layout) -> *mut u8 {
    ALLOCATOR.lock().alloc(layout)
}

pub unsafe fn dealloc_raw(ptr: *mut u8) {
    ALLOCATOR.lock().dealloc(ptr)
}

#[derive(Copy, Clone, Debug)]
pub struct HeapStats {
    pub heap_start: usize,
    pub heap_end: usize,
    pub phys_start: usize,
    pub usable_start: usize,
    pub usable_total: usize,
    pub free_bytes: usize,
    pub largest_free_block: usize,
    pub free_blocks: usize,
    pub initialized: bool,
}

pub fn heap_stats() -> HeapStats {
    let guard = ALLOCATOR.lock();

    let heap_start = guard.heap_virt_start;
    let heap_len = guard.heap_len;
    let heap_end = heap_start.saturating_add(heap_len);
    let usable_start = align_up(heap_start, align_of::<FreeBlock>());
    let usable_total = heap_end.saturating_sub(usable_start);

    let mut free_bytes = 0usize;
    let mut largest_free_block = 0usize;
    let mut free_blocks = 0usize;
    let mut current = guard.head;
    while let Some(block_ptr) = current {
        // Safety: free list nodes are managed by the allocator.
        let block = unsafe { block_ptr.as_ref() };
        free_blocks += 1;
        free_bytes = free_bytes.saturating_add(block.size);
        if block.size > largest_free_block {
            largest_free_block = block.size;
        }
        current = block.next;
    }

    HeapStats {
        heap_start,
        heap_end,
        phys_start: guard.heap_phys_start,
        usable_start,
        usable_total,
        free_bytes,
        largest_free_block,
        free_blocks,
        initialized: guard.initialized,
    }
}

pub fn install_heap_arena(arena: HeapArena) -> bool {
    if arena.length < minimum_block_size() {
        crate::log!(
            "heap: requested arena too small size={} bytes (need >= {})\n",
            arena.length,
            minimum_block_size()
        );
        return false;
    }

    let mut guard = ALLOCATOR.lock();
    if guard.initialized {
        crate::log!("heap: allocator already initialized; cannot swap backing\n");
        return false;
    }

    guard.install_heap(arena.virt_start, arena.phys_start as usize, arena.length);
    phys::register_heap(arena.virt_start, arena.phys_start as usize, arena.length);
    crate::log!(
        "heap: arena virt=0x{:X} phys=0x{:X} size={} MiB\n",
        arena.virt_start,
        arena.phys_start,
        arena.length / (1024 * 1024)
    );
    true
}

const fn minimum_block_size() -> usize {
    size_of::<FreeBlock>() + size_of::<AllocTag>()
}

fn align_up(addr: usize, align: usize) -> usize {
    let mask = align.saturating_sub(1);
    (addr + mask) & !mask
}

fn aligned_payload(block_start: usize, layout: Layout) -> Option<usize> {
    let payload_start = align_up(
        block_start + size_of::<FreeBlock>() + size_of::<AllocTag>(),
        layout.align(),
    );
    if payload_start > usize::MAX - layout.size() {
        None
    } else {
        Some(payload_start)
    }
}

fn alloc_error(layout: Layout) -> ! {
    let stats = heap_stats();
    crate::log!(
        "OOM: alloc request size={} align={}\n",
        layout.size(),
        layout.align()
    );
    crate::log!(
        "OOM: heap virt=0x{:X}..0x{:X} phys=0x{:X} src={:?} usable_start=0x{:X} usable_total={} free_bytes={} largest_free={} free_blocks={} init={}\n",
        stats.heap_start,
        stats.heap_end,
        stats.phys_start,
        stats.source,
        stats.usable_start,
        stats.usable_total,
        stats.free_bytes,
        stats.largest_free_block,
        stats.free_blocks,
        stats.initialized
    );

    unsafe {
        asm!("cli", options(nomem, nostack));
        loop {
            asm!("hlt", options(nomem, nostack));
        }
    }
}
```

## src/main.rs

```rust
#![no_std]
#![no_main]
#![feature(abi_x86_interrupt, f16)]
#![allow(unsafe_op_in_unsafe_fn)]

const _: f16 = 0.0_f16;

#[macro_use]
pub extern crate alloc;

mod allocators;
#[path = "Chronos.rs"]
mod chronos;
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
#[path = "tst/fps.rs"]
mod tst_fps;
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
#[path = "tst/ui2_bgrt.rs"]
mod tst_ui2_bgrt;
#[path = "tst/ui2_imba_athlas_demo.rs"]
mod tst_ui2_imba_athlas_demo;
#[path = "tst/ui2_mandelbrot_demo.rs"]
mod tst_ui2_mandelbrot_demo;
#[path = "tst/ui2_triangle_demo.rs"]
mod tst_ui2_triangle_demo;
#[path = "tst/ws_time.rs"]
mod tst_ws_time;
mod turbo;
mod usb2;
mod wait;
mod x2apic;
mod z7;

use embassy_executor::{Spawner, raw::Executor};
pub(crate) use portio::{inb, inl, inw, outb, outl, outw};
pub use r::pat as pattern;
pub use r::time;
pub use r::{io, path};

fn qjs_imba_athlas_small_provider() -> trueos_qjs::ImbaAthlasView<'static> {
    let atlas = crate::gfx::imba_athlas::imba_athlas_small_view();
    trueos_qjs::ImbaAthlasView {
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

fn qjs_imba_athlas_large_provider() -> trueos_qjs::ImbaAthlasView<'static> {
    let atlas = crate::gfx::imba_athlas::imba_athlas_large_view();
    trueos_qjs::ImbaAthlasView {
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
    crate::log!("long_mode_active: {}\n", cpu::long_mode_active());
    phys::register_memory_metadata();
    phys::init_pmm_from_limine();

    if !phys::try_install_heap_arena_candidates(allocators::install_heap_arena) {
        crate::log!("heap: failed to reserve/install any heap arena\n");
    }

    crate::log!("kmain: step=smp_resp\n");
    if let Some(perf) = limine::bootloader_performance() {
        crate::log!(
            "Boot Performance: reset={}_usec init={}_usec exec={}_usec\n",
            perf.reset_usec(),
            perf.init_usec(),
            perf.exec_usec()
        );
    }
    let smp_resp = limine::smp_response().unwrap();
    crate::log!("kmain: step=lapic_ids\n");
    let lapic_ids: alloc::vec::Vec<u32> = smp_resp.cpus().iter().map(|c| c.lapic_id).collect();
    percpu::install_cpu_slot_lapic_order_owned(lapic_ids);
    crate::log!("kmain: step=cpu_profiles\n");
    cpu::init_profiles(percpu::total_slots());
    crate::log!("kmain: step=percpu_bsp\n");
    percpu::init_bsp();
    crate::log!("kmain: step=dma\n");
    dma::init_from_limine();
    crate::log!("kmain: step=pci_enum\n");
    pci::enumerate_impl();
    crate::log!("kmain: step=pci_done\n");

    #[cfg(feature = "gfx_intel")]
    intel::init_once();

    //vga::cube::tick();
    trueos_qjs::set_imba_athlas_small_provider(qjs_imba_athlas_small_provider);
    trueos_qjs::set_imba_athlas_large_provider(qjs_imba_athlas_large_provider);
    trueos_qjs::host_api_hook::set_context_init_hook(host_api::install);

    pci::vrng::init_once();
    pci::vrng::smoke_test_once();
    crate::rng::init();
    crate::log!("kmain: step=disc_probe\n");
    disc::probe_once();
    crate::log!("kmain: step=acpi\n");
    efi::acpi::ensure_tables();

    // Chronos awake hpet dependend
    efi::acpi::hpet::ensure();
    crate::log!("kmain: step=chronos\n");
    chronos::awake();
    crate::log!("kmain: step=power\n");
    // i hope fmt dont make this syntax 2 row

    power::init();
    crate::log!("kmain: step=smp_init\n");
    smp::init(percpu::total_slots());
    smp::mark_online();
    crate::log!("kmain: step=executor\n");

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
```