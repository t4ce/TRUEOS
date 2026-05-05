use alloc::{boxed::Box, string::String, vec::Vec as AllocVec};
use core::fmt::Write as _;

use crab_usb::usb_if;
use crab_usb::{Device, EndpointBulkIn, EndpointBulkOut};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;

use crate::disc::block;

use super::api::claim_interface;
use super::mass::{self, MassProbeInfo, MassTarget, UasTarget};
use super::scsi::{self, SenseKey};

const MAX_MASS_RUNTIMES: usize = crate::allcaps::storage::USB_MASS_MAX_RUNTIMES;
const MAX_ACTIVE_STREAMS: usize = crate::allcaps::storage::USB_MASS_MAX_ACTIVE_STREAMS;
const MASS_BOT_KEEPALIVE_ENABLED: bool = crate::allcaps::storage::USB_MASS_BOT_KEEPALIVE_ENABLED;
const FORCE_CONSERVATIVE_BOT: bool = crate::allcaps::storage::USB_MASS_FORCE_CONSERVATIVE_BOT;
const MASS_KEEPALIVE_MS: u64 = crate::allcaps::storage::USB_MASS_KEEPALIVE_MS;
const MASS_IO_RETRY_LIMIT: u8 = crate::allcaps::storage::USB_MASS_IO_RETRY_LIMIT;
const MASS_IO_RETRY_DELAY_MS: u64 = crate::allcaps::storage::USB_MASS_IO_RETRY_DELAY_MS;
const MASS_RUNTIME_WAIT_LIMIT: u16 = crate::allcaps::storage::USB_MASS_RUNTIME_WAIT_LIMIT;
const MASS_RUNTIME_WAIT_DELAY_MS: u64 = crate::allcaps::storage::USB_MASS_RUNTIME_WAIT_DELAY_MS;
const MIN_IO_BYTES: usize = crate::allcaps::storage::USB_MASS_MIN_IO_BYTES;
const MAX_IO_BYTES: usize = crate::allcaps::storage::USB_MASS_MAX_IO_BYTES;
const MASS_IO_GROW_SUCCESS_TARGET: u16 = crate::allcaps::storage::USB_MASS_IO_GROW_SUCCESS_TARGET;
const MASS_IO_GROW_SUCCESS_TARGET_FAST_BOT: u16 =
    crate::allcaps::storage::USB_MASS_IO_GROW_SUCCESS_TARGET_FAST_BOT;
const FAST_BOT_INITIAL_IO_BYTES: usize =
    crate::allcaps::storage::USB_MASS_FAST_BOT_INITIAL_IO_BYTES;
const FAST_BOT_WRITE_MAX_IO_BYTES: usize =
    crate::allcaps::storage::USB_MASS_FAST_BOT_WRITE_MAX_IO_BYTES;
const SKHYNIX_USE_UAS: bool = crate::allcaps::storage::USB_MASS_SKHYNIX_USE_UAS;
const USB_DT_INTERFACE: u8 = 0x04;
const USB_DT_ENDPOINT: u8 = 0x05;
const USB_DT_PIPE_USAGE: u8 = 0x24;
const USB_DT_SS_ENDPOINT_COMPANION: u8 = 0x30;
const UAS_PIPE_ID_COMMAND: u8 = 1;
const UAS_PIPE_ID_STATUS: u8 = 2;
const UAS_PIPE_ID_DATA_IN: u8 = 3;
const UAS_PIPE_ID_DATA_OUT: u8 = 4;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveMassStream {
    controller_id: u32,
    slot_id: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum MassIoProfile {
    ConservativeBot,
    FastBot,
    UasSkhynix,
}

enum UsbMassEndpoints {
    Bot {
        bulk_in: EndpointBulkIn,
        bulk_out: EndpointBulkOut,
    },
    UasSkhynix {
        command_out: EndpointBulkOut,
        status_in: EndpointBulkIn,
        data_in: EndpointBulkIn,
        data_out: EndpointBulkOut,
    },
}

struct UsbMassRuntime {
    controller_id: u32,
    slot_id: u32,
    runtime_key: u64,
    vendor_id: u16,
    product_id: u16,
    device: Device,
    interface_number: u8,
    bulk_in_ep: u8,
    bulk_out_ep: u8,
    endpoints: UsbMassEndpoints,
    transport_kind: mass::MassTransportKind,
    io_profile: MassIoProfile,
    port_speed: usb_if::Speed,
    uas_candidate_count: u8,
    io_tag: u32,
    sync_cache_unsupported: bool,
    current_max_io_bytes: usize,
    io_success_streak: u16,
}

unsafe impl Send for UsbMassRuntime {}

static ACTIVE_MASS_STREAMS: Mutex<Vec<ActiveMassStream, MAX_ACTIVE_STREAMS>> =
    Mutex::new(Vec::new());
static MASS_RUNTIMES: Mutex<Vec<UsbMassRuntime, MAX_MASS_RUNTIMES>> = Mutex::new(Vec::new());

#[derive(Copy, Clone)]
struct RegisteredMassDisk {
    runtime_key: u64,
    handle: block::DeviceHandle,
}

static REGISTERED_MASS_DISKS: Mutex<Vec<RegisteredMassDisk, MAX_MASS_RUNTIMES>> =
    Mutex::new(Vec::new());

fn register_active_mass_stream(stream: ActiveMassStream) -> bool {
    let mut streams = ACTIVE_MASS_STREAMS.lock();
    if streams.iter().any(|active| *active == stream) {
        return false;
    }
    streams.push(stream).is_ok()
}

fn unregister_active_mass_stream(stream: ActiveMassStream) {
    let mut streams = ACTIVE_MASS_STREAMS.lock();
    if let Some(idx) = streams.iter().position(|active| *active == stream) {
        streams.remove(idx);
    }
}

fn register_runtime(rt: UsbMassRuntime) {
    let mut runtimes = MASS_RUNTIMES.lock();
    if let Some(existing) = runtimes
        .iter_mut()
        .find(|existing| existing.runtime_key == rt.runtime_key)
    {
        *existing = rt;
        return;
    }
    let _ = runtimes.push(rt);
}

fn take_runtime(runtime_key: u64) -> Option<UsbMassRuntime> {
    let mut runtimes = MASS_RUNTIMES.lock();
    let idx = runtimes
        .iter()
        .position(|rt| rt.runtime_key == runtime_key)?;
    Some(runtimes.remove(idx))
}

async fn take_runtime_wait(runtime_key: u64) -> Option<UsbMassRuntime> {
    for _ in 0..=MASS_RUNTIME_WAIT_LIMIT {
        if let Some(rt) = take_runtime(runtime_key) {
            return Some(rt);
        }
        Timer::after(EmbassyDuration::from_millis(MASS_RUNTIME_WAIT_DELAY_MS)).await;
    }

    crate::log!(
        "crabusb: mass runtime wait timeout key=0x{:016X} waited_ms={}\n",
        runtime_key,
        (MASS_RUNTIME_WAIT_LIMIT as u64 + 1) * MASS_RUNTIME_WAIT_DELAY_MS
    );
    None
}

fn registered_disk(runtime_key: u64) -> Option<block::DeviceHandle> {
    let disks = REGISTERED_MASS_DISKS.lock();
    disks
        .iter()
        .find(|known| known.runtime_key == runtime_key)
        .map(|known| known.handle)
}

fn remember_registered_disk(runtime_key: u64, handle: block::DeviceHandle) {
    let mut disks = REGISTERED_MASS_DISKS.lock();
    if let Some(existing) = disks
        .iter_mut()
        .find(|existing| existing.runtime_key == runtime_key)
    {
        existing.handle = handle;
        return;
    }
    let _ = disks.push(RegisteredMassDisk {
        runtime_key,
        handle,
    });
}

#[derive(Clone)]
struct MassIdentity {
    runtime_key: u64,
    serial: Option<String>,
    key_kind: &'static str,
}

#[derive(Copy, Clone)]
struct UasEndpointRole {
    address: u8,
    max_packet_size: u16,
}

#[derive(Copy, Clone, Default)]
struct UasPipeRoles {
    command: Option<UasEndpointRole>,
    status: Option<UasEndpointRole>,
    data_in: Option<UasEndpointRole>,
    data_out: Option<UasEndpointRole>,
}

fn hex_descriptor(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (idx, byte) in bytes.iter().enumerate() {
        if idx != 0 {
            out.push(' ');
        }
        let _ = write!(out, "{:02X}", byte);
    }
    out
}

fn endpoint_role_text(ep: Option<UasEndpointRole>) -> String {
    match ep {
        Some(ep) => alloc::format!("0x{:02X}/{}", ep.address, ep.max_packet_size),
        None => String::from("none"),
    }
}

fn uas_pipe_label(pipe_id: u8) -> &'static str {
    match pipe_id {
        UAS_PIPE_ID_COMMAND => "command",
        UAS_PIPE_ID_STATUS => "status",
        UAS_PIPE_ID_DATA_IN => "data-in",
        UAS_PIPE_ID_DATA_OUT => "data-out",
        _ => "unknown",
    }
}

