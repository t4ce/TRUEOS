//! Minimal HID boot-protocol keyboard/mouse logging using CrabUSB.
//!
//! Finds the first HID interface matching (class=0x03, subclass=0x01, proto=0x01/0x02),
//! claims the interrupt-in endpoint, and logs raw reports. No retries or hotplug logic;
//! tasks exit on failure or disconnect.

use alloc::string::String;
use crab_usb::{Device, DeviceInfo, EndpointInterruptIn, Interface, TransferError};
use embassy_executor::task;
use embassy_time::Timer;
use usb_if::{
    descriptor::EndpointType,
    host::{ControlSetup, USBError},
    transfer::{Direction, Recipient, Request, RequestType},
};

use core::sync::atomic::{AtomicBool, Ordering};

use crate::usb::xhci;

const HID_CLASS: u8 = 0x03;
const HID_BOOT_SUBCLASS: u8 = 0x01;
const PROTO_KEYBOARD: u8 = 0x01;
const PROTO_MOUSE: u8 = 0x02;

#[derive(Debug)]
pub enum HidError {
    Host(USBError),
    Transfer(TransferError),
    DeviceListFailed,
    DeviceNotFound,
    MissingEndpoint,
    ShortReport,
}

impl From<USBError> for HidError {
    fn from(err: USBError) -> Self {
        Self::Host(err)
    }
}

impl From<TransferError> for HidError {
    fn from(err: TransferError) -> Self {
        Self::Transfer(err)
    }
}

struct BootHidSession {
    _device: Device,
    iface: Interface,
    ep: EndpointInterruptIn,
}

#[task]
pub async fn keyboard_task() {
    loop {
        match run_hid(PROTO_KEYBOARD).await {
            Ok(()) => return,
            Err(err) => {
                crate::log_warn!("[hid-kbd] stopped: {:?}", err);
                Timer::after_millis(500).await;
            }
        }
    }
}

#[task]
pub async fn mouse_task() {
    loop {
        match run_hid(PROTO_MOUSE).await {
            Ok(()) => return,
            Err(HidError::DeviceNotFound) => {
                if !MOUSE_GAVE_UP.swap(true, Ordering::AcqRel) {
                    crate::log_warn!(
                        "[hid-mouse] no boot mouse interface present; stopping retries."
                    );
                }
                return;
            }
            Err(err) => {
                crate::log_warn!("[hid-mouse] stopped: {:?}", err);
                Timer::after_millis(500).await;
            }
        }
    }
}

async fn run_hid(protocol: u8) -> Result<(), HidError> {
    while !xhci::is_ready() {
        Timer::after_millis(50).await;
    }
    let mut session = open_boot_device(protocol)?;
    match protocol {
        PROTO_KEYBOARD => run_keyboard_loop(&mut session).await,
        PROTO_MOUSE => run_mouse_loop(&mut session).await,
        _ => Ok(()),
    }
}

fn open_boot_device(protocol: u8) -> Result<BootHidSession, HidError> {
    // Prime the cache once before attempting to open.
    let _ = xhci::refresh_device_cache();

    // Try cached descriptors first (fast path if we already logged the device).
    if let Some(res) = xhci::with_cached_devices(|devices| try_open_for_devices(devices, protocol))
    {
        if !matches!(res, Err(HidError::DeviceNotFound)) {
            return res;
        }
    }

    // Fall back to a fresh enumeration.
    if let Some(res) = xhci::enumerate_devices(|devices| try_open_for_devices(devices, protocol)) {
        if !matches!(res, Err(HidError::DeviceNotFound)) {
            return res;
        }
    }

    // Final attempt: use whatever is in the cache after enumeration.
    if let Some(res) = xhci::with_cached_devices(|devices| try_open_for_devices(devices, protocol))
    {
        return res;
    }

    Err(HidError::DeviceListFailed)
}

