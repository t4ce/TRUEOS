//! Quarantined, register-level laboratory for the live CrabUSB xHCI owner.

use alloc::{
    collections::VecDeque,
    format,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use embassy_sync::signal::Signal;
use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

use super::crabusb::{
    self,
    diag::{
        XhciControllerSnapshot, XhciDirectRequest, XhciDirectResponse, XhciPortSnapshot,
        XhciWrite64Result, XhciWriteResult,
    },
};

const REQUEST_CAPACITY: usize = 4;
const JOURNAL_CAPACITY: usize = 2_048;
const STABILITY_SAMPLES: usize = 250;
const STABILITY_SAMPLE_INTERVAL_MS: u64 = 2;
const QUIESCE_TIMEOUT_MS: u64 = 5_000;
const RESET_SETTLE_MS: u64 = 150;
const RESET_POLL_MS: u64 = 10;
const RESET_TIMEOUT_MS: u64 = 750;
const TREE_MAX_PATHS: usize = 128;

/// The fused mainboard LED endpoint is an ambient actor, never an implicit
/// mutation target.
pub(crate) const FUSED_LED_PORT: u8 = 11;

const PORT_CCS: u32 = 1 << 0;
const PORT_PED: u32 = 1 << 1;
const PORT_OCA: u32 = 1 << 3;
const PORT_PR: u32 = 1 << 4;
const PORT_PLS_MASK: u32 = 0x0f << 5;
const PORT_PP: u32 = 1 << 9;
const PORT_SPEED_MASK: u32 = 0x0f << 10;
const PORT_PIC_MASK: u32 = 0x03 << 14;
const PORT_LWS: u32 = 1 << 16;
const PORT_CHANGE_MASK: u32 = 0x7f << 17;
const PORT_WAKE_MASK: u32 = 0x07 << 25;
const PORT_DR: u32 = 1 << 30;
const PORT_WPR: u32 = 1 << 31;
const PORT_RO_MASK: u32 = PORT_CCS | PORT_OCA | PORT_SPEED_MASK | PORT_DR;
const PORT_RWS_MASK: u32 = PORT_PLS_MASK | PORT_PP | PORT_PIC_MASK | PORT_WAKE_MASK;

static REQUESTS: Mutex<VecDeque<QueuedRequest>> = Mutex::new(VecDeque::new());
static JOURNAL: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static LATEST_SNAPSHOT: Mutex<Option<XhciControllerSnapshot>> = Mutex::new(None);
static NEXT_RUN_ID: AtomicU64 = AtomicU64::new(1);
static LAST_STAGE: AtomicU32 = AtomicU32::new(0);
static QUARANTINE_REQUESTED: AtomicBool = AtomicBool::new(false);
static QUARANTINE_ACTIVE: AtomicBool = AtomicBool::new(false);
static UAS_IO_INFLIGHT: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LabCommand {
    Status,
    Stage {
        stage: u8,
        port: Option<u8>,
        armed: bool,
        include_fused: bool,
        allow_live_device: bool,
        depth: u8,
    },
    Read32 {
        offset: usize,
    },
    Read64 {
        offset: usize,
    },
    Write32 {
        offset: usize,
        value: u32,
        armed: bool,
        include_fused: bool,
        allow_live_device: bool,
    },
    Write64 {
        offset: usize,
        value: u64,
        armed: bool,
        include_fused: bool,
        allow_live_device: bool,
    },
    ReadModifyWrite32 {
        offset: usize,
        clear_mask: u32,
        set_mask: u32,
        armed: bool,
        include_fused: bool,
        allow_live_device: bool,
    },
    Journal,
}

#[derive(Debug)]
pub(crate) struct LabReport {
    pub run_id: u64,
    pub lines: Vec<String>,
}

type LabResult = Result<LabReport, String>;
type ReplySignal = Signal<crate::wait::EmbassySpinRawMutex, LabResult>;

struct QueuedRequest {
    run_id: u64,
    command: LabCommand,
    reply: Arc<ReplySignal>,
}

pub(crate) struct PendingLabRequest {
    pub run_id: u64,
    reply: Arc<ReplySignal>,
}

impl PendingLabRequest {
    pub async fn wait(self) -> LabResult {
        self.reply.wait().await
    }
}

pub(crate) fn submit(command: LabCommand) -> Result<PendingLabRequest, &'static str> {
    let mut requests = REQUESTS.lock();
    if requests.len() >= REQUEST_CAPACITY {
        return Err("request-queue-full");
    }
    let run_id = next_run_id();
    let reply = Arc::new(Signal::new());
    requests.push_back(QueuedRequest {
        run_id,
        command,
        reply: Arc::clone(&reply),
    });
    Ok(PendingLabRequest { run_id, reply })
}

pub(crate) fn latest_snapshot() -> Option<XhciControllerSnapshot> {
    LATEST_SNAPSHOT.lock().clone()
}

