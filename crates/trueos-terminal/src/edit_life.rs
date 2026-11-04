//! Editor background metadata, committed after the VT cells in an Edit frame.
//! All coordinates are zero-based terminal cells. Unknown or malformed reports
//! leave the last complete report untouched; no document text crosses this path.
use alloc::vec::Vec;

pub const MAX_EXCLUSIONS: usize = 16;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CellRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl CellRect {
    pub fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x && y >= self.y
            && x - self.x < self.width && y - self.y < self.height
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditLife {
    pub enabled: bool,
    /// Only the unobstructed, focused editor page may animate. Disabled is a
    /// separate user preference: inactivity preserves the retained Life world.
    pub active: bool,
    pub viewport: CellRect,
    pub scroll_revision: u64,
    pub emission_revision: u64,
    pub exclusions: Vec<CellRect>,
}

pub(crate) fn parse(payload: &[u8]) -> Option<EditLife> {
    let text = core::str::from_utf8(payload).ok()?.strip_prefix("777;edit-life;")?;
    let mut fields = text.split(';');
    let version = match fields.next()? { "1" => 1, "2" => 2, _ => return None };
    let enabled = match fields.next()? { "0" => false, "1" => true, _ => return None };
    let requested_active = if version == 2 {
        match fields.next()? { "0" => false, "1" => true, _ => return None }
    } else { true };
    fn number<T: core::str::FromStr>(fields: &mut core::str::Split<'_, char>) -> Option<T> {
        let text = fields.next()?;
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) { return None; }
        text.parse().ok()
    }
    fn rect(fields: &mut core::str::Split<'_, char>) -> Option<CellRect> {
        let rect = CellRect {
            x: number(fields)?, y: number(fields)?,
            width: number(fields)?, height: number(fields)?,
        };
        // Dimensions cannot exceed the terminal protocol's representable grid.
        (rect.x.checked_add(rect.width)? <= u16::MAX as u32
            && rect.y.checked_add(rect.height)? <= u16::MAX as u32).then_some(rect)
    }
    let viewport = rect(&mut fields)?;
    if enabled && (viewport.width == 0 || viewport.height == 0) { return None; }
    let scroll_revision = number(&mut fields)?;
    let emission_revision = number(&mut fields)?;
    let count: usize = number(&mut fields)?;
    if count > MAX_EXCLUSIONS { return None; }
    let mut exclusions = Vec::with_capacity(count);
    for _ in 0..count { exclusions.push(rect(&mut fields)?); }
    if fields.next().is_some() { return None; }
    let active = enabled && requested_active && exclusions.is_empty();
    Some(EditLife { enabled, active, viewport, scroll_revision, emission_revision, exclusions })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_versioned_bounded_metadata() {
        let report = parse(b"777;edit-life;1;1;4;2;70;20;42;99;1;5;3;10;4").unwrap();
        assert_eq!(report.scroll_revision, 42);
        assert_eq!(report.emission_revision, 99);
        assert!(report.viewport.contains(73, 21));
        assert!(!report.viewport.contains(74, 21));
        assert!(report.exclusions[0].contains(5, 3));
        for invalid in [
            "777;edit-life;2;1;0;0;1;1;0;0;0",
            "777;edit-life;1;2;0;0;1;1;0;0;0",
            "777;edit-life;1;1;0;0;0;1;0;0;0",
            "777;edit-life;1;1;65535;0;1;1;0;0;0",
            "777;edit-life;1;1;0;0;1;1;0;0;17",
            "777;edit-life;1;1;0;0;1;1;0;0;1",
            "777;edit-life;1;1;0;0;1;1;0;0;0;",
            "777;edit-life;1;1;+0;0;1;1;0;0;0",
        ] { assert!(parse(invalid.as_bytes()).is_none(), "{invalid}"); }
        assert!(!parse(b"777;edit-life;1;0;0;0;0;0;0;0;0").unwrap().enabled);
    }

    #[test]
    fn activity_is_independent_of_enabled_and_legacy_popups_pause() {
        let active = parse(b"777;edit-life;2;1;1;4;2;70;20;42;99;0").unwrap();
        let paused = parse(b"777;edit-life;2;1;0;4;2;70;20;42;99;0").unwrap();
        assert!(active.enabled && active.active);
        assert!(paused.enabled && !paused.active);
        assert_eq!(active.scroll_revision, paused.scroll_revision);
        assert_eq!(active.emission_revision, paused.emission_revision);
        assert!(!parse(b"777;edit-life;2;0;1;0;0;0;0;42;99;0").unwrap().active);
        assert!(parse(b"777;edit-life;1;1;4;2;70;20;42;99;0").unwrap().active);
        assert!(!parse(b"777;edit-life;1;1;4;2;70;20;42;99;1;5;3;10;4").unwrap().active);
        assert!(!parse(b"777;edit-life;2;1;1;4;2;70;20;42;99;1;5;3;10;4").unwrap().active);
        assert!(parse(b"777;edit-life;2;1;2;4;2;70;20;42;99;0").is_none());
    }
}
