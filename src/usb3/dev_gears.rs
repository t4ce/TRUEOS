use alloc::vec::Vec;
use spin::Mutex;

use super::crabusb;

pub const USB_DEVICE_POOL_CAP: usize = 8;
const USB_CLASS_HID: u8 = 0x03;
const USB_HID_SUBCLASS_BOOT: u8 = 0x01;
const USB_HID_PROTOCOL_MOUSE: u8 = 0x02;
const HID_REQ_SET_IDLE: u8 = 0x0A;
const HID_REQ_SET_PROTOCOL: u8 = 0x0B;
const HID_BOOT_PROTOCOL: u16 = 0;
const USB3_BOOT_MOUSE_POOL_CAP: usize = 4;

static USB_DEVICE_POOL: Mutex<UsbDevicePool> = Mutex::new(UsbDevicePool::new());
static USB_BOOT_MOUSE_POOL: Mutex<UsbBootMousePool> = Mutex::new(UsbBootMousePool::new());

struct UsbDevicePool {
    devices: Vec<PooledUsbDevice>,
    pending_worker: Vec<usize>,
}

pub(super) struct PooledUsbDevice {
    pub(super) id: usize,
    pub(super) vendor_id: u16,
    pub(super) product_id: u16,
    pub(super) class: u8,
    pub(super) subclass: u8,
    pub(super) protocol: u8,
    pub(super) device: crabusb::Device,
}

struct UsbBootMousePool {
    devices: Vec<PooledUsbBootMouse>,
}

#[derive(Clone, Copy, Debug)]
struct BootMouseTarget {
    configuration_value: u8,
    interface_number: u8,
    alternate_setting: u8,
    interrupt_in: u8,
    max_packet_size: u16,
    interval: u8,
}

struct PooledUsbBootMouse {
    device: PooledUsbDevice,
    target: BootMouseTarget,
}

impl UsbDevicePool {
    const fn new() -> Self {
        Self {
            devices: Vec::new(),
            pending_worker: Vec::new(),
        }
    }

    fn insert_for_worker(&mut self, device: crabusb::Device) -> Result<usize, crabusb::Device> {
        if self.devices.len() >= USB_DEVICE_POOL_CAP {
            return Err(device);
        }

        let desc = device.descriptor();
        let pooled = PooledUsbDevice {
            id: device.slot_id() as usize,
            vendor_id: desc.vendor_id,
            product_id: desc.product_id,
            class: desc.class,
            subclass: desc.subclass,
            protocol: desc.protocol,
            device,
        };
        let id = pooled.id;
        self.devices.push(pooled);
        self.pending_worker.push(id);
        Ok(id)
    }

    fn pop_pending_worker(&mut self) -> Option<PooledUsbDevice> {
        let id = self.pending_worker.pop()?;
        let idx = self.devices.iter().position(|device| device.id == id)?;
        Some(self.devices.remove(idx))
    }

    fn len(&self) -> usize {
        self.devices.len()
    }
}

impl UsbBootMousePool {
    const fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    fn insert(&mut self, device: PooledUsbBootMouse) -> Result<usize, PooledUsbBootMouse> {
        if self.devices.len() >= USB3_BOOT_MOUSE_POOL_CAP {
            return Err(device);
        }
        self.devices.push(device);
        Ok(self.devices.len())
    }

    fn pop(&mut self) -> Option<PooledUsbBootMouse> {
        self.devices.pop()
    }
}

pub fn handoff_opened_device(device: crabusb::Device) -> Result<usize, crabusb::Device> {
    let mut pool = USB_DEVICE_POOL.lock();
    pool.insert_for_worker(device)?;
    Ok(pool.len())
}

pub fn has_boot_mouse_transport(
    configs: &[crabusb::usb_if::descriptor::ConfigurationDescriptor],
) -> bool {
    !collect_boot_mouse_candidates(configs).is_empty()
}

fn handoff_boot_mouse_device(
    device: PooledUsbDevice,
    target: BootMouseTarget,
) -> Result<usize, PooledUsbBootMouse> {
    let mut pool = USB_BOOT_MOUSE_POOL.lock();
    pool.insert(PooledUsbBootMouse { device, target })
}

