use core::ptr::{NonNull, write_bytes};

use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;

const INTEL_VENDOR_ID: u16 = 0x8086;
const PCI_CLASS_DISPLAY: u8 = 0x03;
const MAX_INTEL_DEVICES: usize = 4;
const CMD_SCRATCH_BYTES: usize = 4096;

#[derive(Copy, Clone, Debug)]
pub struct IntelGfxInfo {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub bar_phys: u64,
    pub bar_size: u64,
    pub mmio_base: NonNull<u8>,
    pub mmio_len: usize,
    pub cmd_scratch_phys: u64,
    pub cmd_scratch_virt: *mut u8,
    pub cmd_scratch_len: usize,
}

unsafe impl Send for IntelGfxInfo {}
unsafe impl Sync for IntelGfxInfo {}

static FIRST_DEVICE: Mutex<Option<IntelGfxInfo>> = Mutex::new(None);
static DEVICES: Mutex<Vec<IntelGfxInfo, MAX_INTEL_DEVICES>> = Mutex::new(Vec::new());

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct RgbVertex {
    x: f32,
    y: f32,
    r: u8,
    g: u8,
    b: u8,
    pad: u8,
}

fn register_intel(mut info: IntelGfxInfo) {
    let mut list = DEVICES.lock();
    let id = list.len();
    if id >= MAX_INTEL_DEVICES {
        crate::log!(
            "gfx-intel: device list full; dropping {:02X}:{:02X}.{}\n",
            info.bus,
            info.slot,
            info.function
        );
        return;
    }

    let mut first = FIRST_DEVICE.lock();
    if first.is_none() {
        *first = Some(info);
    }

    // Store a stable slot index in the scratch page's first byte for easy diagnostics.
    unsafe {
        if !info.cmd_scratch_virt.is_null() {
            *info.cmd_scratch_virt = id as u8;
        }
    }

    let _ = list.push(info);
}

#[inline]
fn is_intel_display(dev: &crate::pci::PciDevice) -> bool {
    dev.vendor == INTEL_VENDOR_ID && dev.class == PCI_CLASS_DISPLAY
}

pub fn init_once() {
    if crate::limine::hhdm_offset().is_none() {
        crate::log!("gfx-intel: no HHDM\n");
        return;
    }

    FIRST_DEVICE.lock().take();
    DEVICES.lock().clear();

    let mut did_match = false;
    crate::pci::with_devices(|list| {
        for dev in list {
            if !is_intel_display(dev) {
                continue;
            }
            did_match = true;

            crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);

            let (bar_lo, bar_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
            if (bar_lo & 0x1) != 0 {
                crate::log!(
                    "gfx-intel: IO BAR unsupported for {:02X}:{:02X}.{}\n",
                    dev.bus,
                    dev.slot,
                    dev.function
                );
                continue;
            }

            let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
            let mut base = (bar_lo & 0xFFFF_FFF0) as u64;
            if is_64 {
                base |= (bar_hi.unwrap_or(0) as u64) << 32;
            }

            if base == 0 {
                crate::log!(
                    "gfx-intel: BAR0 not assigned at {:02X}:{:02X}.{}\n",
                    dev.bus,
                    dev.slot,
                    dev.function
                );
                continue;
            }

            let bar_size =
                crate::pci::bar0_size_bytes(dev.bus, dev.slot, dev.function).unwrap_or(0);
            let mut mmio_len = if bar_size == 0 {
                0x20_000usize
            } else {
                bar_size as usize
            };
            if mmio_len < 0x20_000 {
                mmio_len = 0x20_000;
            }
            if mmio_len > 0x4_00000 {
                mmio_len = 0x4_00000;
            }

            let mmio_base = match crate::pci::mmio::map_mmio_region(base, mmio_len) {
                Ok(ptr) => ptr,
                Err(e) => {
                    crate::log!(
                        "gfx-intel: MMIO map failed {:02X}:{:02X}.{} err={:?}\n",
                        dev.bus,
                        dev.slot,
                        dev.function,
                        e
                    );
                    continue;
                }
            };

            let (cmd_scratch_phys, cmd_scratch_virt) =
                crate::pci::dma::alloc(CMD_SCRATCH_BYTES, CMD_SCRATCH_BYTES)
                    .unwrap_or((0, core::ptr::null_mut()));
            if !cmd_scratch_virt.is_null() {
                unsafe { write_bytes(cmd_scratch_virt, 0, CMD_SCRATCH_BYTES) };
            }

            crate::log!(
                "gfx-intel: claimed {:02X}:{:02X}.{} bar0=0x{:X} size=0x{:X} mmio=0x{:X} scratch=0x{:X}\n",
                dev.bus,
                dev.slot,
                dev.function,
                base,
                bar_size,
                mmio_base.as_ptr() as usize,
                cmd_scratch_phys
            );

            let info = IntelGfxInfo {
                bus: dev.bus,
                slot: dev.slot,
                function: dev.function,
                bar_phys: base,
                bar_size,
                mmio_base,
                mmio_len,
                cmd_scratch_phys,
                cmd_scratch_virt,
                cmd_scratch_len: if cmd_scratch_virt.is_null() {
                    0
                } else {
                    CMD_SCRATCH_BYTES
                },
            };
            register_intel(info);
        }
    });

    if DEVICES.lock().is_empty() {
        if !did_match {
            crate::log!("gfx-intel: no Intel display-class PCI device found\n");
        }
        return;
    }

    crate::v::readiness::set(crate::v::readiness::GFX_INTEL_CLAIMED);
}

#[inline]
pub fn has_claimed_device() -> bool {
    !DEVICES.lock().is_empty()
}

fn centered_triangle() -> [RgbVertex; 3] {
    [
        RgbVertex {
            x: 0.0,
            y: -0.55,
            r: 0xFF,
            g: 0x40,
            b: 0x40,
            pad: 0,
        },
        RgbVertex {
            x: -0.55,
            y: 0.45,
            r: 0x40,
            g: 0xFF,
            b: 0x70,
            pad: 0,
        },
        RgbVertex {
            x: 0.55,
            y: 0.45,
            r: 0x50,
            g: 0x90,
            b: 0xFF,
            pad: 0,
        },
    ]
}

#[embassy_executor::task]
pub async fn centered_triangle_demo_task() {
    if !has_claimed_device() {
        crate::log!("gfx-intel-demo: skipped (no claimed Intel gfx device)\n");
        return;
    }

    crate::gfx::init(crate::limine::framebuffer_response());

    let verts = centered_triangle();
    let ptr = verts.as_ptr() as *const u8;
    let len = core::mem::size_of_val(&verts);

    let mut tries = 0u32;
    loop {
        let rc = unsafe {
            crate::surface::io::cabi::trueos_cabi_gfx_draw_rgb_triangles(0x101018, ptr, len)
        };
        if rc == 0 {
            crate::log!("gfx-intel-demo: centered triangle submitted\n");
            return;
        }

        tries = tries.saturating_add(1);
        if tries == 1 || tries.is_multiple_of(20) {
            crate::log!("gfx-intel-demo: draw retry rc={} tries={}\n", rc, tries);
        }

        if tries >= 200 {
            crate::log!("gfx-intel-demo: giving up after {} retries\n", tries);
            return;
        }

        Timer::after(EmbassyDuration::from_millis(25)).await;
    }
}
