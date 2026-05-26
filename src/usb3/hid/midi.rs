use core::cmp::min;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crab_usb::Device;
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::{Deque, Vec};
use spin::Mutex;

use super::api::{InterfaceEndpointError, claim_interface};

const USB_CLASS_AUDIO: u8 = 0x01;
const USB_SUBCLASS_MIDISTREAMING: u8 = 0x03;
const PIANO_QUEUE_PKTS: usize = 512;
const MIDI_IDLE_SLEEP_MS: u64 = 25;
const MIDI_READ_TIMEOUT_MS: u64 = 1000;
const MAX_ACTIVE_MIDI_STREAMS: usize = 8;
const PIANO_HELD_MAX_NOTES: usize = 8;
const PIANO_DRAIN_DIRECT_AUDIO_ENABLED: bool = false;
const PIANO_AUDIBLE_MIN_MS: u32 = 45;
const PIANO_AUDIBLE_VEL_MS: u32 = 180;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum MidiAdapterKind {
    Generic,
    CasioCtk3500,
}

#[derive(Copy, Clone, Debug)]
struct MidiEp {
    addr: u8,
    max_packet: u16,
}

#[derive(Copy, Clone, Debug)]
struct MidiTarget {
    configuration_value: u8,
    interface_number: u8,
    alternate_setting: u8,
    ep_in: MidiEp,
    ep_out: Option<MidiEp>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveMidiStream {
    controller_id: u32,
    slot_id: u32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct PianoNoteSnapshot {
    pub seq: u16,
    pub note: u8,
    pub velocity: u8,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct PianoHeldSnapshot {
    pub seq: u16,
    pub len: usize,
    pub notes: [u8; PIANO_HELD_MAX_NOTES],
    pub velocities: [u8; PIANO_HELD_MAX_NOTES],
}

#[derive(Copy, Clone, Debug)]
struct PianoHeldState {
    seq: u16,
    len: usize,
    notes: [u8; PIANO_HELD_MAX_NOTES],
    velocities: [u8; PIANO_HELD_MAX_NOTES],
}

impl PianoHeldState {
    const fn empty() -> Self {
        Self {
            seq: 0,
            len: 0,
            notes: [0; PIANO_HELD_MAX_NOTES],
            velocities: [0; PIANO_HELD_MAX_NOTES],
        }
    }

    fn snapshot(&self) -> PianoHeldSnapshot {
        PianoHeldSnapshot {
            seq: self.seq,
            len: self.len,
            notes: self.notes,
            velocities: self.velocities,
        }
    }
}

static ACTIVE_MIDI_STREAMS: Mutex<Vec<ActiveMidiStream, MAX_ACTIVE_MIDI_STREAMS>> =
    Mutex::new(Vec::new());

static PIANO_SLOT: AtomicU32 = AtomicU32::new(0);
static PIANO_CONTROLLER: AtomicU32 = AtomicU32::new(0);
static PIANO_LAST_HEARTBEAT_SECS: AtomicU64 = AtomicU64::new(u64::MAX);
static PIANO_NOTE_SEQ: AtomicU32 = AtomicU32::new(0);
static PIANO_LAST_NOTE: AtomicU32 = AtomicU32::new(0);
static PIANO_HELD: Mutex<PianoHeldState> = Mutex::new(PianoHeldState::empty());
static PIANO_AUDIO_ERRS: AtomicU32 = AtomicU32::new(0);
static PIANO_DRAIN_STARTED: AtomicBool = AtomicBool::new(false);
static PIANO_QUEUE: Mutex<Deque<[u8; 4], PIANO_QUEUE_PKTS>> = Mutex::new(Deque::new());

fn select_adapter(dev_vid: u16, dev_pid: u16) -> MidiAdapterKind {
    if dev_vid == 0x07CF && dev_pid == 0x6803 {
        MidiAdapterKind::CasioCtk3500
    } else {
        MidiAdapterKind::Generic
    }
}

#[inline]
fn is_active_sensing_heartbeat(pkt: &[u8; 4]) -> bool {
    (pkt[0] & 0x0F) == 0x0F && pkt[1] == 0xFE && pkt[2] == 0 && pkt[3] == 0
}

#[inline]
fn secs_to_hms(secs: u64) -> (u64, u64, u64) {
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    (h, m, s)
}

fn piano_set_connected(controller_id: u32, slot_id: u32) {
    PIANO_CONTROLLER.store(controller_id, Ordering::Release);
    PIANO_SLOT.store(slot_id, Ordering::Release);
}

fn piano_set_disconnected(controller_id: u32, slot_id: u32) {
    let cur_slot = PIANO_SLOT.load(Ordering::Acquire);
    let cur_ctrl = PIANO_CONTROLLER.load(Ordering::Acquire);
    if cur_slot == slot_id && cur_ctrl == controller_id {
        PIANO_SLOT.store(0, Ordering::Release);
        PIANO_CONTROLLER.store(0, Ordering::Release);
        PIANO_LAST_HEARTBEAT_SECS.store(u64::MAX, Ordering::Release);
        PIANO_LAST_NOTE.store(0, Ordering::Release);
        *PIANO_HELD.lock() = PianoHeldState::empty();
        let mut q = PIANO_QUEUE.lock();
        while q.pop_front().is_some() {}
    }
}

#[inline]
fn pack_note_snapshot(seq: u16, note: u8, velocity: u8) -> u32 {
    (u32::from(seq) << 16) | (u32::from(note) << 8) | u32::from(velocity)
}

#[inline]
fn unpack_note_snapshot(packed: u32) -> Option<PianoNoteSnapshot> {
    if packed == 0 {
        return None;
    }
    Some(PianoNoteSnapshot {
        seq: (packed >> 16) as u16,
        note: ((packed >> 8) & 0xFF) as u8,
        velocity: (packed & 0xFF) as u8,
    })
}

fn piano_record_note_on(note: u8, velocity: u8) {
    let seq = PIANO_NOTE_SEQ
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1) as u16;
    PIANO_LAST_NOTE.store(pack_note_snapshot(seq, note, velocity), Ordering::Release);
}

fn piano_record_held_note_on(note: u8, velocity: u8) {
    let mut held = PIANO_HELD.lock();
    if let Some(idx) = held.notes[..held.len].iter().position(|&n| n == note) {
        held.velocities[idx] = velocity;
    } else if held.len < PIANO_HELD_MAX_NOTES {
        let idx = held.len;
        held.notes[idx] = note;
        held.velocities[idx] = velocity;
        held.len += 1;
    } else {
        held.notes[0] = note;
        held.velocities[0] = velocity;
    }
    held.seq = held.seq.wrapping_add(1).max(1);
}

fn piano_record_held_note_off(note: u8) {
    let mut held = PIANO_HELD.lock();
    let Some(idx) = held.notes[..held.len].iter().position(|&n| n == note) else {
        return;
    };

    let last = held.len - 1;
    for i in idx..last {
        held.notes[i] = held.notes[i + 1];
        held.velocities[i] = held.velocities[i + 1];
    }
    held.notes[last] = 0;
    held.velocities[last] = 0;
    held.len = last;
    held.seq = held.seq.wrapping_add(1).max(1);
}

pub(crate) fn piano_note_snapshot() -> Option<PianoNoteSnapshot> {
    if PIANO_SLOT.load(Ordering::Acquire) == 0 {
        return None;
    }
    unpack_note_snapshot(PIANO_LAST_NOTE.load(Ordering::Acquire))
}

pub(crate) fn piano_held_snapshot() -> Option<PianoHeldSnapshot> {
    if PIANO_SLOT.load(Ordering::Acquire) == 0 {
        return None;
    }
    Some(PIANO_HELD.lock().snapshot())
}

pub(crate) fn piano_connected() -> bool {
    PIANO_SLOT.load(Ordering::Acquire) != 0
}

fn maybe_start_piano_drain(spawner: &Spawner) {
    if PIANO_DRAIN_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    match piano_drain_loop() {
        Ok(token) => {
            spawner.spawn(token);
            crate::log!("piano: drain loop started\n");
        }
        Err(err) => {
            PIANO_DRAIN_STARTED.store(false, Ordering::Release);
            crate::log!("piano: drain loop token failed: {:?}\n", err);
        }
    }
}

fn piano_push_packet(pkt: [u8; 4]) {
    let mut q = PIANO_QUEUE.lock();
    if q.push_back(pkt).is_err() {
        let _ = q.pop_front();
        let _ = q.push_back(pkt);
    }
}

#[inline]
fn midi_note_on_from_packet(pkt: &[u8; 4]) -> Option<(u8, u8)> {
    let cin = pkt[0] & 0x0F;
    let status = pkt[1] & 0xF0;
    if cin == 0x09 && status == 0x90 && pkt[3] != 0 {
        Some((pkt[2].min(127), pkt[3].min(127)))
    } else {
        None
    }
}

#[inline]
fn midi_note_off_from_packet(pkt: &[u8; 4]) -> Option<u8> {
    let cin = pkt[0] & 0x0F;
    let status = pkt[1] & 0xF0;
    if (cin == 0x08 && status == 0x80) || (cin == 0x09 && status == 0x90 && pkt[3] == 0) {
        Some(pkt[2].min(127))
    } else {
        None
    }
}

fn piano_play_packet(pkt: [u8; 4]) {
    let Some((note, velocity)) = midi_note_on_from_packet(&pkt) else {
        return;
    };

    let duration_ms = PIANO_AUDIBLE_MIN_MS + (u32::from(velocity) * PIANO_AUDIBLE_VEL_MS / 127);
    if let Err(err) = crate::aud::play_midi_note(note, velocity, duration_ms) {
        let n = PIANO_AUDIO_ERRS
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        if n <= 4 {
            crate::log!("piano: audible note failed err={}\n", err);
        }
    }
}

#[embassy_executor::task]
pub async fn piano_drain_loop() {
    async move {
        loop {
            let slot = PIANO_SLOT.load(Ordering::Acquire);
            if slot == 0 {
                Timer::after(EmbassyDuration::from_millis(MIDI_IDLE_SLEEP_MS)).await;
                continue;
            }

            let pkt_opt = { PIANO_QUEUE.lock().pop_front() };
            let Some(pkt) = pkt_opt else {
                Timer::after(EmbassyDuration::from_millis(MIDI_IDLE_SLEEP_MS)).await;
                continue;
            };

            let hb = PIANO_LAST_HEARTBEAT_SECS.load(Ordering::Acquire);
            if hb == u64::MAX {
                crate::log!("piano: --:--:-- ~ {}.{}.{}.{}\n", pkt[0], pkt[1], pkt[2], pkt[3]);
            } else {
                let (h, m, s) = secs_to_hms(hb);
                crate::log!(
                    "piano: {:02}:{:02}:{:02} ~ {}.{}.{}.{}\n",
                    h,
                    m,
                    s,
                    pkt[0],
                    pkt[1],
                    pkt[2],
                    pkt[3]
                );
            }

            if PIANO_DRAIN_DIRECT_AUDIO_ENABLED {
                piano_play_packet(pkt);
            }
        }
    }
    .await;
}

fn register_active_midi_stream(stream: ActiveMidiStream) -> bool {
    let mut streams = ACTIVE_MIDI_STREAMS.lock();
    if streams.iter().any(|active| *active == stream) {
        return false;
    }
    streams.push(stream).is_ok()
}

fn unregister_active_midi_stream(stream: ActiveMidiStream) {
    let mut streams = ACTIVE_MIDI_STREAMS.lock();
    if let Some(idx) = streams.iter().position(|active| *active == stream) {
        streams.remove(idx);
    }
}

fn pick_midi_target(
    configs: &[crab_usb::usb_if::descriptor::ConfigurationDescriptor],
) -> Option<MidiTarget> {
    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                if alt.class != USB_CLASS_AUDIO || alt.subclass != USB_SUBCLASS_MIDISTREAMING {
                    continue;
                }

                let ep_in = alt.endpoints.iter().find_map(|ep| {
                    (ep.transfer_type == crab_usb::usb_if::descriptor::EndpointType::Bulk
                        && ep.direction == crab_usb::usb_if::transfer::Direction::In)
                        .then_some(MidiEp {
                            addr: ep.address,
                            max_packet: ep.max_packet_size,
                        })
                })?;

                let ep_out = alt.endpoints.iter().find_map(|ep| {
                    (ep.transfer_type == crab_usb::usb_if::descriptor::EndpointType::Bulk
                        && ep.direction == crab_usb::usb_if::transfer::Direction::Out)
                        .then_some(MidiEp {
                            addr: ep.address,
                            max_packet: ep.max_packet_size,
                        })
                });

                return Some(MidiTarget {
                    configuration_value: config.configuration_value,
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    ep_in,
                    ep_out,
                });
            }
        }
    }

    None
}

