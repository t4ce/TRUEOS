//! Standard HDA link playback using a cyclic DMA buffer.
//! Register and codec definitions follow Intel HDA Specification rev. 1.0a, §§3 and 7.

use alloc::vec::Vec;
use core::{
    ptr::{copy_nonoverlapping, read_volatile, write_bytes, write_volatile},
    sync::atomic::{Ordering, fence},
};

use crate::{
    net::ring::DmaRegion,
    pci::{self, PciDevice},
};

mod reg {
    pub const GCAP: usize = 0x00;
    pub const VMAJ: usize = 0x03;
    pub const GCTL: usize = 0x08;
    pub const WAKEEN: usize = 0x0c;
    pub const STATESTS: usize = 0x0e;
    pub const INTCTL: usize = 0x20;
    pub const SSYNC: usize = 0x38;
    pub const CORBLBASE: usize = 0x40;
    pub const CORBWP: usize = 0x48;
    pub const CORBRP: usize = 0x4a;
    pub const CORBCTL: usize = 0x4c;
    pub const CORBSTS: usize = 0x4d;
    pub const CORBSIZE: usize = 0x4e;
    pub const RIRBLBASE: usize = 0x50;
    pub const RIRBWP: usize = 0x58;
    pub const RINTCNT: usize = 0x5a;
    pub const RIRBCTL: usize = 0x5c;
    pub const RIRBSTS: usize = 0x5d;
    pub const RIRBSIZE: usize = 0x5e;
    pub const DPLBASE: usize = 0x70;
    pub const SD_BASE: usize = 0x80;
    pub const SD_STRIDE: usize = 0x20;
}

mod sd {
    pub const CTL: usize = 0x00;
    pub const STS: usize = 0x03;
    pub const LPIB: usize = 0x04;
    pub const CBL: usize = 0x08;
    pub const LVI: usize = 0x0c;
    pub const FIFOS: usize = 0x10;
    pub const FMT: usize = 0x12;
    pub const BDLPL: usize = 0x18;
    pub const RESET: u8 = 1;
    pub const RUN: u8 = 1 << 1;
    pub const ERRORS: u8 = (1 << 3) | (1 << 4);
    pub const CLEAR_STATUS: u8 = ERRORS | (1 << 2);
}

mod verb {
    pub const GET_PARAMETER: u32 = 0xf00;
    pub const GET_CONNECTION_LIST: u32 = 0xf02;
    pub const GET_POWER: u32 = 0xf05;
    pub const GET_PIN_CONFIG: u32 = 0xf1c;
    pub const SET_CONNECT: u32 = 0x701;
    pub const SET_POWER: u32 = 0x705;
    pub const SET_STREAM: u32 = 0x706;
    pub const SET_PIN_CONTROL: u32 = 0x707;
    pub const SET_EAPD: u32 = 0x70c;
    pub const SET_FORMAT: u32 = 0x20000;
    pub const SET_AMP: u32 = 0x30000;
}

const PCI_COMMAND: u16 = 0x04;
const BUS_MASTER: u16 = 1 << 2;
const INTX_DISABLE: u16 = 1 << 10;

const CORB_OFFSET: usize = 128;
const POSITION_OFFSET: usize = 1152;
const RIRB_OFFSET: usize = 2048;
const PCM_OFFSET: usize = 4096;
const PERIODS: usize = 4;
const FRAMES_PER_PERIOD: usize = 512;
const FRAME_BYTES: usize = size_of::<[i16; 2]>();
const PERIOD_BYTES: usize = FRAMES_PER_PERIOD * FRAME_BYTES;
const RING_BYTES: usize = PERIODS * PERIOD_BYTES;
const PCM_FORMAT: u16 = 0x0011; // 48 kHz, signed 16-bit stereo
const STREAM_TAG: u8 = 1;
const MAX_POLL_GAP_US: u64 = 20_000;
const CURSOR_STALL_US: u64 = 100_000;

