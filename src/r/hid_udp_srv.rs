//! TRUEOS UDP HID input service.
//!
//! Wire format, little-endian:
//! - bytes 0..4:  "THID"
//! - byte 4:      version, currently 1
//! - byte 5:      kind: 1 mouse, 2 keyboard, 3 tablet
//! - bytes 6..8:  flags
//! - bytes 8..12: sequence number, monotonic per udp device/kind
//! - bytes 12..14: udp device id
//! - bytes 14..16: reserved
//!
//! Payloads:
//! - mouse:    buttons u8, dx i8, dy i8, wheel i8
//! - keyboard: modifiers u8, reserved u8, six HID boot key bytes
//! - tablet:   x_q16 u32, y_q16 u32, buttons u32

use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::task;
use embassy_time::{Duration, Timer};
use heapless::Vec;

use crate::r::net::VNet;

pub const TRUEOS_HID_UDP_PORT: u16 = crate::allports::services::TRUEOS_HID_UDP_PORT;

const MAGIC: &[u8; 4] = b"THID";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 16;
const KIND_MOUSE: u8 = 1;
const KIND_KEYBOARD: u8 = 2;
const KIND_TABLET: u8 = 3;
const DEVICE_STATE_CAP: usize = crate::allcaps::input::HID_UDP_DEVICE_STATE_CAP;

static RX_ACCEPTED: AtomicU32 = AtomicU32::new(0);
static RX_STALE: AtomicU32 = AtomicU32::new(0);
static RX_BAD: AtomicU32 = AtomicU32::new(0);

#[derive(Copy, Clone, Debug)]
struct DeviceSeq {
    device_id: u16,
    kind: u8,
    last_seq: u32,
}

#[derive(Copy, Clone, Debug)]
struct HidUdpFrame<'a> {
    kind: u8,
    flags: u16,
    seq: u32,
    device_id: u16,
    payload: &'a [u8],
}

#[inline]
fn read_u16(data: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(off..off + 2)?.try_into().ok()?))
}

#[inline]
fn read_u32(data: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(off..off + 4)?.try_into().ok()?))
}

fn parse_frame(data: &[u8]) -> Option<HidUdpFrame<'_>> {
    if data.len() < HEADER_LEN || data.get(0..4) != Some(MAGIC) || data[4] != VERSION {
        return None;
    }

    let kind = data[5];
    if !matches!(kind, KIND_MOUSE | KIND_KEYBOARD | KIND_TABLET) {
        return None;
    }

    Some(HidUdpFrame {
        kind,
        flags: read_u16(data, 6)?,
        seq: read_u32(data, 8)?,
        device_id: read_u16(data, 12)?,
        payload: &data[HEADER_LEN..],
    })
}

fn sequence_is_fresh(
    seqs: &mut Vec<DeviceSeq, DEVICE_STATE_CAP>,
    device_id: u16,
    kind: u8,
    seq: u32,
) -> bool {
    if let Some(entry) = seqs
        .iter_mut()
        .find(|entry| entry.device_id == device_id && entry.kind == kind)
    {
        if seq <= entry.last_seq {
            return false;
        }
        entry.last_seq = seq;
        return true;
    }

    if seqs
        .push(DeviceSeq {
            device_id,
            kind,
            last_seq: seq,
        })
        .is_err()
    {
        return false;
    }

    true
}

