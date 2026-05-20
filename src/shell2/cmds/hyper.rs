extern crate alloc;

use alloc::string::String;
use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};
use super::tlb_helper::print_table;
use crate::shell2::shell2_cmd::ParseOutcome;

const HYPER_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const HYPER_MENU_ROWS: [[&str; 2]; 3] = [
    ["status", "Show the kernel Hyper transport surfaces"],
    ["probe", "Describe the background HTTP/1 probe service"],
    ["<url> [path]", "Download URL into TRUEOSFS"],
];
const HYPER_DOWNLOAD_TIMEOUT_MS: u32 = 120_000;
const HYPER_DOWNLOAD_MAX_BYTES: usize = 16 * 1024 * 1024 * 1024;

fn line(io: &'static dyn ShellBackend2, text: &str) {
    print_shell_line(io, text);
}

fn print_status(io: &'static dyn ShellBackend2) {
    line(io, "hyper: client=http1 transport=tokio/vnet");
    line(io, "hyper: http fetch=body+stream-to-trueosfs");
    line(io, "hyper: https fetch=rustls body+stream-to-trueosfs");
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
    let path = without_fragment.rsplit('/').next().unwrap_or(without_fragment);
    if path.is_empty() { "download.bin" } else { path }
}

fn normalize_path(path: &str) -> Result<String, &'static str> {
    crate::r::path::FsPath::parse(path, false)
        .map(|path| path.to_relative_string())
        .map_err(|_| "bad path")
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

    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

    let result = if url.starts_with("http://") {
        let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
            log("hyper: no TRUEOSFS root");
            set_matrix_target_active(&target, false);
            return;
        };
        crate::t::run_on_shared_tokio({
            let url = url.clone();
            let path = path.clone();
            move || async move {
                crate::t::net::http_stream::fetch_http_to_file_hyper_async(
                    url.as_str(),
                    disk,
                    path.as_str(),
                    HYPER_DOWNLOAD_TIMEOUT_MS,
                    HYPER_DOWNLOAD_MAX_BYTES,
                )
                .await
                .map_err(|err| alloc::format!("{:?}", err))
            }
        })
        .await
        .map_err(|err| alloc::format!("shared tokio unavailable ({:?})", err))
        .and_then(|inner| inner)
    } else if url.starts_with("https://") {
        crate::t::run_on_shared_tokio({
            let url = url.clone();
            let path = path.clone();
            move || async move {
                crate::t::net::https::fetch_https_to_file_hyper_async(
                    url.as_str(),
                    path.as_str(),
                    HYPER_DOWNLOAD_TIMEOUT_MS,
                    HYPER_DOWNLOAD_MAX_BYTES,
                )
                .await
                .map_err(|err| alloc::format!("rc={}", err))
            }
        })
        .await
        .map_err(|err| alloc::format!("shared tokio unavailable ({:?})", err))
        .and_then(|inner| inner)
    } else {
        Err(String::from("unsupported URL scheme"))
    };

    match result {
        Ok(()) => log(alloc::format!("hyper: saved {}", path).as_str()),
        Err(err) => log(alloc::format!("hyper: download failed ({})", err).as_str()),
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
