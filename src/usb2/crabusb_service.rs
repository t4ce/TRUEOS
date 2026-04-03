use alloc::string::String;
use core::fmt::Write;
use core::alloc::Layout;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::time::Duration;

use crab_usb::{KernelOp, USBHost};
use dma_api::{DmaAddr, DmaDirection, DmaError, DmaHandle, DmaMapHandle, DmaOp};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

pub(super) struct TrueosCrabUsbKernel;

pub(super) static CRABUSB_KERNEL: TrueosCrabUsbKernel = TrueosCrabUsbKernel;

use super::xhci::MAX_XHCI_CONTROLLERS;

#[derive(Clone, Copy)]
struct BounceMapping {
    orig_virt: usize,
    bounce_virt: usize,
    size: usize,
    direction: DmaDirection,
}

static BOUNCE_MAPPINGS: Mutex<alloc::vec::Vec<BounceMapping>> = Mutex::new(alloc::vec::Vec::new());

#[inline]
fn crabusb_dma_cache_flush(addr: NonNull<u8>, size: usize) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use core::arch::x86_64::{_mm_clflush, _mm_mfence};

        if size == 0 {
            return;
        }

        let line = 64usize;
        let start = (addr.as_ptr() as usize) & !(line - 1);
        let end = (addr.as_ptr() as usize).saturating_add(size);
        let mut ptr = start;
        while ptr < end {
            _mm_clflush(ptr as *const _);
            ptr = ptr.saturating_add(line);
        }
        _mm_mfence();
    }
}

impl DmaOp for TrueosCrabUsbKernel {
    fn page_size(&self) -> usize {
        4096
    }

    fn flush(&self, addr: NonNull<u8>, size: usize) {
        crabusb_dma_cache_flush(addr, size);
    }

    fn invalidate(&self, addr: NonNull<u8>, size: usize) {
        crabusb_dma_cache_flush(addr, size);
    }

    fn flush_invalidate(&self, addr: NonNull<u8>, size: usize) {
        crabusb_dma_cache_flush(addr, size);
    }

    unsafe fn map_single(
        &self,
        dma_mask: u64,
        addr: NonNull<u8>,
        size: NonZeroUsize,
        align: usize,
        direction: DmaDirection,
    ) -> Result<DmaMapHandle, DmaError> {
        let required_align = align.max(1);
        let layout =
            Layout::from_size_align(size.get(), required_align).map_err(DmaError::LayoutError)?;
        let phys = crate::phys::virt_to_phys_checked(addr.as_ptr()).ok_or(DmaError::NoMemory)?;

        let aligned = phys.is_multiple_of(required_align as u64);
        let in_mask = phys
            .checked_add(size.get().saturating_sub(1) as u64)
            .map(|end| end <= dma_mask)
            .unwrap_or(false);

        if aligned && in_mask {
            return Ok(unsafe { DmaMapHandle::new(addr, DmaAddr::from(phys), layout, None) });
        }

        let max_phys_exclusive = if dma_mask == u64::MAX {
            None
        } else {
            dma_mask.checked_add(1)
        };
        let (bounce_phys, bounce_virt) =
            crate::dma::alloc_with_max(layout.size(), layout.align(), max_phys_exclusive)
                .ok_or(DmaError::NoMemory)?;
        let bounce_virt = NonNull::new(bounce_virt).ok_or(DmaError::NoMemory)?;

        if matches!(direction, DmaDirection::ToDevice | DmaDirection::Bidirectional) {
            unsafe {
                core::ptr::copy_nonoverlapping(addr.as_ptr(), bounce_virt.as_ptr(), layout.size())
            };
        }

        BOUNCE_MAPPINGS.lock().push(BounceMapping {
            orig_virt: addr.as_ptr() as usize,
            bounce_virt: bounce_virt.as_ptr() as usize,
            size: layout.size(),
            direction,
        });

        Ok(unsafe {
            DmaMapHandle::new(addr, DmaAddr::from(bounce_phys), layout, Some(bounce_virt))
        })
    }