fn accept_frame(frame: HidUdpFrame<'_>, seqs: &mut Vec<DeviceSeq, DEVICE_STATE_CAP>) -> bool {
    match frame.kind {
        KIND_MOUSE if frame.payload.len() < 4 => return false,
        KIND_KEYBOARD if frame.payload.len() < 8 => return false,
        KIND_TABLET if frame.payload.len() < 12 => return false,
        KIND_TABLET
            if read_u32(frame.payload, 0).is_none()
                || read_u32(frame.payload, 4).is_none()
                || read_u32(frame.payload, 8).is_none() =>
        {
            return false;
        }
        KIND_MOUSE | KIND_KEYBOARD | KIND_TABLET => {}
        _ => return false,
    }

    if !sequence_is_fresh(seqs, frame.device_id, frame.kind, frame.seq) {
        let n = RX_STALE.fetch_add(1, Ordering::Relaxed) + 1;
        if n <= 8 || n.is_power_of_two() {
            crate::log!(
                "hid-udp: stale packet dev={} kind={} seq={} stale_count={}\n",
                frame.device_id,
                frame.kind,
                frame.seq,
                n
            );
        }
        return false;
    }

    match frame.kind {
        KIND_MOUSE => {
            crate::usb2::hid::inject_udp_mouse_boot_report(
                frame.device_id,
                frame.payload[0],
                frame.payload[1] as i8,
                frame.payload[2] as i8,
                frame.payload[3] as i8,
            );
        }
        KIND_KEYBOARD => {
            let keys = [
                frame.payload[2],
                frame.payload[3],
                frame.payload[4],
                frame.payload[5],
                frame.payload[6],
                frame.payload[7],
            ];
            crate::usb2::hid::inject_udp_keyboard_boot_report(
                frame.device_id,
                frame.payload[0],
                keys,
            );
        }
        KIND_TABLET => {
            let Some(x_q16) = read_u32(frame.payload, 0) else {
                return false;
            };
            let Some(y_q16) = read_u32(frame.payload, 4) else {
                return false;
            };
            let Some(buttons) = read_u32(frame.payload, 8) else {
                return false;
            };
            let x = f64::from(x_q16.min(65535)) / 65535.0;
            let y = f64::from(y_q16.min(65535)) / 65535.0;
            crate::usb2::hid::inject_udp_tablet_absolute_event(
                frame.device_id,
                x,
                y,
                buttons,
                frame.flags as u32,
            );
        }
        _ => return false,
    }

    let n = RX_ACCEPTED.fetch_add(1, Ordering::Relaxed) + 1;
    if n <= 8 {
        crate::log!(
            "hid-udp: accepted dev={} kind={} seq={} slot={} count={}\n",
            frame.device_id,
            frame.kind,
            frame.seq,
            crate::usb2::hid::hid_udp_slot_id(frame.device_id),
            n
        );
    }
    true
}

fn handle_packet(data: &[u8], seqs: &mut Vec<DeviceSeq, DEVICE_STATE_CAP>) {
    let Some(frame) = parse_frame(data) else {
        let n = RX_BAD.fetch_add(1, Ordering::Relaxed) + 1;
        if n <= 8 || n.is_power_of_two() {
            crate::log!("hid-udp: ignored bad packet bytes={} bad_count={}\n", data.len(), n);
        }
        return;
    };

    if !accept_frame(frame, seqs) {
        let n = RX_BAD.fetch_add(1, Ordering::Relaxed) + 1;
        if n <= 8 || n.is_power_of_two() {
            crate::log!(
                "hid-udp: ignored invalid packet dev={} kind={} seq={} bytes={} bad_count={}\n",
                frame.device_id,
                frame.kind,
                frame.seq,
                data.len(),
                n
            );
        }
    }
}

#[task]
pub async fn hid_udp_srv_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

    loop {
        let Some(vnet) = VNet::open_primary() else {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        };

        let mut handle = None;
        let mut seqs: Vec<DeviceSeq, DEVICE_STATE_CAP> = Vec::new();
        let _ = vnet.submit(v::vnet::Command::OpenUdp {
            port: TRUEOS_HID_UDP_PORT,
        });
        crate::log!(
            "hid-udp: starting listener udp_port={} controller=0x{:08X}\n",
            TRUEOS_HID_UDP_PORT,
            crate::usb2::hid::HID_UDP_CONTROLLER_ID
        );

        loop {
            if let Some(ev) = vnet.pop_event() {
                match ev {
                    v::vnet::Event::Opened { handle: h, kind }
                        if kind == v::vnet::SocketKind::Udp =>
                    {
                        handle = Some(h);
                        crate::log!(
                            "hid-udp: listener bound handle={} port={}\n",
                            h.0,
                            TRUEOS_HID_UDP_PORT
                        );
                    }
                    v::vnet::Event::Closed { handle: h } if handle == Some(h) => {
                        handle = None;
                        seqs.clear();
                        crate::log!("hid-udp: listener closed, reopening\n");
                        let _ = vnet.submit(v::vnet::Command::OpenUdp {
                            port: TRUEOS_HID_UDP_PORT,
                        });
                    }
                    v::vnet::Event::UdpPacket {
                        handle: h, data, ..
                    } if handle == Some(h) => {
                        handle_packet(data.as_slice(), &mut seqs);
                    }
                    v::vnet::Event::UdpPacketV6 {
                        handle: h, data, ..
                    } if handle == Some(h) => {
                        handle_packet(data.as_slice(), &mut seqs);
                    }
                    v::vnet::Event::Error { msg } => {
                        crate::log!("hid-udp: net error {}\n", msg);
                        Timer::after(Duration::from_millis(500)).await;
                        if handle.is_none() {
                            let _ = vnet.submit(v::vnet::Command::OpenUdp {
                                port: TRUEOS_HID_UDP_PORT,
                            });
                        }
                    }
                    _ => {}
                }
                continue;
            }

            Timer::after(Duration::from_millis(5)).await;
        }
    }
}
