#!/usr/bin/env python3
"""Host-test ADL-S boot, clock-gate and depth-state workarounds with fake MMIO."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def boot_initializer_source() -> str:
    constants = "src/intel/render/constants.rs"
    warmup = "src/intel/render/warmup.rs"
    source = "\n" + "\n".join(constant(constants, name) for name in (
        "RCS_RING_BASE", "RCS_CS_DEBUG_MODE1", "RCS_FF_SLICE_CS_CHICKEN1",
        "RCS_FF_THREAD_MODE", "GEN9_FFSC_PERCTX_PREEMPT_CTRL",
        "FF_DOP_CLOCK_GATE_DISABLE", "GEN12_L3ALLOC", "CHICKEN_RASTER_2",
        "TBIMR_BATCH_SIZE_OVERRIDE", "TBIMR_OPEN_BATCH_ENABLE", "TBIMR_FAST_CLIP",
    ))
    source += "\n" + "\n".join(item(warmup, name) for name in (
        "init_global_rcs_workarounds_for_boot", "global_rcs_workarounds_ready",
        "gfx125_chicken_raster_2_value",
    ))
    source += "\n" + item("src/intel/render/submit.rs", "device_is_gfx125")
    source += "\nmod intel {\n" + item("src/intel/mod.rs", "mask_en")
    return source + r"""
    use std::{cell::RefCell, collections::BTreeMap};
    #[derive(Copy, Clone)]
    pub struct Dev { pub device_id: u16, pub accept_selector_write: bool }
    thread_local! {
        static REGISTERS: RefCell<BTreeMap<usize, u32>> = RefCell::new(BTreeMap::new());
        static WRITES: RefCell<Vec<(usize, u32)>> = const { RefCell::new(Vec::new()) };
    }
    pub fn reset(selector: u32) {
        REGISTERS.with(|registers| {
            let mut registers = registers.borrow_mut();
            registers.clear();
            registers.insert(0x20E0, selector);
        });
        WRITES.with(|writes| writes.borrow_mut().clear());
    }
    pub fn mmio_read(_: Dev, register: usize) -> u32 {
        REGISTERS.with(|registers| *registers.borrow().get(&register).unwrap_or(&0))
    }
    pub fn mmio_write(dev: Dev, register: usize, write: u32) {
        WRITES.with(|writes| writes.borrow_mut().push((register, write)));
        if register == 0x20E0 && !dev.accept_selector_write { return; }
        REGISTERS.with(|registers| {
            let mut registers = registers.borrow_mut();
            let after = if matches!(register, 0x20E0 | 0x20EC | 0x6208) {
                let before = *registers.get(&register).unwrap_or(&0);
                let mask = write >> 16;
                (before & !mask) | (write & mask)
            } else { write };
            registers.insert(register, after);
        });
    }
    pub fn writes_to(register: usize) -> Vec<u32> {
        WRITES.with(|writes| writes.borrow().iter().filter_map(|&(address, value)|
            (address == register).then_some(value)).collect())
    }
}
#[macro_export]
macro_rules! log_important { ($($tokens:tt)*) => {}; }
#[macro_export]
macro_rules! log_info { ($($tokens:tt)*) => {}; }

#[test]
fn adls_boot_selects_context_control_and_preserves_other_bits() {
    for device_id in [0x4680, 0x4682, 0x4688, 0x468A, 0x468B, 0x4690, 0x4692, 0x4693] {
        for before in [0, 0xA5A5_1234, 0xFFFF_FFFF] {
            intel::reset(before);
            let dev = intel::Dev { device_id, accept_selector_write: true };
            assert!(init_global_rcs_workarounds_for_boot(dev));
            assert_eq!(intel::writes_to(0x20E0), [0x4000_4000]);
            assert_eq!(intel::mmio_read(dev, 0x20E0), before | 0x4000);
            assert!(global_rcs_workarounds_ready(dev));
            assert!(init_global_rcs_workarounds_for_boot(dev));
            assert_eq!(intel::mmio_read(dev, 0x20E0), before | 0x4000);
        }
    }
}

#[test]
fn missing_selector_readback_blocks_adls_boot_readiness() {
    intel::reset(0);
    let dev = intel::Dev { device_id: 0x4680, accept_selector_write: false };
    assert!(!init_global_rcs_workarounds_for_boot(dev));
    assert!(!global_rcs_workarounds_ready(dev));
    assert_eq!(intel::mmio_read(dev, 0x20EC) & 2, 2);
    assert_eq!(intel::mmio_read(dev, 0x20A0) & (1 << 19), 1 << 19);
    assert_eq!(intel::mmio_read(dev, 0x20E0), 0);
    assert_eq!(intel::writes_to(0x20E0), [0x4000_4000]);
}

#[test]
fn other_platforms_do_not_gain_a_selector_write_or_readiness_requirement() {
    for device_id in [0, 0x9A49, 0x46D1, 0xA780, 0x56A0, 0x7D55, 0xFFFF] {
        intel::reset(0);
        let dev = intel::Dev { device_id, accept_selector_write: false };
        assert!(init_global_rcs_workarounds_for_boot(dev));
        assert!(intel::writes_to(0x20E0).is_empty());
        assert_eq!(intel::mmio_read(dev, 0x20E0), 0);
    }
}
"""


def main() -> None:
    source = "#![allow(dead_code, unused_variables)]\n" + "\n".join([
        constant("src/intel/render/constants.rs", "GEN12_FF_TESSELLATION_DOP_GATE_DISABLE"),
        item("src/intel/render/warmup.rs", "adls_ff_thread_mode_workaround"),
        item("src/intel/render/warmup.rs", "adls_ff_thread_mode_workaround_tests"),
        constant("src/intel/render/constants.rs", "PIPE_CONTROL_CMD"),
        constant("src/intel/render/constants.rs", "PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE"),
        constant("src/intel/render/constants.rs", "RESULT_DEBUG_DWORD_COUNT"),
        constant("src/intel/render/constants.rs", "RESULT_OA_BEGIN_DWORD"),
        constant("src/intel/render/pipeline.rs", "RESULT_SLOT_DEPTH_STATE_WA_DWORD"),
        item("src/intel/render/pipeline.rs", "adls_depth_state_post_sync_packet"),
        item("src/intel/render/pipeline.rs", "adls_depth_state_post_sync_tests"),
    ])
    source += boot_initializer_source()
    with tempfile.TemporaryDirectory(prefix="trueos-adls-rcs-tests-") as temporary:
        directory = Path(temporary)
        rust_source = directory / "host_tests.rs"
        executable = directory / "host_tests"
        rust_source.write_text(source)
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(rust_source), "-o", str(executable)],
            cwd=ROOT,
            check=True,
        )
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