fn assign_uas_pipe_role(roles: &mut UasPipeRoles, pipe_id: u8, ep: UasEndpointRole) {
    match pipe_id {
        UAS_PIPE_ID_COMMAND => roles.command = Some(ep),
        UAS_PIPE_ID_STATUS => roles.status = Some(ep),
        UAS_PIPE_ID_DATA_IN => roles.data_in = Some(ep),
        UAS_PIPE_ID_DATA_OUT => roles.data_out = Some(ep),
        _ => {}
    }
}

fn uas_target_from_roles(target: UasTarget, roles: UasPipeRoles) -> Option<UasTarget> {
    let command = roles.command?;
    let status = roles.status?;
    let data_in = roles.data_in?;
    let data_out = roles.data_out?;

    Some(UasTarget {
        configuration_value: target.configuration_value,
        interface_number: target.interface_number,
        alternate_setting: target.alternate_setting,
        command_out: command.address,
        status_in: status.address,
        data_in: data_in.address,
        data_out: data_out.address,
        command_out_max_packet_size: command.max_packet_size,
        status_in_max_packet_size: status.max_packet_size,
        data_in_max_packet_size: data_in.max_packet_size,
        data_out_max_packet_size: data_out.max_packet_size,
    })
}

async fn read_raw_config_descriptor(
    device: &mut Device,
    configuration_value: u8,
    vendor_id: u16,
    product_id: u16,
) -> Option<AllocVec<u8>> {
    let config_index = device
        .configurations()
        .iter()
        .position(|cfg| cfg.configuration_value == configuration_value)?
        as u8;

    let setup = usb_if::host::ControlSetup {
        request_type: usb_if::transfer::RequestType::Standard,
        recipient: usb_if::transfer::Recipient::Device,
        request: usb_if::transfer::Request::GetDescriptor,
        value: (u16::from(usb_if::descriptor::DescriptorType::CONFIGURATION.0) << 8)
            | u16::from(config_index),
        index: 0,
    };

    let mut header = alloc::vec![0u8; 9];
    let Ok(got) = device.control_in(setup.clone(), &mut header).await else {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc raw-config header failed cfg={}\n",
            vendor_id,
            product_id,
            configuration_value
        );
        return None;
    };
    if got < header.len() {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc raw-config header short got={} need={}\n",
            vendor_id,
            product_id,
            got,
            header.len()
        );
        return None;
    }

    let total_len = u16::from_le_bytes([header[2], header[3]]) as usize;
    if !(header.len()..=4096).contains(&total_len) {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc raw-config invalid total_len={}\n",
            vendor_id,
            product_id,
            total_len
        );
        return None;
    }

    let mut raw = alloc::vec![0u8; total_len];
    let Ok(got) = device.control_in(setup, raw.as_mut_slice()).await else {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc raw-config read failed cfg={} total_len={}\n",
            vendor_id,
            product_id,
            configuration_value,
            total_len
        );
        return None;
    };
    raw.truncate(got.min(total_len));
    if raw.len() < header.len() {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc raw-config short got={} need={}\n",
            vendor_id,
            product_id,
            raw.len(),
            header.len()
        );
        return None;
    }

    Some(raw)
}

fn parse_and_log_uas_descriptors(
    raw: &[u8],
    target: UasTarget,
    vendor_id: u16,
    product_id: u16,
) -> Option<UasTarget> {
    let verbose = crate::logflag::USB_MASS_UAS_ADVANCED_PROBE_LOGS;
    let mut roles = UasPipeRoles::default();
    let mut active = false;
    let mut current_ep = None;
    let mut offset = 0usize;

    while offset + 2 <= raw.len() {
        let len = raw[offset] as usize;
        let desc_type = raw[offset + 1];
        if len < 2 || offset + len > raw.len() {
            if verbose {
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} uas-desc malformed off={} len={} remaining={}\n",
                    vendor_id,
                    product_id,
                    offset,
                    len,
                    raw.len().saturating_sub(offset)
                );
            }
            break;
        }

        let desc = &raw[offset..offset + len];
        match desc_type {
            USB_DT_INTERFACE if len >= 9 => {
                let interface_number = desc[2];
                let alternate_setting = desc[3];
                active = interface_number == target.interface_number
                    && alternate_setting == target.alternate_setting;
                current_ep = None;
                if active && verbose {
                    crate::log!(
                        "crabusb: mass {:04X}:{:04X} uas-desc interface off={} if#{} alt={} eps={} class={:02X}/{:02X}/{:02X} bytes=[{}]\n",
                        vendor_id,
                        product_id,
                        offset,
                        interface_number,
                        alternate_setting,
                        desc[4],
                        desc[5],
                        desc[6],
                        desc[7],
                        hex_descriptor(desc)
                    );
                }
            }
            USB_DT_ENDPOINT if active && len >= 7 => {
                let address = desc[2];
                let attributes = desc[3];
                let max_packet_size = u16::from_le_bytes([desc[4], desc[5]]) & 0x07FF;
                current_ep = Some(UasEndpointRole {
                    address,
                    max_packet_size,
                });
                if verbose {
                    crate::log!(
                        "crabusb: mass {:04X}:{:04X} uas-desc endpoint off={} ep=0x{:02X} attr=0x{:02X} mps={} intv={} bytes=[{}]\n",
                        vendor_id,
                        product_id,
                        offset,
                        address,
                        attributes,
                        max_packet_size,
                        desc[6],
                        hex_descriptor(desc)
                    );
                }
            }
            USB_DT_SS_ENDPOINT_COMPANION if active => {
                if verbose {
                    let max_burst = desc.get(2).copied().unwrap_or(0);
                    let attrs = desc.get(3).copied().unwrap_or(0);
                    let bytes_per_interval = if desc.len() >= 6 {
                        u16::from_le_bytes([desc[4], desc[5]])
                    } else {
                        0
                    };
                    crate::log!(
                        "crabusb: mass {:04X}:{:04X} uas-desc ss-companion off={} ep={} max_burst={} attr=0x{:02X} bytes_per_interval={} bytes=[{}]\n",
                        vendor_id,
                        product_id,
                        offset,
                        endpoint_role_text(current_ep),
                        max_burst,
                        attrs,
                        bytes_per_interval,
                        hex_descriptor(desc)
                    );
                }
            }
            USB_DT_PIPE_USAGE if active => {
                let pipe_id = desc.get(2).copied().unwrap_or(0);
                if let Some(ep) = current_ep {
                    assign_uas_pipe_role(&mut roles, pipe_id, ep);
                }
                if verbose {
                    crate::log!(
                        "crabusb: mass {:04X}:{:04X} uas-desc pipe-usage off={} ep={} pipe_id={} role={} bytes=[{}]\n",
                        vendor_id,
                        product_id,
                        offset,
                        endpoint_role_text(current_ep),
                        pipe_id,
                        uas_pipe_label(pipe_id),
                        hex_descriptor(desc)
                    );
                }
            }
            _ if active && verbose => {
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} uas-desc extra off={} type=0x{:02X} len={} ep={} bytes=[{}]\n",
                    vendor_id,
                    product_id,
                    offset,
                    desc_type,
                    len,
                    endpoint_role_text(current_ep),
                    hex_descriptor(desc)
                );
            }
            _ => {}
        }

        offset += len;
    }

    if verbose {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc roles command={} status={} data_in={} data_out={}\n",
            vendor_id,
            product_id,
            endpoint_role_text(roles.command),
            endpoint_role_text(roles.status),
            endpoint_role_text(roles.data_in),
            endpoint_role_text(roles.data_out)
        );
    }

    uas_target_from_roles(target, roles)
}