#[embassy_executor::task]
pub async fn usb_device_pool_worker_task() {
    loop {
        let next = {
            let mut pool = USB_DEVICE_POOL.lock();
            pool.pop_pending_worker()
        };

        if let Some(device) = next {
            crate::log!(
                "crabusb: device worker handoff id={} vid={:04x} pid={:04x} class={:02x}:{:02x}:{:02x}\n",
                device.id,
                device.vendor_id,
                device.product_id,
                device.class,
                device.subclass,
                device.protocol
            );
            process_opened_device(device).await;
        } else {
            embassy_time::Timer::after(embassy_time::Duration::from_millis(25)).await;
        }
    }
}

async fn process_opened_device(device: PooledUsbDevice) {
    if device.vendor_id == 0x152e && device.product_id == 0x7001 {
        super::skhynix::start_green_uas(device).await;
    } else {
        crate::log!(
            "crabusb: device worker ignored id={} vid={:04x} pid={:04x} reason=usb3-skhynix-only\n",
            device.id,
            device.vendor_id,
            device.product_id
        );
    }
}

#[embassy_executor::task]
pub async fn usb_boot_mouse_worker_task() {
    loop {
        let next = {
            let mut pool = USB_BOOT_MOUSE_POOL.lock();
            pool.pop()
        };

        if let Some(mouse) = next {
            poll_usb3_boot_mouse(mouse).await;
        } else {
            embassy_time::Timer::after(embassy_time::Duration::from_millis(25)).await;
        }
    }
}

async fn poll_usb3_boot_mouse(mouse: PooledUsbBootMouse) {
    let PooledUsbBootMouse {
        device: mut pooled,
        target,
    } = mouse;

    if let Err(err) = pooled
        .device
        .set_configuration(target.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: boot-mouse {:04x}:{:04x} proof=set-config cfg={} status=failed err={:?}\n",
            pooled.vendor_id,
            pooled.product_id,
            target.configuration_value,
            err
        );
        return;
    }

    if let Err(err) =
        hid_class_control_out(&mut pooled.device, target.interface_number, HID_REQ_SET_IDLE, 0)
            .await
    {
        crate::log_trace!(
            target: "usb";
            "crabusb: boot-mouse {:04x}:{:04x} proof=set-idle if#{} status=failed err={:?}\n",
            pooled.vendor_id,
            pooled.product_id,
            target.interface_number,
            err
        );
    }

    if let Err(err) = hid_class_control_out(
        &mut pooled.device,
        target.interface_number,
        HID_REQ_SET_PROTOCOL,
        HID_BOOT_PROTOCOL,
    )
    .await
    {
        crate::log!(
            "crabusb: boot-mouse {:04x}:{:04x} proof=set-protocol if#{} status=failed err={:?}\n",
            pooled.vendor_id,
            pooled.product_id,
            target.interface_number,
            err
        );
        return;
    }

    if let Err(err) = pooled
        .device
        .claim_interface(target.interface_number, target.alternate_setting)
        .await
    {
        crate::log!(
            "crabusb: boot-mouse {:04x}:{:04x} proof=claim if#{} alt={} status=failed err={:?}\n",
            pooled.vendor_id,
            pooled.product_id,
            target.interface_number,
            target.alternate_setting,
            err
        );
        return;
    }
    crate::log!(
        "crabusb: boot-mouse {:04x}:{:04x} proof=claim if#{} alt={} status=ok\n",
        pooled.vendor_id,
        pooled.product_id,
        target.interface_number,
        target.alternate_setting
    );

    let mut interrupt_in = match pooled.device.endpoint(target.interrupt_in) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            crate::log!(
                "crabusb: boot-mouse {:04x}:{:04x} proof=endpoint ep=0x{:02x} status=failed err={:?}\n",
                pooled.vendor_id,
                pooled.product_id,
                target.interrupt_in,
                err
            );
            return;
        }
    };

    crate::log!(
        "crabusb: boot-mouse {:04x}:{:04x} proof=start id={} if#{} alt={} ep=0x{:02x}/{} interval={} poll=1khz\n",
        pooled.vendor_id,
        pooled.product_id,
        pooled.id,
        target.interface_number,
        target.alternate_setting,
        target.interrupt_in,
        target.max_packet_size,
        target.interval
    );

    let mut last_buttons = 0u32;
    let mut completion_logs = 0u32;
    let mut report_logs = 0u32;
    loop {
        let mut report = [0u8; 8];
        let completion = interrupt_in
            .wait(crabusb::usb_if::endpoint::TransferRequest::interrupt_in(&mut report))
            .await;

        match completion {
            Ok(done) => {
                let len = done.actual_length.min(report.len());
                completion_logs = completion_logs.wrapping_add(1);
                if completion_logs <= 16 || completion_logs.is_multiple_of(256) {
                    crate::log!(
                        "crabusb: boot-mouse {:04x}:{:04x} completion id={} actual={} len={} raw={:02x?} count={}\n",
                        pooled.vendor_id,
                        pooled.product_id,
                        pooled.id,
                        done.actual_length,
                        len,
                        &report[..len],
                        completion_logs
                    );
                }
                if len >= 3 {
                    let buttons = (report[0] & 0x07) as u32;
                    let dx = report[1] as i8;
                    let dy = report[2] as i8;
                    let wheel = if len >= 4 { report[3] as i8 as i16 } else { 0 };
                    if dx != 0 || dy != 0 || wheel != 0 || buttons != last_buttons {
                        report_logs = report_logs.wrapping_add(1);
                        if report_logs <= 16 || report_logs.is_multiple_of(64) {
                            crate::log!(
                                "crabusb: boot-mouse {:04x}:{:04x} report id={} len={} buttons=0x{:02x} dx={} dy={} wheel={} raw={:02x?} count={}\n",
                                pooled.vendor_id,
                                pooled.product_id,
                                pooled.id,
                                len,
                                buttons,
                                dx,
                                dy,
                                wheel,
                                &report[..len],
                                report_logs
                            );
                        }
                        super::hid::inject_usb3_mouse_relative_event(
                            pooled.id as u32,
                            target.interrupt_in as u32,
                            dx,
                            dy,
                            buttons,
                            wheel,
                            0,
                        );
                        last_buttons = buttons;
                    }
                }
            }
            Err(err) => {
                crate::log!(
                    "crabusb: boot-mouse {:04x}:{:04x} proof=poll status=failed err={:?}\n",
                    pooled.vendor_id,
                    pooled.product_id,
                    err
                );
                embassy_time::Timer::after(embassy_time::Duration::from_millis(25)).await;
            }
        }

        embassy_time::Timer::after(embassy_time::Duration::from_millis(1)).await;
    }
}