fn try_open_for_devices(
    devices: &mut [DeviceInfo],
    protocol: u8,
) -> Result<BootHidSession, HidError> {
    for info in devices.iter_mut() {
        if let Some((cfg_val, iface_num, alt, ep_addr)) = find_boot_iface(info, protocol) {
            return open_boot_iface(info, cfg_val, iface_num, alt, ep_addr);
        }
    }
    if !LOGGED_SCAN_ONCE.swap(true, Ordering::AcqRel) {
        log_hid_scan(devices, protocol);
    }
    Err(HidError::DeviceNotFound)
}

fn find_boot_iface(info: &DeviceInfo, protocol: u8) -> Option<(u8, u8, u8, u8)> {
    // Primary path: walk full configuration tree for a boot-class interrupt IN endpoint.
    for config in &info.configurations {
        for iface in &config.interfaces {
            for alt in &iface.alt_settings {
                if alt.class == HID_CLASS
                    && alt.subclass == HID_BOOT_SUBCLASS
                    && alt.protocol == protocol
                {
                    if let Some(ep) = alt.endpoints.iter().find(|ep| is_interrupt_in(ep)) {
                        return Some((
                            config.configuration_value,
                            iface.interface_number,
                            alt.alternate_setting,
                            ep.address,
                        ));
                    }
                }
            }
        }
    }

    // Fallback: if configs are incomplete, scan the flattened interface list.
    for iface in info.interface_descriptors() {
        if iface.class == HID_CLASS
            && iface.subclass == HID_BOOT_SUBCLASS
            && iface.protocol == protocol
        {
            if let Some(ep) = iface.endpoints.iter().find(|ep| is_interrupt_in(ep)) {
                // Fall back to the common single-configuration value of 1 when the flattened
                // descriptor view does not carry a configuration index.
                return Some((
                    1,
                    iface.interface_number,
                    iface.alternate_setting,
                    ep.address,
                ));
            }
        }
    }

    None
}

fn is_interrupt_in(ep: &usb_if::descriptor::EndpointDescriptor) -> bool {
    ep.transfer_type == EndpointType::Interrupt
        && (ep.direction == Direction::In || (ep.address & 0x80) != 0)
}

static LOGGED_SCAN_ONCE: AtomicBool = AtomicBool::new(false);
static MOUSE_GAVE_UP: AtomicBool = AtomicBool::new(false);

fn log_hid_scan(devices: &[DeviceInfo], protocol: u8) {
    crate::log_info!(
        "[hid] scan: {} device(s) searched for proto 0x{:02X}.",
        devices.len(),
        protocol
    );
    for (idx, info) in devices.iter().enumerate() {
        let desc = &info.descriptor;
        crate::log_info!(
            "  [{}] {:04X}:{:04X} class=0x{:02X} subclass=0x{:02X} proto=0x{:02X} cfgs={}",
            idx,
            desc.vendor_id,
            desc.product_id,
            desc.class,
            desc.subclass,
            desc.protocol,
            info.configurations.len()
        );
        for config in &info.configurations {
            crate::log_info!(
                "    cfg {} ifaces={} attrs=0x{:02X} max_power={}mA",
                config.configuration_value,
                config.interfaces.len(),
                config.attributes,
                config.max_power
            );
            for iface in &config.interfaces {
                crate::log_info!(
                    "      iface {} alt_count={} first={:02X}/{:02X}/{:02X}",
                    iface.interface_number,
                    iface.alt_settings.len(),
                    iface.first_alt_setting().class,
                    iface.first_alt_setting().subclass,
                    iface.first_alt_setting().protocol
                );
                for alt in &iface.alt_settings {
                    let mut ep_summary: String = String::new();
                    for ep in &alt.endpoints {
                        let prefix = if ep_summary.is_empty() { "" } else { "," };
                        let _ = alloc::fmt::write(
                            &mut ep_summary,
                            format_args!(
                                "{}0x{:02X} {:?} dir={:?} maxpkt={} interval={}",
                                prefix,
                                ep.address,
                                ep.transfer_type,
                                ep.direction,
                                ep.max_packet_size,
                                ep.interval
                            ),
                        );
                    }
                    crate::log_info!(
                        "        alt {} {:02X}/{:02X}/{:02X} eps=[{}]",
                        alt.alternate_setting,
                        alt.class,
                        alt.subclass,
                        alt.protocol,
                        ep_summary
                    );
                }
            }
        }
    }
}

