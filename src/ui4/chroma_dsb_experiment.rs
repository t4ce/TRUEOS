use super::{
    LUT_ALLOW_DB_DISABLE, LUT_ENABLE, LUT_ENTRIES, PIPE_ENABLE, PIPE_LUT, Registers, Settings,
    arm_table, device, read_table, ready, registers, upload, write_table_mmio,
};
use alloc::{boxed::Box, vec};
use core::sync::atomic::{AtomicBool, Ordering};
use trueos_time::{Duration, Instant, Timer};

const PAIRS: usize = 12;
const SAMPLES: usize = PAIRS * 2;
const TIMEOUT: Duration = Duration::from_millis(100);
const PIPE_ACTIVE: u32 = 1 << 30;
const RIG_DEVICE: u16 = 0x4680;
const RIG_REVISION: u8 = 0x0C;
const ROUTE_CONTROL: u32 = LUT_ENABLE | LUT_ALLOW_DB_DISABLE;
static RUNNING: AtomicBool = AtomicBool::new(false);

pub(crate) struct ChromaDsbReport {
    pub(crate) device_id: u16,
    pub(crate) revision_id: u8,
    pub(crate) samples: usize,
    pub(crate) mmio: ChromaDsbMetrics,
    pub(crate) dsb: ChromaDsbMetrics,
}

pub(crate) struct ChromaDsbMetrics {
    pub(crate) cpu_p50_ns: u64,
    pub(crate) cpu_p95_ns: u64,
    pub(crate) latch_p50_ns: u64,
    pub(crate) latch_p95_ns: u64,
}

struct Run;

impl Drop for Run {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}

// Called with the normal route/LUT locks before changing the table or index.
pub(super) fn permit_lut_access(pipe: usize) -> Result<(), &'static str> {
    if pipe == 0 {
        if RUNNING.load(Ordering::Acquire) {
            return Err("chroma-3dlut-busy");
        }
        if crate::intel::lut_dsb_port_blocked() {
            return Err("chroma-dsb-pipe-a-quarantined");
        }
    }
    Ok(())
}

fn with_pipe_a<T>(
    operation: impl FnOnce(crate::intel::Dev, Registers) -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    let regs = registers(0)?;
    let _route = crate::intel::DISPLAY_COLOR_ROUTE_LOCK.lock();
    let _lut = PIPE_LUT.lock();
    let dev = device(regs)?;
    let pci = crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 0);
    let revision = crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 8) as u8;
    if pci != (u32::from(RIG_DEVICE) << 16) | 0x8086 || revision != RIG_REVISION {
        return Err("chroma-dsb-unsupported-device");
    }
    let config = crate::intel::mmio_read(dev, regs.config);
    if config == u32::MAX || config & (PIPE_ENABLE | PIPE_ACTIVE) != PIPE_ENABLE | PIPE_ACTIVE {
        return Err("chroma-dsb-pipe-a-inactive");
    }
    operation(dev, regs)
}

fn stamp() -> u64 {
    use core::arch::x86_64::{_mm_lfence, _rdtsc};

    unsafe {
        _mm_lfence();
        let ticks = _rdtsc();
        _mm_lfence();
        ticks
    }
}

// Poll in the task, with no color or route lock held across a timer wait.
async fn wait_latch() -> Result<u64, &'static str> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let observed = with_pipe_a(|dev, regs| match ready(dev, regs) {
            Ok(_) => Ok(Some(stamp())),
            Err("chroma-3dlut-busy") => Ok(None),
            Err(error) => Err(error),
        })?;
        if let Some(observed) = observed {
            return Ok(observed);
        }
        if Instant::now() >= deadline {
            return Err("chroma-dsb-lut-latch-timeout");
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}

fn verify_table(
    dev: crate::intel::Dev,
    regs: Registers,
    entries: &[u32],
    readback: &mut [u32],
    original_index: u32,
) -> Result<(), &'static str> {
    if crate::intel::mmio_read(dev, regs.index) != original_index {
        return Err("chroma-dsb-index-mismatch");
    }
    read_table(dev, regs, readback);
    if readback != entries {
        return Err("chroma-dsb-table-mismatch");
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Transport {
    Mmio,
    Dsb,
}

#[derive(Clone, Copy, Default)]
struct Sample {
    cpu_ns: u64,
    latch_ns: u64,
}

async fn trial(
    transport: Transport,
    entries: &[u32],
    readback: &mut [u32],
    verify_before_arm: bool,
) -> Result<Sample, &'static str> {
    let (request, started, mut cpu_ticks, index, control) = with_pipe_a(|dev, regs| {
        let control = ready(dev, regs)? | LUT_ENABLE;
        let index = crate::intel::mmio_read(dev, regs.index);
        if index == u32::MAX {
            return Err("chroma-dsb-index-unavailable");
        }
        let started = stamp();
        let request = match transport {
            Transport::Mmio => {
                upload(dev, regs, entries, control)?;
                None
            }
            Transport::Dsb => Some(crate::intel::start_lut_dsb(dev, entries, index)?),
        };
        Ok((
            request,
            started,
            stamp().saturating_sub(started),
            index,
            control,
        ))
    })?;

    if let Some(request) = request {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let (poll, polling_ticks) = with_pipe_a(|dev, _| {
                let start = stamp();
                Ok((request.poll(dev), stamp().saturating_sub(start)))
            })?;
            cpu_ticks += polling_ticks;
            match poll {
                crate::intel::LutDsbPoll::Complete => break,
                crate::intel::LutDsbPoll::Failed(reason)
                | crate::intel::LutDsbPoll::Uncertain(reason) => return Err(reason),
                crate::intel::LutDsbPoll::Pending if Instant::now() < deadline => {
                    Timer::after(Duration::from_millis(1)).await;
                }
                crate::intel::LutDsbPoll::Pending => return Err("chroma-dsb-upload-timeout"),
            }
        }
        // The first DSB transfer must prove an actual table change before it
        // can arm the LUT. The warm-up's readback is outside the timed series.
        let commit_ticks = with_pipe_a(|dev, regs| {
            if verify_before_arm {
                verify_table(dev, regs, entries, readback, index)?;
            }
            let start = stamp();
            ready(dev, regs)?;
            arm_table(dev, regs, control)?;
            Ok(stamp().saturating_sub(start))
        })?;
        cpu_ticks += commit_ticks;
    }

    let latched = wait_latch().await?;
    with_pipe_a(|dev, regs| verify_table(dev, regs, entries, readback, index))?;
    let tsc_hz = u128::from(crate::time::tsc_hz());
    let nanos = |ticks| (u128::from(ticks) * 1_000_000_000 / tsc_hz) as u64;
    Ok(Sample {
        cpu_ns: nanos(cpu_ticks),
        latch_ns: nanos(latched.saturating_sub(started)),
    })
}

