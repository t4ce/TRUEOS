extern crate alloc;

use alloc::vec::Vec;
use core::ptr::NonNull;

pub(crate) mod xhci {
    pub const MAX_XHCI_CONTROLLERS: usize = 4;
}

pub(crate) mod hid {
    pub use trueos_v::vinput::TrueosHidCursorEvent;

    pub mod classreq {
        #[repr(u8)]
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        pub enum HidReportType {
            Input = 1,
            Output = 2,
            Feature = 3,
        }

        #[inline]
        pub fn get_protocol_slot_sync(
            _controller_id: usize,
            _slot_id: u32,
            _iface: u8,
            _timeout_ms: u64,
        ) -> Option<u8> {
            None
        }

        #[inline]
        pub fn set_protocol_slot_sync(
            _controller_id: usize,
            _slot_id: u32,
            _iface: u8,
            _protocol: u8,
            _timeout_ms: u64,
        ) -> Option<u32> {
            None
        }

        #[inline]
        pub fn get_idle_slot_sync(
            _controller_id: usize,
            _slot_id: u32,
            _iface: u8,
            _report_id: u8,
            _timeout_ms: u64,
        ) -> Option<u8> {
            None
        }

        #[inline]
        pub fn set_idle_slot_sync(
            _controller_id: usize,
            _slot_id: u32,
            _iface: u8,
            _report_id: u8,
            _duration_4ms: u8,
            _timeout_ms: u64,
        ) -> Option<u32> {
            None
        }

        #[inline]
        pub fn get_report_slot_sync(
            _controller_id: usize,
            _slot_id: u32,
            _iface: u8,
            _report_type: HidReportType,
            _report_id: u8,
            _length: usize,
            _timeout_ms: u64,
        ) -> Option<heapless::Vec<u8, 256>> {
            None
        }

        #[inline]
        pub fn set_report_slot_sync(
            _controller_id: usize,
            _slot_id: u32,
            _iface: u8,
            _report_type: HidReportType,
            _report_id: u8,
            _data: &[u8],
            _timeout_ms: u64,
        ) -> Option<u32> {
            None
        }
    }

    #[inline]
    pub fn pop_cursor_event() -> Option<TrueosHidCursorEvent> {
        None
    }

    #[inline]
    pub fn read_cursor_events_since(
        read_seq: u64,
        _out: &mut [TrueosHidCursorEvent],
    ) -> (u64, u32, usize) {
        (read_seq, 0, 0)
    }

    #[inline]
    pub fn inject_virtual_cursor_event(
        _slot_id: u32,
        _x: f64,
        _y: f64,
        _buttons_down: u32,
        _wheel: i16,
        _flags: u32,
    ) {
    }
}

pub(crate) mod input;
mod crabusb_service;

#[derive(Copy, Clone, Debug)]
pub(crate) struct UsbDeviceSummary {
    pub slot_id: u32,
    pub port: u8,
    pub kind: &'static str,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub class: Option<u8>,
    pub subclass: Option<u8>,
    pub protocol: Option<u8>,
}