fn open_boot_iface(
    info: &mut DeviceInfo,
    config_value: u8,
    iface_num: u8,
    alt: u8,
    ep_addr: u8,
) -> Result<BootHidSession, HidError> {
    let mut device = xhci::block_on(info.open()).map_err(HidError::from)?;
    xhci::block_on(device.set_configuration(config_value)).map_err(HidError::from)?;
    let mut iface =
        xhci::block_on(device.claim_interface(iface_num, alt)).map_err(HidError::from)?;
    let ep = iface
        .endpoint_interrupt_in(ep_addr)
        .map_err(|_| HidError::MissingEndpoint)?;
    Ok(BootHidSession {
        _device: device,
        iface,
        ep,
    })
}

async fn run_keyboard_loop(session: &mut BootHidSession) -> Result<(), HidError> {
    configure_boot_keyboard(session).await?;
    const MAX_BOOT_KBD_LEN: usize = 16;
    const MIN_BOOT_KBD_LEN: usize = 8;
    let ep_addr = session.ep.descriptor.address;
    let ep_maxpkt = session.ep.descriptor.max_packet_size;
    let ep_interval = session.ep.descriptor.interval;
    let report_len = core::cmp::max(MIN_BOOT_KBD_LEN, ep_maxpkt as usize).min(MAX_BOOT_KBD_LEN);
    let mut buf: [u8; MAX_BOOT_KBD_LEN] = [0; MAX_BOOT_KBD_LEN];
    let mut last_keys: [u8; 6] = [0; 6];
    crate::log_info!(
        "[hid-kbd] boot keyboard armed (ep=0x{:02X} maxpkt={} interval={}).",
        ep_addr,
        ep_maxpkt,
        ep_interval
    );
    let mut zero_reports: u32 = 0;
    loop {
        let transfer = match session.ep.submit(&mut buf[..report_len]) {
            Ok(fut) => fut,
            Err(err) => {
                crate::log_warn!("[hid-kbd] submit failed: {:?}", err);
                Timer::after_millis(10).await;
                continue;
            }
        };
        let len = match transfer.await {
            Ok(len) => len,
            Err(err) => {
                crate::log_warn!("[hid-kbd] transfer error: {:?}", err);
                Timer::after_millis(10).await;
                continue;
            }
        };
        if len == 0 {
            zero_reports = zero_reports.saturating_add(1);
            if zero_reports <= 5 || zero_reports % 64 == 0 {
                crate::log_warn!("[hid-kbd] zero-length report (count={}).", zero_reports);
            }
            if zero_reports % 8 == 0 {
                if let Err(err) = clear_endpoint_halt(session, ep_addr).await {
                    crate::log_warn!("[hid-kbd] clear-halt failed: {:?}", err);
                } else {
                    crate::log_info!(
                        "[hid-kbd] issued CLEAR_FEATURE(HALT) for ep 0x{:02X}.",
                        ep_addr
                    );
                }
            }
            continue;
        } else {
            zero_reports = 0;
        }
        if len < MIN_BOOT_KBD_LEN {
            crate::log_warn!("[hid-kbd] short report len={}; skipping.", len);
            continue;
        }
        let data = &buf[..len];
        let keys = extract_keys(data);
        log_key_edges(data[0], &keys, &last_keys);
        last_keys = keys;
    }
}

async fn run_mouse_loop(session: &mut BootHidSession) -> Result<(), HidError> {
    let mut buf: [u8; 4] = [0; 4];
    crate::log_info!("[hid-mouse] boot mouse armed.");
    loop {
        let transfer = session.ep.submit(&mut buf).map_err(HidError::from)?;
        let len = transfer.await.map_err(HidError::from)?;
        if len < 3 {
            crate::log_warn!("[hid-mouse] short report len={}; skipping.", len);
            continue;
        }
        let buttons = buf[0];
        let dx = buf[1] as i8;
        let dy = buf[2] as i8;
        crate::log_info!("[hid-mouse] btns=0x{:02X} dx={} dy={}", buttons, dx, dy);
    }
}