async fn refine_uas_target_from_raw_descriptors(
    device: &mut Device,
    target: UasTarget,
    vendor_id: u16,
    product_id: u16,
) -> UasTarget {
    let Some(raw) =
        read_raw_config_descriptor(device, target.configuration_value, vendor_id, product_id).await
    else {
        return target;
    };

    let Some(refined) = parse_and_log_uas_descriptors(&raw, target, vendor_id, product_id) else {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc pipe roles incomplete; keeping descriptor-order target cmd=0x{:02X} status=0x{:02X} data_in=0x{:02X} data_out=0x{:02X}\n",
            vendor_id,
            product_id,
            target.command_out,
            target.status_in,
            target.data_in,
            target.data_out
        );
        return target;
    };

    if refined.command_out != target.command_out
        || refined.status_in != target.status_in
        || refined.data_in != target.data_in
        || refined.data_out != target.data_out
    {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc pipe roles refine cmd 0x{:02X}->0x{:02X} status 0x{:02X}->0x{:02X} data_in 0x{:02X}->0x{:02X} data_out 0x{:02X}->0x{:02X}\n",
            vendor_id,
            product_id,
            target.command_out,
            refined.command_out,
            target.status_in,
            refined.status_in,
            target.data_in,
            refined.data_in,
            target.data_out,
            refined.data_out
        );
    }

    refined
}

fn hash_mix(mut state: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        state ^= u64::from(b);
        state = state.wrapping_mul(0x1000_0000_01B3);
    }
    state
}

fn hash_mix_u16(state: u64, value: u16) -> u64 {
    hash_mix(state, &value.to_le_bytes())
}

fn hash_mix_u32(state: u64, value: u32) -> u64 {
    hash_mix(state, &value.to_le_bytes())
}

async fn build_mass_identity(
    device: &mut Device,
    controller_id: u32,
    slot_id: u32,
) -> MassIdentity {
    let (vendor_id, product_id, serial_index) = {
        let desc = device.descriptor();
        (desc.vendor_id, desc.product_id, desc.serial_number_string_index)
    };
    let serial = super::descriptor::read_optional_string_descriptor(device, serial_index).await;

    if let Some(serial) = serial {
        let mut key = 0xcbf2_9ce4_8422_2325u64;
        key = hash_mix(key, b"usb-mass-serial");
        key = hash_mix_u16(key, vendor_id);
        key = hash_mix_u16(key, product_id);
        key = hash_mix(key, serial.as_bytes());
        return MassIdentity {
            runtime_key: key,
            serial: Some(serial),
            key_kind: "serial",
        };
    }

    let mut key = 0xcbf2_9ce4_8422_2325u64;
    key = hash_mix(key, b"usb-mass-slot");
    key = hash_mix_u32(key, controller_id);
    key = hash_mix_u32(key, slot_id);
    MassIdentity {
        runtime_key: key,
        serial: None,
        key_kind: "slot",
    }
}

#[inline]
fn sense_is_transient(key: SenseKey) -> bool {
    matches!(key, SenseKey::NotReady | SenseKey::UnitAttention | SenseKey::AbortedCommand)
}

fn map_io_error(err: mass::MassProbeError) -> block::Error {
    match err {
        mass::MassProbeError::Transport(_) => block::Error::Io,
        mass::MassProbeError::ShortData { .. } => block::Error::Corrupted,
        mass::MassProbeError::Csw { .. } => block::Error::Io,
    }
}

fn clamp_mass_io_bytes(block_size: usize, bytes: usize) -> usize {
    let bs = block_size.max(1);
    let floor = core::cmp::max(bs, MIN_IO_BYTES.div_ceil(bs) * bs);
    let ceil = core::cmp::max(floor, (MAX_IO_BYTES / bs).max(1) * bs);
    let rounded = core::cmp::max(bs, (bytes / bs).max(1) * bs);
    rounded.clamp(floor, ceil)
}

fn mass_transport_label(kind: mass::MassTransportKind) -> &'static str {
    match kind {
        mass::MassTransportKind::Bot => "bot",
        mass::MassTransportKind::Uas => "uas",
    }
}

fn mass_io_profile_label(profile: MassIoProfile) -> &'static str {
    match profile {
        MassIoProfile::ConservativeBot => "bot-safe",
        MassIoProfile::FastBot => "bot-fast",
        MassIoProfile::UasSkhynix => "uas-skhynix",
    }
}

fn is_skhynix_pssd_x31(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == 0x152E && product_id == 0x7001
}

fn is_superspeed_bulk_bot(port_speed: usb_if::Speed, target: &MassTarget) -> bool {
    matches!(port_speed, usb_if::Speed::SuperSpeed | usb_if::Speed::SuperSpeedPlus)
        && target.bulk_in_max_packet_size >= 1024
        && target.bulk_out_max_packet_size >= 1024
}

fn choose_mass_io_profile(
    transport_kind: mass::MassTransportKind,
    vendor_id: u16,
    product_id: u16,
    port_speed: usb_if::Speed,
    target: &MassTarget,
) -> MassIoProfile {
    let fast_bot_capable = transport_kind == mass::MassTransportKind::Bot
        && is_superspeed_bulk_bot(port_speed, target);

    if fast_bot_capable && is_skhynix_pssd_x31(vendor_id, product_id) {
        return MassIoProfile::FastBot;
    }

    if transport_kind == mass::MassTransportKind::Bot && FORCE_CONSERVATIVE_BOT {
        return MassIoProfile::ConservativeBot;
    }

    if fast_bot_capable {
        MassIoProfile::FastBot
    } else {
        MassIoProfile::ConservativeBot
    }
}

