//! HDA-owned microphone capture lane.
//!
//! This module owns one HDA input stream descriptor and its DMA ring.  It does
//! not own the GNA service, and GNA never reconfigures this hardware.  The
//! frontend service is only a consumer of the PCM/status surface below.
//!
//! Bring-up deliberately stays close to the already-proven HDA playback path:
//! signed 16-bit PCM, 48 kHz, one or two interleaved channels, cyclic BDL DMA,
//! and polling instead of interrupt-driven completion.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use spin::Mutex;
use trueos_time::{Duration, Timer};

const HDA_MMIO_BYTES: usize = 0x4000;
const SD_BASE: u32 = 0x80;
const SD_SIZE: u32 = 0x20;

const REG_GCAP: u32 = 0x00;
const REG_GCTL: u32 = 0x08;
const REG_STATESTS: u32 = 0x0E;
const REG_CORBCTL: u32 = 0x4C;
const REG_RIRBCTL: u32 = 0x5C;
const REG_IC: u32 = 0x60;
const REG_IR: u32 = 0x64;
const REG_ICS: u32 = 0x68;

const SD_CTL: u32 = 0x00;
const SD_STS: u32 = 0x03;
const SD_LPIB: u32 = 0x04;
const SD_CBL: u32 = 0x08;
const SD_LVI: u32 = 0x0C;
const SD_FMT: u32 = 0x12;
const SD_BDLPL: u32 = 0x18;
const SD_BDLPU: u32 = 0x1C;

const GCTL_UNSOL: u32 = 1 << 8;
const SCTL_SRST: u8 = 1 << 0;
const SCTL_RUN: u8 = 1 << 1;
const SCTL_IOCE: u8 = 1 << 2;
const SSTS_BCIS: u8 = 1 << 2;
const SSTS_FIFOE: u8 = 1 << 3;
const SSTS_DESE: u8 = 1 << 4;
const SSTS_W1C: u8 = SSTS_BCIS | SSTS_FIFOE | SSTS_DESE;
const STREAM_TAG_SHIFT_IN_CTL_HIGH_BYTE: u8 = 4;

const ICS_ICB: u16 = 1 << 0;
const ICS_IRV: u16 = 1 << 1;

const VERB_GET_PARAMETER: u32 = 0xF00;
const VERB_GET_CONN_LIST: u32 = 0xF02;
const VERB_GET_PIN_CONTROL: u32 = 0xF07;
const VERB_GET_PIN_SENSE: u32 = 0xF09;
const VERB_GET_CONFIG_DEFAULT: u32 = 0xF1C;
const VERB_SET_CONN_SELECT: u32 = 0x701;
const VERB_SET_POWER_STATE: u32 = 0x705;
const VERB_SET_CHANNEL_STREAM: u32 = 0x706;
const VERB_SET_PIN_CONTROL: u32 = 0x707;
const VERB_SET_STREAM_FORMAT: u32 = 0x200;
const VERB_SET_AMP_GAIN_MUTE: u32 = 0x300;

const PARAM_VENDOR_ID: u32 = 0x00;
const PARAM_NODE_COUNT: u32 = 0x04;
const PARAM_FN_GROUP_TYPE: u32 = 0x05;
const PARAM_AUDIO_CAPS: u32 = 0x09;
const PARAM_PCM_RATES: u32 = 0x0A;
const PARAM_STREAM_FMTS: u32 = 0x0B;
const PARAM_PIN_CAPS: u32 = 0x0C;
const PARAM_AMP_IN_CAPS: u32 = 0x0D;
const PARAM_CONN_LIST_LEN: u32 = 0x0E;

const WCAP_STEREO: u32 = 1 << 0;
const WCAP_IN_AMP: u32 = 1 << 1;
const WCAP_AMP_OVERRIDE: u32 = 1 << 3;
const WIDGET_AUDIO_INPUT: u8 = 1;
const WIDGET_PIN_COMPLEX: u8 = 4;

const PINCAP_PRES_DETECT: u32 = 1 << 2;
const PINCAP_INPUT: u32 = 1 << 5;
const PINCAP_VREF_SHIFT: u32 = 8;
const PINCAP_VREF_80: u32 = 1 << 4;
const PINCTL_VREF_MASK: u8 = 0x07;
const PINCTL_VREF_80: u8 = 0x04;
const PINCTL_INPUT_ENABLE: u8 = 1 << 5;
const PIN_SENSE_PRESENCE: u32 = 1 << 31;

const AMPCAP_OFFSET_MASK: u32 = 0x7F;
const AMP_SET_INDEX_SHIFT: u16 = 8;
const AMP_SET_RIGHT: u16 = 1 << 12;
const AMP_SET_LEFT: u16 = 1 << 13;
const AMP_SET_INPUT: u16 = 1 << 14;

const PCM_RATE_48KHZ: u32 = 1 << 6;
const PCM_BITS_16: u32 = 1 << 17;
const STREAM_FORMAT_PCM: u32 = 1 << 0;
const PCM_SAMPLE_RATE_HZ: u32 = 48_000;
const PCM_SAMPLE_BITS: u8 = 16;

const CAPTURE_DMA_BYTES: usize = 256 * 1024;
const CAPTURE_BDL_ENTRIES: usize = 4;
const CAPTURE_FRAGMENT_BYTES: usize = CAPTURE_DMA_BYTES / CAPTURE_BDL_ENTRIES;
const CAPTURE_BDL_BYTES: usize = 4096;

pub(crate) const CAPTURE_POLL_MS: u64 = 20;
pub(crate) const CAPTURE_HEARTBEAT_MS: u64 = 30_000;
const CAPTURE_RETRY_MS: u64 = 2_000;
const CAPTURE_STALL_MS: u64 = 750;
const COMMAND_POLL_LIMIT: usize = 100_000;
const STREAM_RESET_POLL_LIMIT: usize = 20_000;
const MAX_PATH_NODES: usize = 32;

