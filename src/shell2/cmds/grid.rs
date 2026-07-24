use alloc::string::String;
use alloc::vec::Vec;

use embassy_executor::{Spawner, task};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line, submit_online_to_target,
};

const GRID_APP: &str = "gridpaper";
const GRID_ARCHIVE: &str = "gridpaper.bp";
const GRID_COLUMN_SOFT_CAP: usize = 39;
const GRID_ROW_SOFT_CAP: usize = 55;

#[task(pool_size = 2)]
async fn launch_grid(spawner: Spawner, target: MatrixTarget, app_args: Vec<String>) {
    match super::run::submit_archive_name_to_target_prefer_trueosfs_async(
        target.clone(),
        GRID_ARCHIVE,
        app_args.clone(),
    )
    .await
    {
        Ok(_) => {}
        Err(error) if error == "archive not found" => {
            let mut online_args = Vec::with_capacity(app_args.len().saturating_add(1));
            online_args.push(String::from(GRID_APP));
            online_args.extend(app_args);
            if submit_online_to_target(&spawner, target.clone(), online_args).is_err() {
                print_matrix_target_system_line(&target, "grid: online launch task unavailable");
            }
        }
        Err(error) => print_matrix_target_system_line(
            &target,
            alloc::format!("grid: could not launch {GRID_ARCHIVE}: {error}").as_str(),
        ),
    }
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let trimmed = rest.trim();
    if matches!(trimmed, "help" | "-h" | "--help") {
        print_shell_line(io, "grid: usage `grid [COLUMNSxROWS]`; bounds are 1x1 through 39x55");
        return ParseOutcome::Handled;
    }
    let app_args = if trimmed.is_empty() {
        Vec::new()
    } else if parse_grid_size(trimmed).is_some() {
        alloc::vec![String::from(trimmed)]
    } else {
        print_shell_line(io, "grid: expected one COLUMNSxROWS size within 1x1 and 39x55");
        return ParseOutcome::Handled;
    };

    let target = matrix_target_for_backend(io);
    match launch_grid(*spawner, target, app_args) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "grid: launch task unavailable"),
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