fn initial_mass_io_bytes(
    profile: MassIoProfile,
    port_speed: usb_if::Speed,
    target: &MassTarget,
    block_size: usize,
) -> usize {
    let wanted = match profile {
        MassIoProfile::ConservativeBot => MIN_IO_BYTES,
        MassIoProfile::UasSkhynix => MAX_IO_BYTES,
        MassIoProfile::FastBot => {
            if matches!(port_speed, usb_if::Speed::SuperSpeedPlus)
                || (target.bulk_in_max_packet_size >= 1024
                    && target.bulk_out_max_packet_size >= 1024)
            {
                MAX_IO_BYTES
            } else {
                FAST_BOT_INITIAL_IO_BYTES
            }
        }
    };
    clamp_mass_io_bytes(block_size, wanted)
}

fn mass_io_grow_success_target(rt: &UsbMassRuntime) -> u16 {
    match rt.io_profile {
        MassIoProfile::ConservativeBot => MASS_IO_GROW_SUCCESS_TARGET,
        MassIoProfile::FastBot => MASS_IO_GROW_SUCCESS_TARGET_FAST_BOT,
        MassIoProfile::UasSkhynix => MASS_IO_GROW_SUCCESS_TARGET_FAST_BOT,
    }
}

fn current_mass_io_bytes(rt: &UsbMassRuntime, block_size: usize) -> usize {
    clamp_mass_io_bytes(block_size, rt.current_max_io_bytes)
}

fn current_mass_write_io_bytes(rt: &UsbMassRuntime, block_size: usize) -> usize {
    let cur = current_mass_io_bytes(rt, block_size);
    match rt.io_profile {
        MassIoProfile::ConservativeBot => cur,
        MassIoProfile::UasSkhynix => cur,
        MassIoProfile::FastBot => {
            core::cmp::min(cur, clamp_mass_io_bytes(block_size, FAST_BOT_WRITE_MAX_IO_BYTES))
        }
    }
}

fn mass_io_note_success(rt: &mut UsbMassRuntime, block_size: usize) {
    rt.io_success_streak = rt.io_success_streak.saturating_add(1);
    let cur = current_mass_io_bytes(rt, block_size);
    if rt.io_success_streak >= mass_io_grow_success_target(rt) && cur < MAX_IO_BYTES {
        rt.current_max_io_bytes = clamp_mass_io_bytes(block_size, cur.saturating_mul(2));
        rt.io_success_streak = 0;
    }
}

fn mass_io_backoff(rt: &mut UsbMassRuntime, block_size: usize) {
    let cur = current_mass_io_bytes(rt, block_size);
    rt.current_max_io_bytes = clamp_mass_io_bytes(block_size, core::cmp::max(block_size, cur / 2));
    rt.io_success_streak = 0;
}

fn log_transport_mismatch(rt: &UsbMassRuntime, stage: &'static str) {
    let submit = crab_usb::debug_last_submit();
    let event = crab_usb::debug_last_event();
    crate::log!(
        "crabusb: mass {:04X}:{:04X} transport stage={} key=0x{:X} expect[ctrl={} slot={} out_ep=0x{:02X} in_ep=0x{:02X}] last_submit[slot={} dci={} dir={} stream={} len={} ptr=0x{:X} ring=0x{:X}] last_event[slot={} ep={} cc={} residual={} ptr=0x{:X}]\n",
        rt.vendor_id,
        rt.product_id,
        stage,
        rt.runtime_key,
        rt.controller_id,
        rt.slot_id,
        rt.bulk_out_ep,
        rt.bulk_in_ep,
        submit.slot_id,
        submit.dci,
        submit.direction,
        submit.stream_id,
        submit.len,
        submit.ptr,
        submit.ring_ptr,
        event.slot_id,
        event.ep_id,
        event.completion_code,
        event.residual,
        event.ptr
    );
}

async fn recover_runtime_transport(
    rt: &mut UsbMassRuntime,
    stage: &'static str,
    err: mass::MassProbeError,
) -> Result<(), mass::MassProbeError> {
    if let Some(reason) = err.transport_reason() {
        log_transport_mismatch(rt, stage);
        crate::log!(
            "crabusb: mass {:04X}:{:04X} recovery stage={} reason={} if#{} out_ep=0x{:02X} in_ep=0x{:02X}\n",
            rt.vendor_id,
            rt.product_id,
            stage,
            reason,
            rt.interface_number,
            rt.bulk_out_ep,
            rt.bulk_in_ep
        );
        mass::bot_recovery(&mut rt.device, rt.interface_number, rt.bulk_out_ep, rt.bulk_in_ep)
            .await?;
    }
    Ok(())
}

struct UsbMassBlockDevice {
    runtime_key: u64,
    block_size: u32,
    block_count: u64,
}

impl UsbMassBlockDevice {
    async fn with_runtime<R>(
        &self,
        f: impl for<'a> FnOnce(
            &'a mut UsbMassRuntime,
        ) -> core::pin::Pin<
            Box<dyn core::future::Future<Output = block::Result<R>> + 'a>,
        >,
    ) -> block::Result<R> {
        let mut rt = take_runtime_wait(self.runtime_key)
            .await
            .ok_or(block::Error::NotReady)?;
        let result = f(&mut rt).await;
        register_runtime(rt);
        result
    }
}