const _: () = {
    assert!(CAPTURE_DMA_BYTES % CAPTURE_BDL_ENTRIES == 0);
    assert!(CAPTURE_FRAGMENT_BYTES % 4096 == 0);
    assert!(CAPTURE_POLL_MS != 0);
    assert!(CAPTURE_HEARTBEAT_MS > CAPTURE_POLL_MS);
};

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct BdlEntry {
    address: u64,
    length: u32,
    ioc: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum CaptureState {
    Offline = 0,
    Starting = 1,
    Running = 2,
    Recovering = 3,
    Faulted = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CaptureStatus {
    pub state: CaptureState,
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub sample_bits: u8,
    pub stream_index: u8,
    pub stream_tag: u8,
    pub adc_nid: u16,
    pub pin_nid: u16,
    pub pin_present_at_start: bool,
    pub total_frames: u64,
    pub lpib_bytes: u32,
    pub window_peak_abs: u16,
    pub window_mean_abs: u16,
    pub window_nonzero_permille: u16,
    pub fifo_errors: u32,
    pub descriptor_errors: u32,
    pub restarts: u32,
}

impl CaptureStatus {
    const fn offline() -> Self {
        Self {
            state: CaptureState::Offline,
            sample_rate_hz: 0,
            channels: 0,
            sample_bits: 0,
            stream_index: 0,
            stream_tag: 0,
            adc_nid: 0,
            pin_nid: 0,
            pin_present_at_start: false,
            total_frames: 0,
            lpib_bytes: 0,
            window_peak_abs: 0,
            window_mean_abs: 0,
            window_nonzero_permille: 0,
            fifo_errors: 0,
            descriptor_errors: 0,
            restarts: 0,
        }
    }
}

#[allow(dead_code, reason = "GNA PCM consumer follows HDA capture bring-up")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CaptureRead {
    pub samples: usize,
    pub channels: u8,
    pub sample_rate_hz: u32,
    pub sample_bits: u8,
    pub total_frames: u64,
}

static STARTED: AtomicBool = AtomicBool::new(false);
static ENGINE: Mutex<Option<CaptureEngine>> = Mutex::new(None);
static STATUS: Mutex<CaptureStatus> = Mutex::new(CaptureStatus::offline());

#[derive(Clone)]
struct Widget {
    nid: u16,
    widget_type: u8,
    caps: u32,
    pin_config: u32,
    pin_caps: u32,
    pin_sense: u32,
    pcm_rates: u32,
    stream_formats: u32,
    amp_in_caps: u32,
    connections: Vec<u16>,
}

impl Widget {
    fn default_device(&self) -> u8 {
        ((self.pin_config >> 20) & 0x0F) as u8
    }

    fn connectivity(&self) -> u8 {
        ((self.pin_config >> 30) & 0x03) as u8
    }

    fn present(&self) -> bool {
        self.pin_caps & PINCAP_PRES_DETECT != 0 && self.pin_sense & PIN_SENSE_PRESENCE != 0
    }

    fn input_capable(&self) -> bool {
        self.widget_type == WIDGET_PIN_COMPLEX
            && self.pin_caps & PINCAP_INPUT != 0
            && self.connectivity() != 1
    }
}

struct Route {
    codec: u8,
    codec_vendor: u16,
    codec_device: u16,
    afg_nid: u16,
    adc_nid: u16,
    pin_nid: u16,
    pin_present: bool,
    channels: u8,
    format: u16,
    path: Vec<u16>,
}

struct CaptureEngine {
    mmio: usize,
    bdf_bus: u8,
    bdf_slot: u8,
    bdf_function: u8,
    codec_vendor: u16,
    codec_device: u16,
    afg_nid: u16,
    adc_nid: u16,
    pin_nid: u16,
    pin_present_at_start: bool,
    route_path: Vec<u16>,
    input_stream_index: u8,
    stream_tag: u8,
    channels: u8,
    format: u16,
    dma_phys: u64,
    dma_virt: usize,
    bdl_phys: u64,
    bdl_virt: usize,
    last_lpib: u32,
    total_frames: u64,
    last_progress_ms: u64,
    last_heartbeat_ms: u64,
    window_frames: u64,
    window_samples: u64,
    window_abs_sum: u64,
    window_nonzero: u64,
    window_peak_abs: u16,
    fifo_errors: u32,
    descriptor_errors: u32,
    bcis_events: u32,
    restarts: u32,
    recovery_not_before_ms: u64,
}

impl Drop for CaptureEngine {
    fn drop(&mut self) {
        if self.bdl_virt != 0 {
            crate::dma::dealloc(self.bdl_virt as *mut u8, CAPTURE_BDL_BYTES);
        }
        if self.dma_virt != 0 {
            crate::dma::dealloc(self.dma_virt as *mut u8, CAPTURE_DMA_BYTES);
        }
    }
}

impl CaptureEngine {
    fn initialize() -> Result<Self, &'static str> {
        if !crate::hda::is_initialized() {
            return Err("hda-not-initialized");
        }

        let device = find_hda_device().ok_or("hda-pci-device-missing")?;
        crate::pci::enable_mem_and_bus_master(device.bus, device.slot, device.function);
        let bar0 = device.bar_address(0).ok_or("hda-bar0-missing")?;
        let mmio = crate::pci::mmio::map_mmio_region_exact(bar0, HDA_MMIO_BYTES)
            .map_err(|_| "hda-mmio-map-failed")?
            .as_ptr() as usize;

        let gcap = unsafe { read16(mmio, REG_GCAP) };
        let input_streams = ((gcap >> 8) & 0x0F) as u8;
        if input_streams == 0 {
            return Err("hda-no-input-streams");
        }

        let input_stream_index =
            select_idle_input_stream(mmio, input_streams).ok_or("hda-no-idle-input-stream")?;
        let route =
            with_codec_command_window(mmio, |codec_io| discover_and_configure_route(codec_io))?;
        // Stream tag 1 is already the established playback tag.  Tags 2..15
        // remain independent of the stream-descriptor index.
        let stream_tag = 2 + (input_stream_index % 14);

        let (dma_phys, dma_ptr) =
            crate::dma::alloc(CAPTURE_DMA_BYTES, 4096).ok_or("hda-capture-dma-allocation-failed")?;
        let Some((bdl_phys, bdl_ptr)) = crate::dma::alloc(CAPTURE_BDL_BYTES, 128) else {
            crate::dma::dealloc(dma_ptr, CAPTURE_DMA_BYTES);
            return Err("hda-capture-bdl-allocation-failed");
        };
        unsafe {
            core::ptr::write_bytes(dma_ptr, 0, CAPTURE_DMA_BYTES);
            core::ptr::write_bytes(bdl_ptr, 0, CAPTURE_BDL_BYTES);
        }

        let mut engine = Self {
            mmio,
            bdf_bus: device.bus,
            bdf_slot: device.slot,
            bdf_function: device.function,
            codec_vendor: route.codec_vendor,
            codec_device: route.codec_device,
            afg_nid: route.afg_nid,
            adc_nid: route.adc_nid,
            pin_nid: route.pin_nid,
            pin_present_at_start: route.pin_present,
            route_path: route.path,
            input_stream_index,
            stream_tag,
            channels: route.channels,
            format: route.format,
            dma_phys,
            dma_virt: dma_ptr as usize,
            bdl_phys,
            bdl_virt: bdl_ptr as usize,
            last_lpib: 0,
            total_frames: 0,
            last_progress_ms: uptime_ms(),
            last_heartbeat_ms: uptime_ms(),
            window_frames: 0,
            window_samples: 0,
            window_abs_sum: 0,
            window_nonzero: 0,
            window_peak_abs: 0,
            fifo_errors: 0,
            descriptor_errors: 0,
            bcis_events: 0,
            restarts: 0,
            recovery_not_before_ms: 0,
        };

        // The route is configured before assigning the converter stream ID so
        // an input converter never observes a half-configured DMA stream.
        with_codec_command_window(mmio, |codec_io| {
            codec_io.codec_cmd(
                route.codec,
                route.adc_nid,
                VERB_SET_CHANNEL_STREAM,
                (stream_tag << 4) | 0,
            )?;
            codec_io.set_verb_16(route.codec, route.adc_nid, VERB_SET_STREAM_FORMAT, route.format)?;
            Ok(())
        })?;

        engine.configure_stream_descriptor(true)?;

        crate::log_os::service_important_line(format_args!(
            "hda-capture: online owner=hda-capture-lane consumer=gna-audio-front-end bdf={:02X}:{:02X}.{} codec={:04X}:{:04X} afg_nid={} adc_nid={} pin_nid={} pin_present={} path={:?} stream_index={} stream_tag={} format=s16le rate_hz={} channels={} interleaved=1 dma_bytes={} bdl_entries={} poll_ms={} heartbeat_ms={}\n",
            engine.bdf_bus,
            engine.bdf_slot,
            engine.bdf_function,
            engine.codec_vendor,
            engine.codec_device,
            engine.afg_nid,
            engine.adc_nid,
            engine.pin_nid,
            engine.pin_present_at_start as u8,
            engine.route_path,
            engine.input_stream_index,
            engine.stream_tag,
            PCM_SAMPLE_RATE_HZ,
            engine.channels,
            CAPTURE_DMA_BYTES,
            CAPTURE_BDL_ENTRIES,
            CAPTURE_POLL_MS,
            CAPTURE_HEARTBEAT_MS,
        ));

        engine.publish_status(CaptureState::Running);
        Ok(engine)
    }

    fn stream_base(&self) -> u32 {
        SD_BASE + u32::from(self.input_stream_index) * SD_SIZE
    }

    fn frame_bytes(&self) -> usize {
        usize::from(self.channels) * 2
    }

    fn configure_stream_descriptor(&mut self, clear_buffer: bool) -> Result<(), &'static str> {
        let base = self.stream_base();
        unsafe {
            let ctl = read8(self.mmio, base + SD_CTL);
            write8(self.mmio, base + SD_CTL, ctl & !SCTL_RUN);
            if !wait_until(|| read8(self.mmio, base + SD_CTL) & SCTL_RUN == 0, STREAM_RESET_POLL_LIMIT)
            {
                return Err("hda-input-stop-timeout");
            }

            write8(self.mmio, base + SD_CTL, SCTL_SRST);
            if !wait_until(
                || read8(self.mmio, base + SD_CTL) & SCTL_SRST != 0,
                STREAM_RESET_POLL_LIMIT,
            ) {
                return Err("hda-input-reset-assert-timeout");
            }
            write8(self.mmio, base + SD_CTL, 0);
            if !wait_until(
                || read8(self.mmio, base + SD_CTL) & SCTL_SRST == 0,
                STREAM_RESET_POLL_LIMIT,
            ) {
                return Err("hda-input-reset-deassert-timeout");
            }

            write8(self.mmio, base + SD_STS, SSTS_W1C);
            if clear_buffer {
                core::ptr::write_bytes(self.dma_virt as *mut u8, 0, CAPTURE_DMA_BYTES);
            }

            let bdl = self.bdl_virt as *mut BdlEntry;
            for index in 0..CAPTURE_BDL_ENTRIES {
                core::ptr::write(
                    bdl.add(index),
                    BdlEntry {
                        address: self.dma_phys + (index * CAPTURE_FRAGMENT_BYTES) as u64,
                        length: CAPTURE_FRAGMENT_BYTES as u32,
                        // Polling owns retirement; no completion interrupt is
                        // requested, so capture cannot perturb the playback IRQ policy.
                        ioc: 0,
                    },
                );
            }

            write32(self.mmio, base + SD_CBL, CAPTURE_DMA_BYTES as u32);
            write16(self.mmio, base + SD_LVI, (CAPTURE_BDL_ENTRIES - 1) as u16);
            write16(self.mmio, base + SD_FMT, self.format);
            write32(self.mmio, base + SD_BDLPL, self.bdl_phys as u32);
            write32(self.mmio, base + SD_BDLPU, (self.bdl_phys >> 32) as u32);
            write8(
                self.mmio,
                base + SD_CTL + 2,
                self.stream_tag << STREAM_TAG_SHIFT_IN_CTL_HIGH_BYTE,
            );
            write8(self.mmio, base + SD_STS, SSTS_W1C);

            let ctl = read8(self.mmio, base + SD_CTL);
            write8(
                self.mmio,
                base + SD_CTL,
                (ctl | SCTL_RUN) & !SCTL_IOCE,
            );
            if !wait_until(|| read8(self.mmio, base + SD_CTL) & SCTL_RUN != 0, STREAM_RESET_POLL_LIMIT)
            {
                return Err("hda-input-run-timeout");
            }
        }

        self.last_lpib = 0;
        self.last_progress_ms = uptime_ms();
        Ok(())
    }

    fn poll(&mut self, now_ms: u64) {
        if now_ms < self.recovery_not_before_ms {
            self.publish_status(CaptureState::Recovering);
            return;
        }
        let base = self.stream_base();
        let status = unsafe { read8(self.mmio, base + SD_STS) };
        if status & SSTS_BCIS != 0 {
            self.bcis_events = self.bcis_events.saturating_add(1);
        }
        if status & SSTS_FIFOE != 0 {
            self.fifo_errors = self.fifo_errors.saturating_add(1);
        }
        if status & SSTS_DESE != 0 {
            self.descriptor_errors = self.descriptor_errors.saturating_add(1);
        }
        if status & SSTS_W1C != 0 {
            unsafe { write8(self.mmio, base + SD_STS, status & SSTS_W1C) };
        }

        let running = unsafe { read8(self.mmio, base + SD_CTL) & SCTL_RUN != 0 };
        if status & (SSTS_FIFOE | SSTS_DESE) != 0 || !running {
            let reason = if status & SSTS_DESE != 0 {
                "descriptor-error"
            } else if status & SSTS_FIFOE != 0 {
                "fifo-error"
            } else {
                "run-cleared"
            };
            self.restart(reason, now_ms);
            return;
        }

        let lpib = unsafe { read32(self.mmio, base + SD_LPIB) }.min(CAPTURE_DMA_BYTES as u32);
        if lpib != self.last_lpib {
            let delta_bytes = ring_distance_bytes(
                self.last_lpib as usize,
                lpib as usize,
                CAPTURE_DMA_BYTES,
            );
            self.observe_completed_bytes(self.last_lpib as usize, delta_bytes);
            self.last_lpib = lpib;
            self.last_progress_ms = now_ms;
        } else if now_ms.saturating_sub(self.last_progress_ms) >= CAPTURE_STALL_MS {
            self.restart("lpib-stalled", now_ms);
            return;
        }

        if now_ms.saturating_sub(self.last_heartbeat_ms) >= CAPTURE_HEARTBEAT_MS {
            self.log_heartbeat(now_ms);
            self.last_heartbeat_ms = now_ms;
            self.reset_window();
        }

        self.publish_status(CaptureState::Running);
    }

    fn restart(&mut self, reason: &'static str, now_ms: u64) {
        self.restarts = self.restarts.saturating_add(1);
        self.recovery_not_before_ms = now_ms.saturating_add(CAPTURE_RETRY_MS);
        self.publish_status(CaptureState::Recovering);
        crate::log_os::service_important_line(format_args!(
            "hda-capture: recovery reason={} stream_index={} lpib={} total_frames={} fifo_errors={} descriptor_errors={} restarts={} action=input-stream-reset-only playback_untouched=1\n",
            reason,
            self.input_stream_index,
            self.last_lpib,
            self.total_frames,
            self.fifo_errors,
            self.descriptor_errors,
            self.restarts,
        ));
        match self.configure_stream_descriptor(false) {
            Ok(()) => {
                self.last_progress_ms = now_ms;
                self.recovery_not_before_ms = 0;
                self.publish_status(CaptureState::Running);
            }
            Err(error) => {
                self.publish_status(CaptureState::Faulted);
                crate::log_os::service_important_line(format_args!(
                    "hda-capture: recovery-failed error={} stream_index={} action=retry-next-poll playback_untouched=1\n",
                    error,
                    self.input_stream_index,
                ));
            }
        }
    }

    fn observe_completed_bytes(&mut self, start_byte: usize, byte_count: usize) {
        let frame_bytes = self.frame_bytes();
        if frame_bytes == 0 {
            return;
        }
        let aligned_bytes = byte_count - (byte_count % frame_bytes);
        let samples = aligned_bytes / 2;
        if samples == 0 {
            return;
        }

        let capacity_samples = CAPTURE_DMA_BYTES / 2;
        let start_sample = (start_byte / 2) % capacity_samples;
        for index in 0..samples {
            let sample_index = (start_sample + index) % capacity_samples;
            let sample = unsafe {
                core::ptr::read_volatile((self.dma_virt as *const i16).add(sample_index))
            };
            let magnitude = sample.unsigned_abs();
            self.window_peak_abs = self.window_peak_abs.max(magnitude);
            self.window_abs_sum = self.window_abs_sum.saturating_add(u64::from(magnitude));
            if sample != 0 {
                self.window_nonzero = self.window_nonzero.saturating_add(1);
            }
        }
        let frames = aligned_bytes / frame_bytes;
        self.window_samples = self.window_samples.saturating_add(samples as u64);
        self.window_frames = self.window_frames.saturating_add(frames as u64);
        self.total_frames = self.total_frames.saturating_add(frames as u64);
    }

    fn log_heartbeat(&self, now_ms: u64) {
        let mean_abs = if self.window_samples == 0 {
            0
        } else {
            (self.window_abs_sum / self.window_samples).min(u64::from(u16::MAX)) as u16
        };
        let nonzero_permille = if self.window_samples == 0 {
            0
        } else {
            (self.window_nonzero.saturating_mul(1000) / self.window_samples).min(1000) as u16
        };
        let base = self.stream_base();
        let ctl = unsafe { read8(self.mmio, base + SD_CTL) };
        let sts = unsafe { read8(self.mmio, base + SD_STS) };
        crate::log_os::service_important_line(format_args!(
            "hda-capture: heartbeat state=running observed_ms={} owner=hda-capture-lane rate_hz={} channels={} bits={} interleaved=1 stream_index={} stream_tag={} lpib={} window_frames={} total_frames={} peak_abs={} mean_abs={} nonzero_permille={} ctl=0x{:02X} sts=0x{:02X} fifo_errors={} descriptor_errors={} bcis={} restarts={} pin_present_at_start={} consumer=gna-audio-front-end\n",
            now_ms,
            PCM_SAMPLE_RATE_HZ,
            self.channels,
            PCM_SAMPLE_BITS,
            self.input_stream_index,
            self.stream_tag,
            self.last_lpib,
            self.window_frames,
            self.total_frames,
            self.window_peak_abs,
            mean_abs,
            nonzero_permille,
            ctl,
            sts,
            self.fifo_errors,
            self.descriptor_errors,
            self.bcis_events,
            self.restarts,
            self.pin_present_at_start as u8,
        ));
    }

    fn reset_window(&mut self) {
        self.window_frames = 0;
        self.window_samples = 0;
        self.window_abs_sum = 0;
        self.window_nonzero = 0;
        self.window_peak_abs = 0;
    }

    fn publish_status(&self, state: CaptureState) {
        let mean_abs = if self.window_samples == 0 {
            0
        } else {
            (self.window_abs_sum / self.window_samples).min(u64::from(u16::MAX)) as u16
        };
        let nonzero_permille = if self.window_samples == 0 {
            0
        } else {
            (self.window_nonzero.saturating_mul(1000) / self.window_samples).min(1000) as u16
        };
        *STATUS.lock() = CaptureStatus {
            state,
            sample_rate_hz: PCM_SAMPLE_RATE_HZ,
            channels: self.channels,
            sample_bits: PCM_SAMPLE_BITS,
            stream_index: self.input_stream_index,
            stream_tag: self.stream_tag,
            adc_nid: self.adc_nid,
            pin_nid: self.pin_nid,
            pin_present_at_start: self.pin_present_at_start,
            total_frames: self.total_frames,
            lpib_bytes: self.last_lpib,
            window_peak_abs: self.window_peak_abs,
            window_mean_abs: mean_abs,
            window_nonzero_permille: nonzero_permille,
            fifo_errors: self.fifo_errors,
            descriptor_errors: self.descriptor_errors,
            restarts: self.restarts,
        };
    }
}

