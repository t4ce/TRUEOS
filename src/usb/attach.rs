use super::hub::HubWork;
use super::xhci::{TrbRing, XhciContext};
use super::{cdc_acm, hid, hub, leds, mass, midi, pen, print, uac, DeviceKind, UsbControllerState};

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
    root_port: u8,
    route_string: u32,
    depth: u8,
    hub_queue: &mut heapless::Vec<HubWork, 16>,
) -> Option<DeviceKind> {
    // Device tracking / disconnect cleanup is currently keyed by root-port.
    // For hub children, `target_port` is the downstream hub port (not a root port),
    // so use `root_port` when it is known.
    let registry_port = if root_port != 0 {
        root_port
    } else {
        target_port
    };

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
            slot_id as u32,
            registry_port,
            DeviceKind::Leds,
        );
        return Some(DeviceKind::Leds);
    }

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
            super::register_device(
                state.info.controller_id,
                slot_id as u32,
                registry_port,
                DeviceKind::Uac,
            );
            return Some(DeviceKind::Uac);
        }
    }

    let has_midi = midi::has_midi_streaming_interface(cfg_slice);
    if has_midi {
        if midi::attach_device(midi::AttachParams {
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
                slot_id as u32,
                registry_port,
                DeviceKind::Midi,
            );
            return Some(DeviceKind::Midi);
        }
    }

    if hub::is_hub_device(dev_cls, dev_sub, dev_prot, cfg_slice) {
        if let Ok(desc) = hub::attach_device(hub::AttachParams {
            ctx,
            ep0_ring,
            slot_id,
            cfg: cfg_slice,
            target_port,
            dev_prot,
        })
        .await
        {
            let multi_tt = dev_prot == 2;
            let _ = hub::configure_hub_interrupt(hub::HubInterruptParams {
                ctx,
                cmd_ring: &mut state.cmd_ring,
                slot_id,
                dev_ctx_virt,
                ctx_stride_bytes,
                ctx_stride_words,
                target_port,
                port_count: desc.port_count,
                tt_think_time: desc.tt_think_time,
                multi_tt,
                speed_code,
                cfg: cfg_slice,
            })
            .await;
            // Always program the hub's slot-context fields (Hub bit / ports / TT think time)
            // before enumerating children. Interrupt endpoint config may succeed/fail
            // independently, but the xHC must know this slot represents a hub.
            if hub::configure_hub_context(hub::HubConfigParams {
                ctx,
                cmd_ring: &mut state.cmd_ring,
                slot_id,
                dev_ctx_virt,
                ctx_stride_bytes,
                ctx_stride_words,
                target_port,
                port_count: desc.port_count,
                tt_think_time: desc.tt_think_time,
                multi_tt,
            })
            .await
            .is_err()
            {
                crate::log!("usb: hub slot {} hub slot-context update failed\n", slot_id);
            }

            // High-signal: confirm the hub bit / ports / think-time are actually
            // present in the hub's *output* Slot Context before enumerating children.
            if super::USB_LOG_VERBOSE || dev_prot == 3 {
                unsafe {
                    let slot_ctx = dev_ctx_virt as *const u32;
                    let dw0 = core::ptr::read_volatile(slot_ctx.add(0));
                    let dw1 = core::ptr::read_volatile(slot_ctx.add(1));
                    let dw2 = core::ptr::read_volatile(slot_ctx.add(2));
                    let dw3 = core::ptr::read_volatile(slot_ctx.add(3));
                    let dw4 = core::ptr::read_volatile(slot_ctx.add(4));
                    let dw5 = core::ptr::read_volatile(slot_ctx.add(5));
                    let dw6 = core::ptr::read_volatile(slot_ctx.add(6));
                    let dw7 = core::ptr::read_volatile(slot_ctx.add(7));
                    crate::log!(
                        "usb: hub slot {} slotctx dw0..7=[{:08X} {:08X} {:08X} {:08X} {:08X} {:08X} {:08X} {:08X}] (hub_bit={} mtt={} ctx_entries={} rh_port={} ports_dw1={} ports_dw2={} tt_think_dw2={})\n",
                        slot_id,
                        dw0,
                        dw1,
                        dw2,
                        dw3,
                        dw4,
                        dw5,
                        dw6,
                        dw7,
                        ((dw0 >> 26) & 1),
                        ((dw0 >> 25) & 1),
                        ((dw0 >> 27) & 0x1F),
                        ((dw1 >> 16) & 0xFF),
                        ((dw1 >> 24) & 0xFF),
                        ((dw2 >> 24) & 0xFF),
                        ((dw2 >> 16) & 0x3),
                    );
                }
            }

            hub::register_ep0_ring(ctx, slot_id, ep0_ring);
            usbv!(
                "usb: enum port {} claimed HUB slot={} vid=0x{:04X} pid=0x{:04X}\n",
                target_port,
                slot_id,
                dev_vid,
                dev_pid
            );
            super::register_device(
                state.info.controller_id,
                slot_id as u32,
                registry_port,
                DeviceKind::Hub,
            );
            hub::record_hub_ports(ctx, slot_id, desc.port_count);
            let _ = hub_queue.push(HubWork {
                hub_slot_id: slot_id,
                root_port,
                route_string,
                depth,
                hub_speed_code: speed_code,
                multi_tt,
                port_count: desc.port_count,
                power_on_good_ms: desc.power_on_good_ms,
                tt_think_time: desc.tt_think_time,
            });
            return Some(DeviceKind::Hub);
        }
    }

    // QEMU's USB tablet (0627:0001) is not a boot device; attaching it through the
    // boot-HID path can accidentally bind the wrong interface/endpoint and yield
    // non-moving cursor behavior. Force it through generic HID.
    let is_qemu_tablet = dev_vid == 0x0627 && dev_pid == 0x0001;

    let hid_count = if is_qemu_tablet {
        hid::attach_hid_devices(hid::BootAttachParams {
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
        .unwrap_or(0)
    } else {
        let boot = hid::attach_boot_devices(hid::BootAttachParams {
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

        if boot > 0 {
            boot
        } else {
            // Fall back to generic HID (non-boot) so we can claim devices like LED controllers.
            hid::attach_hid_devices(hid::BootAttachParams {
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
            .unwrap_or(0)
        }
    };

    if hid_count > 0 {
        usbv!(
            "usb: enum port {} claimed HID slot={} vid=0x{:04X} pid=0x{:04X} count={}\n",
            target_port,
            slot_id,
            dev_vid,
            dev_pid,
            hid_count
        );
        super::register_device(
            state.info.controller_id,
            slot_id as u32,
            registry_port,
            DeviceKind::Hid,
        );
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
            slot_id as u32,
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
            slot_id as u32,
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
            slot_id as u32,
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
            slot_id as u32,
            registry_port,
            DeviceKind::Cdc,
        );
        return Some(DeviceKind::Cdc);
    }

    None
}
