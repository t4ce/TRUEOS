//! Precision-palette math shared by the display fade and host regression tests.
pub(super) fn palette(original: &[u32; 1024], amount: i32, precision_enabled: bool) -> [u32; 1024] {
    let weight = amount.unsigned_abs().min(65535);
    core::array::from_fn(|index| {
        let original = if precision_enabled {
            original[index]
        } else {
            let value = index as u32;
            (value << 20) | (value << 10) | value
        };
        let channel = |shift: u32| {
            let value = (original >> shift) & 1023u32;
            if amount < 0 {
                value * (65535 - weight) / 65535
            } else {
                value + (1023 - value) * weight / 65535
            }
        };
        (channel(20) << 20) | (channel(10) << 10) | channel(0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoints_affect_every_channel_and_every_entry() {
        let original = [0x015544ff; 1024];
        assert_eq!(palette(&original, -65535, true), [0; 1024]);
        assert_eq!(palette(&original, 65535, true), [0x3fffffff; 1024]);
        assert_eq!(palette(&original, 0, true), original);
    }
    #[test]
    fn disabled_gamma_uses_identity_and_partial_fades_stay_monotonic() {
        let dark = palette(&[0; 1024], -32768, false);
        let light = palette(&[0; 1024], 32768, false);
        for index in 1..1024 {
            assert!(dark[index] >= dark[index - 1]);
            assert!(light[index] >= light[index - 1]);
        }
        assert_eq!(dark[0], 0);
        assert_eq!(light[1023], 0x3fffffff);
        assert!(dark[512] < palette(&[0; 1024], 0, false)[512]);
        assert!(light[512] > palette(&[0; 1024], 0, false)[512]);
    }
}
