use super::hub::HubWork;
use super::xhci::{TrbRing, XhciContext};
use super::{cdc_acm, hid, hub, mass, pen, print, uac, DeviceKind, UsbControllerState};

macro_rules! usbv {
    ($($tt:tt)*) => {{
        if super::USB_LOG_VERBOSE {
            crate::log!($($tt)*);
        }
    }};
}

pub(crate) async fn try_attach_device(
    state: &mut UsbControllerState,
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    dev_vid: u16,
    dev_pid: u16,
    dev_cls: u8,
    dev_sub: u8,
    dev_prot: u8,
    dev_serial: cdc_acm::UsbSerial,
    cfg_slice: &[u8],
    dev_ctx_virt: *mut u8,
    ctx_stride_bytes: usize,
    ctx_stride_words: usize,
    speed_code: u32,
    target_port: u8,
    root_port: u8,
    route_string: u32,
    depth: u8,
    hub_queue: &mut heapless::Vec<HubWork, 16>,
) -> Option<DeviceKind> {
    let has_uac_out = uac::has_as_out_endpoint(cfg_slice);
    if has_uac_out {
        if uac::attach_device(uac::AttachParams {
            ctx,
            cmd_ring: &mut state.cmd_ring,
            ep0_ring,
            slot_id,
            cfg: cfg_slice,
            dev_ctx_virt,
            ctx_stride_bytes,
            ctx_stride_words,
            speed_code,
            target_port,
        })
        .await
        .is_ok()
        {
            usbv!(
                "usb: enum port {} claimed UAC slot={} vid=0x{:04X} pid=0x{:04X}\n",
                target_port,
                slot_id,
                dev_vid,
                dev_pid
            );
            super::register_device(slot_id as u32, target_port, DeviceKind::Uac);
            return Some(DeviceKind::Uac);
        }
    }

    if hub::is_hub_device(dev_cls, dev_sub, dev_prot, cfg_slice) {
        if let Ok(desc) = hub::attach_device(hub::AttachParams {
            ctx,
            ep0_ring,
            slot_id,
            cfg: cfg_slice,
            target_port,
        })
        .await
        {
            usbv!(
                "usb: enum port {} claimed HUB slot={} vid=0x{:04X} pid=0x{:04X}\n",
                target_port,
                slot_id,
                dev_vid,
                dev_pid
            );
            super::register_device(slot_id as u32, target_port, DeviceKind::Hub);
            hub::record_hub_ports(slot_id, desc.port_count);
            let _ = hub_queue.push(HubWork {
                hub_slot_id: slot_id,
                root_port,
                route_string,
                depth,
                hub_speed_code: speed_code,
                port_count: desc.port_count,
            });
            return Some(DeviceKind::Hub);
        }
    }

    let hid_count = hid::attach_boot_devices(hid::BootAttachParams {
        ctx,
        cmd_ring: &mut state.cmd_ring,
        ep0_ring,
        slot_id,
        cfg: cfg_slice,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        speed_code,
        target_port,
    })
    .await
    .unwrap_or(0);

    if hid_count > 0 {
        usbv!(
            "usb: enum port {} claimed HID slot={} vid=0x{:04X} pid=0x{:04X} count={}\n",
            target_port,
            slot_id,
            dev_vid,
            dev_pid,
            hid_count
        );
        super::register_device(slot_id as u32, target_port, DeviceKind::Hid);
        return Some(DeviceKind::Hid);
    }

    if mass::attach_mass_device(mass::AttachParams {
        ctx,
        cmd_ring: &mut state.cmd_ring,
        ep0_ring,
        slot_id,
        cfg: cfg_slice,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        speed_code,
        target_port,
    })
    .await
    .is_ok()
    {
        usbv!(
            "usb: enum port {} claimed MASS slot={}\n",
            target_port,
            slot_id
        );
        super::register_device(slot_id as u32, target_port, DeviceKind::Mass);
        return Some(DeviceKind::Mass);
    }

    if pen::try_handle(cfg_slice, target_port) {
        usbv!(
            "usb: enum port {} claimed PEN slot={}\n",
            target_port,
            slot_id
        );
        super::register_device(slot_id as u32, target_port, DeviceKind::Pen);
        return Some(DeviceKind::Pen);
    }
    if print::try_handle(cfg_slice, target_port) {
        usbv!(
            "usb: enum port {} claimed PRINTER slot={}\n",
            target_port,
            slot_id
        );
        super::register_device(slot_id as u32, target_port, DeviceKind::Printer);
        return Some(DeviceKind::Printer);
    }

    if cdc_acm::attach_device(cdc_acm::AttachParams {
        ctx,
        cmd_ring: &mut state.cmd_ring,
        ep0_ring,
        slot_id,
        dev_vid,
        dev_pid,
        dev_serial,
        cfg: cfg_slice,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        speed_code,
        target_port,
        desired_baud: 115_200,
    })
    .await
    .is_ok()
    {
        usbv!(
            "usb: enum port {} claimed CDC-ACM slot={}\n",
            target_port,
            slot_id
        );
        super::register_device(slot_id as u32, target_port, DeviceKind::Cdc);
        return Some(DeviceKind::Cdc);
    }

    None
}