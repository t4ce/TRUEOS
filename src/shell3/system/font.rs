use super::{
    MatrixSlotLease, MatrixSlots, MatrixTarget,
    helper::{self, Screen},
    output::DumpTarget,
    picker::{Event, Picker},
    tui::{self, Frontend},
};
use alloc::{string::String, vec::Vec};
use spin::Mutex;
use trueos_time::{Duration, Timer};

const PARTS: [(&str, &str); 5] = [
    ("cp850", "CP850"),
    ("boxd", "Box drawing"),
    ("block", "Block elements"),
    ("brail", "Braille"),
    ("extra", "Extra raster characters"),
];
static RETURNS: Mutex<Vec<(u64, DumpTarget)>> = Mutex::new(Vec::new());

pub(super) fn take_return(frontend: u64) -> Option<DumpTarget> {
    let mut returns = RETURNS.lock();
    let index = returns.iter().position(|(id, _)| *id == frontend)?;
    Some(returns.remove(index).1)
}

pub(super) fn start(frontend: Frontend) -> Result<(), String> {
    let Some((target, spawner)) = helper::admit("font", frontend)? else {
        return Ok(());
    };
    let task = menu_task(target.clone()).map_err(|_| {
        tui::cancel_native_attach(&target);
        String::from("Font menu task pool is full.")
    })?;
    spawner.spawn(task);
    Ok(())
}

fn characters(index: usize) -> Vec<char> {
    match index {
        0 => microfont::CP850_CHARACTERS.to_vec(),
        1 => ('\u{2500}'..='\u{257f}').collect(),
        2 => ('\u{2580}'..='\u{259f}').collect(),
        3 => ('\u{2800}'..='\u{28ff}').collect(),
        _ => {
            let mut extra: Vec<_> = microfont::raster_line::GLYPH_CHARACTERS.iter()
                .chain(microfont::raster_quad::GLYPH_CHARACTERS)
                .copied()
                .filter(|ch| !microfont::CP850_CHARACTERS.contains(ch)
                    && !('\u{2500}'..='\u{259f}').contains(ch)
                    && !('\u{2800}'..='\u{28ff}').contains(ch))
                .collect();
            extra.sort_unstable();
            extra.dedup();
            extra
        }
    }
}

fn specimen(index: usize) -> Vec<String> {
    let mut lines = alloc::vec![PARTS[index].1.into(), String::new()];
    for row in characters(index).chunks(16) {
        let mut line = String::new();
        for &ch in row {
            if !line.is_empty() { line.push(' '); }
            line.push(if ch.is_control() { ' ' } else { ch });
        }
        lines.push(line);
    }
    lines
}

#[trueos_executor::task(pool_size = 2)]
async fn menu_task(target: MatrixTarget) {
    let mut picker = Picker::default();
    let mut screen = Screen::default();
    let labels = PARTS.map(|(_, label)| label);
    while let Some((bytes, _)) = tui::native_read(&target) {
        if !tui::native_visible(&target) {
            Timer::after(Duration::from_millis(20)).await;
            continue;
        }
        let Some(frontend) = tui::native_frontend(&target) else { break; };
        let now = crate::chronos::monotonic_nanos();
        match picker.input(&bytes, PARTS.len(), frontend.rows, now) {
            Some(Event::Choose(index)) => {
                let name = PARTS[index].0;
                let (name, lifetime) = MatrixSlots::allocate_slot(Some(name.into()), name);
                let lease = MatrixSlotLease::from_identity(name, lifetime);
                for line in specimen(index).into_iter().rev() {
                    MatrixSlots::echo_output(&lease, line);
                }
                RETURNS.lock().push((frontend.id, DumpTarget(Some(lease))));
                MatrixSlots::drop_lease(&super::matrix_target_slot_lease(&target));
                break;
            }
            Some(Event::Quit) => tui::native_return(&target),
            _ => {}
        }
        if tui::native_visible(&target) {
            let lines = picker.frame("FONT  MicroFont", "Choose a character set to open in its own Matrix slot.",
                &labels, frontend.rows);
            screen.paint(&target, &lines, frontend.cols, frontend.rows);
        }
        Timer::after(Duration::from_millis(20)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_preserve_their_character_order_and_display_cells() {
        for (index, count) in [(0, 256), (1, 128), (2, 32), (3, 256)] {
            let glyphs = characters(index);
            assert_eq!(glyphs.len(), count);
            let lines = specimen(index);
            assert_eq!(lines.len(), 2 + count / 16);
            for (row, glyphs) in lines[2..].iter().zip(glyphs.chunks(16)) {
                assert_eq!(row.chars().count(), 31);
                assert!(!row.chars().any(char::is_control));
                assert!(row.chars().step_by(2).eq(glyphs.iter().map(|&ch|
                    if ch.is_control() { ' ' } else { ch })));
            }
        }
    }

    #[test]
    fn extras_come_from_raster_records_without_duplicate_groups() {
        let extra = characters(4);
        assert!(extra.windows(2).all(|pair| pair[0] < pair[1]));
        for ch in extra {
            assert!(!microfont::CP850_CHARACTERS.contains(&ch));
            assert!(!('\u{2500}'..='\u{259f}').contains(&ch));
            assert!(!('\u{2800}'..='\u{28ff}').contains(&ch));
            assert!(microfont::raster_line::GLYPH_CHARACTERS.contains(&ch)
                || microfont::raster_quad::GLYPH_CHARACTERS.contains(&ch));
        }
    }
}
