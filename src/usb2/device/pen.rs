use alloc::{boxed::Box, string::String, vec::Vec as AllocVec};

use crab_usb::usb_if;
use crab_usb::{Device, EndpointBulkIn, EndpointBulkOut};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;

use crate::disc::block;

use super::api::claim_interface;
use super::mass::{self, MassProbeInfo, MassTarget};
use super::scsi::{self, SenseKey};

const MAX_MASS_RUNTIMES: usize = crate::allcaps::storage::USB_MASS_MAX_RUNTIMES;
const MAX_ACTIVE_STREAMS: usize = crate::allcaps::storage::USB_MASS_MAX_ACTIVE_STREAMS;
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveMassStream {
    controller_id: u32,
    slot_id: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum MassIoProfile {
    ConservativeBot,
    FastBot,
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
    bulk_in: EndpointBulkIn,
    bulk_out: EndpointBulkOut,
    transport_kind: mass::MassTransportKind,
    io_profile: MassIoProfile,
    port_speed: usb_if::Speed,
    uas_candidate_count: u8,
    bot_tag: u32,
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
    }
}

fn choose_mass_io_profile(
    transport_kind: mass::MassTransportKind,
    port_speed: usb_if::Speed,
    target: &MassTarget,
) -> MassIoProfile {
    if transport_kind == mass::MassTransportKind::Bot
        && matches!(port_speed, usb_if::Speed::SuperSpeed | usb_if::Speed::SuperSpeedPlus)
        && target.bulk_in_max_packet_size >= 1024
        && target.bulk_out_max_packet_size >= 1024
    {
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
    }
}

fn current_mass_io_bytes(rt: &UsbMassRuntime, block_size: usize) -> usize {
    clamp_mass_io_bytes(block_size, rt.current_max_io_bytes)
}

fn current_mass_write_io_bytes(rt: &UsbMassRuntime, block_size: usize) -> usize {
    let cur = current_mass_io_bytes(rt, block_size);
    match rt.io_profile {
        MassIoProfile::ConservativeBot => cur,
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
        "crabusb: mass {:04X}:{:04X} transport stage={} key=0x{:X} expect[ctrl={} slot={} out_ep=0x{:02X} in_ep=0x{:02X}] last_submit[dci={} dir={} len={} ptr=0x{:X}] last_event[slot={} ep={} cc={} residual={} ptr=0x{:X}]\n",
        rt.vendor_id,
        rt.product_id,
        stage,
        rt.runtime_key,
        rt.controller_id,
        rt.slot_id,
        rt.bulk_out_ep,
        rt.bulk_in_ep,
        submit.dci,
        submit.direction,
        submit.len,
        submit.ptr,
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
                            let result = mass::read_blocks_bot(
                                &mut rt.bulk_out,
                                &mut rt.bulk_in,
                                cur_lba as u32,
                                blocks_here as u16,
                                &mut remaining[..bytes_here],
                                rt.bot_tag,
                            )
                            .await;
                            rt.bot_tag = rt.bot_tag.wrapping_add(1);

                            match result {
                                Ok(()) => {
                                    mass_io_note_success(rt, block_size);
                                    break;
                                }
                                Err(err) => {
                                    mass_io_backoff(rt, block_size);
                                    if recover_runtime_transport(rt, "read-10", err).await.is_ok()
                                        && attempts < MASS_IO_RETRY_LIMIT
                                    {
                                        attempts = attempts.wrapping_add(1);
                                        Timer::after(EmbassyDuration::from_millis(MASS_IO_RETRY_DELAY_MS)).await;
                                        continue;
                                    }
                                    let sense =
                                        mass::request_sense_fixed(&mut rt.bulk_out, &mut rt.bulk_in, rt.bot_tag).await;
                                    rt.bot_tag = rt.bot_tag.wrapping_add(1);
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
                        let result = mass::write_blocks_bot(
                            &mut rt.bulk_out,
                            &mut rt.bulk_in,
                            cur_lba as u32,
                            blocks_here as u16,
                            &remaining[..bytes_here],
                            rt.bot_tag,
                        )
                        .await;
                        rt.bot_tag = rt.bot_tag.wrapping_add(1);

                        match result {
                            Ok(()) => {
                                mass_io_note_success(&mut rt, block_size);
                                break;
                            }
                            Err(err) => {
                                mass_io_backoff(&mut rt, block_size);
                                if recover_runtime_transport(&mut rt, "write-10", err).await.is_ok()
                                    && attempts < MASS_IO_RETRY_LIMIT
                                {
                                    attempts = attempts.wrapping_add(1);
                                    Timer::after(EmbassyDuration::from_millis(MASS_IO_RETRY_DELAY_MS)).await;
                                    continue;
                                }
                                let sense =
                                    mass::request_sense_fixed(&mut rt.bulk_out, &mut rt.bulk_in, rt.bot_tag).await;
                                rt.bot_tag = rt.bot_tag.wrapping_add(1);
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

                    match mass::synchronize_cache_bot(&mut rt.bulk_out, &mut rt.bulk_in, rt.bot_tag).await {
                        Ok(()) => {
                            rt.bot_tag = rt.bot_tag.wrapping_add(1);
                            Ok(())
                        }
                        Err(err) => {
                            let _ = recover_runtime_transport(rt, "sync-cache-10", err).await;
                            rt.bot_tag = rt.bot_tag.wrapping_add(1);
                            let sense = mass::request_sense_fixed(&mut rt.bulk_out, &mut rt.bulk_in, rt.bot_tag).await;
                            rt.bot_tag = rt.bot_tag.wrapping_add(1);
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
        bulk_in,
        bulk_out,
        transport_kind,
        io_profile,
        port_speed,
        uas_candidate_count,
        bot_tag: 0x544F_0000 | u32::from(slot),
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
        let keepalive = mass::keepalive_mass_bot(&mut rt.bulk_out, &mut rt.bulk_in, 0).await;
        if let Err(err) = keepalive {
            if recover_runtime_transport(&mut rt, "test-unit-ready", err)
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

pub(crate) async fn maybe_start_mass_storage(
    host: &mut crab_usb::USBHost,
    dev_info: &crab_usb::DeviceInfo,
    spawner: &Spawner,
    controller_id: u32,
) -> bool {
    let transport_plan = mass::inspect_mass_transports(dev_info.configurations());
    let Some(target) = transport_plan.bot else {
        return false;
    };

    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let topology = dev_info.topology();
    let transport_kind = mass::MassTransportKind::Bot;
    let io_profile = choose_mass_io_profile(transport_kind, topology.port_speed, &target);
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

        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas candidate if#{} alt={} cfg={} in=[{}] out=[{}] while binding bot if#{} alt={} proto={:02X}\n",
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
    }

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