const NODE_COUNT: u8 = 0x04;
const FUNCTION_TYPE: u8 = 0x05;
const WIDGET_CAPS: u8 = 0x09;
const PCM_RATES: u8 = 0x0a;
const STREAM_FORMATS: u8 = 0x0b;
const PIN_CAPS: u8 = 0x0c;
const INPUT_AMP: u8 = 0x0d;
const CONNECTION_LIST_LEN: u8 = 0x0e;
const OUTPUT_AMP: u8 = 0x12;

#[repr(C)]
struct BufferDescriptor {
    address: u64,
    length: u32,
    flags: u32,
}

struct Widget {
    nid: u8,
    caps: u32,
    input_amp: u16,
    output_amp: u16,
    connections: Vec<u8>,
    pin_caps: u32,
    pin_priority: u8,
    pcm_output: bool,
}

impl Widget {
    fn kind(&self) -> u32 {
        (self.caps >> 20) & 0xf
    }
}

struct Output {
    codec: u8,
    path: Vec<usize>,
    widgets: Vec<Widget>,
    priority: u8,
}

pub(super) struct Playback {
    device: PciDevice,
    mmio: usize,
    dma: Option<DmaRegion>,
    stream: usize,
    corb_entries: u16,
    rirb_entries: u16,
    rirb_read: u16,
    next_period: usize,
    last_poll: Option<u64>,
    last_position: usize,
    last_progress: u64,
}

impl Playback {
    /// Open after claiming the PCI function.
    pub(super) fn open(device: &PciDevice) -> Result<Self, &'static str> {
        let (bar, _) = pci::read_bar0_raw(device.bus, device.slot, device.function);
        let address = device
            .bar_address(0)
            .filter(|&address| address != 0)
            .ok_or("missing HDA BAR0")?;
        if bar & 1 != 0 || bar == u32::MAX {
            return Err("invalid HDA memory BAR0");
        }
        let mmio = pci::mmio::map_mmio_region_exact(address, 0x1000)
            .map_err(|_| "could not map HDA BAR0")?;
        let dma = DmaRegion::alloc(PCM_OFFSET + RING_BYTES, 4096)
            .ok_or("could not allocate HDA DMA memory")?;
        // Own the allocation exclusively until the controller is programmed below.
        unsafe { write_bytes(dma.virt(), 0, PCM_OFFSET + RING_BYTES) };
        pci::enable_mem_space_only(device.bus, device.slot, device.function);