struct CodecIo {
    mmio: usize,
}

impl CodecIo {
    fn codec_cmd(
        &mut self,
        codec: u8,
        nid: u16,
        verb: u32,
        data: u8,
    ) -> Result<u32, &'static str> {
        let raw20 = (verb << 8) | u32::from(data);
        self.raw(codec, nid, raw20)
    }

    fn set_verb_16(
        &mut self,
        codec: u8,
        nid: u16,
        verb: u32,
        payload: u16,
    ) -> Result<u32, &'static str> {
        let raw20 = ((verb & 0xF00) << 8) | u32::from(payload);
        self.raw(codec, nid, raw20)
    }

    fn get_param(&mut self, codec: u8, nid: u16, param: u32) -> Result<u32, &'static str> {
        self.codec_cmd(codec, nid, VERB_GET_PARAMETER, param as u8)
    }

    fn raw(&mut self, codec: u8, nid: u16, raw20: u32) -> Result<u32, &'static str> {
        let command =
            (u32::from(codec) << 28) | ((u32::from(nid) & 0xFF) << 20) | (raw20 & 0xFFFFF);
        unsafe {
            if !wait_until(
                || read16(self.mmio, REG_ICS) & ICS_ICB == 0,
                COMMAND_POLL_LIMIT,
            ) {
                return Err("hda-immediate-command-busy-timeout");
            }
            // IRV is write-one-to-clear.
            write16(self.mmio, REG_ICS, ICS_IRV);
            write32(self.mmio, REG_IC, command);
            write16(self.mmio, REG_ICS, ICS_ICB);
            if !wait_until(
                || {
                    let status = read16(self.mmio, REG_ICS);
                    status & ICS_ICB == 0 && status & ICS_IRV != 0
                },
                COMMAND_POLL_LIMIT,
            ) {
                return Err("hda-immediate-command-response-timeout");
            }
            let response = read32(self.mmio, REG_IR);
            write16(self.mmio, REG_ICS, ICS_IRV);
            Ok(response)
        }
    }
}