impl block::BlockDevice for UsbMassBlockDevice {
    fn block_size_bytes(&self) -> u32 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn read_blocks<'a>(
        &'a mut self,
        lba: u64,
        blocks: usize,
    ) -> block::BoxFuture<'a, block::Result<AllocVec<u8>>> {
        Box::pin(async move {
            let block_size = self.block_size as usize;
            if block_size == 0 {
                return Err(block::Error::InvalidParam);
            }
            if blocks == 0 {
                return Ok(AllocVec::new());
            }
            let blocks_u64 = blocks as u64;
            let end = lba
                .checked_add(blocks_u64)
                .ok_or(block::Error::OutOfBounds)?;
            if end > self.block_count {
                return Err(block::Error::OutOfBounds);
            }

            let total_bytes = blocks
                .checked_mul(block_size)
                .ok_or(block::Error::InvalidParam)?;
            let mut out = alloc::vec![0u8; total_bytes];

            self.with_runtime(|rt| {
                Box::pin(async move {
                    let mut cur_lba = lba;
                    let mut remaining = out.as_mut_slice();
                    while !remaining.is_empty() {
                        let max_blocks = (current_mass_io_bytes(rt, block_size) / block_size).max(1);
                        let blocks_here = core::cmp::min(max_blocks, remaining.len() / block_size);
                        let bytes_here = blocks_here * block_size;

                        let mut attempts = 0u8;
                        loop {
                            let bulk_out_ep = rt.bulk_out_ep;
                            let bulk_in_ep = rt.bulk_in_ep;
                            let result = match &mut rt.endpoints {
                                UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                                    mass::read_blocks_bot(
                                        bulk_out,
                                        bulk_in,
                                        bulk_out_ep,
                                        bulk_in_ep,
                                        cur_lba as u32,
                                        blocks_here as u16,
                                        &mut remaining[..bytes_here],
                                        rt.io_tag,
                                    )
                                    .await
                                }
                                UsbMassEndpoints::UasSkhynix {
                                    command_out,
                                    status_in,
                                    data_in,
                                    ..
                                } => {
                                    mass::read_blocks_uas_skhynix(
                                        command_out,
                                        status_in,
                                        data_in,
                                        cur_lba as u32,
                                        blocks_here as u16,
                                        &mut remaining[..bytes_here],
                                        rt.io_tag,
                                    )
                                    .await
                                }
                            };
                            rt.io_tag = rt.io_tag.wrapping_add(1);

                            match result {
                                Ok(()) => {
                                    mass_io_note_success(rt, block_size);
                                    break;
                                }
                                Err(err) => {
                                    mass_io_backoff(rt, block_size);
                                    let recovered = if matches!(rt.endpoints, UsbMassEndpoints::Bot { .. }) {
                                        recover_runtime_transport(rt, "read-10", err).await.is_ok()
                                    } else {
                                        false
                                    };
                                    if recovered && attempts < MASS_IO_RETRY_LIMIT
                                    {
                                        attempts = attempts.wrapping_add(1);
                                        Timer::after(EmbassyDuration::from_millis(MASS_IO_RETRY_DELAY_MS)).await;
                                        continue;
                                    }
                                    let sense = match &mut rt.endpoints {
                                        UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                                            mass::request_sense_fixed(
                                                bulk_out,
                                                bulk_in,
                                                bulk_out_ep,
                                                bulk_in_ep,
                                                rt.io_tag,
                                            )
                                            .await
                                        }
                                        UsbMassEndpoints::UasSkhynix {
                                            command_out,
                                            status_in,
                                            data_in,
                                            ..
                                        } => {
                                            mass::request_sense_fixed_uas_skhynix(
                                                command_out,
                                                status_in,
                                                data_in,
                                                rt.io_tag,
                                            )
                                            .await
                                        }
                                    };
                                    rt.io_tag = rt.io_tag.wrapping_add(1);
                                    if let Some(sense) = sense
                                        && sense_is_transient(sense.sense_key)
                                        && attempts < MASS_IO_RETRY_LIMIT
                                    {
                                        attempts = attempts.wrapping_add(1);
                                        Timer::after(EmbassyDuration::from_millis(MASS_IO_RETRY_DELAY_MS)).await;
                                        continue;
                                    }
                                    crate::log!(
                                        "crabusb: mass {:04X}:{:04X} read lba={} blocks={} err={:?} sense={:?}\n",
                                        rt.vendor_id,
                                        rt.product_id,
                                        cur_lba,
                                        blocks_here,
                                        err,
                                        sense.map(|s| (s.sense_key, s.asc, s.ascq))
                                    );
                                    return Err(map_io_error(err));
                                }
                            }
                        }

                        remaining = &mut remaining[bytes_here..];
                        cur_lba = cur_lba.saturating_add(blocks_here as u64);
                    }

                    Ok(core::mem::take(&mut out))
                })
            })
            .await
        })
    }

    fn write_blocks<'a>(
        &'a mut self,
        lba: u64,
        buf: &'a [u8],
    ) -> block::BoxFuture<'a, block::Result<()>> {
        Box::pin(async move {
            let block_size = self.block_size as usize;
            if block_size == 0 || !buf.len().is_multiple_of(block_size) {
                return Err(block::Error::InvalidParam);
            }
            if buf.is_empty() {
                return Ok(());
            }
            let blocks_total = (buf.len() / block_size) as u64;
            let end = lba
                .checked_add(blocks_total)
                .ok_or(block::Error::OutOfBounds)?;
            if end > self.block_count {
                return Err(block::Error::OutOfBounds);
            }

            let mut rt = take_runtime_wait(self.runtime_key)
                .await
                .ok_or(block::Error::NotReady)?;
            let result = async {
                let mut cur_lba = lba;
                let mut remaining = buf;
                while !remaining.is_empty() {
                    let max_blocks =
                        (current_mass_write_io_bytes(&rt, block_size) / block_size).max(1);
                    let blocks_here = core::cmp::min(max_blocks, remaining.len() / block_size);
                    let bytes_here = blocks_here * block_size;

                    let mut attempts = 0u8;
                    loop {
                        let bulk_out_ep = rt.bulk_out_ep;
                        let bulk_in_ep = rt.bulk_in_ep;
                        let result = match &mut rt.endpoints {
                            UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                                mass::write_blocks_bot(
                                    bulk_out,
                                    bulk_in,
                                    bulk_out_ep,
                                    bulk_in_ep,
                                    cur_lba as u32,
                                    blocks_here as u16,
                                    &remaining[..bytes_here],
                                    rt.io_tag,
                                )
                                .await
                            }
                            UsbMassEndpoints::UasSkhynix {
                                command_out,
                                status_in,
                                data_out,
                                ..
                            } => {
                                mass::write_blocks_uas_skhynix(
                                    command_out,
                                    status_in,
                                    data_out,
                                    cur_lba as u32,
                                    blocks_here as u16,
                                    &remaining[..bytes_here],
                                    rt.io_tag,
                                )
                                .await
                            }
                        };
                        rt.io_tag = rt.io_tag.wrapping_add(1);

                        match result {
                            Ok(()) => {
                                mass_io_note_success(&mut rt, block_size);
                                break;
                            }
                            Err(err) => {
                                mass_io_backoff(&mut rt, block_size);
                                let recovered = if matches!(rt.endpoints, UsbMassEndpoints::Bot { .. }) {
                                    recover_runtime_transport(&mut rt, "write-10", err).await.is_ok()
                                } else {
                                    false
                                };
                                if recovered && attempts < MASS_IO_RETRY_LIMIT
                                {
                                    attempts = attempts.wrapping_add(1);
                                    Timer::after(EmbassyDuration::from_millis(MASS_IO_RETRY_DELAY_MS)).await;
                                    continue;
                                }
                                let sense = match &mut rt.endpoints {
                                    UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                                        mass::request_sense_fixed(
                                            bulk_out,
                                            bulk_in,
                                            bulk_out_ep,
                                            bulk_in_ep,
                                            rt.io_tag,
                                        )
                                        .await
                                    }
                                    UsbMassEndpoints::UasSkhynix {
                                        command_out,
                                        status_in,
                                        data_in,
                                        ..
                                    } => {
                                        mass::request_sense_fixed_uas_skhynix(
                                            command_out,
                                            status_in,
                                            data_in,
                                            rt.io_tag,
                                        )
                                        .await
                                    }
                                };
                                rt.io_tag = rt.io_tag.wrapping_add(1);
                                if let Some(sense) = sense
                                    && sense_is_transient(sense.sense_key)
                                    && attempts < MASS_IO_RETRY_LIMIT
                                {
                                    attempts = attempts.wrapping_add(1);
                                    Timer::after(EmbassyDuration::from_millis(MASS_IO_RETRY_DELAY_MS)).await;
                                    continue;
                                }
                                crate::log!(
                                    "crabusb: mass {:04X}:{:04X} write lba={} blocks={} err={:?} sense={:?}\n",
                                    rt.vendor_id,
                                    rt.product_id,
                                    cur_lba,
                                    blocks_here,
                                    err,
                                    sense.map(|s| (s.sense_key, s.asc, s.ascq))
                                );
                                return Err(map_io_error(err));
                            }
                        }
                    }

                    remaining = &remaining[bytes_here..];
                    cur_lba = cur_lba.saturating_add(blocks_here as u64);
                }
                Ok(())
            }
            .await;
            register_runtime(rt);
            result
        })
    }

    fn dma_alignment_bytes(&self) -> u32 {
        1
    }

    fn max_transfer_bytes(&self) -> u64 {
        MAX_IO_BYTES as u64
    }

    fn supports_write(&self) -> bool {
        true
    }

    fn flush<'a>(&'a mut self) -> block::BoxFuture<'a, block::Result<()>> {
        Box::pin(async move {
            self.with_runtime(|rt| {
                Box::pin(async move {
                    if rt.sync_cache_unsupported {
                        return Ok(());
                    }

                    let bulk_out_ep = rt.bulk_out_ep;
                    let bulk_in_ep = rt.bulk_in_ep;
                    let sync_result = match &mut rt.endpoints {
                        UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                            mass::synchronize_cache_bot(
                                bulk_out,
                                bulk_in,
                                bulk_out_ep,
                                bulk_in_ep,
                                rt.io_tag,
                            )
                            .await
                        }
                        UsbMassEndpoints::UasSkhynix {
                            command_out,
                            status_in,
                            ..
                        } => {
                            mass::synchronize_cache_uas_skhynix(
                                command_out,
                                status_in,
                                rt.io_tag,
                            )
                            .await
                        }
                    };
                    match sync_result {
                        Ok(()) => {
                            rt.io_tag = rt.io_tag.wrapping_add(1);
                            Ok(())
                        }
                        Err(err) => {
                            if matches!(rt.endpoints, UsbMassEndpoints::Bot { .. }) {
                                let _ = recover_runtime_transport(rt, "sync-cache-10", err).await;
                            }
                            rt.io_tag = rt.io_tag.wrapping_add(1);
                            let sense = match &mut rt.endpoints {
                                UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                                    mass::request_sense_fixed(
                                        bulk_out,
                                        bulk_in,
                                        bulk_out_ep,
                                        bulk_in_ep,
                                        rt.io_tag,
                                    )
                                    .await
                                }
                                UsbMassEndpoints::UasSkhynix {
                                    command_out,
                                    status_in,
                                    data_in,
                                    ..
                                } => {
                                    mass::request_sense_fixed_uas_skhynix(
                                        command_out,
                                        status_in,
                                        data_in,
                                        rt.io_tag,
                                    )
                                    .await
                                }
                            };
                            rt.io_tag = rt.io_tag.wrapping_add(1);
                            if let Some(sense) = sense
                                && sense.sense_key == scsi::SenseKey::IllegalRequest
                            {
                                crate::log!(
                                    "crabusb: mass {:04X}:{:04X} sync-cache unsupported asc={:#x} ascq={:#x}\n",
                                    rt.vendor_id,
                                    rt.product_id,
                                    sense.asc,
                                    sense.ascq
                                );
                                rt.sync_cache_unsupported = true;
                                return Ok(());
                            }
                            crate::log!(
                                "crabusb: mass {:04X}:{:04X} flush err={:?} sense={:?}\n",
                                rt.vendor_id,
                                rt.product_id,
                                err,
                                sense.map(|s| (s.sense_key, s.asc, s.ascq))
                            );
                            Err(map_io_error(err))
                        }
                    }
                })
            })
            .await
        })
    }
}