pub(crate) async fn refresh_snapshot(host: &mut crabusb::USBHost) -> Result<(), String> {
    snapshot(host).await.map(|_| ())
}

fn next_run_id() -> u64 {
    loop {
        let id = NEXT_RUN_ID.fetch_add(1, Ordering::AcqRel);
        if id != 0 {
            return id;
        }
    }
}

/// Execute at most one request. The USB controller service calls this while it
/// owns `host`, serializing diagnostics with normal root-hub probing.
pub(crate) async fn service_one(host: &mut crabusb::USBHost) -> bool {
    let Some(request) = REQUESTS.lock().pop_front() else {
        return false;
    };
    let result = execute(host, request.run_id, request.command).await;
    request.reply.signal(result);
    true
}

pub(crate) struct UasIoGuard;

impl Drop for UasIoGuard {
    fn drop(&mut self) {
        UAS_IO_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Admit one UAS operation unless a laboratory run is requesting quiescence.
/// The second check closes the race between admission and quarantine entry.
fn try_begin_uas_io() -> crate::disc::block::Result<UasIoGuard> {
    if QUARANTINE_REQUESTED.load(Ordering::Acquire) {
        return Err(crate::disc::block::Error::NotReady);
    }
    UAS_IO_INFLIGHT.fetch_add(1, Ordering::AcqRel);
    if QUARANTINE_REQUESTED.load(Ordering::Acquire) {
        UAS_IO_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
        return Err(crate::disc::block::Error::NotReady);
    }
    Ok(UasIoGuard)
}

pub(crate) async fn begin_uas_io_when_available() -> UasIoGuard {
    loop {
        match try_begin_uas_io() {
            Ok(guard) => return guard,
            Err(_) => Timer::after(Duration::from_millis(1)).await,
        }
    }
}

pub(crate) struct ControllerQuarantineGuard;

impl Drop for ControllerQuarantineGuard {
    fn drop(&mut self) {
        QUARANTINE_ACTIVE.store(false, Ordering::Release);
        QUARANTINE_REQUESTED.store(false, Ordering::Release);
    }
}

pub(crate) async fn enter_controller_quarantine() -> Result<ControllerQuarantineGuard, String> {
    if QUARANTINE_REQUESTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("xhci-lab quarantine is already requested".to_string());
    }
    let deadline = Instant::now() + Duration::from_millis(QUIESCE_TIMEOUT_MS);
    loop {
        let inflight = UAS_IO_INFLIGHT.load(Ordering::Acquire);
        if inflight == 0 {
            QUARANTINE_ACTIVE.store(true, Ordering::Release);
            return Ok(ControllerQuarantineGuard);
        }
        if Instant::now() >= deadline {
            QUARANTINE_REQUESTED.store(false, Ordering::Release);
            return Err(format!(
                "xhci-lab quarantine timed out with {inflight} UAS operation(s) in flight"
            ));
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}

async fn execute(host: &mut crabusb::USBHost, run_id: u64, command: LabCommand) -> LabResult {
    let mut report = LabReport {
        run_id,
        lines: Vec::new(),
    };
    record(&mut report, format!("xhci-wisdom run={run_id} event=begin command={command:?}"));

    let result: Result<(), String> = async {
        match command {
            LabCommand::Status => status(&mut report),
            LabCommand::Stage {
                stage,
                port,
                armed,
                include_fused,
                allow_live_device,
                depth,
            } => {
                LAST_STAGE.store(u32::from(stage), Ordering::Release);
                run_stage(
                    host,
                    &mut report,
                    stage,
                    port,
                    armed,
                    include_fused,
                    allow_live_device,
                    depth,
                )
                .await
            }
            LabCommand::Read32 { offset } => read32(host, &mut report, offset).await,
            LabCommand::Read64 { offset } => read64(host, &mut report, offset).await,
            LabCommand::Write32 {
                offset,
                value,
                armed,
                include_fused,
                allow_live_device,
            } => {
                require_armed(armed)?;
                let _quarantine = enter_controller_quarantine().await?;
                reject_fused_raw_offset(host, offset, include_fused).await?;
                reject_live_raw_offset(host, offset, allow_live_device).await?;
                write32(host, &mut report, offset, value, "raw-write").await
            }
            LabCommand::Write64 {
                offset,
                value,
                armed,
                include_fused,
                allow_live_device,
            } => {
                require_armed(armed)?;
                let _quarantine = enter_controller_quarantine().await?;
                reject_fused_raw_offset(host, offset, include_fused).await?;
                reject_live_raw_offset(host, offset, allow_live_device).await?;
                write64(host, &mut report, offset, value, "raw-write64").await
            }
            LabCommand::ReadModifyWrite32 {
                offset,
                clear_mask,
                set_mask,
                armed,
                include_fused,
                allow_live_device,
            } => {
                require_armed(armed)?;
                let _quarantine = enter_controller_quarantine().await?;
                reject_fused_raw_offset(host, offset, include_fused).await?;
                reject_live_raw_offset(host, offset, allow_live_device).await?;
                rmw32(host, &mut report, offset, clear_mask, set_mask, "raw-rmw").await
            }
            LabCommand::Journal => {
                for line in JOURNAL.lock().iter() {
                    report.lines.push(line.clone());
                }
                Ok(())
            }
        }
    }
    .await;

    match result {
        Ok(()) => {
            record(&mut report, format!("xhci-wisdom run={run_id} event=end status=ok"));
            Ok(report)
        }
        Err(reason) => {
            record(
                &mut report,
                format!("xhci-wisdom run={run_id} event=end status=failed reason={reason}"),
            );
            Err(reason)
        }
    }
}

fn status(report: &mut LabReport) -> Result<(), String> {
    record(
        report,
        format!(
            "xhci-wisdom status queued={} quarantine_requested={} quarantine_active={} uas_inflight={} last_stage={} fused_led_port={}",
            REQUESTS.lock().len(),
            QUARANTINE_REQUESTED.load(Ordering::Acquire) as u8,
            QUARANTINE_ACTIVE.load(Ordering::Acquire) as u8,
            UAS_IO_INFLIGHT.load(Ordering::Acquire),
            LAST_STAGE.load(Ordering::Acquire),
            FUSED_LED_PORT,
        ),
    );
    record(
        report,
        "xhci-wisdom status quarantine_scope=controller-owner+skhynix-uas event_handler=shared-register-lock other_class_io=not-gated"
            .to_string(),
    );
    Ok(())
}

fn require_armed(armed: bool) -> Result<(), String> {
    if armed {
        Ok(())
    } else {
        Err("mutating operation requires the literal `arm` token".to_string())
    }
}

async fn run_stage(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
    stage: u8,
    port: Option<u8>,
    armed: bool,
    include_fused: bool,
    allow_live_device: bool,
    depth: u8,
) -> Result<(), String> {
    match stage {
        1 => stage_census(host, report).await,
        2 => {
            let _quarantine = enter_controller_quarantine().await?;
            stage_stability(host, report).await
        }
        3..=5 => {
            require_armed(armed)?;
            let port = port.ok_or_else(|| format!("stage {stage} requires a target port"))?;
            reject_fused_port(port, include_fused)?;
            let _quarantine = enter_controller_quarantine().await?;
            if stage >= 4 {
                reject_live_port(host, port, allow_live_device).await?;
            }
            match stage {
                3 => stage_neutral_and_rw1c(host, report, port).await,
                4 => stage_reset_ladder(host, report, port).await,
                5 => stage_tree(host, report, port, depth.clamp(1, 3)).await,
                _ => unreachable!(),
            }
        }
        _ => Err("stage must be in 1..=5".to_string()),
    }
}

async fn stage_census(host: &mut crabusb::USBHost, report: &mut LabReport) -> Result<(), String> {
    let snapshot = snapshot(host).await?;
    emit_snapshot(report, "stage1-census", &snapshot);
    Ok(())
}

async fn stage_stability(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
) -> Result<(), String> {
    record(
        report,
        format!(
            "xhci-wisdom stage=2 event=quarantined uas_inflight={} samples={} interval_ms={}",
            UAS_IO_INFLIGHT.load(Ordering::Acquire),
            STABILITY_SAMPLES,
            STABILITY_SAMPLE_INTERVAL_MS
        ),
    );
    let first = snapshot(host).await?;
    emit_snapshot(report, "stage2-baseline", &first);
    let mut previous = first;
    let mut changes = 0usize;
    let mut fused_changes = 0usize;
    for sample in 1..=STABILITY_SAMPLES {
        Timer::after(Duration::from_millis(STABILITY_SAMPLE_INTERVAL_MS)).await;
        let next = snapshot(host).await?;
        changes += emit_port_diffs(
            report,
            "stage2-sample",
            sample,
            None,
            &previous,
            &next,
            &mut fused_changes,
        );
        previous = next;
    }
    record(
        report,
        format!(
            "xhci-wisdom stage=2 event=summary transitions={} fused_ambient_transitions={}",
            changes, fused_changes
        ),
    );
    Ok(())
}

async fn stage_neutral_and_rw1c(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
    port: u8,
) -> Result<(), String> {
    let baseline = snapshot(host).await?;
    require_port(&baseline, port)?;
    emit_snapshot(report, "stage3-baseline", &baseline);
    apply_port_action(host, report, 3, 0, port, PortAction::Neutral).await?;

    let after_neutral = snapshot(host).await?;
    let active_changes = port_snapshot(&after_neutral, port)?.portsc & PORT_CHANGE_MASK;
    record(
        report,
        format!(
            "xhci-wisdom stage=3 port={port} event=rw1c-census active_change_mask=0x{active_changes:08X}"
        ),
    );
    for bit in 17..=23 {
        let mask = 1u32 << bit;
        if active_changes & mask != 0 {
            apply_port_action(host, report, 3, bit as usize, port, PortAction::Ack(mask)).await?;
        }
    }
    Ok(())
}

async fn stage_reset_ladder(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
    port: u8,
) -> Result<(), String> {
    let baseline = snapshot(host).await?;
    require_port(&baseline, port)?;
    emit_snapshot(report, "stage4-baseline", &baseline);
    let major = protocol_major_for_port(&baseline, port);
    record(report, format!("xhci-wisdom stage=4 port={port} event=protocol major={major}"));

    apply_port_action(host, report, 4, 1, port, PortAction::Neutral).await?;
    apply_port_action(host, report, 4, 2, port, PortAction::PowerOn).await?;
    observe_window(host, report, "stage4-power-settle", 4, 2, port, RESET_SETTLE_MS).await?;
    apply_port_action(host, report, 4, 3, port, PortAction::Ack(PORT_CHANGE_MASK)).await?;
    if major >= 3 {
        apply_port_action(host, report, 4, 4, port, PortAction::WarmReset).await?;
        poll_reset(host, report, 4, 4, port, PORT_WPR).await?;
    }
    apply_port_action(host, report, 4, 5, port, PortAction::Reset).await?;
    poll_reset(host, report, 4, 5, port, PORT_PR).await?;
    apply_port_action(host, report, 4, 6, port, PortAction::Ack(PORT_CHANGE_MASK)).await?;
    Ok(())
}

async fn stage_tree(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
    port: u8,
    depth: u8,
) -> Result<(), String> {
    let baseline = snapshot(host).await?;
    require_port(&baseline, port)?;
    emit_snapshot(report, "stage5-baseline", &baseline);
    let major = protocol_major_for_port(&baseline, port);
    let mut actions = vec![
        PortAction::Neutral,
        PortAction::Ack(PORT_CHANGE_MASK),
        PortAction::PowerOff,
        PortAction::PowerOn,
        PortAction::Disable,
        PortAction::Reset,
    ];
    if major >= 3 {
        actions.extend_from_slice(&[
            PortAction::WarmReset,
            PortAction::LinkRxDetect,
            PortAction::LinkU0,
        ]);
    }
    let paths = generate_paths(actions.as_slice(), depth, TREE_MAX_PATHS);
    record(
        report,
        format!(
            "xhci-wisdom stage=5 port={port} event=tree-begin protocol_major={major} actions={} depth={} paths={} capped={}",
            actions.len(),
            depth,
            paths.len(),
            (paths.len() == TREE_MAX_PATHS) as u8
        ),
    );

    for (branch, path) in paths.iter().enumerate() {
        restore_port_baseline(host, report, port, major, branch).await?;
        let path_text = path
            .iter()
            .map(|action| action.name())
            .collect::<Vec<_>>()
            .join(">");
        record(
            report,
            format!(
                "xhci-wisdom stage=5 branch={} port={} event=branch-begin path={}",
                branch + 1,
                port,
                path_text
            ),
        );
        for (step, action) in path.iter().copied().enumerate() {
            apply_port_action(host, report, 5, branch + 1, port, action).await?;
            if matches!(action, PortAction::Reset) {
                poll_reset(host, report, 5, branch + 1, port, PORT_PR).await?;
            } else if matches!(action, PortAction::WarmReset) {
                poll_reset(host, report, 5, branch + 1, port, PORT_WPR).await?;
            } else {
                observe_window(host, report, "tree-step-settle", 5, branch + 1, port, 10).await?;
            }
            record(
                report,
                format!(
                    "xhci-wisdom stage=5 branch={} step={} port={} action={} event=step-end",
                    branch + 1,
                    step + 1,
                    port,
                    action.name()
                ),
            );
        }
    }

    restore_port_baseline(host, report, port, major, paths.len()).await?;
    record(
        report,
        format!("xhci-wisdom stage=5 port={port} event=tree-end branches={}", paths.len()),
    );
    Ok(())
}

async fn restore_port_baseline(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
    port: u8,
    major: u8,
    branch: usize,
) -> Result<(), String> {
    apply_port_action(host, report, 5, branch, port, PortAction::PowerOn).await?;
    observe_window(host, report, "baseline-power-settle", 5, branch, port, RESET_SETTLE_MS).await?;
    let reset = if major >= 3 {
        PortAction::WarmReset
    } else {
        PortAction::Reset
    };
    apply_port_action(host, report, 5, branch, port, reset).await?;
    poll_reset(host, report, 5, branch, port, if major >= 3 { PORT_WPR } else { PORT_PR }).await?;
    apply_port_action(host, report, 5, branch, port, PortAction::Ack(PORT_CHANGE_MASK)).await?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PortAction {
    Neutral,
    Ack(u32),
    PowerOff,
    PowerOn,
    Disable,
    Reset,
    WarmReset,
    LinkRxDetect,
    LinkU0,
}

impl PortAction {
    fn name(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Ack(_) => "ack-changes",
            Self::PowerOff => "power-off",
            Self::PowerOn => "power-on",
            Self::Disable => "disable",
            Self::Reset => "reset",
            Self::WarmReset => "warm-reset",
            Self::LinkRxDetect => "link-rxdetect",
            Self::LinkU0 => "link-u0",
        }
    }

    fn value(self, portsc: u32) -> u32 {
        let neutral = neutral_portsc(portsc);
        match self {
            Self::Neutral => neutral,
            Self::Ack(mask) => neutral | (portsc & mask & PORT_CHANGE_MASK),
            Self::PowerOff => neutral & !PORT_PP,
            Self::PowerOn => neutral | PORT_PP,
            Self::Disable => neutral | PORT_PED,
            Self::Reset => neutral | PORT_PR,
            Self::WarmReset => neutral | PORT_WPR,
            Self::LinkRxDetect => (neutral & !PORT_PLS_MASK) | (5 << 5) | PORT_LWS,
            Self::LinkU0 => (neutral & !PORT_PLS_MASK) | PORT_LWS,
        }
    }
}

fn neutral_portsc(portsc: u32) -> u32 {
    (portsc & PORT_RO_MASK) | (portsc & PORT_RWS_MASK)
}

async fn apply_port_action(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
    stage: u8,
    node: usize,
    port: u8,
    action: PortAction,
) -> Result<XhciWriteResult, String> {
    let before_snapshot = snapshot(host).await?;
    let before = *port_snapshot(&before_snapshot, port)?;
    let offset = portsc_offset(&before_snapshot, port)?;
    let requested = action.value(before.portsc);
    let write = direct_write(host, offset, requested).await?;
    Timer::after(Duration::from_millis(2)).await;
    let after_snapshot = snapshot(host).await?;
    let after = *port_snapshot(&after_snapshot, port)?;
    let mut fused_changes = 0usize;
    let ambient_changes = emit_port_diffs(
        report,
        "action-window",
        node,
        Some(port),
        &before_snapshot,
        &after_snapshot,
        &mut fused_changes,
    );
    record(
        report,
        format!(
            "xhci-wisdom stage={stage} node={node} port={port} action={} offset=0x{:X} before=0x{:08X} requested=0x{:08X} immediate=0x{:08X} observed=0x{:08X} c={} e={} w={} r={} pls={} speed={} changes=0x{:02X} ambient={} fused_ambient={}",
            action.name(),
            offset,
            write.before,
            write.requested,
            write.after,
            after.portsc,
            (after.portsc & PORT_CCS != 0) as u8,
            (after.portsc & PORT_PED != 0) as u8,
            (after.portsc & PORT_PP != 0) as u8,
            (after.portsc & (PORT_PR | PORT_WPR) != 0) as u8,
            (after.portsc >> 5) & 0x0f,
            (after.portsc >> 10) & 0x0f,
            (after.portsc & PORT_CHANGE_MASK) >> 17,
            ambient_changes,
            fused_changes,
        ),
    );
    Ok(write)
}

async fn poll_reset(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
    stage: u8,
    node: usize,
    port: u8,
    reset_mask: u32,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(RESET_TIMEOUT_MS);
    let mut previous = snapshot(host).await?;
    let mut sample = 0usize;
    loop {
        let current_portsc = port_snapshot(&previous, port)?.portsc;
        if current_portsc & reset_mask == 0 {
            record(
                report,
                format!(
                    "xhci-wisdom stage={stage} node={node} port={port} event=reset-complete samples={sample} portsc=0x{current_portsc:08X}"
                ),
            );
            return Ok(());
        }
        if Instant::now() >= deadline {
            record(
                report,
                format!(
                    "xhci-wisdom stage={stage} node={node} port={port} event=reset-timeout samples={sample} portsc=0x{current_portsc:08X}"
                ),
            );
            return Ok(());
        }
        Timer::after(Duration::from_millis(RESET_POLL_MS)).await;
        sample += 1;
        let next = snapshot(host).await?;
        let mut fused_changes = 0usize;
        emit_port_diffs(
            report,
            "reset-poll",
            sample,
            Some(port),
            &previous,
            &next,
            &mut fused_changes,
        );
        previous = next;
    }
}

async fn observe_window(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
    label: &'static str,
    stage: u8,
    node: usize,
    port: u8,
    duration_ms: u64,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(duration_ms);
    let mut previous = snapshot(host).await?;
    let mut sample = 0usize;
    let mut transitions = 0usize;
    let mut fused_changes = 0usize;
    loop {
        if Instant::now() >= deadline {
            break;
        }
        Timer::after(Duration::from_millis(RESET_POLL_MS)).await;
        sample += 1;
        let next = snapshot(host).await?;
        transitions += emit_port_diffs(
            report,
            label,
            sample,
            Some(port),
            &previous,
            &next,
            &mut fused_changes,
        );
        previous = next;
    }
    record(
        report,
        format!(
            "xhci-wisdom stage={stage} node={node} port={port} event=observe-window label={label} duration_ms={duration_ms} samples={sample} transitions={transitions} fused_ambient={fused_changes}"
        ),
    );
    Ok(())
}

fn generate_paths(actions: &[PortAction], depth: u8, max_paths: usize) -> Vec<Vec<PortAction>> {
    fn extend(
        actions: &[PortAction],
        remaining: u8,
        current: &mut Vec<PortAction>,
        out: &mut Vec<Vec<PortAction>>,
        max_paths: usize,
    ) {
        if remaining == 0 || out.len() >= max_paths {
            return;
        }
        for action in actions.iter().copied() {
            if out.len() >= max_paths {
                break;
            }
            current.push(action);
            out.push(current.clone());
            extend(actions, remaining - 1, current, out, max_paths);
            current.pop();
        }
    }

    let mut out = Vec::new();
    extend(actions, depth, &mut Vec::new(), &mut out, max_paths);
    out
}

async fn reject_fused_raw_offset(
    host: &mut crabusb::USBHost,
    offset: usize,
    include_fused: bool,
) -> Result<(), String> {
    if include_fused {
        return Ok(());
    }
    let snapshot = snapshot(host).await?;
    if let Ok(fused_base) = portsc_offset(&snapshot, FUSED_LED_PORT) {
        if (fused_base..fused_base + 0x10).contains(&offset) {
            return Err(format!(
                "offset 0x{offset:X} belongs to fused LED port {}; add literal `fused` override",
                FUSED_LED_PORT
            ));
        }
    }
    if raw_port_for_offset(&snapshot, offset).is_none()
        && port_snapshot(&snapshot, FUSED_LED_PORT).is_ok_and(|port| port.portsc & PORT_CCS != 0)
    {
        return Err(format!(
            "controller-global write can disrupt fused LED port {}; add literal `fused` override",
            FUSED_LED_PORT
        ));
    }
    Ok(())
}

async fn reject_live_raw_offset(
    host: &mut crabusb::USBHost,
    offset: usize,
    allow_live_device: bool,
) -> Result<(), String> {
    if allow_live_device {
        return Ok(());
    }
    let snapshot = snapshot(host).await?;
    if let Some(port) = raw_port_for_offset(&snapshot, offset) {
        reject_live_port_snapshot(&snapshot, port, false)?;
    } else if let Some(port) = snapshot
        .ports
        .iter()
        .find(|port| port.port_id != FUSED_LED_PORT && port.portsc & PORT_CCS != 0)
    {
        return Err(format!(
            "controller-global write can disrupt connected port {}; add literal `live` override",
            port.port_id
        ));
    }
    Ok(())
}

async fn reject_live_port(
    host: &mut crabusb::USBHost,
    port: u8,
    allow_live_device: bool,
) -> Result<(), String> {
    let snapshot = snapshot(host).await?;
    reject_live_port_snapshot(&snapshot, port, allow_live_device)
}

fn reject_live_port_snapshot(
    snapshot: &XhciControllerSnapshot,
    port: u8,
    allow_live_device: bool,
) -> Result<(), String> {
    let portsc = port_snapshot(snapshot, port)?.portsc;
    if portsc & PORT_CCS != 0 && !allow_live_device {
        Err(format!(
            "port {port} is physically connected (PORTSC.CCS=1); add literal `live` to acknowledge disruption"
        ))
    } else {
        Ok(())
    }
}

fn raw_port_for_offset(snapshot: &XhciControllerSnapshot, offset: usize) -> Option<u8> {
    snapshot.ports.iter().find_map(|port| {
        let base = portsc_offset(snapshot, port.port_id).ok()?;
        (base..base + 0x10)
            .contains(&offset)
            .then_some(port.port_id)
    })
}

fn reject_fused_port(port: u8, include_fused: bool) -> Result<(), String> {
    if port == FUSED_LED_PORT && !include_fused {
        Err(format!(
            "port {FUSED_LED_PORT} is the fused LED ambient actor; add literal `fused` override"
        ))
    } else {
        Ok(())
    }
}

async fn read32(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
    offset: usize,
) -> Result<(), String> {
    let response = host
        .xhci_direct(XhciDirectRequest::Read32 { offset })
        .await
        .map_err(|err| format!("direct read failed: {err:?}"))?;
    match response {
        XhciDirectResponse::Read32 { offset, value } => {
            record(
                report,
                format!("xhci-wisdom op=read32 offset=0x{offset:X} value=0x{value:08X}"),
            );
            Ok(())
        }
        other => Err(format!("direct read returned unexpected response: {other:?}")),
    }
}

async fn read64(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
    offset: usize,
) -> Result<(), String> {
    let response = host
        .xhci_direct(XhciDirectRequest::Read64 { offset })
        .await
        .map_err(|err| format!("direct read64 failed: {err:?}"))?;
    match response {
        XhciDirectResponse::Read64 { offset, value } => {
            record(
                report,
                format!("xhci-wisdom op=read64 offset=0x{offset:X} value=0x{value:016X}"),
            );
            Ok(())
        }
        other => Err(format!("direct read64 returned unexpected response: {other:?}")),
    }
}

async fn write32(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
    offset: usize,
    value: u32,
    label: &'static str,
) -> Result<(), String> {
    let write = direct_write(host, offset, value).await?;
    record_write(report, label, write);
    Ok(())
}

async fn direct_write(
    host: &mut crabusb::USBHost,
    offset: usize,
    value: u32,
) -> Result<XhciWriteResult, String> {
    let response = host
        .xhci_direct(XhciDirectRequest::Write32 { offset, value })
        .await
        .map_err(|err| format!("direct write failed: {err:?}"))?;
    match response {
        XhciDirectResponse::Write32(write) => Ok(write),
        other => Err(format!("direct write returned unexpected response: {other:?}")),
    }
}

async fn write64(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
    offset: usize,
    value: u64,
    label: &'static str,
) -> Result<(), String> {
    let response = host
        .xhci_direct(XhciDirectRequest::Write64 { offset, value })
        .await
        .map_err(|err| format!("direct write64 failed: {err:?}"))?;
    match response {
        XhciDirectResponse::Write64(write) => {
            record_write64(report, label, write);
            Ok(())
        }
        other => Err(format!("direct write64 returned unexpected response: {other:?}")),
    }
}

async fn rmw32(
    host: &mut crabusb::USBHost,
    report: &mut LabReport,
    offset: usize,
    clear_mask: u32,
    set_mask: u32,
    label: &'static str,
) -> Result<(), String> {
    let response = host
        .xhci_direct(XhciDirectRequest::ReadModifyWrite32 {
            offset,
            clear_mask,
            set_mask,
        })
        .await
        .map_err(|err| format!("direct RMW failed: {err:?}"))?;
    match response {
        XhciDirectResponse::Write32(write) => {
            record_write(report, label, write);
            Ok(())
        }
        other => Err(format!("direct RMW returned unexpected response: {other:?}")),
    }
}

fn record_write(report: &mut LabReport, label: &'static str, write: XhciWriteResult) {
    record(
        report,
        format!(
            "xhci-wisdom op={label} offset=0x{:X} before=0x{:08X} requested=0x{:08X} after=0x{:08X}",
            write.offset, write.before, write.requested, write.after
        ),
    );
}

fn record_write64(report: &mut LabReport, label: &'static str, write: XhciWrite64Result) {
    record(
        report,
        format!(
            "xhci-wisdom op={label} offset=0x{:X} before=0x{:016X} requested=0x{:016X} after=0x{:016X}",
            write.offset, write.before, write.requested, write.after
        ),
    );
}

async fn snapshot(host: &mut crabusb::USBHost) -> Result<XhciControllerSnapshot, String> {
    let response = host
        .xhci_direct(XhciDirectRequest::Snapshot)
        .await
        .map_err(|err| format!("xHCI snapshot failed: {err:?}"))?;
    match response {
        XhciDirectResponse::Snapshot(snapshot) => {
            *LATEST_SNAPSHOT.lock() = Some(snapshot.clone());
            Ok(snapshot)
        }
        other => Err(format!("snapshot returned unexpected response: {other:?}")),
    }
}

fn emit_snapshot(report: &mut LabReport, label: &'static str, snapshot: &XhciControllerSnapshot) {
    record(
        report,
        format!(
            "xhci-wisdom snapshot={label} mmio_len=0x{:X} caplen=0x{:02X} hciver=0x{:04X} hcs1=0x{:08X} hcs2=0x{:08X} hcs3=0x{:08X} hcc1=0x{:08X} hcc2=0x{:08X} dboff=0x{:X} rtsoff=0x{:X}",
            snapshot.mmio_len,
            snapshot.caplength,
            snapshot.hciversion,
            snapshot.hcsparams1,
            snapshot.hcsparams2,
            snapshot.hcsparams3,
            snapshot.hccparams1,
            snapshot.hccparams2,
            snapshot.dboff,
            snapshot.rtsoff,
        ),
    );
    record(
        report,
        format!(
            "xhci-wisdom snapshot={label} usbcmd=0x{:08X} usbsts=0x{:08X} pagesize=0x{:08X} dnctrl=0x{:08X} crcr=0x{:016X} dcbaap=0x{:016X} config=0x{:08X} mfindex=0x{:08X} iman=0x{:08X} imod=0x{:08X} erstsz=0x{:08X} erstba=0x{:016X} erdp=0x{:016X}",
            snapshot.usbcmd,
            snapshot.usbsts,
            snapshot.pagesize,
            snapshot.dnctrl,
            snapshot.crcr,
            snapshot.dcbaap,
            snapshot.config,
            snapshot.mfindex,
            snapshot.iman,
            snapshot.imod,
            snapshot.erstsz,
            snapshot.erstba,
            snapshot.erdp,
        ),
    );
    for protocol in snapshot.protocols.iter() {
        record(
            report,
            format!(
                "xhci-wisdom snapshot={label} protocol={}.{:02X} name=0x{:08X} ports={}..={} count={} slot_type={} psi={}",
                protocol.major,
                protocol.minor,
                protocol.name,
                protocol.port_offset,
                protocol
                    .port_offset
                    .saturating_add(protocol.port_count.saturating_sub(1)),
                protocol.port_count,
                protocol.slot_type,
                protocol.psi_count,
            ),
        );
    }
    for port in snapshot.ports.iter() {
        record(
            report,
            format!(
                "xhci-wisdom snapshot={label} port={} protocol={} portsc=0x{:08X} portpmsc=0x{:08X} portli=0x{:08X} porthlpmc=0x{:08X} c={} e={} w={} r={} pls={} speed={} fused_ambient={}",
                port.port_id,
                protocol_major_for_port(snapshot, port.port_id),
                port.portsc,
                port.portpmsc,
                port.portli,
                port.porthlpmc,
                (port.portsc & PORT_CCS != 0) as u8,
                (port.portsc & PORT_PED != 0) as u8,
                (port.portsc & PORT_PP != 0) as u8,
                (port.portsc & (PORT_PR | PORT_WPR) != 0) as u8,
                (port.portsc >> 5) & 0x0f,
                (port.portsc >> 10) & 0x0f,
                (port.port_id == FUSED_LED_PORT) as u8,
            ),
        );
    }
}

fn emit_port_diffs(
    report: &mut LabReport,
    label: &'static str,
    sample: usize,
    target: Option<u8>,
    before: &XhciControllerSnapshot,
    after: &XhciControllerSnapshot,
    fused_changes: &mut usize,
) -> usize {
    let mut changes = 0usize;
    for old in before.ports.iter() {
        let Some(new) = after
            .ports
            .iter()
            .find(|candidate| candidate.port_id == old.port_id)
        else {
            continue;
        };
        if old == new {
            continue;
        }
        changes += 1;
        let ambient = target.is_some_and(|port| port != old.port_id);
        let fused = old.port_id == FUSED_LED_PORT;
        if fused {
            *fused_changes += 1;
        }
        record(
            report,
            format!(
                "xhci-wisdom observation={label} sample={sample} port={} target={} ambient={} fused_ambient={} portsc=0x{:08X}->0x{:08X} portpmsc=0x{:08X}->0x{:08X} portli=0x{:08X}->0x{:08X} porthlpmc=0x{:08X}->0x{:08X}",
                old.port_id,
                target.map_or(0, u8::from),
                ambient as u8,
                fused as u8,
                old.portsc,
                new.portsc,
                old.portpmsc,
                new.portpmsc,
                old.portli,
                new.portli,
                old.porthlpmc,
                new.porthlpmc,
            ),
        );
    }
    changes
}

fn port_snapshot(snapshot: &XhciControllerSnapshot, port: u8) -> Result<&XhciPortSnapshot, String> {
    snapshot
        .ports
        .iter()
        .find(|candidate| candidate.port_id == port)
        .ok_or_else(|| format!("xHCI port {port} is outside the implemented port set"))
}

fn require_port(snapshot: &XhciControllerSnapshot, port: u8) -> Result<(), String> {
    port_snapshot(snapshot, port).map(|_| ())
}

fn portsc_offset(snapshot: &XhciControllerSnapshot, port: u8) -> Result<usize, String> {
    require_port(snapshot, port)?;
    Ok(usize::from(snapshot.caplength) + 0x400 + (usize::from(port) - 1) * 0x10)
}

fn protocol_major_for_port(snapshot: &XhciControllerSnapshot, port: u8) -> u8 {
    snapshot
        .protocols
        .iter()
        .find(|protocol| {
            let start = u16::from(protocol.port_offset);
            let end = start + u16::from(protocol.port_count);
            let port = u16::from(port);
            port >= start && port < end
        })
        .map_or(0, |protocol| protocol.major)
}

fn record(report: &mut LabReport, line: String) {
    crate::log_info!(target: "usb"; "{}\n", line);
    report.lines.push(line.clone());
    let mut journal = JOURNAL.lock();
    if journal.len() >= JOURNAL_CAPACITY {
        journal.pop_front();
    }
    journal.push_back(line);
}
