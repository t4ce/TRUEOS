//! Per-terminal font size, expressed in half-size steps of the default font.
//! Scale 2 is 100%; the versioned OSC accepts exactly scales 1 through 8.

pub(crate) fn parse(payload: &[u8]) -> Option<u32> {
    let scale = payload.strip_prefix(b"777;font-scale;1;")?;
    match scale {
        [value @ b'1'..=b'8'] => Some(u32::from(*value - b'0')),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn accepts_only_the_versioned_bounded_scale() {
        for scale in 1..=8 {
            let payload = alloc::format!("777;font-scale;1;{scale}");
            assert_eq!(parse(payload.as_bytes()), Some(scale));
        }
        for payload in [
            "777;font-scale;1;0", "777;font-scale;1;9", "777;font-scale;1;256",
            "777;font-scale;1;01", "777;font-scale;1;+2", "777;font-scale;1;-2",
            "777;font-scale;1; 2", "777;font-scale;1;2;", "777;font-scale;1;2x",
            "777;font-scale;1;", "777;font-scale;2;2", "777;font-scale;2",
            "777;other;1;2", "font-scale;1;2",
        ] {
            assert_eq!(parse(payload.as_bytes()), None, "{payload}");
        }
        assert_eq!(parse(b"777;font-scale;1;\xff"), None);
    }
}