    unsafe fn unmap_single(&self, handle: DmaMapHandle) {
        if let Some(alloc_virt) = handle.alloc_virt() {
            let mapping = {
                let mut mappings = BOUNCE_MAPPINGS.lock();
                mappings
                    .iter()
                    .position(|entry| entry.bounce_virt == alloc_virt.as_ptr() as usize)
                    .map(|idx| mappings.swap_remove(idx))
            };

            if let Some(mapping) = mapping
                && matches!(
                    mapping.direction,
                    DmaDirection::FromDevice | DmaDirection::Bidirectional
                )
            {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        mapping.bounce_virt as *const u8,
                        mapping.orig_virt as *mut u8,
                        mapping.size,
                    );
                }
            }

            crate::dma::dealloc(alloc_virt.as_ptr(), handle.size());
        }
    }

    unsafe fn alloc_coherent(&self, dma_mask: u64, layout: Layout) -> Option<DmaHandle> {
        let max_phys_exclusive = if dma_mask == u64::MAX {
            None
        } else {
            dma_mask.checked_add(1)
        };
        let (phys, virt) =
            crate::dma::alloc_with_max(layout.size(), layout.align(), max_phys_exclusive)?;
        let virt = NonNull::new(virt)?;
        Some(unsafe { DmaHandle::new(virt, DmaAddr::from(phys), layout) })
    }

    unsafe fn dealloc_coherent(&self, handle: DmaHandle) {
        crate::dma::dealloc(handle.as_ptr().as_ptr(), handle.size());
    }
}

impl KernelOp for TrueosCrabUsbKernel {
    fn delay(&self, duration: Duration) {
        let millis = duration.as_millis();
        if millis == 0 {
            return;
        }
        let timeout_ms = millis.min(u128::from(u64::MAX)) as u64;
        let _ = crate::wait::spin_until_timeout(timeout_ms, || false);
    }
}

fn connected_ports_summary(controller_id: usize) -> String {
    let mut out = String::new();
    let mut any = false;

    if let Some(diag) = super::controller_mmio_diag(controller_id) {
        for port in diag.ports {
            if (port.portsc & 0x1) == 0 {
                continue;
            }
            if any {
                let _ = out.write_str(",");
            }
            let _ = write!(out, "{}", port.port_id);
            any = true;
        }
    }

    if !any {
        out.push('-');
    }

    out
}

#[embassy_executor::task(pool_size = MAX_XHCI_CONTROLLERS)]
pub async fn bsp_service(controller_index: usize) {
    const RETRY_MS: u64 = 1000;

    loop {
        let Some(info) = super::controller_by_index(controller_index) else {
            Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
            continue;
        };

        crate::pci::enable_mem_and_bus_master(info.bus, info.slot, info.function);

        let mut host = match USBHost::new_xhci(info.mmio_base, &CRABUSB_KERNEL) {
            Ok(host) => host,
            Err(_) => {
                Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
                continue;
            }
        };

        if host.init().await.is_ok() {
            let connected_ports = connected_ports_summary(info.index);
            crate::log!(
                "crabusb: init successful ctrl={} bdf={:02X}:{:02X}.{} vid={:04X} pid={:04X} mmio={:p} ports={}\n",
                info.index,
                info.bus,
                info.slot,
                info.function,
                info.vendor_id,
                info.device_id,
                info.mmio_base,
                connected_ports
            );

            if let Ok(devices) = host.probe_devices().await {
                crate::log!(
                    "crabusb: probe ctrl={} devices={}\n",
                    info.index,
                    devices.len()
                );

                for dev in devices.iter() {
                    let desc = dev.descriptor();
                    let topo = dev.topology();
                    let if_count: usize = dev
                        .configurations()
                        .iter()
                        .map(|cfg| cfg.interfaces.len())
                        .sum();
                    let ep_count: usize = dev
                        .configurations()
                        .iter()
                        .flat_map(|cfg| cfg.interfaces.iter())
                        .flat_map(|interface| interface.alt_settings.iter())
                        .map(|alt| alt.endpoints.len())
                        .sum();

                    crate::log!(
                        "crabusb: dev ctrl={} root_port={} vid={:04X} pid={:04X} class={:02X} subclass={:02X} proto={:02X} ifs={} eps={}\n",
                        info.index,
                        topo.root_port_id,
                        desc.vendor_id,
                        desc.product_id,
                        desc.class,
                        desc.subclass,
                        desc.protocol,
                        if_count,
                        ep_count
                    );
                }
            }

            loop {
                Timer::after(EmbassyDuration::from_secs(3600)).await;
            }
        }

        Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
    }
}
