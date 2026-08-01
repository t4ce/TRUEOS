//! Scoped Intel Hardware-managed P-state (HWP) performance requests.
//!
//! HWP MSRs are core-local architectural state.  A caller must keep this guard
//! on the same non-preemptive worker for its whole lifetime.  The guard is
//! deliberately fail-closed: CPUID must advertise the HWP base registers and
//! firmware must already have enabled HWP before this module will execute a
//! single `WRMSR`.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use raw_cpuid::CpuId;
use x86_64::registers::model_specific::Msr;

const IA32_PM_ENABLE: u32 = 0x770;
const IA32_HWP_CAPABILITIES: u32 = 0x771;
const IA32_HWP_REQUEST: u32 = 0x774;

const PM_ENABLE_HWP: u64 = 1;
const BYTE_MASK: u64 = 0xff;
const REQUEST_MIN_SHIFT: u32 = 0;
const REQUEST_MAX_SHIFT: u32 = 8;
const REQUEST_DESIRED_SHIFT: u32 = 16;
const REQUEST_EPP_SHIFT: u32 = 24;
const CAP_HIGHEST_SHIFT: u32 = 0;
const CAP_GUARANTEED_SHIFT: u32 = 8;
const CAP_MOST_EFFICIENT_SHIFT: u32 = 16;
const CAP_LOWEST_SHIFT: u32 = 24;

// Only the first real request gets begin/end audit lines.  A 200-slice Kokoro
// invocation must not turn this diagnostic into a logging workload.
const AUDIT_UNCLAIMED: u8 = 0;
const AUDIT_ACTIVE: u8 = 1;
const AUDIT_FINISHED: u8 = 2;
static ACTIVATION_AUDIT: AtomicU8 = AtomicU8::new(AUDIT_UNCLAIMED);
static UNAVAILABLE_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HwpRequestError {
    Unsupported,
    Disabled,
    InvalidCapabilities,
}