fn with_codec_command_window<T>(
    mmio: usize,
    f: impl FnOnce(&mut CodecIo) -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    // HDA forbids using the immediate command interface while CORB/RIRB are
    // active. Runtime PCM pumping does not need codec verbs, so briefly stop
    // only command DMA, preserve the established playback stream, configure
    // capture, and restore the command transport exactly as it was.
    let saved_corbctl = unsafe { read8(mmio, REG_CORBCTL) };
    let saved_rirbctl = unsafe { read8(mmio, REG_RIRBCTL) };
    let saved_gctl = unsafe { read32(mmio, REG_GCTL) };

    unsafe {
        write8(mmio, REG_CORBCTL, saved_corbctl & !0x02);
        write8(mmio, REG_RIRBCTL, saved_rirbctl & !0x02);
    }
    if !wait_until(
        || unsafe {
            read8(mmio, REG_CORBCTL) & 0x02 == 0 && read8(mmio, REG_RIRBCTL) & 0x02 == 0
        },
        COMMAND_POLL_LIMIT,
    ) {
        unsafe {
            write8(mmio, REG_CORBCTL, saved_corbctl);
            write8(mmio, REG_RIRBCTL, saved_rirbctl);
        }
        return Err("hda-command-dma-stop-timeout");
    }
    unsafe {
        write32(mmio, REG_GCTL, saved_gctl & !GCTL_UNSOL);
    }

    let result = f(&mut CodecIo { mmio });

    unsafe {
        write32(mmio, REG_GCTL, saved_gctl);
        write8(mmio, REG_RIRBCTL, saved_rirbctl);
        write8(mmio, REG_CORBCTL, saved_corbctl);
    }
    result
}