async fn with_timeout_or_none<F: core::future::Future>(
    fut: F,
    timeout_ms: u64,
) -> Option<F::Output> {
    let mut fut = core::pin::pin!(fut);
    let mut timeout = core::pin::pin!(Timer::after(EmbassyDuration::from_millis(timeout_ms)));

    core::future::poll_fn(|cx| {
        if let core::task::Poll::Ready(out) = fut.as_mut().poll(cx) {
            return core::task::Poll::Ready(Some(out));
        }
        if timeout.as_mut().poll(cx).is_ready() {
            return core::task::Poll::Ready(None);
        }
        core::task::Poll::Pending
    })
    .await
}

fn handle_midi_packets(adapter: MidiAdapterKind, sample: &[u8]) {
    for chunk in sample.chunks_exact(4) {
        let pkt = [chunk[0], chunk[1], chunk[2], chunk[3]];
        if adapter == MidiAdapterKind::CasioCtk3500 {
            if is_active_sensing_heartbeat(&pkt) {
                let now =
                    crate::time::unix_time_seconds().unwrap_or_else(crate::time::uptime_seconds);
                PIANO_LAST_HEARTBEAT_SECS.store(now, Ordering::Release);
                continue;
            }
            if let Some((note, velocity)) = midi_note_on_from_packet(&pkt) {
                piano_record_note_on(note, velocity);
                piano_record_held_note_on(note, velocity);
            } else if let Some(note) = midi_note_off_from_packet(&pkt) {
                piano_record_held_note_off(note);
            }
            piano_push_packet(pkt);
        }
    }
}

