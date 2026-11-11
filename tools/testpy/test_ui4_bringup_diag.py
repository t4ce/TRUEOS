#!/usr/bin/env python3
"""Exercise production bootstrap evidence when the attempted plane is inactive."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    harness = r'''
#![allow(dead_code)]
extern crate alloc;
use alloc::string::String;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
mod spin {
    pub struct Once<T>(std::sync::OnceLock<T>);
    impl<T> Once<T> {
        pub const fn new() -> Self { Self(std::sync::OnceLock::new()) }
        pub fn get(&self) -> Option<&T> { self.0.get() }
        pub fn call_once(&self, init: impl FnOnce() -> T) -> &T { self.0.get_or_init(init) }
    }
}
#[macro_export] macro_rules! log_important {
    (target: $target:literal; $($args:tt)*) => { let _ = format_args!($($args)*); };
}
#[macro_export] macro_rules! log_error {
    (target: $target:literal; $($args:tt)*) => { let _ = format_args!($($args)*); };
}
mod intel {
    #[derive(Clone, Copy)] pub struct Dev { pub bus: u8, pub slot: u8, pub function: u8 }
    pub static GENERATION: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    pub static DBUF_S1: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0xC0000000);
    pub static DBUF_S2: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0x80000000);
    pub static DBUF_READS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    pub static READS: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
    pub static SAMPLE_CTL_ENABLED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    pub static SAMPLE_CTL: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    pub fn mmio_read(_: Dev, offset: usize) -> u32 {
        READS.lock().unwrap().push(offset);
        if offset == 0x70180 && SAMPLE_CTL_ENABLED.load(core::sync::atomic::Ordering::Acquire) {
            return SAMPLE_CTL.load(core::sync::atomic::Ordering::Acquire);
        }
        // Independent literal Pipe C PS_WIN_POS/SZ addresses. Reading the
        // HSCALE/reserved addresses above PS_CTRL must not pass these checks.
        match offset {
            0x69170 => return 0xC0FE0001,
            0x69174 => return 0xC0FE0002,
            0x69270 => return 0xC0FE0003,
            0x69274 => return 0xC0FE0004,
            _ => {},
        }
        if let Some(control) = match offset { 0x45008 => Some(&DBUF_S1), 0x44FE8 => Some(&DBUF_S2), _ => None } {
            DBUF_READS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            return control.load(core::sync::atomic::Ordering::Acquire);
        }
        offset as u32 + GENERATION.load(core::sync::atomic::Ordering::Acquire)
    }
    pub mod tgl_native_panel {
        use core::sync::atomic::{AtomicBool, Ordering};
        pub static ADOPTED: AtomicBool = AtomicBool::new(false);
        pub fn ui4_handoff_active() -> bool { ADOPTED.load(Ordering::Acquire) }
    }
    // No write, polling, submission or reset API is available to this fixture.
}
mod pci {
    pub static DEVICE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0x9A49_8086);
    pub fn config_read_u32(_: u8, _: u8, _: u8, _: u16) -> u32 {
        DEVICE.load(core::sync::atomic::Ordering::Acquire)
    }
}
@REGISTERS@
const UNIVERSAL_PLANE_SLOTS: usize = 5;
const MAX_UNIVERSAL_PLANE_SLOTS: usize = 7;
const PIPE_FRMCOUNT_A: usize = 0x70040;
const PIPE_MMIO_STRIDE: usize = 0x1000;
static UI4_RGBA8_PLANE_STACK_STATE: AtomicU32 = AtomicU32::new(3);
static PRIMARY_BOOT_SURFACE_INIT: AtomicBool = AtomicBool::new(true);
#[derive(Clone, Copy)] struct PipeInfo { name: &'static str, slot: usize, pipe_src_off: usize }
fn native_ui4_for_pipe(pipe: PipeInfo) -> bool {
    pipe.slot == 0 && intel::tgl_native_panel::ui4_handoff_active()
}
fn plane_slot_count(pipe: PipeInfo) -> usize {
    if native_ui4_for_pipe(pipe) { MAX_UNIVERSAL_PLANE_SLOTS } else { UNIVERSAL_PLANE_SLOTS }
}
#[path = "@LAYOUT@"] mod tgl_ui4_layout;
const PIPES: [PipeInfo; 4] = [PipeInfo { name: "pipe-a", slot: 0, pipe_src_off: 0x6001C }, PipeInfo { name: "pipe-b", slot: 1, pipe_src_off: 0x6101C }, PipeInfo { name: "pipe-c", slot: 2, pipe_src_off: 0x6201C }, PipeInfo { name: "pipe-d", slot: 3, pipe_src_off: 0x6301C }];
fn active_pipe(_: intel::Dev) -> Option<PipeInfo> { None }
fn ui4_rgba8_plane_stack_ready(_: PipeInfo) -> bool { false }
#[path = "@SOURCE@"] mod bringup_diag;

#[test]
fn failed_bootstrap_retains_attempted_pipe_and_failure_time_registers() {
    let dev = intel::Dev { bus: 0, slot: 2, function: 0 };
    bringup_diag::select_pipe(PIPES[2]);
    bringup_diag::checkpoint("static-contract-and-dbuf", 4);
    bringup_diag::latch([0x1000, 0x2000, 0x3000, 0x4000, 0x5000, 0, 0], 0x0f, 5_000_000);
    bringup_diag::capture_firmware(dev);
    bringup_diag::failed("dbuf-readback", dev);
    intel::GENERATION.store(0x10, Ordering::Release);
    intel::DBUF_S1.store(0x40000000, Ordering::Release);
    intel::DBUF_S2.store(0xC0000000, Ordering::Release);
    let mut current = String::new();
    let dbuf_reads_before = intel::DBUF_READS.load(Ordering::Relaxed);
    bringup_diag::append(&mut current, dev);
    assert_eq!(intel::DBUF_READS.load(Ordering::Relaxed) - dbuf_reads_before, 2,
        "each DBUF control must be read once without polling");
    assert!(current.contains("display_dbuf slice=s1 register=0x45008 raw=0x40000000 power_request=false power_state=true capture=read-only-no-poll"));
    assert!(current.contains("display_dbuf slice=s2 register=0x44FE8 raw=0xC0000000 power_request=true power_state=true capture=read-only-no-poll"));
    let scalers: Vec<_> = current.lines().filter(|line| line.starts_with("display_scaler pipe=pipe-c ")).collect();
    assert_eq!(scalers.len(), 2);
    assert!(scalers[0].contains("index=0 ") && scalers[0].contains("pos=0xC0FE0001 size=0xC0FE0002"));
    assert!(scalers[1].contains("index=1 ") && scalers[1].contains("pos=0xC0FE0003 size=0xC0FE0004"));
    assert!(current.contains("stage=static-contract-and-dbuf failure=dbuf-readback plane_slot=4 pipe_slot=2"));
    assert!(current.contains("pipe=pipe-c"));
    assert!(current.contains("last_live_mask=0xF last_polls=5000000"));
    assert!(current.contains("logical_ui4_slots=5 physical_universal_planes_per_pipe=7"));
    assert_eq!(current.lines().filter(|line| line.starts_with("physical_plane pipe=")).count(), 28);
    assert!(current.contains("physical_plane pipe=pipe-c slot=4 hardware_plane=5 logical_ui4_slot=Some(4) enabled=false base=0x72580 ctl=0x00072590"));
    assert!(current.contains("physical_plane pipe=pipe-c slot=6 hardware_plane=7 logical_ui4_slot=None enabled=false base=0x72780 ctl=0x00072790"));
    let mut retained = String::new();
    bringup_diag::append_retained(&mut retained);
    assert!(retained.contains("display_dbuf slice=s1 register=0x45008 raw=0xC0000000 power_request=true power_state=true"));
    assert!(retained.contains("display_dbuf slice=s2 register=0x44FE8 raw=0x80000000 power_request=true power_state=false"));
    assert!(!retained.contains("raw=0x40000000 power_request=false power_state=true"));
    assert!(retained.contains("base=0x72580 ctl=0x00072580"));
    assert!(!retained.contains("ctl=0x00072590"));

    // Adoption changes only logical annotation. The register inventory must
    // continue reading physical banks in order, including every extra plane.
    intel::tgl_native_panel::ADOPTED.store(true, Ordering::Release);
    bringup_diag::select_pipe(PIPES[0]);
    bringup_diag::checkpoint("plane-arm", 6);
    bringup_diag::latch([0x1000, 0x2000, 0x3000, 0x4000, 0x5000, 0x6000, 0x7000], 0x7f, 1);
    let mut adopted = String::new();
    bringup_diag::append(&mut adopted, dev);
    assert!(adopted.contains("last_live_mask=0x7F last_polls=1 wanted=[00001000, 00002000, 00003000, 00004000, 00005000, 00006000, 00007000]"));
    assert!(adopted.contains("logical_ui4_slots=7 physical_universal_planes_per_pipe=7"));
    assert_eq!(adopted.lines().filter(|line| line.starts_with("physical_plane pipe=")).count(), 28);
    for (physical, logical) in [(4, 5), (5, 6), (6, 4)] {
        let base = UNI_PLANE_BASE + physical * UNI_PLANE_SLOT_STRIDE;
        assert!(adopted.contains(&format!(
            "physical_plane pipe=pipe-a slot={} hardware_plane={} logical_ui4_slot=Some({}) enabled=false base=0x{:X} ctl=0x{:08X}",
            physical, physical + 1, logical, base, base + 0x10)));
    }
    assert!(adopted.contains("physical_plane pipe=pipe-b slot=6 hardware_plane=7 logical_ui4_slot=None"));

    // The supplemental snapshot has independent literal register expectations.
    // Every requested word is read exactly once; none overlaps the old capture.
    const EXTRA_REGISTERS: [usize; 17] = [
        0x44400, 0x44404, 0x44408, 0x4440C, 0x60848, 0x60910,
        0x43208, 0x43210, 0x43214, 0x43228, 0x46000, 0x46070, 0x51004,
        0x45038, 0x45048, 0x4504C, 0x7003C,
    ];
    // Hardware plane 5 deliberately jumps over the cursor bank at 0x70900.
    const SELECTIVE_FETCH_WORDS: [[usize; 4]; 7] = [
        [0x70890, 0x70894, 0x70898, 0x7089C],
        [0x708B0, 0x708B4, 0x708B8, 0x708BC],
        [0x708D0, 0x708D4, 0x708D8, 0x708DC],
        [0x708F0, 0x708F4, 0x708F8, 0x708FC],
        [0x70920, 0x70924, 0x70928, 0x7092C],
        [0x70940, 0x70944, 0x70948, 0x7094C],
        [0x70960, 0x70964, 0x70968, 0x7096C],
    ];
    intel::READS.lock().unwrap().clear();
    intel::SAMPLE_CTL_ENABLED.store(true, Ordering::Release);
    // Odd ICL format value exercises expanded format bit 23 independently of
    // the old four-bit field; all inherited transform/compression bits are set.
    intel::SAMPLE_CTL.store(0x8E808113, Ordering::Release);
    intel::DBUF_S1.store(0xC0400000, Ordering::Release);
    let mut sampled = String::new();
    bringup_diag::append(&mut sampled, dev);
    let reads = intel::READS.lock().unwrap();
    let extras: Vec<_> = sampled.lines().filter(|line| line.starts_with("display_tgl_register ")).collect();
    assert_eq!(extras.len(), 17);
    for register in EXTRA_REGISTERS {
        assert_eq!(reads.iter().filter(|&&offset| offset == register).count(), 1,
            "extra register 0x{register:X} must be read once without polling");
        assert_eq!(extras.iter().filter(|line| line.contains(&format!(
            "register=0x{:X} raw=0x{:08X} capture=read-only-no-ack", register, register + 0x10))).count(), 1);
    }
    let selective: Vec<_> = sampled.lines().filter(|line| line.starts_with("physical_plane_selective_fetch ")).collect();
    assert_eq!(selective.len(), 28);
    for pipe_delta in [0, 0x1000, 0x2000, 0x3000] {
        for quartet in SELECTIVE_FETCH_WORDS {
            for register in quartet {
                assert_eq!(reads.iter().filter(|&&offset| offset == register + pipe_delta).count(), 1,
                    "selective-fetch register 0x{:X} must be read once", register + pipe_delta);
            }
            assert!(selective.iter().any(|line| line.contains(&format!(
                "register=0x{:X} ctl=0x{:08X} enabled=false pos=0x{:08X} size=0x{:08X} offset=0x{:08X}",
                quartet[0] + pipe_delta, quartet[0] + pipe_delta + 0x10,
                quartet[1] + pipe_delta + 0x10, quartet[2] + pipe_delta + 0x10,
                quartet[3] + pipe_delta + 0x10))));
        }
    }
    assert!(!reads.contains(&0x70910), "plane 5 must not use the cursor-adjacent gap");
    drop(reads);
    assert!(sampled.lines().any(|line| line.starts_with("physical_plane pipe=pipe-a slot=0 ")
        && line.contains("fetch_format=0x1D render_decompression=true media_decompression=true mirror_x=true rotation=3")));
    assert!(sampled.contains("raw=0xC0400000 power_request=true power_state=true capture=read-only-no-poll tracker_state_service=8"));

    // The desktop inventory retains its five-bank policy and selected-pipe
    // scope; neither the larger evidence array nor laptop mapping widens it.
    // CTL decoding uses the already-read word on desktops as well.
    pci::DEVICE.store(0x4680_8086, Ordering::Release);
    intel::tgl_native_panel::ADOPTED.store(false, Ordering::Release);
    bringup_diag::select_pipe(PIPES[0]);
    intel::READS.lock().unwrap().clear();
    intel::SAMPLE_CTL.store(0x94000000, Ordering::Release);
    let mut desktop = String::new();
    bringup_diag::append(&mut desktop, dev);
    assert!(desktop.contains("logical_ui4_slots=5 physical_universal_planes_per_pipe=5"));
    assert_eq!(desktop.lines().filter(|line| line.starts_with("physical_plane pipe=")).count(), 5);
    assert!(!desktop.contains("physical_plane pipe=pipe-c"));
    assert!(!desktop.contains("hardware_plane=6"));
    assert!(!desktop.contains("display_tgl_register "));
    assert!(!desktop.contains("physical_plane_selective_fetch "));
    assert!(desktop.lines().any(|line| line.starts_with("physical_plane pipe=pipe-a slot=0 ")
        && line.contains("fetch_format=0x08 render_decompression=false media_decompression=false mirror_x=false rotation=0")));
    let desktop_reads = intel::READS.lock().unwrap();
    for register in EXTRA_REGISTERS {
        assert!(!desktop_reads.contains(&register), "desktop must not read extra TGL register 0x{register:X}");
    }
    for pipe_delta in [0, 0x1000, 0x2000, 0x3000] {
        for quartet in SELECTIVE_FETCH_WORDS {
            for register in quartet {
                assert!(!desktop_reads.contains(&(register + pipe_delta)),
                    "desktop must not read selective-fetch register 0x{:X}", register + pipe_delta);
            }
        }
    }
}
'''.replace("@SOURCE@", str(ROOT / "src/intel/display/bringup_diag.rs")).replace(
        "@LAYOUT@", str(ROOT / "src/intel/display/tgl_ui4_layout.rs"))
    registers = (ROOT / "src/intel/display/regs.rs").read_text()
    names = "PIPECONF_A TRANS_DDI_FUNC_CTL_A TRANS_HTOTAL_A TRANS_HSYNC_A TRANS_VTOTAL_A TRANS_VSYNC_A TRANS_PSR_CTL_A TRANS_PSR_STATUS_A TRANS_PSR2_CTL_A TRANS_PSR2_STATUS_A PLANE_CTL_ENABLE UNI_PLANE_WM_TRANS_OFF UNI_PLANE_WM_SAGV_OFF UNI_PLANE_WM_SAGV_TRANS_OFF PIPE_SCALER_0_A_CTRL PIPE_SCALER_ENABLE PIPE_SCALER_BINDING_MASK PIPE_SCALER_WIN_POS_FROM_CTRL PIPE_SCALER_WIN_SIZE_FROM_CTRL PIPE_SCALER_VPHASE_FROM_CTRL PIPE_SCALER_HPHASE_FROM_CTRL".split()
    names += "UNI_PLANE_BASE UNI_PLANE_PIPE_STRIDE UNI_PLANE_SLOT_STRIDE UNI_PLANE_CTL_OFF UNI_PLANE_BUF_CFG_OFF UNI_PLANE_SURF_OFF UNI_PLANE_SURFLIVE_OFF UNI_PLANE_STRIDE_OFF UNI_PLANE_POS_OFF UNI_PLANE_SIZE_OFF UNI_PLANE_OFFSET_OFF UNI_PLANE_COLOR_CTL_OFF UNI_PLANE_WM_0_OFF".split()
    declarations = []
    for name in names:
        declaration = re.search(rf"const {name}:[^;]+;", registers)
        assert declaration, name
        declarations.append(declaration.group())
    harness = harness.replace("@REGISTERS@", "\n".join(declarations))
    with tempfile.TemporaryDirectory(prefix="trueos-ui4-bringup-") as directory:
        work = Path(directory)
        (work / "test.rs").write_text(harness)
        binary = work / "tests"
        subprocess.run(["rustc", "+" + channel.group(1), "--edition=2024", "--test",
                        "--target", "x86_64-unknown-linux-gnu", str(work / "test.rs"),
                        "-o", str(binary)], cwd=work, check=True)
        subprocess.run([str(binary)], cwd=work, check=True)


if __name__ == "__main__":
    main()