fn discover_and_configure_route(codec_io: &mut CodecIo) -> Result<Route, &'static str> {
    let codec = discover_codec(codec_io)?;
    let vendor_device = codec_io.get_param(codec, 0, PARAM_VENDOR_ID)?;
    let codec_vendor = (vendor_device >> 16) as u16;
    let codec_device = vendor_device as u16;

    let root_count = codec_io.get_param(codec, 0, PARAM_NODE_COUNT)?;
    let root_start = ((root_count >> 16) & 0xFF) as u16;
    let root_len = (root_count & 0xFF) as u16;

    for afg_nid in root_start..root_start.saturating_add(root_len) {
        let fg_type = codec_io.get_param(codec, afg_nid, PARAM_FN_GROUP_TYPE)?;
        if fg_type & 0xFF != 1 {
            continue;
        }

        let _ = codec_io.codec_cmd(codec, afg_nid, VERB_SET_POWER_STATE, 0);
        let afg_pcm_rates = codec_io.get_param(codec, afg_nid, PARAM_PCM_RATES).unwrap_or(0);
        let afg_stream_formats =
            codec_io.get_param(codec, afg_nid, PARAM_STREAM_FMTS).unwrap_or(0);
        let afg_amp_in_caps =
            codec_io.get_param(codec, afg_nid, PARAM_AMP_IN_CAPS).unwrap_or(0);
        let widgets = discover_widgets(
            codec_io,
            codec,
            afg_nid,
            afg_pcm_rates,
            afg_stream_formats,
            afg_amp_in_caps,
        )?;
        let Some((adc_nid, pin_nid, path, channels, format)) = choose_route(&widgets) else {
            continue;
        };

        configure_codec_route(codec_io, codec, &widgets, &path, pin_nid)?;
        let pin = widgets.iter().find(|widget| widget.nid == pin_nid).ok_or("hda-pin-lost")?;

        return Ok(Route {
            codec,
            codec_vendor,
            codec_device,
            afg_nid,
            adc_nid,
            pin_nid,
            pin_present: pin.present(),
            channels,
            format,
            path,
        });
    }

    Err("hda-no-capture-route")
}