        let mut playback = Self {
            device: *device,
            mmio: mmio.as_ptr() as usize,
            dma: Some(dma),
            stream: 0,
            corb_entries: 0,
            rirb_entries: 0,
            rirb_read: 0,
            next_period: 0,
            last_poll: None,
            last_position: 0,
            last_progress: 0,
        };
        let caps = playback.read::<u16>(reg::GCAP);
        if playback.read::<u8>(reg::VMAJ) != 1 || caps >> 12 == 0 {
            return Err("no standard HDA output stream");
        }
        playback.stream = reg::SD_BASE + usize::from((caps >> 8) & 0xf) * reg::SD_STRIDE;
        let codecs = playback.reset_controller()?;
        playback.configure_command_dma()?;
        let output = playback
            .find_output(codecs)?
            .ok_or("no analog HDA PCM output route")?;
        playback.configure_output(&output)?;
        playback.program_stream()?;
        Ok(playback)
    }

    fn dma(&self) -> &DmaRegion {
        self.dma.as_ref().unwrap()
    }

    // Offsets and access widths match the register descriptions in HDA §3.
    fn read<T: Copy>(&self, offset: usize) -> T {
        unsafe { read_volatile((self.mmio + offset) as *const T) }
    }

    fn write<T: Copy>(&self, offset: usize, value: T) {
        unsafe { write_volatile((self.mmio + offset) as *mut T, value) }
    }

    fn write_base(&self, reg: usize, offset: usize) {
        let address = self.dma().phys() + offset as u64;
        self.write(reg + 4, (address >> 32) as u32);
        self.write(reg, address as u32);
    }

    fn reset_controller(&self) -> Result<u16, &'static str> {
        self.write(reg::INTCTL, 0u32);
        self.write(reg::WAKEEN, 0u16);
        self.write(reg::STATESTS, 0x7fffu16);
        self.write(reg::GCTL, 0u32);
        if !wait_us(10_000, || self.read::<u32>(reg::GCTL) & 1 == 0) {
            return Err("HDA controller failed to enter reset");
        }
        delay_us(100);
        self.write(reg::GCTL, 1u32);
        if !wait_us(10_000, || self.read::<u32>(reg::GCTL) & 1 != 0) {
            return Err("HDA controller failed to leave reset");
        }
        self.write(reg::SSYNC, 0u32);
        // Some Intel controllers only keep LPIB updated with the table enabled.
        // Program its base before starting any DMA engine.
        self.write_base(reg::DPLBASE, POSITION_OFFSET);
        self.write(reg::DPLBASE, self.read::<u32>(reg::DPLBASE) | 1);
        // The first 25 link frames are reserved for codec initialization.
        delay_us(1_000);
        if !wait_us(50_000, || self.read::<u16>(reg::STATESTS) & 0x7fff != 0) {
            return Err("no HDA codec responded to reset");
        }
        Ok(self.read::<u16>(reg::STATESTS) & 0x7fff)
    }

    fn configure_command_dma(&mut self) -> Result<(), &'static str> {
        self.write(reg::CORBCTL, 0u8);
        self.write(reg::RIRBCTL, 0u8);
        self.corb_entries = self.set_ring_size(reg::CORBSIZE)?;
        self.rirb_entries = self.set_ring_size(reg::RIRBSIZE)?;
        self.write_base(reg::CORBLBASE, CORB_OFFSET);
        self.write_base(reg::RIRBLBASE, RIRB_OFFSET);
        self.write(reg::CORBRP, 1u16 << 15);
        // Some controllers self-clear CORBRPRST. In both cases the pointer
        // must read zero after the reset handshake.
        delay_us(10);
        self.write(reg::CORBRP, 0u16);
        if !wait_us(1_000, || self.read::<u16>(reg::CORBRP) == 0) {
            return Err("HDA command ring pointer reset failed");
        }
        self.write(reg::CORBWP, 0u16);
        self.write(reg::RIRBWP, 1u16 << 15);
        self.write(reg::RINTCNT, 1u16);
        self.write(reg::CORBSTS, 1u8);
        self.write(reg::RIRBSTS, 5u8);

        let dev = &self.device;
        let command = pci::config_read_u16(dev.bus, dev.slot, dev.function, PCI_COMMAND);
        pci::config_write_u16(
            dev.bus,
            dev.slot,
            dev.function,
            PCI_COMMAND,
            command | BUS_MASTER | INTX_DISABLE,
        );
        fence(Ordering::SeqCst);
        self.write(reg::RIRBCTL, 2u8);
        self.write(reg::CORBCTL, 2u8);
        Ok(())
    }

    fn set_ring_size(&self, register: usize) -> Result<u16, &'static str> {
        let capabilities = self.read::<u8>(register);
        let (selector, entries) = [(2, 256), (1, 16), (0, 2)]
            .into_iter()
            .find(|&(selector, _)| capabilities & (0x10 << selector) != 0)
            .ok_or("HDA codec ring reports no supported size")?;
        self.write(register, selector);
        Ok(entries)
    }

    fn command(&mut self, codec: u8, nid: u8, payload: u32) -> Result<u32, &'static str> {
        let command = (u32::from(codec) << 28) | (u32::from(nid) << 20) | payload;
        let write_index = (self.read::<u16>(reg::CORBWP) + 1) % self.corb_entries;
        let corb = unsafe { self.dma().virt().add(CORB_OFFSET).cast::<u32>() };
        unsafe { write_volatile(corb.add(usize::from(write_index)), command) };
        fence(Ordering::Release);
        self.write(reg::CORBWP, write_index);

        if !wait_us(10_000, || {
            self.read::<u16>(reg::RIRBWP) & 0xff != self.rirb_read
        }) {
            return Err("HDA codec response timed out");
        }
        self.rirb_read = (self.rirb_read + 1) % self.rirb_entries;
        let rirb = unsafe { self.dma().virt().add(RIRB_OFFSET).cast::<u64>() };
        fence(Ordering::Acquire);
        let response = unsafe { read_volatile(rirb.add(usize::from(self.rirb_read))) };
        if (response >> 32) & 0x1f != u64::from(codec) {
            return Err("unexpected HDA codec response");
        }
        if self.read::<u8>(reg::RIRBSTS) & 4 != 0 {
            return Err("HDA codec response ring overrun");
        }
        self.write(reg::RIRBSTS, 1u8);
        Ok(response as u32)
    }

    fn verb(&mut self, codec: u8, nid: u8, verb: u32, data: u8) -> Result<u32, &'static str> {
        self.command(codec, nid, (verb << 8) | u32::from(data))
    }

    fn parameter(&mut self, codec: u8, nid: u8, param: u8) -> Result<u32, &'static str> {
        self.verb(codec, nid, verb::GET_PARAMETER, param)
    }

    fn power_on(&mut self, codec: u8, nid: u8) -> Result<(), &'static str> {
        self.verb(codec, nid, verb::SET_POWER, 0)?;
        let deadline = embassy_time_driver::now().saturating_add(ticks_for_us(200_000));
        loop {
            let power = self.verb(codec, nid, verb::GET_POWER, 0)?;
            if power & 0x100 != 0 {
                return Err("HDA codec rejected D0");
            }
            if power & 0xf0 == 0 {
                return Ok(());
            }
            if embassy_time_driver::now() >= deadline {
                return Err("HDA codec did not reach D0");
            }
            delay_us(100);
        }
    }

    fn find_output(&mut self, codecs: u16) -> Result<Option<Output>, &'static str> {
        let mut best: Option<Output> = None;
        for codec in (0..15).filter(|codec| codecs & (1 << codec) != 0) {
            let groups = node_range(self.parameter(codec, 0, NODE_COUNT)?);
            for group in groups {
                if self.parameter(codec, group, FUNCTION_TYPE)? & 0xff != 1 {
                    continue;
                }
                self.power_on(codec, group)?;
                let widgets = self.read_widgets(codec, group)?;
                let Some((path, priority)) = find_route(&widgets) else {
                    continue;
                };
                if priority == 1 {
                    return Ok(Some(Output {
                        codec,
                        path,
                        widgets,
                        priority,
                    }));
                }
                if best
                    .as_ref()
                    .is_none_or(|output| priority < output.priority)
                {
                    best = Some(Output {
                        codec,
                        path,
                        widgets,
                        priority,
                    });
                }
            }
        }
        Ok(best)
    }

    fn read_widgets(&mut self, codec: u8, group: u8) -> Result<Vec<Widget>, &'static str> {
        let mut widgets = Vec::new();
        let input_amp = self.parameter(codec, group, INPUT_AMP)?;
        let output_amp = self.parameter(codec, group, OUTPUT_AMP)?;
        let rates = self.parameter(codec, group, PCM_RATES)?;
        let formats = self.parameter(codec, group, STREAM_FORMATS)?;
        let nodes = node_range(self.parameter(codec, group, NODE_COUNT)?);

        for nid in nodes {
            let caps = self.parameter(codec, nid, WIDGET_CAPS)?;
            let kind = (caps >> 20) & 0xf;
            let amp_override = caps & (1 << 3) != 0;
            let input = if amp_override && caps & (1 << 1) != 0 {
                self.parameter(codec, nid, INPUT_AMP)?
            } else {
                input_amp
            };
            let output = if amp_override && caps & (1 << 2) != 0 {
                self.parameter(codec, nid, OUTPUT_AMP)?
            } else {
                output_amp
            };
            let pcm_output = if kind == 0 && caps & 1 != 0 && caps & (1 << 9) == 0 {
                let (rates, formats) = if caps & (1 << 4) != 0 {
                    (
                        self.parameter(codec, nid, PCM_RATES)?,
                        self.parameter(codec, nid, STREAM_FORMATS)?,
                    )
                } else {
                    (rates, formats)
                };
                formats & 1 != 0 && rates & ((1 << 17) | (1 << 6)) == (1 << 17) | (1 << 6)
            } else {
                false
            };
            let (pin_caps, pin_priority) = if kind == 4 {
                let config = self.verb(codec, nid, verb::GET_PIN_CONFIG, 0)?;
                let pin_caps = self.parameter(codec, nid, PIN_CAPS)?;
                let priority = match ((config >> 30) & 3, (config >> 20) & 0xf) {
                    (1, _) => u8::MAX,
                    (_, 1) => 1, // Speaker
                    (_, 0) => 2, // Line out
                    (_, 2) => 3, // Headphone
                    _ => u8::MAX,
                };
                (pin_caps, priority)
            } else {
                (0, u8::MAX)
            };
            let connections = if caps & (1 << 8) != 0 {
                self.connections(codec, nid)?
            } else {
                Vec::new()
            };
            widgets.push(Widget {
                nid,
                caps,
                connections,
                pin_caps,
                pin_priority,
                pcm_output,
                input_amp: unity_gain(input),
                output_amp: unity_gain(output),
            });
        }
        Ok(widgets)
    }

    fn connections(&mut self, codec: u8, nid: u8) -> Result<Vec<u8>, &'static str> {
        let config = self.parameter(codec, nid, CONNECTION_LIST_LEN)?;
        let width = if config & 0x80 != 0 { 16 } else { 8 };
        let count = (config & 0x7f) as usize;
        let per_response = 32 / width;
        let mask = (1u32 << (width - 1)) - 1;
        let mut connections = Vec::new();
        let mut previous = 0;
        for offset in (0..count).step_by(per_response) {
            let entries = self.verb(codec, nid, verb::GET_CONNECTION_LIST, offset as u8)?;
            for index in 0..per_response.min(count - offset) {
                let entry = entries >> (index * width);
                let target = (entry & mask) as u8;
                if entry & (1 << (width - 1)) == 0 {
                    connections.push(target);
                } else if target > previous {
                    connections.extend(previous + 1..=target);
                }
                previous = target;
            }
        }
        Ok(connections)
    }

    fn configure_output(&mut self, output: &Output) -> Result<(), &'static str> {
        let codec = output.codec;
        for &index in &output.path {
            let widget = &output.widgets[index];
            if widget.caps & (1 << 10) != 0 {
                self.power_on(codec, widget.nid)?;
            }
        }
        for (position, &index) in output.path.iter().enumerate() {
            let widget = &output.widgets[index];
            if let Some(&next) = output.path.get(position + 1) {
                let next_nid = output.widgets[next].nid;
                let input = widget
                    .connections
                    .iter()
                    .position(|&nid| nid == next_nid)
                    .ok_or("HDA route is missing a connection")?;
                if widget.kind() != 2 && widget.connections.len() > 1 {
                    self.verb(codec, widget.nid, verb::SET_CONNECT, input as u8)?;
                }
                if widget.caps & (1 << 1) != 0 {
                    self.command(
                        codec,
                        widget.nid,
                        verb::SET_AMP
                            | 0x7000
                            | ((input as u32) << 8)
                            | u32::from(widget.input_amp),
                    )?;
                }
            }
            if widget.caps & (1 << 2) != 0 {
                self.command(
                    codec,
                    widget.nid,
                    verb::SET_AMP | 0xb000 | u32::from(widget.output_amp),
                )?;
            }
            if widget.pcm_output {
                self.verb(codec, widget.nid, verb::SET_STREAM, STREAM_TAG << 4)?;
                self.command(codec, widget.nid, verb::SET_FORMAT | u32::from(PCM_FORMAT))?;
            }
        }
        let pin = &output.widgets[output.path[0]];
        let headphone_amp = if pin.pin_priority == 3 && pin.pin_caps & (1 << 3) != 0 {
            0x80
        } else {
            0
        };
        self.verb(codec, pin.nid, verb::SET_PIN_CONTROL, 0x40 | headphone_amp)?;
        if pin.pin_caps & (1 << 16) != 0 {
            self.verb(codec, pin.nid, verb::SET_EAPD, 2)?;
        }
        crate::log_info!(target: "global"; "hda: analog PCM output codec={} pin=0x{:02x}\n",
            codec, pin.nid);
        Ok(())
    }

    fn program_stream(&mut self) -> Result<(), &'static str> {
        let control = self.stream + sd::CTL;
        self.write(control, 0u8);
        if !wait_us(1_000, || self.read::<u8>(control) & sd::RUN == 0) {
            return Err("HDA playback stream failed to stop");
        }
        self.write(control, sd::RESET);
        if !wait_us(1_000, || self.read::<u8>(control) & sd::RESET != 0) {
            return Err("HDA playback stream failed to enter reset");
        }
        self.write(control, 0u8);
        if !wait_us(1_000, || self.read::<u8>(control) & sd::RESET == 0) {
            return Err("HDA playback stream failed to leave reset");
        }

        let bdl = self.dma().virt().cast::<BufferDescriptor>();
        for period in 0..PERIODS {
            let address = self.dma().phys() + (PCM_OFFSET + period * PERIOD_BYTES) as u64;
            unsafe {
                bdl.add(period).write(BufferDescriptor {
                    address,
                    length: PERIOD_BYTES as u32,
                    flags: 0,
                })
            };
        }
        self.write(self.stream + sd::STS, sd::CLEAR_STATUS);
        self.write(self.stream + sd::CBL, RING_BYTES as u32);
        self.write(self.stream + sd::LVI, (PERIODS - 1) as u16);
        self.write(self.stream + sd::FMT, PCM_FORMAT);
        self.write_base(self.stream + sd::BDLPL, 0);
        self.write(control + 2, STREAM_TAG << 4);
        if usize::from(self.read::<u16>(self.stream + sd::FIFOS)) + 1 >= PERIOD_BYTES {
            return Err("HDA stream FIFO exceeds playback period");
        }
        self.last_poll = None;
        self.next_period = 0;
        Ok(())
    }

    /// Reuse periods already transmitted on the link. Keep a period clear of
    /// the read cursor to account for data the controller prefetches into its FIFO.
    pub(super) fn refill(
        &mut self,
        mut render: impl FnMut(&mut [[i16; 2]]),
    ) -> Result<(), &'static str> {
        let mut frames = [[0; 2]; FRAMES_PER_PERIOD];
        let current_tick = embassy_time_driver::now();
        let Some(last_poll) = self.last_poll else {
            for period in 0..PERIODS {
                render(&mut frames);
                self.write_period(period, &frames);
            }
            let started_at = embassy_time_driver::now();
            self.last_poll = Some(started_at);
            self.last_progress = started_at;
            self.last_position = 0;
            self.write(self.stream + sd::CTL, sd::RUN);
            return Ok(());
        };

        if self.read::<u8>(self.stream + sd::STS) & sd::ERRORS != 0 {
            return Err("HDA playback DMA reported an error");
        }
        if current_tick.saturating_sub(last_poll) >= ticks_for_us(MAX_POLL_GAP_US) {
            // A delayed poll makes the hardware's wrap ambiguous. Re-prime
            // from fresh audio rather than touching periods DMA could be reading.
            self.program_stream()?;
            return Ok(());
        }
        let position = self.current_position()?;
        if position != self.last_position {
            self.last_position = position;
            self.last_progress = current_tick;
        } else if current_tick.saturating_sub(self.last_progress) >= ticks_for_us(CURSOR_STALL_US) {
            return Err("HDA playback DMA cursor stalled");
        }
        let current_period = position / PERIOD_BYTES;
        while self.next_period != current_period {
            let period = self.next_period;
            render(&mut frames);
            let now = self.current_position()? / PERIOD_BYTES;
            if period == now
                || period == (now + 1) % PERIODS
                || embassy_time_driver::now().saturating_sub(current_tick)
                    >= ticks_for_us(MAX_POLL_GAP_US)
            {
                self.program_stream()?;
                return Ok(());
            }
            self.write_period(period, &frames);
            self.next_period = (period + 1) % PERIODS;
        }
        self.last_poll = Some(current_tick);
        Ok(())
    }

    fn current_position(&self) -> Result<usize, &'static str> {
        let position = self.read::<u32>(self.stream + sd::LPIB) as usize;
        if position > RING_BYTES {
            return Err("HDA playback DMA cursor is outside its buffer");
        }
        Ok(position % RING_BYTES)
    }

    fn write_period(&self, period: usize, frames: &[[i16; 2]; FRAMES_PER_PERIOD]) {
        let offset = PCM_OFFSET + period * PERIOD_BYTES;
        let cursor = unsafe { self.dma().virt().add(offset).cast::<[i16; 2]>() };
        unsafe { copy_nonoverlapping(frames.as_ptr(), cursor, FRAMES_PER_PERIOD) };
        fence(Ordering::SeqCst);
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        if self.stream != 0 {
            self.write(reg::INTCTL, 0u32);
            self.write(reg::GCTL, 0u32);
        }
        let device = &self.device;
        let command = pci::config_read_u16(device.bus, device.slot, device.function, PCI_COMMAND);
        pci::config_write_u16(
            device.bus,
            device.slot,
            device.function,
            PCI_COMMAND,
            (command & !BUS_MASTER) | INTX_DISABLE,
        );
        if self.stream != 0 && !wait_us(10_000, || self.read::<u32>(reg::GCTL) & 1 == 0) {
            // Retain the allocation if the controller failed to acknowledge reset.
            if let Some(dma) = self.dma.take() {
                core::mem::forget(dma);
            }
        }
    }
}

