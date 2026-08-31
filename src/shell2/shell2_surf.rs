use alloc::boxed::Box;
use alloc::string::String;
use core::sync::atomic::{AtomicU32, Ordering};
use trueos_executor::{SendSpawner, Spawner};

use super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line, submit_online_launch_script_to_target,
};
use crate::surfer::html_shack::{self, HtmlRoad, HtmlShackFileError};

const SOLARA_APP: &str = "solara";
const SOLARA_ARCHIVE: &str = "solara.bp";
const SOLARA_SURF_LAUNCH_HEADER: &str = "solara-surf-v1";
const SURF_HANDOFF_DIR: &str = "apps/common/solara/surf";
static SURF_HANDOFF_SEQUENCE: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfPromptPrefix {
    Http,
    Https,
    File,
    Html,
}

pub(crate) enum SurfSubmit {
    Url(String),
    File(String),
    Html(String),
}

async fn persist_solara_handoff(html: &html_shack::Html) -> Result<String, String> {
    let disk = crate::r::fs::trueosfs::primary_root_handle()
        .ok_or_else(|| String::from("no TRUEOSFS root mounted"))?;
    match crate::r::fs::trueosfs::dir_create_all_async(disk, SURF_HANDOFF_DIR).await {
        Ok(true) => {}
        Ok(false) => return Err(String::from("no space for handoff directory")),
        Err(error) => {
            return Err(alloc::format!("create handoff directory failed: {error:?}"));
        }
    }

    let sequence = SURF_HANDOFF_SEQUENCE.fetch_add(1, Ordering::AcqRel);
    let tag = alloc::format!("surf-{sequence:08x}");
    let path = alloc::format!("{SURF_HANDOFF_DIR}/{tag}.html");
    let handle = crate::r::fs::trueosfs::file_write_begin_typed_async(
        disk,
        path.as_str(),
        html.html.len() as u64,
        infer::ContentTypeId::HTML,
    )
    .await
    .map_err(|error| alloc::format!("write begin failed: {error:?}"))?
    .ok_or_else(|| String::from("write begin failed: no space"))?;
    if let Err(error) =
        crate::r::fs::trueosfs::file_write_chunk_async(handle, html.html.as_bytes()).await
    {
        let _ = crate::r::fs::trueosfs::file_write_abort_async(handle).await;
        return Err(alloc::format!("write chunk failed: {error:?}"));
    }
    if let Err(error) = crate::r::fs::trueosfs::file_write_finish_async(handle).await {
        let _ = crate::r::fs::trueosfs::file_write_abort_async(handle).await;
        return Err(alloc::format!("write finish failed: {error:?}"));
    }

    crate::log!(
        "shell2-surf: solara handoff ready tag={} bytes={} url={}\n",
        tag,
        html.html.len(),
        html.url
    );
    Ok(tag)
}

async fn remove_solara_handoff(tag: &str) {
    let path = alloc::format!("{SURF_HANDOFF_DIR}/{tag}.html");
    if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
        let _ = crate::r::fs::trueosfs::file_delete_async(disk, path.as_str()).await;
    }
}

fn solara_surf_launch_script(tag: &str, source_url: &str) -> Result<String, &'static str> {
    if source_url.is_empty() {
        return Err("source URL is empty");
    }
    if source_url
        .bytes()
        .any(|byte| matches!(byte, b'\r' | b'\n' | 0))
    {
        return Err("source URL contains a control character");
    }
    Ok(alloc::format!("{SOLARA_SURF_LAUNCH_HEADER}\n{tag}\n{source_url}"))
}

#[trueos_executor::task(pool_size = 4)]
async fn launch_solara_task(target: MatrixTarget, html: html_shack::Html) {
    let tag = match persist_solara_handoff(&html).await {
        Ok(tag) => tag,
        Err(error) => {
            print_matrix_target_system_line(
                &target,
                alloc::format!("surf: Solara handoff failed: {error}").as_str(),
            );
            return;
        }
    };
    let launch_script = match solara_surf_launch_script(tag.as_str(), html.url.as_str()) {
        Ok(script) => script,
        Err(error) => {
            remove_solara_handoff(tag.as_str()).await;
            print_matrix_target_system_line(
                &target,
                alloc::format!("surf: Solara runtime handoff failed: {error}").as_str(),
            );
            return;
        }
    };

    match super::cmds::run::submit_archive_name_to_target_from_app_db_with_launch_script_async(
        target.clone(),
        SOLARA_ARCHIVE,
        launch_script.clone(),
    )
    .await
    {
        Ok(source) => {
            print_matrix_target_system_line(
                &target,
                alloc::format!("surf: Solara render queued tag={tag} source={source}").as_str(),
            );
        }
        Err(error) if error == "archive not found" => {
            let spawner = unsafe { Spawner::for_current_executor().await };
            if submit_online_launch_script_to_target(
                &spawner,
                target.clone(),
                SOLARA_APP,
                launch_script.as_str(),
            )
            .is_err()
            {
                remove_solara_handoff(tag.as_str()).await;
                print_matrix_target_system_line(
                    &target,
                    "surf: Solara online launch task unavailable",
                );
            }
        }
        Err(error) => {
            remove_solara_handoff(tag.as_str()).await;
            print_matrix_target_system_line(
                &target,
                alloc::format!("surf: could not launch {SOLARA_ARCHIVE}: {error}").as_str(),
            );
        }
    }
}