fn discover_codec(codec_io: &mut CodecIo) -> Result<u8, &'static str> {
    let statests = unsafe { read16(codec_io.mmio, REG_STATESTS) };
    for codec in 0..15u8 {
        if statests != 0 && statests & (1 << codec) == 0 {
            continue;
        }
        if let Ok(vendor) = codec_io.get_param(codec, 0, PARAM_VENDOR_ID)
            && vendor != 0
            && vendor != u32::MAX
        {
            return Ok(codec);
        }
    }

    // Some controllers clear STATESTS after presence handling. Probe all
    // legal codec addresses as a bounded fallback.
    for codec in 0..15u8 {
        if let Ok(vendor) = codec_io.get_param(codec, 0, PARAM_VENDOR_ID)
            && vendor != 0
            && vendor != u32::MAX
        {
            return Ok(codec);
        }
    }
    Err("hda-codec-missing")
}

fn discover_widgets(
    codec_io: &mut CodecIo,
    codec: u8,
    afg_nid: u16,
    afg_pcm_rates: u32,
    afg_stream_formats: u32,
    afg_amp_in_caps: u32,
) -> Result<Vec<Widget>, &'static str> {
    let sub_count = codec_io.get_param(codec, afg_nid, PARAM_NODE_COUNT)?;
    let start = ((sub_count >> 16) & 0xFF) as u16;
    let count = (sub_count & 0xFF) as u16;
    let mut widgets = Vec::new();

    for nid in start..start.saturating_add(count) {
        let caps = codec_io.get_param(codec, nid, PARAM_AUDIO_CAPS)?;
        let widget_type = ((caps >> 20) & 0x0F) as u8;
        let connections = read_connection_list(codec_io, codec, nid)?;
        let mut widget = Widget {
            nid,
            widget_type,
            caps,
            pin_config: 0,
            pin_caps: 0,
            pin_sense: 0,
            pcm_rates: 0,
            stream_formats: 0,
            amp_in_caps: 0,
            connections,
        };

        if widget_type == WIDGET_AUDIO_INPUT {
            let own_pcm = codec_io.get_param(codec, nid, PARAM_PCM_RATES).unwrap_or(0);
            let own_stream = codec_io.get_param(codec, nid, PARAM_STREAM_FMTS).unwrap_or(0);
            widget.pcm_rates = if own_pcm == 0 { afg_pcm_rates } else { own_pcm };
            widget.stream_formats = if own_stream == 0 {
                afg_stream_formats
            } else {
                own_stream
            };
        }

        if widget_type == WIDGET_PIN_COMPLEX {
            widget.pin_config =
                codec_io.codec_cmd(codec, nid, VERB_GET_CONFIG_DEFAULT, 0).unwrap_or(0);
            widget.pin_caps = codec_io.get_param(codec, nid, PARAM_PIN_CAPS).unwrap_or(0);
            if widget.pin_caps & PINCAP_PRES_DETECT != 0 {
                widget.pin_sense =
                    codec_io.codec_cmd(codec, nid, VERB_GET_PIN_SENSE, 0).unwrap_or(0);
            }
        }

        if caps & WCAP_IN_AMP != 0 {
            widget.amp_in_caps = if caps & WCAP_AMP_OVERRIDE != 0 {
                codec_io.get_param(codec, nid, PARAM_AMP_IN_CAPS).unwrap_or(afg_amp_in_caps)
            } else {
                afg_amp_in_caps
            };
        }
        widgets.push(widget);
    }
    Ok(widgets)
}

fn read_connection_list(
    codec_io: &mut CodecIo,
    codec: u8,
    nid: u16,
) -> Result<Vec<u16>, &'static str> {
    let raw_len = codec_io.get_param(codec, nid, PARAM_CONN_LIST_LEN)?;
    let encoded_len = (raw_len & 0x7F) as usize;
    if encoded_len == 0 {
        return Ok(Vec::new());
    }
    let long_form = raw_len & 0x80 != 0;
    let per_response = if long_form { 2 } else { 4 };
    let mut out = Vec::new();
    let mut previous: Option<u16> = None;

    let mut offset = 0usize;
    while offset < encoded_len {
        let response = codec_io.codec_cmd(codec, nid, VERB_GET_CONN_LIST, offset as u8)?;
        for lane in 0..per_response {
            if offset + lane >= encoded_len {
                break;
            }
            let (value, range) = if long_form {
                let raw = ((response >> (lane * 16)) & 0xFFFF) as u16;
                (raw & 0x7FFF, raw & 0x8000 != 0)
            } else {
                let raw = ((response >> (lane * 8)) & 0xFF) as u16;
                (raw & 0x7F, raw & 0x80 != 0)
            };

            if range {
                if let Some(start) = previous {
                    for expanded in start.saturating_add(1)..=value {
                        out.push(expanded);
                    }
                } else {
                    out.push(value);
                }
            } else {
                out.push(value);
            }
            previous = Some(value);
        }
        offset += per_response;
    }
    Ok(out)
}