async fn hid_class_control_out(
    device: &mut crabusb::Device,
    interface_number: u8,
    request: u8,
    value: u16,
) -> Result<usize, crabusb::usb_if::err::TransferError> {
    device
        .control_out(
            crabusb::usb_if::host::ControlSetup {
                request_type: crabusb::usb_if::transfer::RequestType::Class,
                recipient: crabusb::usb_if::transfer::Recipient::Interface,
                request: crabusb::usb_if::transfer::Request::Other(request),
                value,
                index: interface_number as u16,
            },
            &[],
        )
        .await
}

fn collect_boot_mouse_candidates(
    configs: &[crabusb::usb_if::descriptor::ConfigurationDescriptor],
) -> Vec<BootMouseTarget> {
    let mut out = Vec::new();

    for config in configs {
        for interface in &config.interfaces {
            for alt in &interface.alt_settings {
                if alt.class != USB_CLASS_HID
                    || alt.subclass != USB_HID_SUBCLASS_BOOT
                    || alt.protocol != USB_HID_PROTOCOL_MOUSE
                {
                    continue;
                }

                for ep in &alt.endpoints {
                    if ep.transfer_type != crabusb::usb_if::descriptor::EndpointType::Interrupt {
                        continue;
                    }
                    if ep.direction != crabusb::usb_if::transfer::Direction::In {
                        continue;
                    }
                    out.push(BootMouseTarget {
                        configuration_value: config.configuration_value,
                        interface_number: interface.interface_number,
                        alternate_setting: alt.alternate_setting,
                        interrupt_in: ep.address,
                        max_packet_size: ep.max_packet_size,
                        interval: ep.interval,
                    });
                }
            }
        }
    }

    out
}

fn pick_boot_mouse_target(candidates: &[BootMouseTarget]) -> Option<BootMouseTarget> {
    candidates.first().copied()
}
