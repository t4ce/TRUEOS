use super::xhci::{TrbRing, XhciContext};
use super::{
    DeviceKind, UsbControllerState, cdc_acm, hid, leds, mass, midi, non_generic, pen, print, uac,
};

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
    cfg_slice: &[u8],
    dev_ctx_virt: *mut u8,
    ctx_stride_bytes: usize,
    ctx_stride_words: usize,
    speed_code: u32,
    target_port: u8,
) -> Option<DeviceKind> {
    let registry_port = target_port;

    // Known USB LED controllers.
    if leds::is_supported_led_controller(dev_vid, dev_pid) {
        crate::log!(
            "usb: attach: trying leds driver port={} slot={}\n",
            target_port,
            slot_id
        );
    }
    if leds::attach_device(leds::AttachParams {
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
        dev_vid,
        dev_pid,
    })
    .await
    .is_ok()
    {
        usbv!(
            "usb: enum port {} claimed LEDS slot={} vid=0x{:04X} pid=0x{:04X}\n",
            target_port,
            slot_id,
            dev_vid,
            dev_pid
        );
        super::register_device(
            state.info.controller_id,
            slot_id,
            registry_port,
            DeviceKind::Leds,
        );
        return Some(DeviceKind::Leds);
    }

    let has_uac_out = uac::has_as_out_endpoint(cfg_slice);
    if has_uac_out
        && uac::attach_device(uac::AttachParams {
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
        super::register_device(
            state.info.controller_id,
            slot_id,
            registry_port,
            DeviceKind::Uac,
        );
        return Some(DeviceKind::Uac);
    }

    let has_midi = midi::has_midi_streaming_interface(cfg_slice);
    if has_midi
        && midi::attach_device(midi::AttachParams {
            ctx,
            cmd_ring: &mut state.cmd_ring,
            ep0_ring,
            slot_id,
            cfg: cfg_slice,
            dev_ctx_virt,
            ctx_stride_bytes,
            ctx_stride_words,
            target_port,
            dev_vid,
            dev_pid,
        })
        .await
        .is_ok()
    {
        usbv!(
            "usb: enum port {} claimed MIDI slot={} vid=0x{:04X} pid=0x{:04X}\n",
            target_port,
            slot_id,
            dev_vid,
            dev_pid
        );
        super::register_device(
            state.info.controller_id,
            slot_id,
            registry_port,
            DeviceKind::Midi,
        );
        return Some(DeviceKind::Midi);
    }

    let _ = dev_cls;
    let _ = dev_sub;
    let _ = dev_prot;

    let hid_count = hid::attach_hid_devices(hid::BootAttachParams {
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
            "usb: enum port {} claimed HID slot={} vid=0x{:04X} pid=0x{:04X} (interfaces claimed: {})\n",
            target_port,
            slot_id,
            dev_vid,
            dev_pid,
            hid_count
        );

        if super::USB_LOG_VERBOSE {
            non_generic::log_hid_non_generic_descriptor_tables(
                ctx,
                ep0_ring,
                slot_id,
                target_port,
                cfg_slice,
            )
            .await;
        }

        super::register_device(
            state.info.controller_id,
            slot_id,
            registry_port,
            DeviceKind::Hid,
        );
        return Some(DeviceKind::Hid);
    }

    if super::USB_LOG_VERBOSE {
        non_generic::log_mass_non_generic_descriptor_table(target_port, slot_id, cfg_slice);
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
        super::register_device(
            state.info.controller_id,
            slot_id,
            registry_port,
            DeviceKind::Mass,
        );
        return Some(DeviceKind::Mass);
    }

    if pen::try_handle(cfg_slice, target_port) {
        usbv!(
            "usb: enum port {} claimed PEN slot={}\n",
            target_port,
            slot_id
        );
        super::register_device(
            state.info.controller_id,
            slot_id,
            registry_port,
            DeviceKind::Pen,
        );
        return Some(DeviceKind::Pen);
    }
    if print::try_handle(cfg_slice, target_port) {
        usbv!(
            "usb: enum port {} claimed PRINTER slot={}\n",
            target_port,
            slot_id
        );
        super::register_device(
            state.info.controller_id,
            slot_id,
            registry_port,
            DeviceKind::Printer,
        );
        return Some(DeviceKind::Printer);
    }

    if cdc_acm::attach_device(cdc_acm::AttachParams {
        ctx,
        cmd_ring: &mut state.cmd_ring,
        ep0_ring,
        slot_id,
        dev_vid,
        dev_pid,
        cfg: cfg_slice,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
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
        super::register_device(
            state.info.controller_id,
            slot_id,
            registry_port,
            DeviceKind::Cdc,
        );
        return Some(DeviceKind::Cdc);
    }

    None
}