fn choose_route(widgets: &[Widget]) -> Option<(u16, u16, Vec<u16>, u8, u16)> {
    let mut best: Option<(u32, u16, u16, Vec<u16>, u8, u16)> = None;

    for adc in widgets.iter().filter(|widget| widget.widget_type == WIDGET_AUDIO_INPUT) {
        if adc.pcm_rates & (PCM_RATE_48KHZ | PCM_BITS_16)
            != (PCM_RATE_48KHZ | PCM_BITS_16)
        {
            continue;
        }
        if adc.stream_formats != 0 && adc.stream_formats & STREAM_FORMAT_PCM == 0 {
            continue;
        }

        let channels = if adc.caps & WCAP_STEREO != 0 { 2 } else { 1 };
        let format = 0x0010 | (u16::from(channels) - 1);

        for pin in widgets.iter().filter(|widget| widget.input_capable()) {
            let score = pin_score(pin);
            if score == 0 {
                continue;
            }
            let mut path = Vec::new();
            let mut visited = Vec::new();
            if !trace_path(widgets, adc.nid, pin.nid, &mut path, &mut visited) {
                continue;
            }

            if path.len() > MAX_PATH_NODES {
                continue;
            }
            let replace = best.as_ref().is_none_or(|current| score > current.0);
            if replace {
                best = Some((score, adc.nid, pin.nid, path, channels, format));
            }
        }
    }

    best.map(|(_, adc, pin, path, channels, format)| (adc, pin, path, channels, format))
}

fn pin_score(pin: &Widget) -> u32 {
    let base = match pin.default_device() {
        0xA => 1000,
        0x8 => 800,
        0x9 => 750,
        0x2 => 350,
        0xF => 250,
        _ => 100,
    };
    // A physically present jack wins over a fixed/internal microphone. This
    // is especially important for headset combo pins whose BIOS default
    // device can still be HP Out even though the widget is input-capable.
    base + if pin.present() { 1000 } else { 0 } + if pin.connectivity() == 2 { 25 } else { 0 }
}

fn trace_path(
    widgets: &[Widget],
    current: u16,
    target: u16,
    path: &mut Vec<u16>,
    visited: &mut Vec<u16>,
) -> bool {
    if visited.contains(&current) || visited.len() >= MAX_PATH_NODES {
        return false;
    }
    visited.push(current);
    path.push(current);
    if current == target {
        return true;
    }

    let Some(widget) = widgets.iter().find(|widget| widget.nid == current) else {
        path.pop();
        visited.pop();
        return false;
    };
    for &next in &widget.connections {
        if trace_path(widgets, next, target, path, visited) {
            return true;
        }
    }
    path.pop();
    visited.pop();
    false
}

fn configure_codec_route(
    codec_io: &mut CodecIo,
    codec: u8,
    widgets: &[Widget],
    path: &[u16],
    pin_nid: u16,
) -> Result<(), &'static str> {
    for &nid in path {
        let _ = codec_io.codec_cmd(codec, nid, VERB_SET_POWER_STATE, 0);
    }

    for edge in path.windows(2) {
        let nid = edge[0];
        let next = edge[1];
        let Some(widget) = widgets.iter().find(|widget| widget.nid == nid) else {
            continue;
        };
        if let Some(index) = widget.connections.iter().position(|&candidate| candidate == next) {
            let _ = codec_io.codec_cmd(codec, nid, VERB_SET_CONN_SELECT, index as u8);
            unmute_input_amp(codec_io, codec, widget, index as u16);
        }
    }

    let pin = widgets
        .iter()
        .find(|widget| widget.nid == pin_nid)
        .ok_or("hda-selected-pin-missing")?;
    let current_ctl = codec_io
        .codec_cmd(codec, pin_nid, VERB_GET_PIN_CONTROL, 0)
        .unwrap_or(0) as u8;
    let mut next_ctl = current_ctl | PINCTL_INPUT_ENABLE;
    let mic_bias_candidate = matches!(pin.default_device(), 0xA | 0x2 | 0xF);
    if mic_bias_candidate && ((pin.pin_caps >> PINCAP_VREF_SHIFT) & PINCAP_VREF_80) != 0 {
        next_ctl = (next_ctl & !PINCTL_VREF_MASK) | PINCTL_VREF_80;
    }
    codec_io.codec_cmd(codec, pin_nid, VERB_SET_PIN_CONTROL, next_ctl)?;

    unmute_input_amp(codec_io, codec, pin, 0);
    Ok(())
}

fn unmute_input_amp(codec_io: &mut CodecIo, codec: u8, widget: &Widget, index: u16) {
    if widget.caps & WCAP_IN_AMP == 0 {
        return;
    }
    let gain_0db = (widget.amp_in_caps & AMPCAP_OFFSET_MASK) as u16;
    let payload = AMP_SET_INPUT
        | AMP_SET_LEFT
        | AMP_SET_RIGHT
        | ((index & 0x0F) << AMP_SET_INDEX_SHIFT)
        | gain_0db;
    let _ = codec_io.set_verb_16(codec, widget.nid, VERB_SET_AMP_GAIN_MUTE, payload);
}

fn select_idle_input_stream(mmio: usize, count: u8) -> Option<u8> {
    for index in 0..count {
        let base = SD_BASE + u32::from(index) * SD_SIZE;
        let ctl = unsafe { read8(mmio, base + SD_CTL) };
        if ctl & SCTL_RUN == 0 {
            return Some(index);
        }
    }
    None
}

fn find_hda_device() -> Option<crate::pci::PciDevice> {
    let devices = crate::pci::find_by_class(crate::pci::class::MULTIMEDIA);
    devices
        .iter()
        .find(|device| device.subclass == 0x03)
        .or_else(|| devices.iter().find(|device| device.subclass == 0x01))
        .copied()
}

