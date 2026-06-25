extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};
use super::tlb_helper::print_table;
use crate::shell2::shell2_cmd::ParseOutcome;

const HYPER_DOWNLOAD_TIMEOUT_MS: u64 = 90_000;
const HYPER_DOWNLOAD_MAX_BYTES: usize = 128 * 1024 * 1024;

const HYPER_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const HYPER_MENU_ROWS: [[&str; 2]; 3] = [
    ["status", "Show the kernel Hyper transport surfaces"],
    ["probe", "Describe the background HTTP/1 probe service"],
    ["<url> [path]", "Download URL into TRUEOSFS"],
];

fn line(io: &'static dyn ShellBackend2, text: &str) {
    print_shell_line(io, text);
}

fn print_status(io: &'static dyn ShellBackend2) {
    line(io, "hyper: client=http1 transport=hyper-vnet");
    line(io, "hyper: http fetch=byte lane");
    line(io, "hyper: https fetch=tls-socket");
    line(io, "hyper: probe=spawn-svc hyper-http1-probe");
}

fn print_probe(io: &'static dyn ShellBackend2) {
    line(io, "hyper probe: boot loopback validates HTTP/1 client");
    line(io, "hyper probe: background net probe waits for socket+gateway readiness");
    line(io, "hyper probe: target example.de:80 GET /");
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_table(io, &HYPER_MENU_HEADERS, &HYPER_MENU_ROWS);
}

fn normalize_url(input: &str) -> Result<String, &'static str> {
    let url = input.trim();
    if url.is_empty() {
        return Err("empty url");
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return Ok(String::from(url));
    }
    Ok(alloc::format!("https://{url}"))
}

fn basename_from_url(url: &str) -> &str {
    let without_query = url.split('?').next().unwrap_or(url);
    let without_fragment = without_query.split('#').next().unwrap_or(without_query);
    let path = without_fragment
        .rsplit('/')
        .next()
        .unwrap_or(without_fragment);
    if path.is_empty() {
        "download.bin"
    } else {
        path
    }
}

fn normalize_path(path: &str) -> Result<String, &'static str> {
    crate::r::path::FsPath::parse(path, false)
        .map(|path| path.to_relative_string())
        .map_err(|_| "bad path")
}

fn write_file(path: &str, bytes: &[u8]) -> Result<(), String> {
    let handle = crate::r::io::kfs::write_file_begin(path, bytes.len() as u64)
        .map_err(|err| format!("write begin failed: {:?}", err))?;
    if let Err(err) = crate::r::io::kfs::write_file_chunk(handle, bytes) {
        let _ = crate::r::io::kfs::write_file_abort(handle);
        return Err(format!("write chunk failed: {:?}", err));
    }
    crate::r::io::kfs::write_file_finish(handle)
        .map_err(|err| format!("write finish failed: {:?}", err))
}

async fn fetch_download_bytes(url: String) -> Result<Vec<u8>, String> {
    crate::r::net::https::get_bytes_shared(
        url.as_str(),
        HYPER_DOWNLOAD_TIMEOUT_MS as u32,
        HYPER_DOWNLOAD_MAX_BYTES,
    )
    .await
}

fn submit_download(spawner: &Spawner, io: &'static dyn ShellBackend2, url: String, path: String) {
    let target = matrix_target_for_backend(io);
    print_matrix_target_line(
        &target,
        alloc::format!("hyper: download {} -> {}", url, path).as_str(),
    );

    set_matrix_target_active(&target, true);
    match hyper_download_task(target.clone(), url, path) {
        Ok(token) => {
            spawner.spawn(token);
        }
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "hyper: spawn failed");
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn hyper_download_task(target: MatrixTarget, url: String, path: String) {
    let log = |line: &str| print_matrix_target_line(&target, line);

    match fetch_download_bytes(url.clone()).await {
        Ok(bytes) => match write_file(path.as_str(), bytes.as_slice()) {
            Ok(()) => log(format!("hyper: saved {} bytes -> {}", bytes.len(), path).as_str()),
            Err(err) => log(format!("hyper: write failed: {}", err).as_str()),
        },
        Err(err) => log(format!("hyper: download failed: {}", err).as_str()),
    }
    set_matrix_target_active(&target, false);
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    match args.next() {
        None | Some("status") => print_status(io),
        Some("probe") => print_probe(io),
        Some("help") | Some("-h") | Some("--help") => print_usage(io),
        Some(url_arg) => {
            let url = match normalize_url(url_arg) {
                Ok(url) => url,
                Err(err) => {
                    line(io, alloc::format!("hyper: {}", err).as_str());
                    return ParseOutcome::Handled;
                }
            };
            let path = match args.next() {
                Some(path_arg) => match normalize_path(path_arg) {
                    Ok(path) => path,
                    Err(err) => {
                        line(io, alloc::format!("hyper: {}", err).as_str());
                        return ParseOutcome::Handled;
                    }
                },
                None => String::from(basename_from_url(url.as_str())),
            };
            if args.next().is_some() {
                line(io, "hyper: usage `hyper <url> [path]`");
                return ParseOutcome::Handled;
            }
            submit_download(spawner, io, url, path);
        }
    }

    ParseOutcome::Handled
}
