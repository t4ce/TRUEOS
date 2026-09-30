//! RTL8153's CDC ECM configuration: one device owner, three independent pipes.
use super::{cdc, crabusb};
use crate::net::usb::{QUEUE_CAP, Shared, UsbNic};
use alloc::vec;
use crabusb::usb_if::{
    endpoint::TransferRequest,
    host::ControlSetup,
    transfer::{Recipient, Request, RequestType},
};

pub(super) async fn maybe_start(
    host: &mut crabusb::USBHost,
    info: &crabusb::DeviceInfo,
    spawner: &trueos_executor::Spawner,
    controller: u32,
) -> bool {
    if (info.vendor_id(), info.product_id()) != (0x0bda, 0x8153) {
        return false;
    }
    let Some(target) = info
        .configurations()
        .iter()
        .find_map(|config| cdc::target(&config.raw))
    else {
        crate::log!(
            "usb-net: RTL8153 ctrl={} slot={} no valid ECM descriptors; not claimed\n",
            controller,
            info.id()
        );
        return true;
    };
    match host.open_device(info).await {
        Ok(device) => match usb_rtl8153_worker_task(device, target, controller, *spawner) {
            Ok(task) => spawner.spawn(task),
            Err(err) => crate::log!(
                "usb-net: worker spawn failed ctrl={} slot={} err={:?}\n",
                controller,
                info.id(),
                err
            ),
        },
        Err(err) => crate::log!(
            "usb-net: open failed ctrl={} slot={} err={:?}\n",
            controller,
            info.id(),
            err
        ),
    }
    true
}

fn fail(shared: &Shared, pipe: &str) {
    let mut state = shared.lock();
    state.failed = true;
    state.link = crate::net::device::LinkState::down();
    state.rx.clear();
    state.tx.clear();
    crate::log!("usb-net: transport stopped pipe={} link=down\n", pipe);
}

async fn initialize(
    device: &mut crabusb::Device,
    t: cdc::EcmTarget,
) -> Result<[u8; 6], &'static str> {
    device
        .set_configuration(t.configuration)
        .await
        .map_err(|_| "set configuration")?;
    let mac_text = device
        .string_descriptor(t.mac_index)
        .await
        .map_err(|_| "read MAC string")?;
    let mac = cdc::mac(&mac_text).ok_or("invalid MAC string")?;
    device
        .claim_interface(t.control, t.control_alt)
        .await
        .map_err(|_| "claim control interface")?;
    device
        .claim_interface(t.data, t.data_alt)
        .await
        .map_err(|_| "claim data interface")?;
    // Directed, broadcast and all multicast (ARP, DHCP, IPv6 discovery).
    device
        .control_out(
            ControlSetup {
                request_type: RequestType::Class,
                recipient: Recipient::Interface,
                request: Request::Other(0x43),
                value: 0x0e,
                index: t.control as u16,
            },
            &[],
        )
        .await
        .map_err(|_| "set Ethernet packet filter")?;
    Ok(mac)
}

