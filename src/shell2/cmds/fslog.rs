use alloc::format;
use alloc::string::String;

use super::super::{ShellBackend2, line_width_for_backend, print_shell_line};
use crate::disc::block::{self, DeviceHandle};
use crate::shell2::shell2_cmd::ParseOutcome;

const DEFAULT_MAX_RECORDS: usize = 256;
const MAX_MAX_RECORDS: usize = 4096;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "fslog: usage `fslog [disc-id] [--max N]`");
}

fn parse_args(rest: &str) -> Result<(Option<u32>, usize), &'static str> {
    let mut disk_id = None;
    let mut max_records = DEFAULT_MAX_RECORDS;
    let mut args = rest.split_whitespace();

    while let Some(arg) = args.next() {
        match arg {
            "--max" | "-n" => {
                let Some(n) = args.next() else {
                    return Err("missing max");
                };
                max_records = n.parse::<usize>().map_err(|_| "bad max")?;
            }
            _ if arg.starts_with("--max=") => {
                max_records = arg[6..].parse::<usize>().map_err(|_| "bad max")?;
            }
            _ => {
                if disk_id.is_some() {
                    return Err("too many disks");
                }
                disk_id = Some(super::tlb_helper::parse_disc_id_raw(arg).ok_or("bad disk id")?);
            }
        }
    }

    Ok((disk_id, max_records.clamp(1, MAX_MAX_RECORDS)))
}

fn select_disk(disk_id: Option<u32>) -> Result<DeviceHandle, &'static str> {
    match disk_id {
        Some(raw) => super::tlb_helper::select_top_level_disk(raw).ok_or("disk not found"),
        None => crate::r::fs::trueosfs::primary_root_handle().ok_or("no TRUEOSFS root"),
    }
}

fn kind_text(kind: trueos_fs::LogKind) -> &'static str {
    match kind {
        trueos_fs::LogKind::Put => "put",
        trueos_fs::LogKind::Delete => "del",
        trueos_fs::LogKind::IndexCheckpoint => "ckpt",
    }
}

fn name_text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::from("-");
    }
    match core::str::from_utf8(bytes) {
        Ok(s) => String::from(s),
        Err(_) => format!("<{} bytes non-utf8>", bytes.len()),
    }
}

fn extra_text(record: &trueos_fs::RawLogRecord) -> String {
    if let Some(lba) = record.delete_ref_lba {
        return format!("ref={lba:08x}");
    }
    if let Some(replay_from) = record.checkpoint_replay_from_rel_blocks {
        return format!(
            "replay={} entries={}",
            replay_from,
            record.checkpoint_entry_count.unwrap_or(0)
        );
    }
    String::from("-")
}

fn stop_text(stop: &trueos_fs::RawLogStop) -> String {
    match stop {
        trueos_fs::RawLogStop::End => String::from("end"),
        trueos_fs::RawLogStop::MaxRecords => String::from("max-records"),
        trueos_fs::RawLogStop::InvalidHeader { lba } => format!("invalid-header@{lba}"),
        trueos_fs::RawLogStop::Uncommitted { lba } => format!("uncommitted@{lba}"),
        trueos_fs::RawLogStop::InvalidShape { lba } => format!("invalid-shape@{lba}"),
    }
}

fn scan(
    disk: DeviceHandle,
    max_records: usize,
) -> Result<Option<trueos_fs::RawLogScan>, block::Error> {
    crate::wait::spawn_and_wait_local(async move {
        crate::r::fs::trueosfs::raw_log_scan_async(disk, max_records).await
    })
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let (disk_id, max_records) = match parse_args(rest) {
        Ok(v) => v,
        Err(err) => {
            print_shell_line(io, format!("fslog: {err}").as_str());
            usage(io);
            return ParseOutcome::Handled;
        }
    };

    let disk = match select_disk(disk_id) {
        Ok(disk) => disk,
        Err(err) => {
            print_shell_line(io, format!("fslog: {err}").as_str());
            if disk_id.is_some() {
                super::tlb_helper::print_disk_choice_table(
                    io,
                    "fslog",
                    "disk devices",
                    super::tlb_helper::collect_top_level_disk_choices().as_slice(),
                );
            }
            return ParseOutcome::Handled;
        }
    };

    let info = disk.info();
    let scan = match scan(disk, max_records) {
        Ok(Some(scan)) => scan,
        Ok(None) => {
            print_shell_line(io, "fslog: disk has no TRUEOSFS placement");
            return ParseOutcome::Handled;
        }
        Err(err) => {
            print_shell_line(io, format!("fslog: scan failed: {err:?}").as_str());
            return ParseOutcome::Handled;
        }
    };

    print_shell_line(
        io,
        format!(
            "fslog: disk={} ({}) bs={} data_lba={} log_head_rel={} checkpoint_rel={} records={} stop={}",
            info.id.raw(),
            info.id,
            scan.block_size,
            scan.data_lba,
            scan.superblock.log_head_rel_blocks,
            scan.superblock.checkpoint_rel_blocks,
            scan.records.len(),
            stop_text(&scan.stop)
        )
        .as_str(),
    );

    let headers = ["FileID", "Rel", "Blocks", "Kind", "Data", "Name", "Extra"];
    let table = super::tlb_helper::TlbTable::with_width(
        &headers,
        line_width_for_backend(io).saturating_sub(2),
    )
    .with_max_col_widths(&[8, 8, 6, 4, 8, 0, 18]);
    table.emit_header(|text| print_shell_line(io, text));
    for record in scan.records.iter() {
        let id = format!("{:08x}", record.entry_lba);
        let rel = format!("{}", record.rel_blocks);
        let blocks = format!("{}", record.blocks);
        let data = format!("{}", record.data_len);
        let name = name_text(record.name.as_slice());
        let extra = extra_text(record);
        let row = [
            id.as_str(),
            rel.as_str(),
            blocks.as_str(),
            kind_text(record.kind),
            data.as_str(),
            name.as_str(),
            extra.as_str(),
        ];
        table.emit_row(&row, |text| print_shell_line(io, text));
    }
    table.emit_footer(|text| print_shell_line(io, text));

    ParseOutcome::Handled
}