struct Original {
    control: u32,
    index: u32,
    entries: Box<[u32]>,
}

async fn restore(original: &Original, readback: &mut [u32]) -> Result<(), &'static str> {
    wait_latch().await?;
    with_pipe_a(|dev, regs| {
        ready(dev, regs)?;
        write_table_mmio(dev, regs, &original.entries);
        crate::intel::mmio_write(dev, regs.index, original.index);
        if original.control & LUT_ENABLE != 0 {
            arm_table(dev, regs, original.control)?;
        } else {
            crate::intel::mmio_write(dev, regs.control, original.control & ROUTE_CONTROL);
        }
        Ok(())
    })?;
    wait_latch().await?;
    with_pipe_a(|dev, regs| {
        if ready(dev, regs)? & ROUTE_CONTROL != original.control & ROUTE_CONTROL {
            return Err("chroma-dsb-control-restore-mismatch");
        }
        verify_table(dev, regs, &original.entries, readback, original.index)
    })
}

fn percentile(mut samples: [u64; SAMPLES]) -> (u64, u64) {
    samples.sort_unstable();
    (samples[SAMPLES / 2 - 1], samples[(SAMPLES * 95).div_ceil(100) - 1])
}

fn metrics(samples: [Sample; SAMPLES]) -> ChromaDsbMetrics {
    let (cpu_p50_ns, cpu_p95_ns) = percentile(samples.map(|sample| sample.cpu_ns));
    let (latch_p50_ns, latch_p95_ns) = percentile(samples.map(|sample| sample.latch_ns));
    ChromaDsbMetrics { cpu_p50_ns, cpu_p95_ns, latch_p50_ns, latch_p95_ns }
}

fn grade_table(settings: Settings) -> Box<[u32]> {
    let mut entries = vec![0; LUT_ENTRIES].into_boxed_slice();
    settings.build_table((&mut *entries).try_into().expect("LUT entry count"));
    entries
}

async fn compare(original: &[u32], readback: &mut [u32]) -> Result<ChromaDsbReport, &'static str> {
    let identity = grade_table(Settings::default());
    let muted = grade_table(Settings { saturation: 0.65, ..Settings::default() });
    // Keep the original grade active until DSB has proven it can change the
    // complete staging table. Use a different grade even if muted is active.
    let (dsb_warmup, mmio_warmup) = if original == &*muted {
        (&identity, &muted)
    } else {
        (&muted, &identity)
    };
    trial(Transport::Dsb, dsb_warmup, readback, true).await?;
    trial(Transport::Mmio, mmio_warmup, readback, false).await?;

    let mut samples = [[Sample::default(); SAMPLES]; 2];
    let transports = [Transport::Mmio, Transport::Dsb];
    for pair in 0..PAIRS {
        let first = pair % 2;
        for transport in [first, 1 - first] {
            for (offset, grade) in [&identity, &muted].into_iter().enumerate() {
                samples[transport][pair * 2 + offset] = trial(transports[transport], grade, readback, false).await?;
            }
        }
    }
    let [mmio, dsb] = samples.map(metrics);
    Ok(ChromaDsbReport { device_id: RIG_DEVICE, revision_id: RIG_REVISION, samples: SAMPLES, mmio, dsb })
}

pub(crate) async fn benchmark_chroma_dsb() -> Result<ChromaDsbReport, &'static str> {
    RUNNING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "chroma-dsb-already-running")?;
    let _run = Run;
    if crate::intel::lut_dsb_port_blocked() {
        return Err("chroma-dsb-pipe-a-quarantined");
    }
    with_pipe_a(|dev, _| crate::intel::check_lut_dsb_idle(dev))?;
    wait_latch().await?;
    let mut entries = vec![0; LUT_ENTRIES].into_boxed_slice();
    let (control, index) = with_pipe_a(|dev, regs| {
        let control = ready(dev, regs)?;
        let index = crate::intel::mmio_read(dev, regs.index);
        if index == u32::MAX {
            return Err("chroma-dsb-index-unavailable");
        }
        read_table(dev, regs, &mut entries);
        Ok((control, index))
    })?;
    let original = Original { control, index, entries };
    let mut readback = vec![0; LUT_ENTRIES].into_boxed_slice();
    let result = compare(&original.entries, &mut readback).await;
    if crate::intel::lut_dsb_port_blocked() {
        // Drop of an outstanding DMA token retains the command backing and
        // denies further writes. A CPU restore could race its last MMIO.
        return result.and(Err("chroma-dsb-pipe-a-quarantined"));
    }
    restore(&original, &mut readback).await.map_err(|_| "chroma-dsb-original-restoration-failed")?;
    result
}