fn register_block_device(
    identity: &MassIdentity,
    vendor_id: u16,
    product_id: u16,
    probe: &MassProbeInfo,
) -> block::DeviceHandle {
    if let Some(handle) = registered_disk(identity.runtime_key) {
        return handle;
    }

    let label = alloc::format!("usbms-{:04X}:{:04X}", vendor_id, product_id);
    let mut desc = block::DeviceDescriptor::new(block::DeviceKind::Unknown).with_label(label);
    if let Some(serial) = identity.serial.as_deref() {
        desc = desc.with_serial(serial);
    }

    let handle = block::register_device(
        desc,
        UsbMassBlockDevice {
            runtime_key: identity.runtime_key,
            block_size: probe.block_size.max(1),
            block_count: probe.block_count.max(1),
        },
    );
    remember_registered_disk(identity.runtime_key, handle);
    handle
}

#[embassy_executor::task(pool_size = 8)]
pub async fn mass_storage_task(
    mut device: Device,
    controller_id: u32,
    target: MassTarget,
    transport_kind: mass::MassTransportKind,
    io_profile: MassIoProfile,
    port_speed: usb_if::Speed,
    uas_candidate_count: u8,
) {
    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let slot = device.slot_id();
    let active_stream = ActiveMassStream {
        controller_id,
        slot_id: u32::from(slot),
    };

    if let Err(err) = device
        .ep_ctrl()
        .set_configuration(target.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} set cfg={} failed: {:?}\n",
            vendor_id,
            product_id,
            target.configuration_value,
            err
        );
    }

    let mut interface =
        match claim_interface(&mut device, target.interface_number, target.alternate_setting).await
        {
            Ok(interface) => interface,
            Err(err) => {
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} claim failed if#{} alt={}: {:?}\n",
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    err
                );
                unregister_active_mass_stream(active_stream);
                return;
            }
        };

    let bulk_out = match interface.endpoint_bulk_out(target.bulk_out).await {
        Ok(ep) => ep,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} bulk_out open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.bulk_out,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };
    let bulk_in = match interface.endpoint_bulk_in(target.bulk_in).await {
        Ok(ep) => ep,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} bulk_in open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.bulk_in,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };
    drop(interface);

    let mut bulk_out = bulk_out;
    let mut bulk_in = bulk_in;
    let probe = match mass::probe_mass_bot(
        &mut device,
        &mut bulk_out,
        &mut bulk_in,
        target.interface_number,
        target.bulk_out,
        target.bulk_in,
    )
    .await
    {
        Ok(info) => info,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} probe failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };

    let identity = build_mass_identity(&mut device, controller_id, u32::from(slot)).await;
    let existing_handle = registered_disk(identity.runtime_key);
    let handle = register_block_device(&identity, vendor_id, product_id, &probe);
    let attach_mode = if existing_handle.is_some() {
        "reattached"
    } else {
        "registered"
    };
    let initial_io_bytes =
        initial_mass_io_bytes(io_profile, port_speed, &target, probe.block_size as usize);
    crate::log!(
        "crabusb: mass {:04X}:{:04X} ready slot={} if#{} alt={} cfg={} bulk_in=0x{:02X} bulk_out=0x{:02X} in_mps={} out_mps={} disk={} mode={} label={:?} serial={:?} key={} transport={} profile={} init_io={} speed={:?} uas_candidates={} bs={} blocks={} vendor='{}' product='{}'\n",
        vendor_id,
        product_id,
        slot,
        target.interface_number,
        target.alternate_setting,
        target.configuration_value,
        target.bulk_in,
        target.bulk_out,
        target.bulk_in_max_packet_size,
        target.bulk_out_max_packet_size,
        handle.id(),
        attach_mode,
        handle.info().label,
        identity.serial.as_deref(),
        identity.key_kind,
        mass_transport_label(transport_kind),
        mass_io_profile_label(io_profile),
        initial_io_bytes,
        port_speed,
        uas_candidate_count,
        probe.block_size,
        probe.block_count,
        probe.vendor,
        probe.product
    );

    register_runtime(UsbMassRuntime {
        controller_id,
        slot_id: u32::from(slot),
        runtime_key: identity.runtime_key,
        vendor_id,
        product_id,
        device,
        interface_number: target.interface_number,
        bulk_in_ep: target.bulk_in,
        bulk_out_ep: target.bulk_out,
        endpoints: UsbMassEndpoints::Bot { bulk_in, bulk_out },
        transport_kind,
        io_profile,
        port_speed,
        uas_candidate_count,
        io_tag: 0x544F_0000 | u32::from(slot),
        sync_cache_unsupported: false,
        current_max_io_bytes: initial_io_bytes,
        io_success_streak: 0,
    });

    loop {
        Timer::after(EmbassyDuration::from_millis(MASS_KEEPALIVE_MS)).await;
        let Some(mut rt) = take_runtime(identity.runtime_key) else {
            continue;
        };

        let mut recovered = false;
        let bulk_out_ep = rt.bulk_out_ep;
        let bulk_in_ep = rt.bulk_in_ep;
        let keepalive = match &mut rt.endpoints {
            UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                if MASS_BOT_KEEPALIVE_ENABLED {
                    mass::keepalive_mass_bot(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, 0).await
                } else {
                    Ok(())
                }
            }
            UsbMassEndpoints::UasSkhynix {
                command_out,
                status_in,
                ..
            } => {
                let result =
                    mass::keepalive_mass_uas_skhynix(command_out, status_in, rt.io_tag).await;
                rt.io_tag = rt.io_tag.wrapping_add(1);
                result
            }
        };
        if let Err(err) = keepalive {
            if matches!(rt.endpoints, UsbMassEndpoints::Bot { .. })
                && recover_runtime_transport(&mut rt, "test-unit-ready", err)
                    .await
                    .is_ok()
            {
                recovered = true;
            } else {
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} lifecycle stop slot={} err={:?}\n",
                    vendor_id,
                    product_id,
                    slot,
                    err
                );
                break;
            }
        }

        register_runtime(rt);
        if recovered {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} keepalive recovered slot={}\n",
                vendor_id,
                product_id,
                slot
            );
        }
    }

    unregister_active_mass_stream(active_stream);
}