impl HwpRequestError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Disabled => "not-enabled-by-firmware",
            Self::InvalidCapabilities => "invalid-capabilities",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HwpCapabilities {
    pub highest: u8,
    pub guaranteed: u8,
    pub most_efficient: u8,
    pub lowest: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HwpRequestFields {
    pub minimum: u8,
    pub maximum: u8,
    pub desired: u8,
    pub energy_performance_preference: u8,
}

/// A core-local HWP request restored exactly when the synchronous slice ends.
///
/// TRUEOS AP executors are pinned and non-preemptive across a synchronous
/// `run_slice` call.  Moving this guard across an `.await` would violate that
/// requirement, so it intentionally has no async-facing API.
pub struct ScopedHwpPerformance {
    previous_request: u64,
    requested: u64,
    audit: bool,
    wrote_request: bool,
}

impl ScopedHwpPerformance {
    /// Request this logical processor's highest HWP performance for a bounded
    /// synchronous section.
    ///
    /// No `WRMSR` is executed unless all of these conditions hold:
    ///
    /// * the CPU is GenuineIntel and advertises both MSR and HWP support;
    /// * `IA32_PM_ENABLE.HWP_ENABLE` is already set; and
    /// * `IA32_HWP_CAPABILITIES` contains a coherent non-zero range.
    pub fn try_begin() -> Result<Self, HwpRequestError> {
        let support = local_hwp_support().ok_or_else(|| {
            log_unavailable_once(HwpRequestError::Unsupported, 0);
            HwpRequestError::Unsupported
        })?;

        // CPUID.06H:EAX.HWP architecturally enumerates this MSR.  We only read
        // it here; ownership of enabling HWP stays with firmware/platform init.
        let pm_enable = unsafe { Msr::new(IA32_PM_ENABLE).read() };
        if (pm_enable & PM_ENABLE_HWP) == 0 {
            log_unavailable_once(HwpRequestError::Disabled, pm_enable);
            return Err(HwpRequestError::Disabled);
        }

        let caps_raw = unsafe { Msr::new(IA32_HWP_CAPABILITIES).read() };
        let caps = decode_capabilities(caps_raw);
        if caps.highest == 0 || caps.lowest == 0 || caps.lowest > caps.highest {
            log_unavailable_once(HwpRequestError::InvalidCapabilities, pm_enable);
            return Err(HwpRequestError::InvalidCapabilities);
        }

        let previous_request = unsafe { Msr::new(IA32_HWP_REQUEST).read() };
        let requested = encode_performance_request(previous_request, caps, support.epp);
        let wrote_request = requested != previous_request;
        if wrote_request {
            // All feature and enable checks have completed before this first
            // write.  Preserve high/reserved fields and unsupported EPP bits.
            unsafe { Msr::new(IA32_HWP_REQUEST).write(requested) };
        }

        let audit = ACTIVATION_AUDIT
            .compare_exchange(AUDIT_UNCLAIMED, AUDIT_ACTIVE, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        if audit {
            crate::log_info!(
                target: "ttstt";
                "ttstt: hwp boost stage=begin slot={} pm_enable=0x{:016X} caps_raw=0x{:016X} highest={} guaranteed={} efficient={} lowest={} epp_supported={} previous=0x{:016X} requested=0x{:016X} wrote={} scope=kokoro-tts-slice restore=exact-on-drop\n",
                crate::percpu::current_slot_via_cpuid(),
                pm_enable,
                caps_raw,
                caps.highest,
                caps.guaranteed,
                caps.most_efficient,
                caps.lowest,
                support.epp as u8,
                previous_request,
                requested,
                wrote_request as u8,
            );
        }

        Ok(Self {
            previous_request,
            requested,
            audit,
            wrote_request,
        })
    }

    pub fn previous_request(&self) -> u64 {
        self.previous_request
    }

    pub fn requested(&self) -> u64 {
        self.requested
    }
}

impl Drop for ScopedHwpPerformance {
    fn drop(&mut self) {
        if self.wrote_request {
            // Restore the complete saved image, including every reserved,
            // activity-window, package-control, and preference bit.
            unsafe { Msr::new(IA32_HWP_REQUEST).write(self.previous_request) };
        }

        if self.audit {
            let observed = unsafe { Msr::new(IA32_HWP_REQUEST).read() };
            crate::log_info!(
                target: "ttstt";
                "ttstt: hwp boost stage=end slot={} restored={} previous=0x{:016X} boost=0x{:016X} observed=0x{:016X} scope=kokoro-tts-slice\n",
                crate::percpu::current_slot_via_cpuid(),
                (observed == self.previous_request) as u8,
                self.previous_request,
                self.requested,
                observed,
            );
            ACTIVATION_AUDIT.store(AUDIT_FINISHED, Ordering::Release);
        }
    }
}

#[derive(Clone, Copy)]
struct LocalHwpSupport {
    epp: bool,
}

fn local_hwp_support() -> Option<LocalHwpSupport> {
    let cpuid = CpuId::new();
    let intel = cpuid
        .get_vendor_info()
        .map(|vendor| vendor.as_str() == "GenuineIntel")
        .unwrap_or(false);
    let msr = cpuid
        .get_feature_info()
        .map(|features| features.has_msr())
        .unwrap_or(false);
    let power = cpuid.get_thermal_power_info()?;
    if !intel || !msr || !power.has_hwp() {
        return None;
    }
    Some(LocalHwpSupport {
        epp: power.has_hwp_energy_performance_preference(),
    })
}

fn log_unavailable_once(error: HwpRequestError, pm_enable: u64) {
    if !UNAVAILABLE_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_info!(
            target: "ttstt";
            "ttstt: hwp boost available=0 reason={} pm_enable=0x{:016X} scope=kokoro-tts-slice writes=0 fallback=unchanged\n",
            error.as_str(),
            pm_enable,
        );
    }
}

const fn byte(raw: u64, shift: u32) -> u8 {
    ((raw >> shift) & BYTE_MASK) as u8
}

const fn replace_byte(raw: u64, shift: u32, value: u8) -> u64 {
    let mask = BYTE_MASK << shift;
    (raw & !mask) | ((value as u64) << shift)
}

pub const fn decode_capabilities(raw: u64) -> HwpCapabilities {
    HwpCapabilities {
        highest: byte(raw, CAP_HIGHEST_SHIFT),
        guaranteed: byte(raw, CAP_GUARANTEED_SHIFT),
        most_efficient: byte(raw, CAP_MOST_EFFICIENT_SHIFT),
        lowest: byte(raw, CAP_LOWEST_SHIFT),
    }
}

pub const fn decode_request(raw: u64) -> HwpRequestFields {
    HwpRequestFields {
        minimum: byte(raw, REQUEST_MIN_SHIFT),
        maximum: byte(raw, REQUEST_MAX_SHIFT),
        desired: byte(raw, REQUEST_DESIRED_SHIFT),
        energy_performance_preference: byte(raw, REQUEST_EPP_SHIFT),
    }
}

pub const fn encode_performance_request(
    previous: u64,
    caps: HwpCapabilities,
    epp_supported: bool,
) -> u64 {
    let requested = replace_byte(previous, REQUEST_MIN_SHIFT, caps.highest);
    let requested = replace_byte(requested, REQUEST_MAX_SHIFT, caps.highest);
    let requested = replace_byte(requested, REQUEST_DESIRED_SHIFT, caps.highest);
    if epp_supported {
        // EPP 0 is the architectural performance preference.
        replace_byte(requested, REQUEST_EPP_SHIFT, 0)
    } else {
        // Bits 31:24 are reserved when CPUID does not enumerate EPP.
        requested
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_follow_architectural_byte_order() {
        let caps = decode_capabilities(0x0000_0000_1234_5678);
        assert_eq!(
            caps,
            HwpCapabilities {
                highest: 0x78,
                guaranteed: 0x56,
                most_efficient: 0x34,
                lowest: 0x12,
            }
        );
    }

    #[test]
    fn performance_request_preserves_every_non_request_bit() {
        let previous = 0xA5AA_55C3_DEAD_BEEF;
        let caps = HwpCapabilities {
            highest: 0x9c,
            guaranteed: 0x70,
            most_efficient: 0x30,
            lowest: 0x18,
        };
        let requested = encode_performance_request(previous, caps, true);
        assert_eq!(requested & 0xffff_ffff_0000_0000, previous & 0xffff_ffff_0000_0000);
        assert_eq!(
            decode_request(requested),
            HwpRequestFields {
                minimum: 0x9c,
                maximum: 0x9c,
                desired: 0x9c,
                energy_performance_preference: 0,
            }
        );
    }

    #[test]
    fn unsupported_epp_byte_is_preserved() {
        let previous = 0x0123_4567_AA01_0203;
        let caps = HwpCapabilities {
            highest: 0x80,
            guaranteed: 0x60,
            most_efficient: 0x30,
            lowest: 0x10,
        };
        let requested = encode_performance_request(previous, caps, false);
        assert_eq!(decode_request(requested).energy_performance_preference, 0xaa);
        assert_eq!(requested & 0xffff_ffff_0000_0000, previous & 0xffff_ffff_0000_0000);
    }

    #[test]
    fn exact_restore_image_is_the_unmodified_previous_value() {
        let previous = 0xFEDC_BA98_7654_3210;
        let requested = encode_performance_request(
            previous,
            HwpCapabilities {
                highest: 0xff,
                guaranteed: 0x80,
                most_efficient: 0x40,
                lowest: 0x20,
            },
            true,
        );
        assert_ne!(requested, previous);
        let restored = previous;
        assert_eq!(restored, 0xFEDC_BA98_7654_3210);
    }
}