#[trueos_executor::task(pool_size = 2)]
async fn usb_rtl8153_worker_task(
    mut device: crabusb::Device,
    t: cdc::EcmTarget,
    controller: u32,
    spawner: trueos_executor::Spawner,
) {
    let slot = device.slot_id();
    let mac = match initialize(&mut device, t).await {
        Ok(mac) => mac,
        Err(stage) => {
            crate::log!("usb-net: init failed ctrl={} slot={} stage={}\n", controller, slot, stage);
            return;
        }
    };
    let (Ok(mut rx), Ok(mut tx), Ok(mut status)) =
        (device.endpoint(t.bulk_in), device.endpoint(t.bulk_out), device.endpoint(t.notification))
    else {
        crate::log!("usb-net: endpoint open failed ctrl={} slot={}\n", controller, slot);
        return;
    };
    let nic = UsbNic::new(mac);
    let shared = nic.shared.clone();
    let index = match crate::net::register_usb_nic(nic) {
        Ok(index) => index,
        Err(err) => {
            crate::log!("usb-net: register failed reason={}\n", err);
            return;
        }
    };
    match crate::net::adapter::net_service_task(index) {
        Ok(task) => spawner.spawn(task),
        Err(err) => {
            fail(&shared, "service-spawn");
            crate::log!("usb-net: service spawn failed {:?}\n", err);
            return;
        }
    }
    crate::log!(
        "usb-net: ready ctrl={} slot={} vid=0bda pid=8153 net={} cfg={} control={} data={}/{} rx={:02x} tx={:02x} status={:02x} mac={:02x?} link=await-notification\n",
        controller,
        slot,
        index,
        t.configuration,
        t.control,
        t.data,
        t.data_alt,
        t.bulk_in,
        t.bulk_out,
        t.notification,
        mac
    );

    // Each wait retains its DMA buffer until completion. No timer/select drops a
    // pending USB future. A failed pipe marks the shared NIC down; other pending
    // pipes keep their buffers and the device owner alive until they complete.
    let receive = async {
        let mut buffer = vec![0u8; 2048];
        loop {
            if shared.lock().failed {
                break;
            }
            match rx.wait(TransferRequest::bulk_in(&mut buffer)).await {
                Ok(done) if (14..=1514).contains(&done.actual_length) => {
                    let mut state = shared.lock();
                    if !state.failed && state.rx.len() < QUEUE_CAP {
                        state.rx.push_back(buffer[..done.actual_length].to_vec());
                    }
                }
                Ok(_) => {}
                Err(err) => {
                    crate::log!("usb-net: RX error {:?}\n", err);
                    fail(&shared, "rx");
                    break;
                }
            }
        }
    };
    let transmit = async {
        loop {
            if shared.lock().failed {
                break;
            }
            let frame = shared.lock().tx.pop_front();
            if let Some(mut frame) = frame {
                frame.resize(frame.len().max(60), 0);
                // ECM uses short packets to terminate frames. As with usbnet's
                // FLAG_ETHER path, an extra padding byte avoids a full-size tail.
                if frame.len().is_multiple_of(t.out_packet as usize) {
                    frame.push(0);
                }
                match tx.wait(TransferRequest::bulk_out(&frame)).await {
                    Ok(done) if done.actual_length == frame.len() => {}
                    result => {
                        crate::log!("usb-net: TX failed {:?}\n", result);
                        fail(&shared, "tx");
                        break;
                    }
                }
            } else {
                trueos_time::Timer::after(trueos_time::Duration::from_millis(1)).await;
            }
        }
    };
    let notifications = async {
        let mut buffer = vec![0u8; 16];
        let mut speed_payload = false;
        loop {
            if shared.lock().failed {
                break;
            }
            let len = match status
                .wait(TransferRequest::interrupt_in(&mut buffer))
                .await
            {
                Ok(done) => done.actual_length.min(buffer.len()),
                Err(err) => {
                    crate::log!("usb-net: status error {:?}\n", err);
                    fail(&shared, "status");
                    break;
                }
            };
            if speed_payload {
                speed_payload = false;
                if len == 8 {
                    shared.lock().link.speed_mbps =
                        u32::from_le_bytes(buffer[..4].try_into().unwrap()) / 1_000_000;
                }
                continue;
            }
            if len < 8
                || buffer[0] != 0xa1
                || u16::from_le_bytes([buffer[4], buffer[5]]) != t.control as u16
            {
                continue;
            }
            let payload = u16::from_le_bytes([buffer[6], buffer[7]]);
            match buffer[1] {
                0 if payload == 0 => {
                    let up = u16::from_le_bytes([buffer[2], buffer[3]]) != 0;
                    let mut state = shared.lock();
                    if state.failed {
                        break;
                    }
                    state.link.up = up;
                    if !up {
                        state.link.speed_mbps = 0;
                        state.tx.clear();
                    }
                    crate::log!("usb-net: net={} link={}\n", index, if up { "up" } else { "down" });
                    drop(state);
                    if up {
                        crate::net::prefer_link_up_device(index);
                    }
                }
                0x2a if payload == 8 => {
                    if len == 16 {
                        shared.lock().link.speed_mbps =
                            u32::from_le_bytes(buffer[8..12].try_into().unwrap()) / 1_000_000;
                    } else if len == 8 {
                        speed_payload = true;
                    }
                }
                _ => {}
            }
        }
    };
    use core::{future::Future, task::Poll};
    let mut receive = core::pin::pin!(receive);
    let mut transmit = core::pin::pin!(transmit);
    let mut notifications = core::pin::pin!(notifications);
    let mut done = [false; 3];
    core::future::poll_fn(|cx| {
        if !done[0] {
            done[0] = receive.as_mut().poll(cx).is_ready();
        }
        if !done[1] {
            done[1] = transmit.as_mut().poll(cx).is_ready();
        }
        if !done[2] {
            done[2] = notifications.as_mut().poll(cx).is_ready();
        }
        if done.iter().all(|done| *done) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
    drop(device);
}
