//! Bounded presentation metadata for outlines in the visible terminal grid.

const MAX_OUTLINES: usize = 12;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CellRect {
    pub column: u16,
    pub row: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CellOutlines {
    count: usize,
    rects: [CellRect; MAX_OUTLINES],
}

impl CellOutlines {
    pub fn as_slice(&self) -> &[CellRect] {
        &self.rects[..self.count]
    }
}

/// Recognize a dedicated write without touching a general VT byte stream.
pub fn is_wire_report(bytes: &[u8]) -> bool {
    bytes
        .strip_prefix(b"\x1b]")
        .and_then(|payload| payload.strip_suffix(b"\x07"))
        .and_then(|payload| parse(payload, u16::MAX as usize, u16::MAX as usize))
        .is_some()
}

pub(crate) fn parse(payload: &[u8], columns: usize, rows: usize) -> Option<CellOutlines> {
    let text = core::str::from_utf8(payload)
        .ok()?
        .strip_prefix("777;cell-outlines;1;")?;
    let mut fields = text.split(';');
    let mut outlines = CellOutlines::default();
    outlines.count = usize::from(number(&mut fields)?);
    if outlines.count > MAX_OUTLINES {
        return None;
    }
    for rect in &mut outlines.rects[..outlines.count] {
        *rect = CellRect {
            column: number(&mut fields)?,
            row: number(&mut fields)?,
            width: number(&mut fields)?,
            height: number(&mut fields)?,
        };
        if rect.width == 0
            || rect.height == 0
            || usize::from(rect.column) + usize::from(rect.width) > columns
            || usize::from(rect.row) + usize::from(rect.height) > rows
        {
            return None;
        }
    }
    fields.next().is_none().then_some(outlines)
}

fn number<'a>(fields: &mut impl Iterator<Item = &'a str>) -> Option<u16> {
    let field = fields.next()?;
    if field.is_empty() || !field.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    field.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_cell_rectangles_keep_their_order() {
        let report = parse(b"777;cell-outlines;1;2;6;2;4;1;2;4;5;1", 10, 5).unwrap();
        assert_eq!(
            report.as_slice(),
            &[
                CellRect {
                    column: 6,
                    row: 2,
                    width: 4,
                    height: 1
                },
                CellRect {
                    column: 2,
                    row: 4,
                    width: 5,
                    height: 1
                },
            ]
        );
        assert_eq!(
            parse(b"777;cell-outlines;1;0", 10, 5),
            Some(CellOutlines::default())
        );
        for invalid in [
            "777;cell-outlines;2;0",
            "777;cell-outlines;1;13",
            "777;cell-outlines;1;1;6;2;5;1",
            "777;cell-outlines;1;1;0;2;0;1",
            "777;cell-outlines;1;1;+0;2;1;1",
            "777;cell-outlines;1;1;0;2;1",
            "777;cell-outlines;1;0;",
        ] {
            assert_eq!(parse(invalid.as_bytes(), 10, 5), None, "{invalid}");
        }
    }

    #[test]
    fn recognizes_only_a_single_complete_reserved_write() {
        assert!(is_wire_report(b"\x1b]777;cell-outlines;1;1;2;3;1;1\x07"));
        assert!(is_wire_report(b"\x1b]777;cell-outlines;1;0\x07"));
        for bytes in [
            b"x\x1b]777;cell-outlines;1;0\x07".as_slice(),
            b"\x1b]777;cell-outlines;1;0\x07x",
            b"\x1b]777;cell-outlines;1;0",
            b"\x1b]777;cell-outlines;1;0\x07\x1b]777;cell-outlines;1;0\x07",
            b"\x1b]777;edit-life;2;0\x07",
        ] {
            assert!(!is_wire_report(bytes));
        }
    }
}
