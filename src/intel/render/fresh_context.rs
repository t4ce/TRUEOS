//! First-use engine-restore policy for the physical Tiger Lake experiment.
//!
//! TGL PRM Vol8 pp5--6 describes restore inhibit for an uninitialized engine
//! image and hardware population on the following context save. Linux mirrors
//! this in lrc_init_state()/__lrc_init_regs(): no golden image means inhibit.
//! https://kiwitree.net/~lina/intel-gfx-docs/prm/tgl/intel-gfx-prm-osrc-tgl-vol-08-command-stream-programming.pdf
//! https://github.com/torvalds/linux/blob/master/drivers/gpu/drm/i915/gt/intel_lrc.c
//!
//! This corrects cold initialization only. Its effect on the observed RCS0
//! wedge remains unvalidated; live context images stay owned by the GPU.

pub(crate) const fn requires_initial_engine_restore_inhibit(
    physical_vendor: u16,
    physical_device: u16,
    physical_revision: u8,
) -> bool {
    physical_vendor == 0x8086 && physical_device == 0x9a49 && physical_revision == 0x01
}

/// Read the claimed device's physical PCI identity, bypassing the 4680 alias.
/// Call only while constructing a fresh context without a golden engine image.
pub(crate) fn inhibit_uninitialized_tgl_engine_restore() -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let identity = crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 0);
    let revision = crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 8) as u8;
    requires_initial_engine_restore_inhibit(identity as u16, (identity >> 16) as u16, revision)
}
