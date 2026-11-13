//! The first owned route in the shared Gen11/Gen12 graphics interrupt tree.
//! Unknown sources belong to their eventual dispatcher, never to WD capture.

pub(super) const GFX_MASTER: usize = 0x190010;
pub(super) const DISPLAY_CONTROL: usize = 0x44200;
pub(super) const MISC_MASK: usize = 0x44464;
pub(super) const MISC_IDENTITY: usize = 0x44468;
pub(super) const MISC_ENABLE: usize = 0x4446c;
pub(super) const WD_MASK: usize = 0x6e560;
pub(super) const WD_IDENTITY: usize = 0x6e564;
// The headless WD route reserves B; its conventional transcoder stays off.
const WD_PIPE_SLOT: usize = 1;
pub(super) const MASTER_ENABLE: u32 = 1 << 31;
pub(super) const MASTER_DISPLAY: u32 = 1 << 16;
pub(super) const MASTER_GT1: u32 = 1 << 1;
pub(super) const GT_BANK1: usize = 0x19001c;
pub(super) const GT_SELECTOR1: usize = 0x190074;
pub(super) const GT_IDENTITY1: usize = 0x190064;
pub(super) const VIDEO_ENABLE: usize = 0x190034;
pub(super) const VCS01_MASK: usize = 0x1900a8;
pub(super) const VCS23_MASK: usize = 0x1900ac;
pub(super) const VIDEO_USER_ENABLE: u32 = 1 << 16;
pub(super) const VCS0_BANK_BIT: u32 = 1;
pub(super) const ENGINE_USER: u32 = 1;
pub(super) const IDENTITY_VALID: u32 = 1 << 31;
const VCS0_USER_IDENTITY: u32 = IDENTITY_VALID | (1 << 16) | ENGINE_USER;
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
pub(super) const REQUIRED_MMIO_BYTES: usize = VCS23_MASK + 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Routes {
    pub wd: bool,
    pub vcs0: bool,
}

impl Routes {
    pub(super) fn master_bits(self) -> u32 {
        (if self.wd { MASTER_DISPLAY } else { 0 }) | (if self.vcs0 { MASTER_GT1 } else { 0 })
    }
    pub(super) fn any(self) -> bool {
        self.wd || self.vcs0
    }
    pub(super) fn master_enable(self, saved: u32) -> u32 {
        if self.any() {
            MASTER_ENABLE
        } else {
            saved & MASTER_ENABLE
        }
    }
}

pub(super) trait Registers {
    fn read(&mut self, register: usize) -> u32;
    fn write(&mut self, register: usize, value: u32);
    fn timestamp(&mut self) -> u64;
}

pub(super) fn unowned_enable(
    registers: &mut impl Registers,
    owned: Routes,
    wd_powered: bool,
) -> Option<(usize, u32)> {
    for register in UNOWNED_ENABLES {
        let enabled = registers.read(register);
        let allowed = match register {
            VIDEO_ENABLE if owned.vcs0 => VIDEO_USER_ENABLE,
            MISC_ENABLE if owned.wd => MISC_WD0,
            _ => 0,
        };
        if enabled & !allowed != 0 {
            return Some((register, enabled));
        }
    }
    // Pipe B is explicitly powered by WD admission. Other pipes' per-pipe
    // registers are read only while active, preserving power-domain rules.
    for pipe in 0..4 {
        if (pipe == WD_PIPE_SLOT && wd_powered) || registers.read(0x70008 + pipe * 0x1000) & (1 << 30) != 0 {
            let register = 0x4440c + pipe * 0x10;
            let enabled = registers.read(register);
            if enabled != 0 {
                return Some((register, enabled));
            }
        }
    }
    None
}

pub(super) fn unowned_wd_enable(registers: &mut impl Registers) -> Option<(usize, u32)> {
    // This check is separate because GT admission does not hold WD power.
    let wd_mask = registers.read(WD_MASK);
    (wd_mask & 0xffff != 0xffff).then_some((WD_MASK, wd_mask))
}