#[derive(Clone, Copy)]
pub(crate) struct TlbUsbController {
    pub index: usize,
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub mmio_base: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct TlbUsbDevice {
    pub controller_index: usize,
    pub vendor_id: u16,
    pub product_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub config_count: usize,
    pub interface_count: usize,
}

pub(crate) struct TlbUsbSnapshot {
    pub controllers: Vec<TlbUsbController>,
    pub devices: Vec<TlbUsbDevice>,
    pub probe_error: Option<&'static str>,
}

fn decode_mmio_bar(bar_lo: u32, bar_hi: Option<u32>) -> Option<u64> {
    if bar_lo == 0 || bar_lo == 0xFFFF_FFFF || (bar_lo & 0x1) != 0 {
        return None;
    }

    let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
    let base = if is_64 {
        (((bar_hi.unwrap_or(0) as u64) << 32) | (bar_lo as u64)) & !0xFu64
    } else {
        (bar_lo as u64) & !0xFu64
    };
    (base != 0).then_some(base)
}

pub(crate) fn pci_usb_controllers() -> Vec<TlbUsbController> {
    const PCI_CLASS_SERIAL_BUS: u8 = 0x0C;
    const PCI_SUBCLASS_USB: u8 = 0x03;
    const PCI_PROGIF_XHCI: u8 = 0x30;

    crate::pci::enumerate_impl();

    let mut ctrls = Vec::new();
    crate::pci::with_devices(|list| {
        for dev in list.iter() {
            if dev.class != PCI_CLASS_SERIAL_BUS
                || dev.subclass != PCI_SUBCLASS_USB
                || dev.prog_if != PCI_PROGIF_XHCI
            {
                continue;
            }

            let (bar0_lo, bar0_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
            let Some(mmio_base) = decode_mmio_bar(bar0_lo, bar0_hi) else {
                continue;
            };

            ctrls.push(TlbUsbController {
                index: ctrls.len(),
                bus: dev.bus,
                slot: dev.slot,
                function: dev.function,
                vendor_id: dev.vendor,
                device_id: dev.device,
                mmio_base,
            });
        }
    });
    ctrls
}

#[inline]
pub(crate) fn discover_first_controller() -> Option<TlbUsbController> {
    pci_usb_controllers().into_iter().next()
}

#[inline]
fn controller_by_index(controller_id: usize) -> Option<TlbUsbController> {
    pci_usb_controllers()
        .into_iter()
        .find(|info| info.index == controller_id)
}

fn classify_descriptor_kind(desc: &crab_usb::usb_if::descriptor::DeviceDescriptor) -> &'static str {
    match desc.class {
        0x03 => "hid",
        0x08 => "mass",
        0x07 => "printer",
        0x02 => "cdc",
        0x01 => "uac",
        _ => "unknown",
    }
}

pub(crate) fn list_device_summaries(controller_id: usize) -> Vec<UsbDeviceSummary> {
    let Some(info) = controller_by_index(controller_id) else {
        return Vec::new();
    };

    crate::wait::spawn_and_wait_local(async move {
        crate::pci::enable_mem_and_bus_master(info.bus, info.slot, info.function);
        let Some(mmio) = NonNull::new(info.mmio_base as *mut u8) else {
            return Vec::new();
        };

        let mut host =
            match crab_usb::USBHost::new_xhci(mmio, &self::crabusb_service::CRABUSB_KERNEL) {
                Ok(host) => host,
                Err(_) => return Vec::new(),
            };
        if host.init().await.is_err() {
            return Vec::new();
        }

        let found = match host.probe_devices().await {
            Ok(found) => found,
            Err(_) => return Vec::new(),
        };

        let mut out = Vec::new();
        for dev_info in found.iter() {
            let desc = dev_info.descriptor();
            let slot_id = match host.open_device(dev_info).await {
                Ok(device) => u32::from(device.slot_id()),
                Err(_) => 0,
            };
            out.push(UsbDeviceSummary {
                slot_id,
                port: 0,
                kind: classify_descriptor_kind(desc),
                vid: Some(desc.vendor_id),
                pid: Some(desc.product_id),
                class: Some(desc.class),
                subclass: Some(desc.subclass),
                protocol: Some(desc.protocol),
            });
        }
        out
    })
}

pub(crate) mod syscall {
    use alloc::vec;
    use alloc::vec::Vec;
    use core::ptr::NonNull;

    use crab_usb::usb_if::descriptor::DescriptorType;
    use crab_usb::usb_if::host::ControlSetup;
    use crab_usb::usb_if::transfer::{Recipient, Request, RequestType};

    const DESC_MAX: usize = 256;

    fn with_device_descriptor_read(
        controller_id: usize,
        slot_id: u32,
        setup: ControlSetup,
        length: u16,
    ) -> Option<Vec<u8>> {
        let info = super::controller_by_index(controller_id)?;
        crate::wait::spawn_and_wait_local(async move {
            crate::pci::enable_mem_and_bus_master(info.bus, info.slot, info.function);
            let mmio = NonNull::new(info.mmio_base as *mut u8)?;
            let mut host =
                crab_usb::USBHost::new_xhci(mmio, &super::crabusb_service::CRABUSB_KERNEL).ok()?;
            host.init().await.ok()?;
            let found = host.probe_devices().await.ok()?;

            for dev_info in found.iter() {
                let mut device = host.open_device(dev_info).await.ok()?;
                if u32::from(device.slot_id()) != slot_id {
                    continue;
                }
                let mut buf = vec![0u8; usize::from(length).min(DESC_MAX)];
                let read = device.control_in(setup, buf.as_mut_slice()).await.ok()?;
                buf.truncate(read.min(buf.len()));
                return Some(buf);
            }
            None
        })
    }

