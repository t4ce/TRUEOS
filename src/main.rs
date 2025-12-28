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

extern crate alloc;

mod allocators;
mod acpi;
mod limine;
mod limlog;
mod vga;
mod pci;
mod usb;
mod time;
mod phys;
mod rng;
mod files;
mod uefi;

use core::{fmt::{self, Write}, panic::PanicInfo};
use ::acpi::sdt::hpet;
use embassy_executor::{raw::Executor, Spawner};
use ::limine::mp::Cpu as LimineCpu;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
use spin::Once;
use crate::usb::usb_scout;
use embassy_time::{Duration as EmbassyDuration, Timer};
use crate::pci::mmio;

static SMP_RESP: Once<&'static ::limine::response::MpResponse> = Once::new();

const BSP_EXECUTOR_SIZE: usize = core::mem::size_of::<Executor>();

#[repr(C, align(64))]
struct ExecutorStorage([u8; BSP_EXECUTOR_SIZE]);

#[link_section = ".data"]
static mut BSP_EXECUTOR_STORAGE: ExecutorStorage = ExecutorStorage([0xA5; BSP_EXECUTOR_SIZE]);

#[inline(always)]
unsafe fn init_bsp_executor() -> &'static Executor {
    let storage_ptr = core::ptr::addr_of_mut!(BSP_EXECUTOR_STORAGE);
    let bsp_executor_ptr = (*storage_ptr).0.as_mut_ptr() as *mut Executor;
    core::ptr::write(bsp_executor_ptr, Executor::new(core::ptr::null_mut()));
    &*bsp_executor_ptr
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {debugcon_write_byte(b'!');}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe { enable_sse(); }
    vga::init(limine::framebuffer_response());

    // If booted via UEFI, parse+log the EFI System Table once.
    // uefi::log_system_table_once(); bugged. never worked.
    
    // limlog::log_limine_markers(); log_memmap_once();
    phys::register_memory_metadata();

    pci::dma::init_from_limine(); // pci::dma::alloc_test_once();
    pci::enumerate_once(); // pci::log_devices_once();
    
    acpi::ensure_tables();
    acpi::bgrt::log_once();
    acpi::hpet::ensure(); rng::log_rng_caps();
    
    //pci::tga::init_once();
    usb::xhci::init_once();

    allocators::alloc_demo();

    let resp = *SMP_RESP.call_once(|| limine::smp_response().expect("LIMINE SMP MISSING"));
    for cpu in resp.cpus() {
        cpu.goto_address.write(ap_entry);
    }

    let bsp_executor = unsafe { init_bsp_executor() };
    let spawner = bsp_executor.spawner();

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

    let _ = spawner.spawn(input_logger());

    vga::render_framebuffer_banner("FalseOS");

    let white = 0x00_FF_FF_FF;
    let (_, bg, shadow) = vga::current_colors().unwrap_or((white, 0, vga::DEFAULT_SHADOW_COLOR));
    vga::logln("highlight", vga::PINK_FG_COLOR, bg, shadow);

    

    //files::create_demo_file(); needs hardware qemu param i guess

    let mut counter: u64 = 0;
    loop {
        if counter % 10_000 == 0 {
            time::poll();
            unsafe { bsp_executor.poll() };
        }

        if counter % 1_000_000 == 0 {
            vga::cube::tick();
        }
        
        // Periodic rescan for hotplug. Safe because `usb_scout` is now init-once + rescan.
        if counter % 100_000_000 == 0 {
            debugcon_write_byte(b'0');
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
    
    let total_slots = SMP_RESP
        .get()
        .expect("SMP response missing")
        .cpus()
        .len();

    let slot = (cpu.lapic_id as usize) % total_slots;

    let mut counter: u64 = 0;
    loop {
        if counter % 10_000_000 == 0 {
            vga::draw_header_square(total_slots, slot, vga::DEFAULT_SHADOW_COLOR, (counter % 360) as u32);
            
        }
        if counter % 100_000_000 == 0 {
            debugcon_write_byte(b'0' + cpu.lapic_id as u8);
        }
        counter = counter.wrapping_add(1);
    }
}

#[embassy_executor::task]
async fn input_logger() {
    loop {
        if let Some(evt) = usb::input::pop_event() {
            match evt {
                usb::input::InputEvent::Keyboard(kbd) => {
                    let shift = (kbd.modifiers & (1 << 1)) != 0 || (kbd.modifiers & (1 << 5)) != 0;
                    if let Some(&code) = kbd.keys.iter().find(|&&c| c != 0) {
                            debugconf!(
                                "[keybd]\n"
                            );
                    } 
                }
                usb::input::InputEvent::Mouse(mouse) => {
                    if mouse.buttons != 0 || mouse.dx != 0 || mouse.dy != 0 || mouse.wheel != 0 {
                        debugconf!(
                            "[mouse]\n"
                        );
                    }
                }
            }
        } else {
            Timer::after(EmbassyDuration::from_millis(5)).await;
        }
    }
}

#[inline(always)]
pub(crate) fn debugcon_write_str(s: &str) {
    for &b in s.as_bytes() {
        unsafe { outb(0xE9, b) };
    }
}

#[inline(always)]
pub(crate) fn debugcon_write_byte(b: u8) {
    unsafe { outb(0xE9, b) };
}

pub(crate) struct DebugCon;

impl Write for DebugCon {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        debugcon_write_str(s);
        Ok(())
    }
}

#[macro_export]
macro_rules! debugconf {
    ($($tt:tt)*) => {{
        let _ = core::fmt::write(&mut $crate::DebugCon, format_args!($($tt)*));
        let white = 0x00_FF_FF_FF;
        let (_, bg, shadow) = $crate::vga::current_colors()
            .unwrap_or((white, 0, $crate::vga::DEFAULT_SHADOW_COLOR));
        let _ = $crate::vga::log_fmt(format_args!($($tt)*), white, bg, shadow);
    }};
}

#[inline(always)]
pub(crate) unsafe fn inb(port: u16) -> u8 {
    let mut value: u8;
    core::arch::asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack, preserves_flags));
    value
}

#[inline(always)]
pub(crate) unsafe fn inw(port: u16) -> u16 {
    let mut value: u16;
    core::arch::asm!("in ax, dx", out("ax") value, in("dx") port, options(nomem, nostack, preserves_flags));
    value
}

#[inline(always)]
pub(crate) unsafe fn inl(port: u16) -> u32 {
    let mut value: u32;
    core::arch::asm!("in eax, dx", out("eax") value, in("dx") port, options(nomem, nostack, preserves_flags));
    value
}

#[inline(always)]
pub(crate) unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
pub(crate) unsafe fn outw(port: u16, val: u16) {
    core::arch::asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
pub(crate) unsafe fn outl(port: u16, val: u32) {
    core::arch::asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags));
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
    const EFER_MSR: u32 = 0xC000_0080;
    const EFER_LMA_BIT: u64 = 1 << 10;

    unsafe {
        let mut lo: u32 = 0;
        let mut hi: u32 = 0;
        core::arch::asm!(
            "rdmsr",
            in("ecx") EFER_MSR,
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags)
        );
        let efer = ((hi as u64) << 32) | lo as u64;
        (efer & EFER_LMA_BIT) != 0
    }
}
