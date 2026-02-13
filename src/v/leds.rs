use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::{Deque, Vec};
use spin::Mutex;

use trueos_v::vled::{Effect, Rgb8};

/// Virtual LED handle.
///
/// Semantics:
/// - Cheap to clone/copy.
/// - Commands are queued to a single kernel-owned writer loop.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VLed {
    token: u32,
}

const LED_POOL_SIZE: usize = 256;
const POOL_WORDS: usize = LED_POOL_SIZE / 64;
const MAX_HANDLES: usize = 64;

#[derive(Copy, Clone, Debug)]
struct LedScope {
    token: u32,
    start: u16,
    len: u16,
}

static FALLBACK_SEQ: AtomicU32 = AtomicU32::new(1);

static SCOPES: Mutex<Vec<LedScope, MAX_HANDLES>> = Mutex::new(Vec::new());
static POOL_USED: Mutex<[u64; POOL_WORDS]> = Mutex::new([0; POOL_WORDS]);
static POOL_COLORS: Mutex<[Rgb8; LED_POOL_SIZE]> = Mutex::new([Rgb8::new(0, 0, 0); LED_POOL_SIZE]);

#[derive(Clone, Debug)]
enum LedCmd {
    SetRgb { owner: u32, rgb: Rgb8 },
    SetData {
        owner: u32,
        data: Vec<Rgb8, LED_POOL_SIZE>,
    },
    SetEffect { owner: u32, effect: Effect },
    RawOut {
        owner: u32,
        report_id: u8,
        data: Vec<u8, 64>,
    },
}

const CMD_Q_CAP: usize = 64;
static CMD_Q: Mutex<Deque<LedCmd, CMD_Q_CAP>> = Mutex::new(Deque::new());

fn is_used(bits: &[u64; POOL_WORDS], idx: usize) -> bool {
    let word = idx / 64;
    let bit = idx % 64;
    (bits[word] & (1u64 << bit)) != 0
}

fn set_used(bits: &mut [u64; POOL_WORDS], idx: usize) {
    let word = idx / 64;
    let bit = idx % 64;
    bits[word] |= 1u64 << bit;
}

fn clear_used(bits: &mut [u64; POOL_WORDS], idx: usize) {
    let word = idx / 64;
    let bit = idx % 64;
    bits[word] &= !(1u64 << bit);
}

fn scope_range(token: u32) -> Option<(usize, usize)> {
    let scopes = SCOPES.lock();
    for s in scopes.iter() {
        if s.token == token {
            let start = s.start as usize;
            let len = s.len as usize;
            return Some((start, len));
        }
    }
    None
}

fn token_in_use(token: u32) -> bool {
    let scopes = SCOPES.lock();
    scopes.iter().any(|s| s.token == token)
}

fn gen_token() -> u32 {
    let mut bytes = [0u8; 4];
    if crate::rng::fill_bytes(&mut bytes) {
        let v = u32::from_le_bytes(bytes);
        if v != 0 {
            return v;
        }
    }
    FALLBACK_SEQ.fetch_add(1, Ordering::Relaxed).max(1)
}

