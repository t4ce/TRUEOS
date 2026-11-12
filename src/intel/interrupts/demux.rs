//! The first owned route in the shared Gen11/Gen12 graphics interrupt tree.
//! Unknown sources belong to their eventual dispatcher, never to WD capture.

pub(super) const GFX_MASTER: usize = 0x190010;
pub(super) const DISPLAY_CONTROL: usize = 0x44200;
pub(super) const MISC_MASK: usize = 0x44464;
pub(super) const MISC_IDENTITY: usize = 0x44468;
pub(super) const MISC_ENABLE: usize = 0x4446c;
pub(super) const WD_MASK: usize = 0x6e560;
pub(super) const WD_IDENTITY: usize = 0x6e564;
pub(super) const MASTER_ENABLE: u32 = 1 << 31;
pub(super) const MASTER_DISPLAY: u32 = 1 << 16;
pub(super) const DISPLAY_MISC: u32 = 1 << 22;
pub(super) const MISC_WD0: u32 = 1 << 23;
// PRM Vol 2d p1155: bit 2 means writes finished BEFORE the memory flush.
// Only bit 7 proves that the captured pixels can be accessed.
pub(super) const WD_FRAME_COMPLETE: u32 = 1 << 7;
// Gen11/ADL-S class enables, GU_MISC/PCU and global display branches.
// Only the new WD route may become live when the graphics master is enabled.
pub(super) const UNOWNED_ENABLES: [usize; 11] = [
    0x190030,
    0x190034,
    0x190038,
    0x19003c,
    0x190040,
    0x190044,
    0x444fc,
    0x444ec,
    0x4444c,
    0x4447c,
    MISC_ENABLE,
];
pub(super) const REQUIRED_MMIO_BYTES: usize = 0x190048;

pub(super) trait Registers {
    fn read(&mut self, register: usize) -> u32;
    fn write(&mut self, register: usize, value: u32);
}