async fn configure_boot_keyboard(session: &mut BootHidSession) -> Result<(), HidError> {
    // Some keyboards remain in report protocol; nudge them into boot protocol and clear idle.
    let iface = session.iface.descriptor.interface_number as u16;
    let set_idle = ControlSetup {
        request_type: RequestType::Class,
        recipient: Recipient::Interface,
        request: Request::Other(0x0A), // SET_IDLE
        value: 0,                      // duration=0, report ID=0
        index: iface,
    };
    session
        .iface
        .control_out(set_idle, &[])
        .await
        .map_err(HidError::from)?
        .await
        .map_err(HidError::from)?;

    let set_protocol = ControlSetup {
        request_type: RequestType::Class,
        recipient: Recipient::Interface,
        request: Request::Other(0x0B), // SET_PROTOCOL
        value: 0,                      // boot protocol
        index: iface,
    };
    session
        .iface
        .control_out(set_protocol, &[])
        .await
        .map_err(HidError::from)?
        .await
        .map_err(HidError::from)?;

    Ok(())
}

fn usage_to_name(usage: u8) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    match usage {
        0x04..=0x1d => Some(LETTERS[(usage - 0x04) as usize]),
        0x1e => Some("1"),
        0x1f => Some("2"),
        0x20 => Some("3"),
        0x21 => Some("4"),
        0x22 => Some("5"),
        0x23 => Some("6"),
        0x24 => Some("7"),
        0x25 => Some("8"),
        0x26 => Some("9"),
        0x27 => Some("0"),
        0x28 => Some("Enter"),
        0x29 => Some("Esc"),
        0x2A => Some("Backspace"),
        0x2B => Some("Tab"),
        0x2C => Some("Space"),
        0x2D => Some("-"),
        0x2E => Some("="),
        0x2F => Some("["),
        0x30 => Some("]"),
        0x31 => Some("\\"),
        0x33 => Some(";"),
        0x34 => Some("'"),
        0x35 => Some("`"),
        0x36 => Some(","),
        0x37 => Some("."),
        0x38 => Some("/"),
        0x39 => Some("CapsLock"),
        _ => None,
    }
}

fn extract_keys(report: &[u8]) -> [u8; 6] {
    let mut out = [0u8; 6];
    for (idx, slot) in out.iter_mut().enumerate() {
        *slot = report.get(idx + 2).copied().unwrap_or(0);
    }
    out
}

fn log_key_edges(mods: u8, keys: &[u8; 6], last: &[u8; 6]) {
    for &code in keys.iter().filter(|c| **c != 0) {
        if !last.contains(&code) {
            log_one_key("down", mods, code);
        }
    }
    for &code in last.iter().filter(|c| **c != 0) {
        if !keys.contains(&code) {
            log_one_key("up", mods, code);
        }
    }
}

fn log_one_key(kind: &str, mods: u8, code: u8) {
    let name = usage_to_name(code).unwrap_or("?");
    crate::log_info!(
        "[hid-kbd] {:>4} 0x{:02X}({}) mods=0x{:02X}",
        kind,
        code,
        name,
        mods
    );
}

async fn clear_endpoint_halt(session: &mut BootHidSession, ep_addr: u8) -> Result<(), HidError> {
    let setup = ControlSetup {
        request_type: RequestType::Standard,
        recipient: Recipient::Endpoint,
        request: Request::ClearFeature,
        value: 0, // ENDPOINT_HALT
        index: ep_addr as u16,
    };
    session
        .iface
        .control_out(setup, &[])
        .await
        .map_err(HidError::from)?
        .await
        .map_err(HidError::from)?;
    Ok(())
}
