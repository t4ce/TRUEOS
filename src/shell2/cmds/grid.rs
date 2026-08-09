use alloc::{string::String, vec::Vec};
use embassy_executor::Spawner;
use embassy_executor::task;

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line, submit_online_to_target,
};

const GRID_APP: &str = "gridpaper";
const GRID_ARCHIVE: &str = "gridpaper.bp";

const GRID_COLUMN_SOFT_CAP: u32 = 39;
const GRID_ROW_SOFT_CAP: u32 = 55;
const GRID_MIN_SCALE_PERCENT: u16 = 1;
const GRID_MAX_SCALE_PERCENT: u16 = 800;
const GRID_DEFAULT_SCALE_PERCENT: u16 = 100;

#[task(pool_size = 10)]
async fn launch_gridpaper(
    spawner: Spawner,
    target: MatrixTarget,
    columns: u32,
    rows: u32,
    scale_percent: u16,
) {
    let args = alloc::vec![
        alloc::format!("{columns}x{rows}"),
        alloc::format!("{scale_percent}%"),
    ];
    match super::run::submit_archive_name_to_target_prefer_trueosfs_with_instance_async(
        target.clone(),
        GRID_ARCHIVE,
        args.clone(),
        crate::hv::BlueprintInstanceRequest::default(),
    )
    .await
    {
        Ok(_) => {}
        Err(error) if error == "archive not found" => {
            let mut online_args = Vec::<String>::with_capacity(args.len() + 1);
            online_args.push(String::from(GRID_APP));
            online_args.extend(args);
            if submit_online_to_target(&spawner, target.clone(), online_args).is_err() {
                print_matrix_target_system_line(
                    &target,
                    "grid: online Gridpaper launch task unavailable",
                );
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
        print_shell_line(
            io,
            "grid: usage `grid [COLUMNSxROWS] [SCALE%]`; defaults to 39x55 at 100%",
        );
        print_shell_line(
            io,
            "grid: launches one Gridpaper Blueprint backed by the ten-worker kernel scene pool",
        );
        print_shell_line(
            io,
            "grid: secondary-click opens its printer menu; PrintScreen queues the default; Escape closes",
        );
        return ParseOutcome::Handled;
    }

    let Some((columns, rows, scale_percent)) = parse_grid_request(trimmed) else {
        print_shell_line(
            io,
            "grid: expected `[COLUMNSxROWS] [SCALE%]` within 1x1..39x55 and 1%..800%",
        );
        return ParseOutcome::Handled;
    };

    let target = matrix_target_for_backend(io);
    match launch_gridpaper(*spawner, target, columns, rows, scale_percent) {
        Ok(token) => {
            spawner.spawn(token);
            print_shell_line(
                io,
                alloc::format!(
                    "grid: Gridpaper Blueprint requested {columns}x{rows} at {scale_percent}% (kernel scene pool)"
                )
                .as_str(),
            );
            print_shell_line(
                io,
                "grid: secondary-click selects a printer; PrintScreen uses the default; Escape closes",
            );
        }
        Err(_) => print_shell_line(io, "grid: Blueprint launch task unavailable"),
    }
    ParseOutcome::Handled
}

fn parse_grid_request(value: &str) -> Option<(u32, u32, u16)> {
    if value.is_empty() {
        return Some((GRID_COLUMN_SOFT_CAP, GRID_ROW_SOFT_CAP, GRID_DEFAULT_SCALE_PERCENT));
    }
    let mut fields = value.split_whitespace();
    let size = fields.next()?;
    let scale = fields.next();
    if fields.next().is_some() {
        return None;
    }
    let (columns, rows) = parse_grid_size(size)?;
    let scale_percent = match scale {
        Some(value) => parse_scale_percent(value)?,
        None => GRID_DEFAULT_SCALE_PERCENT,
    };
    Some((columns, rows, scale_percent))
}

fn parse_grid_size(value: &str) -> Option<(u32, u32)> {
    let (columns, rows) = value
        .split_once('x')
        .or_else(|| value.split_once('X'))
        .or_else(|| value.split_once("by"))?;
    let columns = columns.parse::<u32>().ok()?;
    let rows = rows.parse::<u32>().ok()?;
    (columns != 0 && columns <= GRID_COLUMN_SOFT_CAP && rows != 0 && rows <= GRID_ROW_SOFT_CAP)
        .then_some((columns, rows))
}

fn parse_scale_percent(value: &str) -> Option<u16> {
    let percent = value
        .strip_suffix('%')
        .unwrap_or(value)
        .parse::<u16>()
        .ok()?;
    (GRID_MIN_SCALE_PERCENT..=GRID_MAX_SCALE_PERCENT)
        .contains(&percent)
        .then_some(percent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_request_defaults_to_full_native_scene() {
        assert_eq!(parse_grid_request(""), Some((39, 55, 100)));
        assert_eq!(parse_grid_request("12x20"), Some((12, 20, 100)));
    }

    #[test]
    fn grid_request_accepts_bounded_size_and_scale() {
        assert_eq!(parse_grid_request("1x1 1%"), Some((1, 1, 1)));
        assert_eq!(parse_grid_request("39X55 800"), Some((39, 55, 800)));
        assert_eq!(parse_grid_request("12by20 150%"), Some((12, 20, 150)));
        for value in [
            "0x1",
            "1x0",
            "40x1",
            "1x56",
            "1x1 0%",
            "1x1 801%",
            "1x1 100% extra",
        ] {
            assert_eq!(parse_grid_request(value), None, "{value}");
        }
    }
}