#[embassy_executor::task(pool_size = 4)]
pub async fn midi_stream_task(mut device: Device, controller_id: u32, target: MidiTarget) {
    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let adapter = select_adapter(vendor_id, product_id);
    let slot_id = u32::from(device.slot_id());
    let active_stream = ActiveMidiStream {
        controller_id,
        slot_id,
    };

    if let Err(err) = device.set_configuration(target.configuration_value).await {
        crate::log!(
            "crabusb: midi {:04X}:{:04X} set cfg={} failed: {:?}\n",
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
                    "crabusb: midi {:04X}:{:04X} claim failed if#{} alt={}: {:?}\n",
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    err
                );
                unregister_active_midi_stream(active_stream);
                return;
            }
        };

    let mut bulk_in = match interface.endpoint_bulk_in(target.ep_in.addr).await {
        Ok(ep) => ep,
        Err(InterfaceEndpointError::WrongKind { address, expected }) => {
            crate::log!(
                "crabusb: midi {:04X}:{:04X} bulk_in kind mismatch ep=0x{:02X} got=0x{:02X} expected={}\n",
                vendor_id,
                product_id,
                target.ep_in.addr,
                address,
                expected
            );
            unregister_active_midi_stream(active_stream);
            return;
        }
        Err(InterfaceEndpointError::Usb(err)) => {
            crate::log!(
                "crabusb: midi {:04X}:{:04X} bulk_in open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.ep_in.addr,
                err
            );
            unregister_active_midi_stream(active_stream);
            return;
        }
    };

    let bulk_out_opened = match target.ep_out {
        Some(ep_out) => match interface.endpoint_bulk_out(ep_out.addr).await {
            Ok(_ep) => true,
            Err(InterfaceEndpointError::WrongKind { address, expected }) => {
                crate::log!(
                    "crabusb: midi {:04X}:{:04X} bulk_out kind mismatch ep=0x{:02X} got=0x{:02X} expected={}\n",
                    vendor_id,
                    product_id,
                    ep_out.addr,
                    address,
                    expected
                );
                false
            }
            Err(InterfaceEndpointError::Usb(err)) => {
                crate::log!(
                    "crabusb: midi {:04X}:{:04X} bulk_out open failed ep=0x{:02X}: {:?}\n",
                    vendor_id,
                    product_id,
                    ep_out.addr,
                    err
                );
                false
            }
        },
        None => false,
    };
    drop(interface);

    if adapter == MidiAdapterKind::CasioCtk3500 {
        piano_set_connected(controller_id, slot_id);
        crate::r::readiness::set(crate::r::readiness::PIANO_CLAIMED);
    }

    crate::log!(
        "crabusb: midi {:04X}:{:04X} ready slot={} if#{} alt={} cfg={} bulk_in=0x{:02X} in_mps={} bulk_out={} out_ep={} adapter={:?}\n",
        vendor_id,
        product_id,
        slot_id,
        target.interface_number,
        target.alternate_setting,
        target.configuration_value,
        target.ep_in.addr,
        target.ep_in.max_packet,
        bulk_out_opened,
        target.ep_out.map(|ep| ep.addr).unwrap_or(0),
        adapter
    );

    let mut rx = alloc::vec![0u8; usize::from(target.ep_in.max_packet.max(64))];
    let mut timeout_logs = 0u32;

    loop {
        match with_timeout_or_none(bulk_in.submit_and_wait(rx.as_mut_slice()), MIDI_READ_TIMEOUT_MS)
            .await
        {
            None => {
                timeout_logs = timeout_logs.wrapping_add(1);
                if timeout_logs <= 8 || timeout_logs.is_multiple_of(32) {
                    crate::log!(
                        "crabusb: midi {:04X}:{:04X} read timeout ep=0x{:02X} count={}\n",
                        vendor_id,
                        product_id,
                        target.ep_in.addr,
                        timeout_logs
                    );
                }
            }
            Some(Ok(read)) => {
                timeout_logs = 0;
                if read == 0 {
                    Timer::after(EmbassyDuration::from_millis(1)).await;
                    continue;
                }
                let sample = &rx[..read.min(rx.len())];
                if sample.len() >= 4 {
                    handle_midi_packets(adapter, sample);
                } else {
                    crate::log!(
                        "crabusb: midi {:04X}:{:04X} short packet len={} bytes={:02X?}\n",
                        vendor_id,
                        product_id,
                        sample.len(),
                        &sample[..min(sample.len(), 8)]
                    );
                }
            }
            Some(Err(err)) => {
                crate::log!(
                    "crabusb: midi {:04X}:{:04X} stream stop ep=0x{:02X} err={:?}\n",
                    vendor_id,
                    product_id,
                    target.ep_in.addr,
                    err
                );
                break;
            }
        }
    }

    if adapter == MidiAdapterKind::CasioCtk3500 {
        piano_set_disconnected(controller_id, slot_id);
    }
    unregister_active_midi_stream(active_stream);
}