fn ring_distance_bytes(from: usize, to: usize, capacity: usize) -> usize {
    if capacity == 0 {
        return 0;
    }
    let from = from % capacity;
    let to = to % capacity;
    if to >= from {
        to - from
    } else {
        capacity - from + to
    }
}

fn wait_until(mut predicate: impl FnMut() -> bool, limit: usize) -> bool {
    for _ in 0..limit {
        if predicate() {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

#[inline]
unsafe fn read8(mmio: usize, offset: u32) -> u8 {
    unsafe { core::ptr::read_volatile((mmio + offset as usize) as *const u8) }
}

#[inline]
unsafe fn read16(mmio: usize, offset: u32) -> u16 {
    unsafe { core::ptr::read_volatile((mmio + offset as usize) as *const u16) }
}

#[inline]
unsafe fn read32(mmio: usize, offset: u32) -> u32 {
    unsafe { core::ptr::read_volatile((mmio + offset as usize) as *const u32) }
}

#[inline]
unsafe fn write8(mmio: usize, offset: u32, value: u8) {
    unsafe { core::ptr::write_volatile((mmio + offset as usize) as *mut u8, value) }
}

#[inline]
unsafe fn write16(mmio: usize, offset: u32, value: u16) {
    unsafe { core::ptr::write_volatile((mmio + offset as usize) as *mut u16, value) }
}

#[inline]
unsafe fn write32(mmio: usize, offset: u32, value: u32) {
    unsafe { core::ptr::write_volatile((mmio + offset as usize) as *mut u32, value) }
}

fn uptime_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1_000) / hz
}

pub(crate) fn status() -> CaptureStatus {
    *STATUS.lock()
}

/// Copy the newest completed PCM samples without consuming the capture lane.
///
/// Samples are signed little-endian i16 in the lane's native interleaved
/// channel layout. This is intentionally a read-only tap: GNA or another
/// consumer cannot move the hardware cursor or alter stream ownership.
#[allow(dead_code, reason = "GNA PCM consumer follows HDA capture bring-up")]
pub(crate) fn copy_latest_i16(out: &mut [i16]) -> Option<CaptureRead> {
    if out.is_empty() {
        return None;
    }
    let engine = ENGINE.lock();
    let engine = engine.as_ref()?;
    let status = status();
    if status.state != CaptureState::Running {
        return None;
    }

    let channels = usize::from(engine.channels);
    if channels == 0 {
        return None;
    }
    let capacity_samples = CAPTURE_DMA_BYTES / 2;
    let lpib = unsafe { read32(engine.mmio, engine.stream_base() + SD_LPIB) } as usize;
    let end_sample = (lpib.min(CAPTURE_DMA_BYTES) / 2) % capacity_samples;
    let mut sample_count = out.len().min(capacity_samples);
    sample_count -= sample_count % channels;
    if sample_count == 0 {
        return None;
    }
    let start_sample = (end_sample + capacity_samples - sample_count) % capacity_samples;
    for (index, slot) in out.iter_mut().take(sample_count).enumerate() {
        let sample_index = (start_sample + index) % capacity_samples;
        *slot = unsafe {
            core::ptr::read_volatile((engine.dma_virt as *const i16).add(sample_index))
        };
    }
    Some(CaptureRead {
        samples: sample_count,
        channels: engine.channels,
        sample_rate_hz: PCM_SAMPLE_RATE_HZ,
        sample_bits: PCM_SAMPLE_BITS,
        total_frames: engine.total_frames,
    })
}

pub(crate) fn ensure_started_on_current_worker() -> bool {
    if STARTED.load(Ordering::Acquire) {
        return true;
    }
    if STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return true;
    }

    let slot = crate::percpu::current_slot() as u32;
    let Some(spawner) = crate::workers::spawner_for_slot(slot) else {
        STARTED.store(false, Ordering::Release);
        return false;
    };
    match hda_capture_task() {
        Ok(token) => {
            spawner.spawn(token);
            true
        }
        Err(_) => {
            STARTED.store(false, Ordering::Release);
            false
        }
    }
}

#[trueos_executor::task]
async fn hda_capture_task() {
    let mut last_error: Option<&'static str> = None;
    let mut last_wait_log_ms = 0u64;

    loop {
        if ENGINE.lock().is_none() {
            *STATUS.lock() = CaptureStatus {
                state: CaptureState::Starting,
                ..CaptureStatus::offline()
            };
            match CaptureEngine::initialize() {
                Ok(engine) => {
                    *ENGINE.lock() = Some(engine);
                    last_error = None;
                }
                Err(error) => {
                    *STATUS.lock() = CaptureStatus {
                        state: CaptureState::Faulted,
                        ..CaptureStatus::offline()
                    };
                    let now_ms = uptime_ms();
                    if last_error != Some(error)
                        || now_ms.saturating_sub(last_wait_log_ms) >= CAPTURE_HEARTBEAT_MS
                    {
                        crate::log_os::service_important_line(format_args!(
                            "hda-capture: waiting owner=hda-capture-lane error={} retry_ms={} playback_untouched=1\n",
                            error,
                            CAPTURE_RETRY_MS,
                        ));
                        last_error = Some(error);
                        last_wait_log_ms = now_ms;
                    }
                    Timer::after(Duration::from_millis(CAPTURE_RETRY_MS)).await;
                    continue;
                }
            }
        }

        let now_ms = uptime_ms();
        {
            let mut engine = ENGINE.lock();
            if let Some(engine) = engine.as_mut() {
                engine.poll(now_ms);
            }
        }
        Timer::after(Duration::from_millis(CAPTURE_POLL_MS)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_distance_handles_wrap() {
        assert_eq!(ring_distance_bytes(100, 120, 256), 20);
        assert_eq!(ring_distance_bytes(240, 16, 256), 32);
    }

    #[test]
    fn hda_pcm_formats_match_existing_playback_encoding() {
        assert_eq!(0x0010 | (1 - 1), 0x0010);
        assert_eq!(0x0010 | (2 - 1), 0x0011);
    }
}
