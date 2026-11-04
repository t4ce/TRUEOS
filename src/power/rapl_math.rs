//! Hardware-independent RAPL admission and counter arithmetic.

// Deliberately limited to the observed Raptor Lake-S rig (06_BF).
// Other models and optional domains need a documented profile or fault-safe
// MSR probing before admission. CPUID's MSR bit alone is insufficient.
pub fn package_profile_supported(
    intel: bool,
    msr: bool,
    hypervisor: bool,
    family: u8,
    model: u8,
) -> bool {
    intel && msr && !hypervisor && family == 6 && model == 0xbf
}

pub fn delta_joules(start: u32, end: u32, joules_per_tick: f64) -> f64 {
    end.wrapping_sub(start) as f64 * joules_per_tick
}

pub fn average_watts(start: u32, end: u32, unit: f64, seconds: f64) -> Option<f64> {
    if !unit.is_finite() || unit <= 0.0 || !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    let watts = delta_joules(start, end, unit) / seconds;
    watts.is_finite().then_some(watts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_rejects_unknown_and_virtual_hardware() {
        assert!(package_profile_supported(true, true, false, 6, 0xbf));
        for caps in [
            (false, true, false, 6, 0xbf),
            (true, false, false, 6, 0xbf),
            (true, true, true, 6, 0xbf),
            (true, true, false, 6, 0xbe),
            (true, true, false, 7, 0xbf),
        ] {
            assert!(!package_profile_supported(
                caps.0, caps.1, caps.2, caps.3, caps.4
            ));
        }
    }

    #[test]
    fn known_energy_over_elapsed_time_and_counter_wrap() {
        assert_eq!(average_watts(100, 164, 0.25, 2.0), Some(8.0));
        assert_eq!(average_watts(u32::MAX - 3, 4, 0.25, 2.0), Some(1.0));
        assert_eq!(average_watts(0, 0, 0.25, 2.0), Some(0.0));
    }

    #[test]
    fn invalid_units_and_intervals_never_produce_watts() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(average_watts(0, 100, bad, 1.0), None);
            assert_eq!(average_watts(0, 100, 0.25, bad), None);
        }
    }
}