fn spawn_solara_handoff(
    spawner: SendSpawner,
    target: MatrixTarget,
    html: html_shack::Html,
) -> bool {
    match launch_solara_task(target, html) {
        Ok(token) => {
            spawner.spawn(token);
            true
        }
        Err(_) => false,
    }
}

pub(crate) fn try_inline_html(line: &str) -> Option<String> {
    let candidate = strip_wrapping_quotes(line.trim());
    if !looks_like_inline_html(candidate) {
        return None;
    }
    Some(String::from(candidate))
}

pub(crate) fn try_parse_with_prefix(line: &str, prefix: SurfPromptPrefix) -> Option<SurfSubmit> {
    if let Some(html) = try_inline_html(line) {
        return Some(SurfSubmit::Html(html));
    }
    if let Some(file_ref) = try_file_reference(line) {
        return Some(SurfSubmit::File(file_ref));
    }

    let candidate = strip_wrapping_quotes(line.trim());
    if candidate.is_empty() {
        return None;
    }

    match prefix {
        SurfPromptPrefix::Html => Some(SurfSubmit::Html(String::from(candidate))),
        SurfPromptPrefix::File => Some(SurfSubmit::File(String::from(candidate))),
        SurfPromptPrefix::Http | SurfPromptPrefix::Https => {
            if candidate.split_whitespace().nth(1).is_some() || !is_url_token(candidate) {
                return None;
            }
            Some(SurfSubmit::Url(prepare_url_with_prefix(candidate, prefix)))
        }
    }
}

pub(crate) fn try_file_reference(line: &str) -> Option<String> {
    let candidate = strip_wrapping_quotes(line.trim());
    let path = candidate.strip_prefix("file://")?;
    if path.trim().is_empty() {
        return None;
    }
    Some(String::from(path))
}

pub(crate) fn load_inline_html(spawner: &Spawner, io: &'static dyn ShellBackend2, html: String) {
    let html = html_shack::prepare_ready_inline_html(html);
    enqueue_and_launch_html(spawner, io, html);
}

pub(crate) fn load_file_reference(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    file_ref: &str,
) {
    match html_shack::prepare_ready_file_html(file_ref) {
        Ok(html) => enqueue_and_launch_html(spawner, io, html),
        Err(HtmlShackFileError::NoRoot) => {
            print_shell_line(io, "surf: no TRUEOSFS root mounted");
        }
        Err(HtmlShackFileError::NotFound) => {
            print_shell_line(io, "surf: file not found");
        }
        Err(HtmlShackFileError::ReadFailed) => {
            print_shell_line(io, "surf: file read failed");
        }
    }
}

fn enqueue_and_launch_html(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    html: html_shack::Html,
) {
    let _ = html_shack::with_html_shack(|shack| shack.put_ready_html(html.clone()));
    let target = matrix_target_for_backend(io);
    if spawn_solara_handoff(spawner.make_send(), target, html) {
        print_shell_line(io, "surf: Solara handoff queued");
    } else {
        print_shell_line(io, "surf: Solara launch busy");
    }
}

pub(crate) fn prepare_call_with_url(spawner: &Spawner, io: &'static dyn ShellBackend2, url: &str) {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return;
    }

    if trimmed.len() > 256 {
        print_shell_line(io, "surf: url too long (max 256 chars)");
        return;
    }

    let road = if trimmed
        .get(..8)
        .map(|p| p.eq_ignore_ascii_case("https://"))
        .unwrap_or(false)
    {
        HtmlRoad::Https
    } else {
        HtmlRoad::Http
    };

    let task_spawner = spawner.make_send();
    let target = matrix_target_for_backend(io);
    let callback = Box::new(move |html| {
        if !spawn_solara_handoff(task_spawner, target.clone(), html) {
            print_matrix_target_system_line(&target, "surf: Solara launch busy");
        }
    });
    let _ = html_shack::with_html_shack(|shack| shack.get_ready(trimmed, road, Some(callback)));
    print_shell_line(io, "shack enque");
}

fn prepare_url_with_prefix(host: &str, prefix: SurfPromptPrefix) -> String {
    if has_known_scheme(host) {
        return String::from(host);
    }

    let mut url = String::from(match prefix {
        SurfPromptPrefix::Http => "http://",
        SurfPromptPrefix::Https => "https://",
        SurfPromptPrefix::File => "file://",
        SurfPromptPrefix::Html => "html://",
    });
    url.push_str(host);
    url
}

fn strip_wrapping_quotes(s: &str) -> &str {
    if s.len() >= 2 {
        let b = s.as_bytes();
        let first = b[0];
        let last = b[b.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return s[1..s.len() - 1].trim();
        }
    }
    s
}

fn has_http_scheme(s: &str) -> bool {
    s.get(..7)
        .map(|p| p.eq_ignore_ascii_case("http://"))
        .unwrap_or(false)
        || s.get(..8)
            .map(|p| p.eq_ignore_ascii_case("https://"))
            .unwrap_or(false)
}

fn has_known_scheme(s: &str) -> bool {
    has_http_scheme(s)
        || s.get(..7)
            .map(|p| p.eq_ignore_ascii_case("file://"))
            .unwrap_or(false)
        || s.get(..7)
            .map(|p| p.eq_ignore_ascii_case("html://"))
            .unwrap_or(false)
}

fn is_url_token(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(char::is_whitespace)
}

fn looks_like_inline_html(s: &str) -> bool {
    let lower = s.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }

    (lower.starts_with("<html") && lower.ends_with("</html>"))
        || lower.starts_with("<!doctype html")
        || (lower.starts_with('<') && lower.ends_with('>') && lower.contains("</"))
}
