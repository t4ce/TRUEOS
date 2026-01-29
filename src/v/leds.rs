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
    id: u32,
}

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Debug)]
enum LedCmd {
    SetRgb { owner: u32, rgb: Rgb8 },
    SetEffect { owner: u32, effect: Effect },
    RawOut {
        owner: u32,
        report_id: u8,
        data: Vec<u8, 64>,
    },
}

const CMD_Q_CAP: usize = 64;
static CMD_Q: Mutex<Deque<LedCmd, CMD_Q_CAP>> = Mutex::new(Deque::new());

/// Allocate a new virtual LED handle.
///
/// This does not require the hardware to be present.
pub fn alloc() -> VLed {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    VLed { id }
}

impl VLed {
    pub fn set_rgb(&self, rgb: Rgb8) {
        push_cmd(LedCmd::SetRgb {
            owner: self.id,
            rgb,
        });
    }

    pub fn set_effect(&self, effect: Effect) {
        push_cmd(LedCmd::SetEffect {
            owner: self.id,
            effect,
        });
    }

    pub fn send_raw_out(&self, report_id: u8, data: &[u8]) {
        let mut v: Vec<u8, 64> = Vec::new();
        let _ = v.extend_from_slice(&data[..core::cmp::min(data.len(), 64)]);
        push_cmd(LedCmd::RawOut {
            owner: self.id,
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

#[embassy_executor::task]
pub async fn task() {
    crate::log!("v_leds: service online\n");

    let mut last_offline: bool = true;
    let mut last_rgb: Option<Rgb8> = None;
    let mut last_effect: Option<Effect> = None;

    loop {
        let online = crate::usb::leds::is_online();
        if online && last_offline {
            // Device (re)appeared; replay last desired state.
            if let Some(rgb) = last_rgb {
                let (rid, data) = encode_set_rgb(rgb);
                let _ = crate::usb::leds::send_output_report_first(rid, &data).await;
            }
            if let Some(effect) = last_effect {
                let (rid, data) = encode_set_effect(effect);
                let _ = crate::usb::leds::send_output_report_first(rid, &data).await;
            }
        }
        last_offline = !online;

        let cmds = drain_cmds(16);
        if cmds.is_empty() {
            Timer::after(EmbassyDuration::from_millis(20)).await;
            continue;
        }

        for cmd in cmds.iter() {
            match cmd {
                LedCmd::SetRgb { rgb, .. } => {
                    last_rgb = Some(*rgb);
                    if online {
                        let (rid, data) = encode_set_rgb(*rgb);
                        let _ = crate::usb::leds::send_output_report_first(rid, &data).await;
                    }
                }
                LedCmd::SetEffect { effect, .. } => {
                    last_effect = Some(*effect);
                    if online {
                        let (rid, data) = encode_set_effect(*effect);
                        let _ = crate::usb::leds::send_output_report_first(rid, &data).await;
                    }
                }
                LedCmd::RawOut { report_id, data, .. } => {
                    if online {
                        let _ = crate::usb::leds::send_output_report_first(*report_id, data).await;
                    }
                }
            }
        }

        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}