pub(crate) async fn maybe_start_midi(
    host: &mut crab_usb::USBHost,
    dev_info: &crab_usb::DeviceInfo,
    spawner: &Spawner,
    controller_id: u32,
) -> bool {
    let Some(target) = pick_midi_target(dev_info.configurations()) else {
        return false;
    };

    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let adapter = select_adapter(vendor_id, product_id);

    if adapter == MidiAdapterKind::CasioCtk3500 {
        maybe_start_piano_drain(spawner);
    }

    let device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            crate::log!(
                "crabusb: midi {:04X}:{:04X} open failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
            return true;
        }
    };

    let active_stream = ActiveMidiStream {
        controller_id,
        slot_id: u32::from(device.slot_id()),
    };
    if !register_active_midi_stream(active_stream) {
        return true;
    }

    match midi_stream_task(device, controller_id, target) {
        Ok(token) => {
            spawner.spawn(token);
            crate::log!(
                "crabusb: midi {:04X}:{:04X} handoff if#{} alt={} cfg={} bulk_in=0x{:02X} in_mps={} bulk_out={} out_ep={}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.alternate_setting,
                target.configuration_value,
                target.ep_in.addr,
                target.ep_in.max_packet,
                target.ep_out.is_some(),
                target.ep_out.map(|ep| ep.addr).unwrap_or(0)
            );
        }
        Err(err) => {
            unregister_active_midi_stream(active_stream);
            crate::log!(
                "crabusb: midi {:04X}:{:04X} spawn failed if#{} alt={}: {:?}\n",
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