fn node_range(response: u32) -> core::ops::Range<u8> {
    let start = ((response >> 16) & 0x7f) as u8;
    let count = (response & 0xff) as u8;
    start..start.saturating_add(count).min(128)
}

fn unity_gain(caps: u32) -> u16 {
    ((caps & 0x7f).min((caps >> 8) & 0x7f)) as u16
}

fn find_route(widgets: &[Widget]) -> Option<(Vec<usize>, u8)> {
    for priority in 1..=3 {
        for (index, widget) in widgets.iter().enumerate() {
            if widget.pin_priority != priority || widget.pin_caps & (1 << 4) == 0 {
                continue;
            }
            let mut visited = [false; 128];
            let mut path = Vec::new();
            if trace_output(widgets, index, &mut visited, &mut path) {
                return Some((path, priority));
            }
        }
    }
    None
}

fn trace_output(
    widgets: &[Widget],
    index: usize,
    visited: &mut [bool; 128],
    path: &mut Vec<usize>,
) -> bool {
    let widget = &widgets[index];
    if visited[usize::from(widget.nid)] {
        return false;
    }
    visited[usize::from(widget.nid)] = true;
    path.push(index);
    if widget.pcm_output {
        return true;
    }
    for (input, &target) in widget.connections.iter().enumerate() {
        if input >= 16 && widget.caps & (1 << 1) != 0 {
            break;
        }
        if let Some(next) = widgets.iter().position(|widget| widget.nid == target) {
            if matches!(widgets[next].kind(), 0 | 2 | 3)
                && trace_output(widgets, next, visited, path)
            {
                return true;
            }
        }
    }
    path.pop();
    false
}

fn ticks_for_us(us: u64) -> u64 {
    (us * embassy_time_driver::TICK_HZ).div_ceil(1_000_000)
}

fn wait_us(us: u64, mut check: impl FnMut() -> bool) -> bool {
    let deadline = embassy_time_driver::now().saturating_add(ticks_for_us(us));
    while !check() {
        if embassy_time_driver::now() >= deadline {
            return false;
        }
        core::hint::spin_loop();
    }
    true
}

fn delay_us(us: u64) {
    wait_us(us, || false);
}