#[embassy_executor::task(pool_size = 2)]
pub async fn mass_storage_uas_skhynix_task(
    mut device: Device,
    controller_id: u32,
    target: UasTarget,
    port_speed: usb_if::Speed,
    uas_candidate_count: u8,
) {
    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let slot = device.slot_id();
    let active_stream = ActiveMassStream {
        controller_id,
        slot_id: u32::from(slot),
    };

    if let Err(err) = device
        .ep_ctrl()
        .set_configuration(target.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-skhynix set cfg={} failed: {:?}\n",
            vendor_id,
            product_id,
            target.configuration_value,
            err
        );
    }

    let target =
        refine_uas_target_from_raw_descriptors(&mut device, target, vendor_id, product_id).await;

    let mut interface =
        match claim_interface(&mut device, target.interface_number, target.alternate_setting).await
        {
            Ok(interface) => interface,
            Err(err) => {
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} uas-skhynix claim failed if#{} alt={}: {:?}\n",
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    err
                );
                unregister_active_mass_stream(active_stream);
                return;
            }
        };
    if crate::logflag::USB_MASS_UAS_ADVANCED_PROBE_LOGS {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-adv claimed if#{} alt={} cfg={} cmd_out=0x{:02X} status_in=0x{:02X} data_in=0x{:02X} data_out=0x{:02X}\n",
            vendor_id,
            product_id,
            target.interface_number,
            target.alternate_setting,
            target.configuration_value,
            target.command_out,
            target.status_in,
            target.data_in,
            target.data_out
        );
    }

    let command_out = match interface.endpoint_bulk_out(target.command_out).await {
        Ok(ep) => ep,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix command_out open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.command_out,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };
    let data_out = match interface.endpoint_bulk_out(target.data_out).await {
        Ok(ep) => ep,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix data_out open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.data_out,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };
    let status_in = match interface.endpoint_bulk_in(target.status_in).await {
        Ok(ep) => ep,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix status_in open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.status_in,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };
    let data_in = match interface.endpoint_bulk_in(target.data_in).await {
        Ok(ep) => ep,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix data_in open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.data_in,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };
    if crate::logflag::USB_MASS_UAS_ADVANCED_PROBE_LOGS {
        let stream_cfg = crab_usb::debug_last_stream_config();
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-adv endpoints-open cmd_out=0x{:02X}/{} status_in=0x{:02X}/{} data_in=0x{:02X}/{} data_out=0x{:02X}/{} last_stream_cfg[slot={} dci={} ep=0x{:02X} count={} maxp={} burst={} mps={} ctx=0x{:X} ring1=0x{:X}]\n",
            vendor_id,
            product_id,
            target.command_out,
            target.command_out_max_packet_size,
            target.status_in,
            target.status_in_max_packet_size,
            target.data_in,
            target.data_in_max_packet_size,
            target.data_out,
            target.data_out_max_packet_size,
            stream_cfg.slot_id,
            stream_cfg.dci,
            stream_cfg.ep_addr,
            stream_cfg.stream_count,
            stream_cfg.max_primary_streams,
            stream_cfg.max_burst,
            stream_cfg.max_packet_size,
            stream_cfg.ctx_ptr,
            stream_cfg.ring1_ptr
        );
    }
    drop(interface);

    let mut command_out = command_out;
    let mut status_in = status_in;
    let mut data_in = data_in;
    let mut data_out = data_out;
    let probe = match mass::exercise_mass_uas_skhynix(
        &mut command_out,
        &mut status_in,
        &mut data_in,
        &mut data_out,
    )
    .await
    {
        Ok(info) => info,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix exercise failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };

    let identity = build_mass_identity(&mut device, controller_id, u32::from(slot)).await;
    let existing_handle = registered_disk(identity.runtime_key);
    let handle = register_block_device(&identity, vendor_id, product_id, &probe);
    let attach_mode = if existing_handle.is_some() {
        "reattached"
    } else {
        "registered"
    };
    let initial_io_bytes = clamp_mass_io_bytes(probe.block_size as usize, MAX_IO_BYTES);
    crate::log!(
        "crabusb: mass {:04X}:{:04X} uas-skhynix ready slot={} if#{} alt={} cfg={} cmd_out=0x{:02X} status_in=0x{:02X} data_in=0x{:02X} data_out=0x{:02X} mps={}/{}/{}/{} disk={} mode={} label={:?} serial={:?} key={} transport={} profile={} init_io={} speed={:?} uas_candidates={} bs={} blocks={} vendor='{}' product='{}'\n",
        vendor_id,
        product_id,
        slot,
        target.interface_number,
        target.alternate_setting,
        target.configuration_value,
        target.command_out,
        target.status_in,
        target.data_in,
        target.data_out,
        target.command_out_max_packet_size,
        target.status_in_max_packet_size,
        target.data_in_max_packet_size,
        target.data_out_max_packet_size,
        handle.id(),
        attach_mode,
        handle.info().label,
        identity.serial.as_deref(),
        identity.key_kind,
        mass_transport_label(mass::MassTransportKind::Uas),
        mass_io_profile_label(MassIoProfile::UasSkhynix),
        initial_io_bytes,
        port_speed,
        uas_candidate_count,
        probe.block_size,
        probe.block_count,
        probe.vendor,
        probe.product
    );

    register_runtime(UsbMassRuntime {
        controller_id,
        slot_id: u32::from(slot),
        runtime_key: identity.runtime_key,
        vendor_id,
        product_id,
        device,
        interface_number: target.interface_number,
        bulk_in_ep: target.status_in,
        bulk_out_ep: target.command_out,
        endpoints: UsbMassEndpoints::UasSkhynix {
            command_out,
            status_in,
            data_in,
            data_out,
        },
        transport_kind: mass::MassTransportKind::Uas,
        io_profile: MassIoProfile::UasSkhynix,
        port_speed,
        uas_candidate_count,
        io_tag: 0x5541_0000 | u32::from(slot),
        sync_cache_unsupported: false,
        current_max_io_bytes: initial_io_bytes,
        io_success_streak: 0,
    });

    loop {
        Timer::after(EmbassyDuration::from_millis(MASS_KEEPALIVE_MS)).await;
        let Some(mut rt) = take_runtime(identity.runtime_key) else {
            continue;
        };

        let bulk_out_ep = rt.bulk_out_ep;
        let bulk_in_ep = rt.bulk_in_ep;
        let keepalive = match &mut rt.endpoints {
            UsbMassEndpoints::UasSkhynix {
                command_out,
                status_in,
                ..
            } => {
                let result =
                    mass::keepalive_mass_uas_skhynix(command_out, status_in, rt.io_tag).await;
                rt.io_tag = rt.io_tag.wrapping_add(1);
                result
            }
            UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                mass::keepalive_mass_bot(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, 0).await
            }
        };
        if let Err(err) = keepalive {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix lifecycle stop slot={} err={:?}\n",
                vendor_id,
                product_id,
                slot,
                err
            );
            break;
        }

        register_runtime(rt);
    }

    unregister_active_mass_stream(active_stream);
}

