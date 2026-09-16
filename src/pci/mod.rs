#![allow(hidden_glob_reexports)]

pub mod bar_alloc;
pub mod mmio;
pub mod nvme;
pub(crate) mod nvme_backend;
mod pci;
pub mod pciids;
pub mod vrng;
pub use pci::*;

// EXPERIMENTAL TGL bring-up lane.
//
// The i7-1185G7 test laptop exposes Tiger Lake-LP GT2 as 8086:9A49 rev01.
// TRUEOS's current vertically validated Xe-LP path is sealed to ADL-S 8086:4680
// rev0C all the way through device claim, render admission, and the generated
// IGC/Zebin artifact contracts. For this branch only, deliberately present that
// one real TGL GPU to the rest of TRUEOS as the already admitted ADL-S identity.
//
// This is intentionally a compatibility experiment, not a support claim. Raw
// dword PCI config reads remain untouched, so diagnostics can still recover the
// physical 8086:9A49 identity while normal u8/u16 consumers exercise the full
// existing ADL-S path without rebaking or weakening every individual seal.
const EXP_TGL_VENDOR_ID: u16 = 0x8086;
const EXP_TGL_DEVICE_ID: u16 = 0x9A49;
const EXP_TGL_REVISION_ID: u8 = 0x01;
const EXP_ADLS_DEVICE_ID: u16 = 0x4680;
const EXP_ADLS_REVISION_ID: u8 = 0x0C;

#[inline]
fn exp_tgl_9a49_at_bdf(bus: u8, slot: u8, function: u8) -> bool {
    let identity = pci::config_read_u32(bus, slot, function, 0x00);
    (identity as u16) == EXP_TGL_VENDOR_ID && ((identity >> 16) as u16) == EXP_TGL_DEVICE_ID
}

/// Enumerate the physical inventory, then alias only the Tiger Lake GT2 test
/// GPU to the exact ADL-S identity consumed by the current Intel stack.
///
/// Keeping this at the exported PCI boundary means every downstream TRUEOS
/// subsystem sees one coherent experimental identity instead of accumulating
/// one-off 0x9A49 exceptions throughout display, render, Spirit, and GPGPU.
pub fn enumerate_impl() {
    pci::enumerate_impl();

    let mut alias = None;
    pci::with_devices(|devices| {
        if let Some(raw) = devices
            .iter()
            .find(|dev| dev.vendor == EXP_TGL_VENDOR_ID && dev.device == EXP_TGL_DEVICE_ID)
        {
            let mut exposed = *raw;
            exposed.device = EXP_ADLS_DEVICE_ID;
            exposed.device_id = EXP_ADLS_DEVICE_ID;
            alias = Some((exposed, raw.bus, raw.slot, raw.function));
        }
    });

    if let Some((exposed, bus, slot, function)) = alias {
        let raw_revision = pci::config_read_u8(bus, slot, function, 0x08);
        pci::publish_discovered_device(exposed);
        crate::log!(
            "pci/exp-tgl-alias: {:02X}:{:02X}.{} physical={:04X}:{:04X} rev=0x{:02X} exposed={:04X}:{:04X} rev=0x{:02X} purpose=full-xelp-adls-compatibility-probe\n",
            bus,
            slot,
            function,
            EXP_TGL_VENDOR_ID,
            EXP_TGL_DEVICE_ID,
            raw_revision,
            EXP_TGL_VENDOR_ID,
            EXP_ADLS_DEVICE_ID,
            EXP_ADLS_REVISION_ID,
        );
    }
}

/// Preserve ordinary config semantics except for the exact generated-artifact
/// revision seal on the physical TGL test GPU.
pub fn config_read_u8(bus: u8, slot: u8, function: u8, offset: u16) -> u8 {
    let raw = pci::config_read_u8(bus, slot, function, offset);
    if offset == 0x08 && exp_tgl_9a49_at_bdf(bus, slot, function) {
        EXP_ADLS_REVISION_ID
    } else {
        raw
    }
}

/// Present the TGL GT2 device ID as ADL-S to exported 16-bit config consumers.
/// Raw `config_read_u32(..., 0x00)` remains physical and therefore diagnostic.
pub fn config_read_u16(bus: u8, slot: u8, function: u8, offset: u16) -> u16 {
    let raw = pci::config_read_u16(bus, slot, function, offset);
    if offset == 0x02 && raw == EXP_TGL_DEVICE_ID && exp_tgl_9a49_at_bdf(bus, slot, function) {
        EXP_ADLS_DEVICE_ID
    } else {
        raw
    }
}

const _: () = {
    assert!(EXP_TGL_DEVICE_ID != EXP_ADLS_DEVICE_ID);
    assert!(EXP_TGL_REVISION_ID != EXP_ADLS_REVISION_ID);
};