pub(super) fn unowned_enable(registers: &mut impl Registers) -> Option<(usize, u32)> {
    for register in UNOWNED_ENABLES {
        let enabled = registers.read(register);
        if enabled != 0 {
            return Some((register, enabled));
        }
    }
    // Pipe C is explicitly powered by WD admission. Other pipes' per-pipe
    // registers are read only while active, preserving power-domain rules.
    for pipe in 0..4 {
        if pipe == 2 || registers.read(0x70008 + pipe * 0x1000) & (1 << 30) != 0 {
            let register = 0x4440c + pipe * 0x10;
            let enabled = registers.read(register);
            if enabled != 0 {
                return Some((register, enabled));
            }
        }
    }
    let wd_mask = registers.read(WD_MASK);
    if wd_mask & 0xffff != 0xffff {
        return Some((WD_MASK, wd_mask));
    }
    None
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Dispatch {
    pub frame_complete: bool,
    pub unexpected: bool,
    pub master: u32,
    pub display: u32,
    pub misc: u32,
    pub wd: u32,
}

/// Mask the master while sampling its level indications, then acknowledge
/// owned leaf bits before their parent. The caller controls re-enabling the
/// master; an unexpected source must remain pending for its actual owner.
pub(super) fn dispatch(registers: &mut impl Registers, wd_powered: bool) -> Dispatch {
    registers.write(GFX_MASTER, 0);
    let master = registers.read(GFX_MASTER) & !MASTER_ENABLE;
    let mut result = Dispatch {
        master,
        unexpected: master & !MASTER_DISPLAY != 0,
        ..Dispatch::default()
    };
    if master & MASTER_DISPLAY == 0 {
        return result;
    }
    result.display = registers.read(DISPLAY_CONTROL) & !MASTER_ENABLE;
    registers.write(DISPLAY_CONTROL, 0);
    result.unexpected |= result.display & !DISPLAY_MISC != 0;
    if result.display & DISPLAY_MISC != 0 {
        result.misc = registers.read(MISC_IDENTITY);
        result.unexpected |= result.misc & !MISC_WD0 != 0;
        if result.misc & MISC_WD0 != 0 {
            if wd_powered {
                result.wd = registers.read(WD_IDENTITY);
                result.unexpected |= result.wd & !WD_FRAME_COMPLETE != 0;
                if result.wd & WD_FRAME_COMPLETE != 0 {
                    registers.write(WD_IDENTITY, WD_FRAME_COMPLETE);
                    // Flush the leaf acknowledgement before clearing its
                    // level-triggered parent. Preserve WD_FRAME_STATUS.
                    let remaining = registers.read(WD_IDENTITY);
                    result.frame_complete = true;
                    if remaining == 0 {
                        registers.write(MISC_IDENTITY, MISC_WD0);
                        let _ = registers.read(MISC_IDENTITY);
                    } else {
                        result.unexpected = true;
                    }
                }
            } else {
                // Never read WD registers after its power lease is released.
                result.unexpected = true;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Mock {
        values: BTreeMap<usize, u32>,
        writes: Vec<(usize, u32)>,
        reads: Vec<usize>,
    }
    impl Mock {
        fn wd(bits: u32) -> Self {
            Self {
                values: [
                    (GFX_MASTER, MASTER_DISPLAY),
                    (DISPLAY_CONTROL, DISPLAY_MISC),
                    (MISC_IDENTITY, MISC_WD0),
                    (WD_IDENTITY, bits),
                ]
                .into(),
                writes: Vec::new(),
                reads: Vec::new(),
            }
        }
    }
    impl Registers for Mock {
        fn read(&mut self, register: usize) -> u32 {
            self.reads.push(register);
            *self.values.get(&register).unwrap_or(&0)
        }
        fn write(&mut self, register: usize, value: u32) {
            self.writes.push((register, value));
            if register == WD_IDENTITY || register == MISC_IDENTITY {
                *self.values.entry(register).or_default() &= !value;
            }
        }
    }

    #[test]
    fn frame_completion_acknowledges_leaf_before_parent_and_preserves_status() {
        let mut registers = Mock::wd(WD_FRAME_COMPLETE);
        let result = dispatch(&mut registers, true);
        assert!(result.frame_complete);
        assert!(!result.unexpected);
        assert_eq!(
            registers.writes,
            [
                (GFX_MASTER, 0),
                (DISPLAY_CONTROL, 0),
                (WD_IDENTITY, WD_FRAME_COMPLETE),
                (MISC_IDENTITY, MISC_WD0)
            ]
        );
        assert!(!registers.reads.contains(&0x6e568));
    }

    #[test]
    fn writes_complete_is_never_treated_as_access_ready_or_acknowledged() {
        let mut registers = Mock::wd(1 << 2);
        let result = dispatch(&mut registers, true);
        assert!(!result.frame_complete);
        assert!(result.unexpected);
        assert_eq!(registers.values[&WD_IDENTITY], 1 << 2);
        assert_eq!(registers.values[&MISC_IDENTITY], MISC_WD0);
    }

    #[test]
    fn shared_gt_and_unowned_display_sources_remain_for_their_owner() {
        let mut registers = Mock::wd(WD_FRAME_COMPLETE);
        registers.values.insert(GFX_MASTER, MASTER_DISPLAY | 1);
        registers.values.insert(MISC_IDENTITY, MISC_WD0 | (1 << 19));
        let result = dispatch(&mut registers, true);
        assert!(result.frame_complete);
        assert!(result.unexpected);
        assert_eq!(registers.values[&MISC_IDENTITY], 1 << 19);
        assert!(
            registers.writes.iter().all(|(register, _)| !matches!(
                *register,
                0x190018 | 0x19001c | 0x190060 | 0x190064
            ))
        );
    }

    #[test]
    fn powered_off_wd_is_not_read() {
        let mut registers = Mock::wd(WD_FRAME_COMPLETE);
        assert!(dispatch(&mut registers, false).unexpected);
        assert!(!registers.reads.contains(&WD_IDENTITY));
        assert!(
            !registers
                .writes
                .iter()
                .any(|(register, _)| *register == WD_IDENTITY)
        );
    }

    #[test]
    fn admission_rejects_dormant_unowned_enables_and_unmasked_wd_events() {
        let mut registers = Mock::wd(0);
        registers.values.insert(WD_MASK, 0xffff);
        assert_eq!(unowned_enable(&mut registers), None);
        registers.values.insert(0x190038, 1 << 16);
        assert_eq!(unowned_enable(&mut registers), Some((0x190038, 1 << 16)));
        registers.values.insert(0x190038, 0);
        registers.values.insert(WD_MASK, 0xffff & !(1 << 2));
        assert_eq!(unowned_enable(&mut registers), Some((WD_MASK, 0xfffb)));
    }

    #[test]
    fn admission_does_not_access_inactive_pipe_interrupt_registers() {
        let mut registers = Mock::wd(0);
        registers.values.insert(WD_MASK, 0xffff);
        registers.values.insert(0x70008, 1 << 30);
        assert_eq!(unowned_enable(&mut registers), None);
        assert!(registers.reads.contains(&0x4440c));
        assert!(registers.reads.contains(&0x4442c));
        assert!(!registers.reads.contains(&0x4441c));
        assert!(!registers.reads.contains(&0x4443c));
    }
}