pub(crate) async fn maybe_start_mass_storage(
    host: &mut crab_usb::USBHost,
    dev_info: &crab_usb::DeviceInfo,
    spawner: &Spawner,
    controller_id: u32,
) -> bool {
    let transport_plan = mass::inspect_mass_transports(dev_info.configurations());
    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let topology = dev_info.topology();
    let uas_candidate_count = transport_plan.uas.len().min(u8::MAX as usize) as u8;

    for uas in transport_plan.uas.iter() {
        let mut bulk_in = String::new();
        for (idx, ep) in uas.bulk_in.iter().enumerate() {
            if idx > 0 {
                bulk_in.push(',');
            }
            bulk_in
                .push_str(alloc::format!("0x{:02X}/{}", ep.address, ep.max_packet_size).as_str());
        }

        let mut bulk_out = String::new();
        for (idx, ep) in uas.bulk_out.iter().enumerate() {
            if idx > 0 {
                bulk_out.push(',');
            }
            bulk_out
                .push_str(alloc::format!("0x{:02X}/{}", ep.address, ep.max_packet_size).as_str());
        }

        if let Some(target) = transport_plan.bot {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas candidate if#{} alt={} cfg={} in=[{}] out=[{}] bot_target if#{} alt={} proto={:02X}\n",
                vendor_id,
                product_id,
                uas.interface_number,
                uas.alternate_setting,
                uas.configuration_value,
                bulk_in,
                bulk_out,
                target.interface_number,
                target.alternate_setting,
                target.protocol,
            );
        } else {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas candidate if#{} alt={} cfg={} in=[{}] out=[{}] bot_target=none\n",
                vendor_id,
                product_id,
                uas.interface_number,
                uas.alternate_setting,
                uas.configuration_value,
                bulk_in,
                bulk_out,
            );
        }
    }

    if SKHYNIX_USE_UAS && is_skhynix_pssd_x31(vendor_id, product_id) {
        let Some(uas_target) =
            mass::pick_skhynix_uas_target(vendor_id, product_id, &transport_plan.uas)
        else {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix selected but fixed endpoint target is missing; no bot fallback\n",
                vendor_id,
                product_id
            );
            return true;
        };

        let device = match host.open_device(dev_info).await {
            Ok(device) => device,
            Err(err) => {
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} uas-skhynix open failed: {:?}\n",
                    vendor_id,
                    product_id,
                    err
                );
                return true;
            }
        };

        let active_stream = ActiveMassStream {
            controller_id,
            slot_id: u32::from(device.slot_id()),
        };
        if !register_active_mass_stream(active_stream) {
            return true;
        }

        match mass_storage_uas_skhynix_task(
            device,
            controller_id,
            uas_target,
            topology.port_speed,
            uas_candidate_count,
        ) {
            Ok(token) => {
                spawner.spawn(token);
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} uas-skhynix handoff if#{} alt={} cfg={} cmd_out=0x{:02X} status_in=0x{:02X} data_in=0x{:02X} data_out=0x{:02X} transport={} profile={} speed={:?} uas_candidates={}\n",
                    vendor_id,
                    product_id,
                    uas_target.interface_number,
                    uas_target.alternate_setting,
                    uas_target.configuration_value,
                    uas_target.command_out,
                    uas_target.status_in,
                    uas_target.data_in,
                    uas_target.data_out,
                    mass_transport_label(mass::MassTransportKind::Uas),
                    mass_io_profile_label(MassIoProfile::UasSkhynix),
                    topology.port_speed,
                    uas_candidate_count,
                );
            }
            Err(err) => {
                unregister_active_mass_stream(active_stream);
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} uas-skhynix spawn failed if#{} alt={}: {:?}\n",
                    vendor_id,
                    product_id,
                    uas_target.interface_number,
                    uas_target.alternate_setting,
                    err
                );
            }
        }

        return true;
    }

    let Some(target) = transport_plan.bot else {
        return false;
    };
    let transport_kind = mass::MassTransportKind::Bot;
    let io_profile =
        choose_mass_io_profile(transport_kind, vendor_id, product_id, topology.port_speed, &target);

    let device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} open failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
            return true;
        }
    };

    let active_stream = ActiveMassStream {
        controller_id,
        slot_id: u32::from(device.slot_id()),
    };
    if !register_active_mass_stream(active_stream) {
        return true;
    }

    match mass_storage_task(
        device,
        controller_id,
        target,
        transport_kind,
        io_profile,
        topology.port_speed,
        uas_candidate_count,
    ) {
        Ok(token) => {
            spawner.spawn(token);
            crate::log!(
                "crabusb: mass {:04X}:{:04X} handoff if#{} alt={} cfg={} bulk_in=0x{:02X} bulk_out=0x{:02X} in_mps={} out_mps={} transport={} profile={} speed={:?} uas_candidates={}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.alternate_setting,
                target.configuration_value,
                target.bulk_in,
                target.bulk_out,
                target.bulk_in_max_packet_size,
                target.bulk_out_max_packet_size,
                mass_transport_label(transport_kind),
                mass_io_profile_label(io_profile),
                topology.port_speed,
                uas_candidate_count,
            );
        }
        Err(err) => {
            unregister_active_mass_stream(active_stream);
            crate::log!(
                "crabusb: mass {:04X}:{:04X} spawn failed if#{} alt={}: {:?}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.alternate_setting,
                err
            );
        }
    }

    true
}
