use embassy_executor::Spawner;

use super::super::shell2_cmd::ParseOutcome;
use super::super::{ShellBackend2, print_shell_line};

const GRID_COLUMN_SOFT_CAP: usize = 39;
const GRID_ROW_SOFT_CAP: usize = 55;

pub(crate) fn try_parse(
    _spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let trimmed = rest.trim();
    if matches!(trimmed, "help" | "-h" | "--help") {
        print_shell_line(io, "grid: usage `grid [COLUMNSxROWS]`; bounds are 1x1 through 39x55");
        return ParseOutcome::Handled;
    }
    let (columns, rows) = if trimmed.is_empty() {
        (GRID_COLUMN_SOFT_CAP, GRID_ROW_SOFT_CAP)
    } else if parse_grid_size(trimmed).is_some() {
        parse_grid_size(trimmed).expect("validated Gridpaper extent")
    } else {
        print_shell_line(io, "grid: expected one COLUMNSxROWS size within 1x1 and 39x55");
        return ParseOutcome::Handled;
    };

    match crate::r::gridpaper_service::request_shell_grid(columns as u32, rows as u32) {
        Ok(_) => print_shell_line(
            io,
            alloc::format!(
                "grid: kernel Gridpaper requested {columns}x{rows} (no Blueprint container)"
            )
            .as_str(),
        ),
        Err(error) => print_shell_line(
            io,
            alloc::format!("grid: kernel Gridpaper request failed: {error:?}").as_str(),
        ),
    }
    ParseOutcome::Handled
}

fn parse_grid_size(value: &str) -> Option<(usize, usize)> {
    let (columns, rows) = value
        .split_once('x')
        .or_else(|| value.split_once('X'))
        .or_else(|| value.split_once("by"))?;
    let columns = columns.parse::<usize>().ok()?;
    let rows = rows.parse::<usize>().ok()?;
    (columns != 0 && columns <= GRID_COLUMN_SOFT_CAP && rows != 0 && rows <= GRID_ROW_SOFT_CAP)
        .then_some((columns, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_size_parser_accepts_every_bounded_positive_combination() {
        for columns in 1..=GRID_COLUMN_SOFT_CAP {
            for rows in 1..=GRID_ROW_SOFT_CAP {
                let value = alloc::format!("{columns}x{rows}");
                assert_eq!(parse_grid_size(value.as_str()), Some((columns, rows)));
            }
        }
        for value in ["0x1", "1x0", "40x1", "1x56", "1", "1x1x1"] {
            assert_eq!(parse_grid_size(value), None);
        }
    }
}