fn alloc_pool_range(count: usize) -> Option<usize> {
    if count == 0 || count > LED_POOL_SIZE {
        return None;
    }

    let mut used = POOL_USED.lock();

    // First-fit contiguous allocator.
    for start in 0..=(LED_POOL_SIZE - count) {
        let mut ok = true;
        for i in 0..count {
            if is_used(&used, start + i) {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }

        for i in 0..count {
            set_used(&mut used, start + i);
        }
        return Some(start);
    }

    None
}

fn rollback_pool_range(start: usize, count: usize) {
    if count == 0 || start >= LED_POOL_SIZE {
        return;
    }
    let end = core::cmp::min(LED_POOL_SIZE, start.saturating_add(count));
    let mut used = POOL_USED.lock();
    for idx in start..end {
        clear_used(&mut used, idx);
    }
}

/// Allocate a new virtual LED handle with an explicit LED count.
///
/// This is a capability boundary: the returned handle is scoped to a private
/// segment of the virtual 256-LED pool and cannot affect LEDs outside its range.
///
/// This does not require the hardware to be present.
pub fn alloc(count: usize) -> Option<VLed> {
    let start = alloc_pool_range(count)?;
    let len_u16: u16 = count.try_into().ok()?;

    let mut token = gen_token();
    for _ in 0..8 {
        if token != 0 && !token_in_use(token) {
            break;
        }
        token = gen_token();
    }
    if token == 0 || token_in_use(token) {
        rollback_pool_range(start, count);
        return None;
    }

    let mut scopes = SCOPES.lock();
    if scopes.push(LedScope {
        token,
        start: start as u16,
        len: len_u16,
    }).is_err() {
        rollback_pool_range(start, count);
        return None;
    }

    Some(VLed { token })
}

impl VLed {
    pub fn set_rgb(&self, rgb: Rgb8) {
        push_cmd(LedCmd::SetRgb {
            owner: self.token,
            rgb,
        });
    }

    /// Update (part of) this handle's LED segment.
    ///
    /// `colors[0]` maps to the first LED in this handle's segment.
    pub fn set_leds(&self, colors: &[Rgb8]) {
        let mut v: Vec<Rgb8, LED_POOL_SIZE> = Vec::new();
        let take = core::cmp::min(colors.len(), LED_POOL_SIZE);
        let _ = v.extend_from_slice(&colors[..take]);
        push_cmd(LedCmd::SetData {
            owner: self.token,
            data: v,
        });
    }

    pub fn set_effect(&self, effect: Effect) {
        push_cmd(LedCmd::SetEffect {
            owner: self.token,
            effect,
        });
    }

    pub(crate) fn send_raw_out(&self, report_id: u8, data: &[u8]) {
        let mut v: Vec<u8, 64> = Vec::new();
        let _ = v.extend_from_slice(&data[..core::cmp::min(data.len(), 64)]);
        push_cmd(LedCmd::RawOut {
            owner: self.token,
            report_id,
            data: v,
        });
    }
}

fn push_cmd(cmd: LedCmd) {
    let mut q = CMD_Q.lock();
    if let Err(cmd) = q.push_back(cmd) {
        // Drop oldest to make room (keep system responsive).
        let _ = q.pop_front();
        let _ = q.push_back(cmd);
    }
}

fn drain_cmds(max: usize) -> Vec<LedCmd, CMD_Q_CAP> {
    let mut out: Vec<LedCmd, CMD_Q_CAP> = Vec::new();
    let mut q = CMD_Q.lock();
    for _ in 0..max {
        let Some(cmd) = q.pop_front() else { break };
        let _ = out.push(cmd);
    }
    out
}

fn encode_set_rgb(rgb: Rgb8) -> (u8, Vec<u8, 64>) {
    // Best-effort default encoding until the protocol is fully reversed.
    // Many simple vendor HID LED devices use an output report without a report ID.
    // Format guess: [0x01, R, G, B]
    let mut out: Vec<u8, 64> = Vec::new();
    let _ = out.push(0x01);
    let _ = out.push(rgb.r);
    let _ = out.push(rgb.g);
    let _ = out.push(rgb.b);
    (0, out)
}

fn encode_set_effect(effect: Effect) -> (u8, Vec<u8, 64>) {
    // Best-effort placeholder encoding.
    // Format guess: [0x02, effect_id]
    let effect_id = match effect {
        Effect::Off => 0x00,
        Effect::Solid => 0x01,
        Effect::Breathing => 0x02,
        Effect::Rainbow => 0x03,
    };
    let mut out: Vec<u8, 64> = Vec::new();
    let _ = out.push(0x02);
    let _ = out.push(effect_id);
    (0, out)
}

fn apply_set_rgb(owner: u32, rgb: Rgb8) {
    let Some((start, len)) = scope_range(owner) else {
        return;
    };
    let mut colors = POOL_COLORS.lock();
    for i in 0..len {
        colors[start + i] = rgb;
    }
}

fn apply_set_data(owner: u32, data: &[Rgb8]) {
    let Some((start, len)) = scope_range(owner) else {
        return;
    };
    let mut colors = POOL_COLORS.lock();
    let take = core::cmp::min(len, data.len());
    for i in 0..take {
        colors[start + i] = data[i];
    }
}

fn mixed_rgb_from_pool() -> Rgb8 {
    let used = POOL_USED.lock();
    let colors = POOL_COLORS.lock();

    let mut n: u32 = 0;
    let mut sum_r: u32 = 0;
    let mut sum_g: u32 = 0;
    let mut sum_b: u32 = 0;

    for idx in 0..LED_POOL_SIZE {
        if !is_used(&used, idx) {
            continue;
        }
        let c = colors[idx];
        sum_r = sum_r.wrapping_add(c.r as u32);
        sum_g = sum_g.wrapping_add(c.g as u32);
        sum_b = sum_b.wrapping_add(c.b as u32);
        n = n.wrapping_add(1);
    }

    if n == 0 {
        return Rgb8::new(0, 0, 0);
    }

    Rgb8::new((sum_r / n) as u8, (sum_g / n) as u8, (sum_b / n) as u8)
}

#[embassy_executor::task]
pub async fn task() {
    async move {
        crate::log!("v_leds: service online\n");
        let raw_probe = alloc(1);

        let mut last_offline: bool = true;
        let mut last_sent_rgb: Option<Rgb8> = None;
        let mut last_effect: Option<Effect> = None;
        let mut last_effect_owner: Option<u32> = None;

        loop {
            let online = crate::usb::leds::is_online();
            let cmds = drain_cmds(16);
            if cmds.is_empty() {
                Timer::after(EmbassyDuration::from_millis(20)).await;
            }

            for cmd in cmds.iter() {
                match cmd {
                    LedCmd::SetRgb { owner, rgb } => {
                        apply_set_rgb(*owner, *rgb);
                    }
                    LedCmd::SetData { owner, data } => {
                        apply_set_data(*owner, data.as_slice());
                    }
                    LedCmd::SetEffect { owner, effect } => {
                        if scope_range(*owner).is_some() {
                            last_effect = Some(*effect);
                            last_effect_owner = Some(*owner);
                        }
                    }
                    LedCmd::RawOut {
                        owner,
                        report_id,
                        data,
                    } => {
                        if online && scope_range(*owner).is_some() {
                            let _ = crate::usb::leds::send_output_report_first(*report_id, data).await;
                        }
                    }
                }
            }

            if online {
                let effect_ok = last_effect_owner
                    .and_then(|owner| scope_range(owner))
                    .is_some();
                if !effect_ok {
                    last_effect = None;
                    last_effect_owner = None;
                }

                // Attempt to send RAW RGB data for the first 20 LEDs (Addressable Mode Probe)
                // instead of averaging them into a single color.
                // Packet format: [R, G, B, R, G, B, ... ] up to 64 bytes.
                {
                    let colors = POOL_COLORS.lock();
                    let mut packet: Vec<u8, 64> = Vec::new();
                    // 21 LEDs * 3 bytes = 63 bytes, fits in 64 byte HID report
                    for i in 0..21 {
                        let c = colors[i];
                        let _ = packet.push(c.r);
                        let _ = packet.push(c.g);
                        let _ = packet.push(c.b);
                    }
                    // Send using preferred ID (usually 0 for these streams)
                    let _ = crate::usb::leds::send_preferred_output_report_first(&packet).await;
                }

                if last_offline {
                    if let Some(probe) = raw_probe {
                        // Kick one raw OUT report when xHCI LED runtime comes online.
                        probe.send_raw_out(0, &[0x00]);
                    }

                    if let Some(effect) = last_effect {
                        let (rid, data) = encode_set_effect(effect);
                        if rid == 0 {
                            let _ = crate::usb::leds::send_preferred_output_report_first(&data).await;
                        } else {
                            let _ = crate::usb::leds::send_output_report_first(rid, &data).await;
                        }
                    }
                }
            }

            last_offline = !online;

            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    }.await;
}

#[embassy_executor::task]
pub async fn color_cycle_task() {
    async move {
        crate::log!("v_leds: rainbow generator online (20ms)\n");

        let Some(all_leds) = alloc(LED_POOL_SIZE) else {
            crate::log!("v_leds: alloc({}) failed\n", LED_POOL_SIZE);
            return;
        };

        fn wheel(mut pos: u8) -> Rgb8 {
            pos = 255 - pos;
            if pos < 85 {
                Rgb8::new(255 - pos * 3, 0, pos * 3)
            } else if pos < 170 {
                let pos = pos - 85;
                Rgb8::new(0, pos * 3, 255 - pos * 3)
            } else {
                let pos = pos - 170;
                Rgb8::new(pos * 3, 255 - pos * 3, 0)
            }
        }

        let mut buf: Vec<Rgb8, LED_POOL_SIZE> = Vec::new();
        for _ in 0..LED_POOL_SIZE {
            let _ = buf.push(Rgb8::new(0, 0, 0));
        }

        let mut offset: u8 = 0;

        loop {
            for i in 0..LED_POOL_SIZE {
                buf[i] = wheel((i as u8).wrapping_add(offset));
            }
            all_leds.set_leds(&buf);
            offset = offset.wrapping_add(2);

            Timer::after(EmbassyDuration::from_millis(20)).await;
        }
    }.await;
}