/// Video decode's class-wide enable applies to all VCS instances. Mask VCS2's
/// user event before enabling the class, preserving its GuC/decoder ownership.
/// Other event bits and the VECS half of the enable register stay unchanged.
pub(super) fn configure_vcs0_user(registers: &mut impl Registers, vcs2_present: bool) -> bool {
    let enabled = registers.read(VIDEO_ENABLE);
    if enabled != 0 {
        return false;
    }
    let vcs0_mask = registers.read(VCS01_MASK);
    let vcs2_mask = if vcs2_present {
        registers.read(VCS23_MASK)
    } else {
        0
    };
    if vcs2_present {
        registers.write(VCS23_MASK, vcs2_mask | ENGINE_USER);
    }
    registers.write(VCS01_MASK, vcs0_mask & !ENGINE_USER);
    registers.write(VIDEO_ENABLE, VIDEO_USER_ENABLE);
    let accepted = registers.read(VIDEO_ENABLE) == VIDEO_USER_ENABLE
        && registers.read(VCS01_MASK) & ENGINE_USER == 0
        && (!vcs2_present || registers.read(VCS23_MASK) & ENGINE_USER != 0);
    if !accepted {
        registers.write(VIDEO_ENABLE, enabled);
        registers.write(VCS01_MASK, vcs0_mask);
        if vcs2_present {
            registers.write(VCS23_MASK, vcs2_mask);
        }
        let _ = registers.read(VIDEO_ENABLE);
    }
    accepted
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Dispatch {
    pub frame_complete: bool,
    pub vcs0_user: bool,
    pub unexpected: bool,
    pub master: u32,
    pub display: u32,
    pub misc: u32,
    pub wd: u32,
    pub gt_bank: u32,
    pub gt_identity: u32,
    pub identity_timeout: bool,
}

/// Mask the master while sampling its level indications, then acknowledge
/// owned leaf bits before their parent. The caller controls re-enabling the
/// master; an unexpected source must remain pending for its actual owner.
pub(super) fn dispatch(
    registers: &mut impl Registers,
    routes: Routes,
    identity_timeout: u64,
) -> Dispatch {
    registers.write(GFX_MASTER, 0);
    let master = registers.read(GFX_MASTER) & !MASTER_ENABLE;
    let mut result = Dispatch {
        master,
        unexpected: master & !routes.master_bits() != 0,
        ..Dispatch::default()
    };
    if master & MASTER_GT1 != 0 && routes.vcs0 {
        dispatch_vcs0(registers, identity_timeout, &mut result);
    }
    if master & MASTER_DISPLAY == 0 || !routes.wd {
        return result;
    }
    result.display = registers.read(DISPLAY_CONTROL) & !MASTER_ENABLE;
    registers.write(DISPLAY_CONTROL, 0);
    result.unexpected |= result.display & !DISPLAY_MISC != 0;
    if result.display & DISPLAY_MISC != 0 {
        result.misc = registers.read(MISC_IDENTITY);
        result.unexpected |= result.misc & !MISC_WD0 != 0;
        if result.misc & MISC_WD0 != 0 {
            if routes.wd {
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

fn dispatch_vcs0(registers: &mut impl Registers, timeout: u64, result: &mut Dispatch) {
    result.gt_bank = registers.read(GT_BANK1);
    result.unexpected |= result.gt_bank & !VCS0_BANK_BIT != 0;
    if result.gt_bank & VCS0_BANK_BIT == 0 {
        return;
    }
    registers.write(GT_SELECTOR1, VCS0_BANK_BIT);
    let started = registers.timestamp();
    // Intel's selector handshake is not an engine-completion wait. Linux
    // gen11_gt_engine_identity() bounds it at about 100 us. The iteration cap
    // also bounds this ISR if the clock or hardware becomes unresponsive.
    for _ in 0..4096 {
        result.gt_identity = registers.read(GT_IDENTITY1);
        if result.gt_identity & IDENTITY_VALID != 0 {
            // HW class 1 is video decode (not the userspace class numbering).
            // A mixed vector would consume an unowned context-switch/error
            // event if VALID were acknowledged. Preserve it and fault closed.
            if result.gt_identity == VCS0_USER_IDENTITY {
                registers.write(GT_IDENTITY1, IDENTITY_VALID);
                registers.write(GT_BANK1, VCS0_BANK_BIT);
                result.vcs0_user = true;
            } else {
                result.unexpected = true;
            }
            return;
        }
        if registers.timestamp().wrapping_sub(started) >= timeout {
            break;
        }
        core::hint::spin_loop();
    }
    result.identity_timeout = true;
    result.unexpected = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Mock {
        values: BTreeMap<usize, u32>,
        writes: Vec<(usize, u32)>,
        reads: Vec<usize>,
        clock: u64,
        identity_delay: usize,
        ignore_video_enable: bool,
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
                clock: 0,
                identity_delay: 0,
                ignore_video_enable: false,
            }
        }
    }
    impl Registers for Mock {
        fn read(&mut self, register: usize) -> u32 {
            self.reads.push(register);
            if register == GT_IDENTITY1 && self.identity_delay != 0 {
                self.identity_delay -= 1;
                return 0;
            }
            *self.values.get(&register).unwrap_or(&0)
        }
        fn timestamp(&mut self) -> u64 {
            self.clock += 10;
            self.clock
        }
        fn write(&mut self, register: usize, value: u32) {
            self.writes.push((register, value));
            if register == WD_IDENTITY || register == MISC_IDENTITY || register == GT_BANK1 {
                *self.values.entry(register).or_default() &= !value;
            } else if register == GT_IDENTITY1 && value == IDENTITY_VALID {
                self.values.insert(register, 0);
            } else if register == GFX_MASTER || register == DISPLAY_CONTROL {
                let pending = self.values.entry(register).or_default();
                *pending = (*pending & !MASTER_ENABLE) | (value & MASTER_ENABLE);
            } else if register != VIDEO_ENABLE || !self.ignore_video_enable {
                self.values.insert(register, value);
            }
        }
    }

    #[test]
    fn frame_completion_acknowledges_leaf_before_parent_and_preserves_status() {
        let mut registers = Mock::wd(WD_FRAME_COMPLETE);
        let result = dispatch(
            &mut registers,
            Routes {
                wd: true,
                vcs0: false,
            },
            100,
        );
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
        let result = dispatch(
            &mut registers,
            Routes {
                wd: true,
                vcs0: false,
            },
            100,
        );
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
        let result = dispatch(
            &mut registers,
            Routes {
                wd: true,
                vcs0: false,
            },
            100,
        );
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
        assert!(dispatch(&mut registers, Routes::default(), 100).unexpected);
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
        assert_eq!(
            unowned_enable(
                &mut registers,
                Routes {
                    wd: true,
                    vcs0: false
                },
                true
            ),
            None
        );
        registers.values.insert(0x190038, 1 << 16);
        assert_eq!(
            unowned_enable(
                &mut registers,
                Routes {
                    wd: true,
                    vcs0: false
                },
                true
            ),
            Some((0x190038, 1 << 16))
        );
        registers.values.insert(0x190038, 0);
        registers.values.insert(WD_MASK, 0xffff & !(1 << 2));
        assert_eq!(unowned_wd_enable(&mut registers), Some((WD_MASK, 0xfffb)));
    }

    #[test]
    fn admission_does_not_access_inactive_pipe_interrupt_registers() {
        let mut registers = Mock::wd(0);
        registers.values.insert(WD_MASK, 0xffff);
        registers.values.insert(0x70008, 1 << 30);
        assert_eq!(
            unowned_enable(
                &mut registers,
                Routes {
                    wd: true,
                    vcs0: false
                },
                true
            ),
            None
        );
        assert!(registers.reads.contains(&0x4440c));
        assert!(registers.reads.contains(&0x4441c));
        assert!(!registers.reads.contains(&0x4442c));
        assert!(!registers.reads.contains(&0x4443c));
    }

    #[test]
    fn wd_admission_checks_headless_b_interrupt_ownership_without_reading_c() {
        let mut registers = Mock::wd(0);
        registers.values.insert(0x4441c, 1 << 3);
        registers.values.insert(0x4442c, u32::MAX);
        assert_eq!(unowned_enable(&mut registers, Routes::default(), true),
            Some((0x4441c, 1 << 3)));
        assert!(!registers.reads.contains(&0x4442c));
        registers.reads.clear();
        assert_eq!(unowned_enable(&mut registers, Routes::default(), false), None);
        assert!(!registers.reads.contains(&0x4441c));
        assert!(!registers.reads.contains(&0x4442c));
    }

    fn vcs0(identity: u32) -> Mock {
        let mut registers = Mock::wd(0);
        registers.values.insert(GFX_MASTER, MASTER_GT1);
        registers.values.insert(GT_BANK1, VCS0_BANK_BIT);
        registers.values.insert(GT_IDENTITY1, identity);
        registers
    }

    #[test]
    fn vcs0_identity_is_acknowledged_before_bank_and_never_reads_wd() {
        let mut registers = vcs0(VCS0_USER_IDENTITY);
        let result = dispatch(
            &mut registers,
            Routes {
                wd: false,
                vcs0: true,
            },
            100,
        );
        assert!(result.vcs0_user);
        assert!(!result.frame_complete);
        assert!(!result.unexpected);
        assert_eq!(
            registers.writes,
            [
                (GFX_MASTER, 0),
                (GT_SELECTOR1, 1),
                (GT_IDENTITY1, IDENTITY_VALID),
                (GT_BANK1, 1)
            ]
        );
        assert!(
            !registers
                .reads
                .iter()
                .any(|register| (0x6e000..0x6f000).contains(register))
        );
        assert_eq!(registers.values[&GT_BANK1], 0);
    }

    #[test]
    fn both_wd_and_vcs0_are_serviced_by_one_shared_interrupt() {
        let mut registers = vcs0(VCS0_USER_IDENTITY);
        registers
            .values
            .insert(GFX_MASTER, MASTER_GT1 | MASTER_DISPLAY);
        registers.values.insert(WD_IDENTITY, WD_FRAME_COMPLETE);
        let result = dispatch(
            &mut registers,
            Routes {
                wd: true,
                vcs0: true,
            },
            100,
        );
        assert!(result.vcs0_user && result.frame_complete);
        assert!(!result.unexpected);
    }

    #[test]
    fn mixed_context_switch_or_other_engine_identity_is_not_consumed() {
        for identity in [
            VCS0_USER_IDENTITY | (1 << 8),
            IDENTITY_VALID | (2 << 16) | 1,
            VCS0_USER_IDENTITY | (2 << 20),
        ] {
            let mut registers = vcs0(identity);
            let result = dispatch(
                &mut registers,
                Routes {
                    wd: false,
                    vcs0: true,
                },
                100,
            );
            assert!(result.unexpected);
            assert!(!result.vcs0_user);
            assert_eq!(registers.values[&GT_IDENTITY1], identity);
            assert_eq!(registers.values[&GT_BANK1], 1);
            assert!(
                !registers
                    .writes
                    .iter()
                    .any(|(register, _)| *register == GT_IDENTITY1 || *register == GT_BANK1)
            );
        }
    }

    #[test]
    fn guc_bank_and_other_vcs_bits_remain_pending_for_their_owner() {
        let mut registers = vcs0(VCS0_USER_IDENTITY);
        registers.values.insert(GFX_MASTER, MASTER_GT1 | 1);
        registers.values.insert(GT_BANK1, 1 | (1 << 2));
        let result = dispatch(
            &mut registers,
            Routes {
                wd: false,
                vcs0: true,
            },
            100,
        );
        assert!(result.vcs0_user && result.unexpected);
        assert_eq!(registers.values[&GT_BANK1], 1 << 2);
        assert!(!registers.reads.contains(&0x190018));
        assert!(
            !registers
                .writes
                .iter()
                .any(|(register, _)| *register == 0x190018)
        );
    }

    #[test]
    fn selector_wait_is_bounded_and_does_not_ack_invalid_identity() {
        let mut registers = vcs0(0);
        let result = dispatch(
            &mut registers,
            Routes {
                wd: false,
                vcs0: true,
            },
            30,
        );
        assert!(result.identity_timeout && result.unexpected);
        assert!(!result.vcs0_user);
        assert_eq!(registers.values[&GT_BANK1], 1);
        assert!(
            registers
                .reads
                .iter()
                .filter(|register| **register == GT_IDENTITY1)
                .count()
                <= 4
        );
    }

    #[test]
    fn selector_can_publish_its_identity_after_several_reads() {
        let mut registers = vcs0(VCS0_USER_IDENTITY);
        registers.identity_delay = 2;
        let result = dispatch(
            &mut registers,
            Routes {
                wd: false,
                vcs0: true,
            },
            100,
        );
        assert!(result.vcs0_user && !result.unexpected);
        assert_eq!(
            registers
                .reads
                .iter()
                .filter(|register| **register == GT_IDENTITY1)
                .count(),
            3
        );
    }

    #[test]
    fn class_enable_masks_vcs2_user_without_changing_context_switch_bits() {
        let mut registers = Mock::wd(0);
        let vcs0_original = 0xabcd_ffff;
        let vcs2_original = 0x1234_00f0;
        registers.values.insert(VCS01_MASK, vcs0_original);
        registers.values.insert(VCS23_MASK, vcs2_original);
        assert!(configure_vcs0_user(&mut registers, true));
        assert_eq!(registers.values[&VIDEO_ENABLE], 1 << 16);
        assert_eq!(registers.values[&VCS01_MASK], vcs0_original & !1);
        assert_eq!(registers.values[&VCS23_MASK], vcs2_original | 1);
        assert_eq!(registers.writes[0], (VCS23_MASK, vcs2_original | 1));
        assert_eq!(
            registers.values[&VCS01_MASK] & (1 << 8),
            vcs0_original & (1 << 8)
        );
    }

    #[test]
    fn failed_class_enable_restores_original_masks() {
        let mut registers = Mock::wd(0);
        registers.values.insert(VCS01_MASK, 0xff);
        registers.values.insert(VCS23_MASK, 0x20);
        registers.ignore_video_enable = true;
        assert!(!configure_vcs0_user(&mut registers, true));
        assert_eq!(registers.values[&VCS01_MASK], 0xff);
        assert_eq!(registers.values[&VCS23_MASK], 0x20);
    }

    #[test]
    fn gt_admission_never_reads_powered_off_wd_and_allows_only_owned_class_bit() {
        let mut registers = Mock::wd(0);
        registers.values.insert(VIDEO_ENABLE, VIDEO_USER_ENABLE);
        let owned = Routes {
            wd: false,
            vcs0: true,
        };
        assert_eq!(unowned_enable(&mut registers, owned, false), None);
        assert!(!registers.reads.contains(&WD_MASK));
        assert!(!registers.reads.contains(&0x4442c));
        registers
            .values
            .insert(VIDEO_ENABLE, VIDEO_USER_ENABLE | (1 << 24));
        assert_eq!(
            unowned_enable(&mut registers, owned, false),
            Some((VIDEO_ENABLE, 0x1010000))
        );
    }

    #[test]
    fn retained_vcs0_keeps_master_enabled_after_wd_power_lease_ends() {
        let both = Routes {
            wd: true,
            vcs0: true,
        };
        assert_eq!(both.master_enable(0), MASTER_ENABLE);
        let after_wd = Routes { wd: false, ..both };
        assert_eq!(after_wd.master_enable(0), MASTER_ENABLE);
        assert_eq!(after_wd.master_bits(), MASTER_GT1);
        assert_eq!(Routes::default().master_enable(0), 0);
    }
}
