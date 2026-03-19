use core::alloc::Layout;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;

use crab_usb::{
    DmaAddr, DmaDirection, DmaError, DmaHandle, DmaMapHandle, DmaOp, Event, KernelOp, USBHost,
};
use embassy_time::{Duration as EmbassyDuration, Timer};

struct TrueosCrabUsbKernel;

static CRABUSB_KERNEL: TrueosCrabUsbKernel = TrueosCrabUsbKernel;
static INITIAL_SNAPSHOT_LOGGED: AtomicBool = AtomicBool::new(false);

impl DmaOp for TrueosCrabUsbKernel {
    fn page_size(&self) -> usize {
        4096
    }

    unsafe fn map_single(
        &self,
        dma_mask: u64,
        addr: NonNull<u8>,
        size: NonZeroUsize,
        align: usize,
        _direction: DmaDirection,
    ) -> Result<DmaMapHandle, DmaError> {
        let layout = Layout::from_size_align(size.get(), align.max(1))
            .map_err(DmaError::LayoutError)?;
        let phys = crate::phys::virt_to_phys_checked(addr.as_ptr()).ok_or(DmaError::NoMemory)?;
        if phys > dma_mask {
            return Err(DmaError::DmaMaskNotMatch {
                addr: DmaAddr::from(phys),
                mask: dma_mask,
            });
        }
        Ok(unsafe { DmaMapHandle::new(addr, DmaAddr::from(phys), layout, None) })
    }

    unsafe fn unmap_single(&self, _handle: DmaMapHandle) {}

    unsafe fn alloc_coherent(&self, dma_mask: u64, layout: Layout) -> Option<DmaHandle> {
        let max_phys_exclusive = if dma_mask == u64::MAX {
            None
        } else {
            dma_mask.checked_add(1)
        };
        let (phys, virt) =
            crate::pci::dma::alloc_with_max(layout.size(), layout.align(), max_phys_exclusive)?;
        let virt = NonNull::new(virt)?;
        Some(unsafe { DmaHandle::new(virt, DmaAddr::from(phys), layout) })
    }

    unsafe fn dealloc_coherent(&self, handle: DmaHandle) {
        crate::pci::dma::dealloc(handle.as_ptr().as_ptr(), handle.size());
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

async fn probe_and_log(host: &mut USBHost) {
    match host.probe_devices().await {
        Ok(devices) => {
            crate::log!("crabusb: probe found {} device(s)\n", devices.len());
            for dev in devices.iter() {
                let desc = dev.descriptor();
                crate::log!(
                    "crabusb: dev {:04X}:{:04X} class={:02X} subclass={:02X} proto={:02X}\n",
                    desc.vendor_id,
                    desc.product_id,
                    desc.class,
                    desc.subclass,
                    desc.protocol
                );
            }
        }
        Err(err) => crate::log!("crabusb: probe failed: {:?}\n", err),
    }
}

fn log_one_time_snapshot(info: super::xhci::XhcInfo) {
    if INITIAL_SNAPSHOT_LOGGED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    crate::log!(
        "crabusb: one-time snapshot controller={} bdf={:02X}:{:02X}.{}\n",
        info.controller_id,
        info.bus,
        info.slot,
        info.function
    );

    let ports = super::port_snapshot(info.controller_id);
    crate::log!("crabusb: one-time root-port snapshot count={}\n", ports.len());
    for port in ports.iter() {
        crate::log!(
            "crabusb: port {} connected={} enabled={} speed={} kind={:?} vid={:?} pid={:?}\n",
            port.port_id,
            port.connected,
            port.enabled,
            port.speed,
            port.device_kind,
            port.vid,
            port.pid
        );
    }
}

fn discover_first_controller() -> Option<super::xhci::XhcInfo> {
    crate::pci::enumerate_impl();
    super::xhci::init_once();
    super::xhci::xhc_list().iter().copied().next()
}

#[embassy_executor::task]
pub async fn bsp_service() {
    const OFFLINE_RETRY_MS: u64 = 1000;

    loop {
        let Some(info) = discover_first_controller() else {
            crate::log!(
                "crabusb: no xhci controller available yet; retrying on BSP\n"
            );
            Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
            continue;
        };

        crate::log!(
            "crabusb: BSP service binding controller {} at {:02X}:{:02X}.{}\n",
            info.controller_id,
            info.bus,
            info.slot,
            info.function
        );

        let mut host = match USBHost::new_xhci(info.mmio_base, &CRABUSB_KERNEL) {
            Ok(host) => host,
            Err(err) => {
                crate::log!(
                    "crabusb: failed to create host for controller {}: {:?}\n",
                    info.controller_id,
                    err
                );
                Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
                continue;
            }
        };

        let event_handler = host.create_event_handler();

        if let Err(err) = host.init().await {
            crate::log!(
                "crabusb: host init failed for controller {}: {:?}\n",
                info.controller_id,
                err
            );
            Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
            continue;
        }

        crate::log!(
            "crabusb: host init complete for controller {}\n",
            info.controller_id
        );
        log_one_time_snapshot(info);
        probe_and_log(&mut host).await;

        let mut idle_ticks = 0u32;
        let mut need_rediscover = false;
        loop {
            match event_handler.handle_event() {
                Event::Nothing => {
                    idle_ticks = idle_ticks.wrapping_add(1);
                    if idle_ticks >= 200 {
                        idle_ticks = 0;
                        probe_and_log(&mut host).await;
                    }
                    Timer::after(EmbassyDuration::from_millis(10)).await;
                }
                Event::PortChange { port } => {
                    idle_ticks = 0;
                    crate::log!("crabusb: port change on root port {}\n", port);
                    probe_and_log(&mut host).await;
                }
                Event::Stopped => {
                    crate::log!("crabusb: event handler stopped; rediscovering controller\n");
                    need_rediscover = true;
                    break;
                }
            }
        }

        if need_rediscover {
            Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
        }
    }
}