    pub fn port_reset(_controller_id: usize, _port_idx: usize) -> i32 {
        -1
    }

    pub fn control_get_descriptor(
        controller_id: usize,
        slot_id: u32,
        desc_type: u8,
        desc_index: u8,
        length: u16,
        _timeout_ms: u64,
    ) -> Option<Vec<u8>> {
        with_device_descriptor_read(
            controller_id,
            slot_id,
            ControlSetup {
                request_type: RequestType::Standard,
                recipient: Recipient::Device,
                request: Request::GetDescriptor,
                value: ((DescriptorType::from(desc_type).0 as u16) << 8) | u16::from(desc_index),
                index: 0,
            },
            length,
        )
    }

    pub fn control_get_hid_descriptor(
        controller_id: usize,
        slot_id: u32,
        interface_number: u16,
        length: u16,
        _timeout_ms: u64,
    ) -> Option<Vec<u8>> {
        with_device_descriptor_read(
            controller_id,
            slot_id,
            ControlSetup {
                request_type: RequestType::Standard,
                recipient: Recipient::Interface,
                request: Request::GetDescriptor,
                value: (0x21u16) << 8,
                index: interface_number,
            },
            length,
        )
    }

    pub fn control_get_hid_report_descriptor(
        controller_id: usize,
        slot_id: u32,
        interface_number: u16,
        length: u16,
        _timeout_ms: u64,
    ) -> Option<Vec<u8>> {
        with_device_descriptor_read(
            controller_id,
            slot_id,
            ControlSetup {
                request_type: RequestType::Standard,
                recipient: Recipient::Interface,
                request: Request::GetDescriptor,
                value: (0x22u16) << 8,
                index: interface_number,
            },
            length,
        )
    }

    pub fn read_transfer_event(
        _controller_id: usize,
        _slot_id: u32,
        _ep_target: u32,
    ) -> Option<(u32, u32)> {
        None
    }
}

pub(crate) fn tlb_snapshot() -> TlbUsbSnapshot {
    let controllers = pci_usb_controllers();
    if controllers.is_empty() {
        return TlbUsbSnapshot {
            controllers,
            devices: Vec::new(),
            probe_error: None,
        };
    }

    let probe_ctrl = controllers[0];
    let devices = crate::wait::spawn_and_wait_local(async move {
        let mut out = Vec::new();

        crate::pci::enable_mem_and_bus_master(probe_ctrl.bus, probe_ctrl.slot, probe_ctrl.function);

        let Some(mmio) = NonNull::new(probe_ctrl.mmio_base as *mut u8) else {
            return Err("mmio");
        };

        let mut host =
            match crab_usb::USBHost::new_xhci(mmio, &self::crabusb_service::CRABUSB_KERNEL) {
            Ok(host) => host,
            Err(_) => return Err("host-new"),
        };

        if host.init().await.is_err() {
            return Err("host-init");
        }

        match host.probe_devices().await {
            Ok(found) => {
                for dev in found.iter() {
                    let desc = dev.descriptor();
                    out.push(TlbUsbDevice {
                        controller_index: probe_ctrl.index,
                        vendor_id: desc.vendor_id,
                        product_id: desc.product_id,
                        class: desc.class,
                        subclass: desc.subclass,
                        protocol: desc.protocol,
                        config_count: dev.configurations().len(),
                        interface_count: dev.interface_descriptors().count(),
                    });
                }
                Ok(out)
            }
            Err(_) => Err("probe"),
        }
    });

    match devices {
        Ok(devices) => TlbUsbSnapshot {
            controllers,
            devices,
            probe_error: None,
        },
        Err(probe_error) => TlbUsbSnapshot {
            controllers,
            devices: Vec::new(),
            probe_error: Some(probe_error),
        },
    }
}

pub(crate) use self::crabusb_service::{
    audio_task as crabusb_audio_task, bsp_service as crabusb_bsp_service,
    event_pump_task as crabusb_event_pump_task, truekey_task as crabusb_truekey_task,
};
