use alloc::boxed::Box;
use alloc::string::String;
use core::sync::atomic::{AtomicU32, Ordering};
use trueos_executor::{SendSpawner, Spawner};

use super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line, submit_online_launch_script_to_target,
};
use crate::surfer::html_shack::{self, HtmlRoad};

const SOLARA_APP: &str = "solara";
const SOLARA_ARCHIVE: &str = "solara.bp";
const SURF_HANDOFF_DIR: &str = "apps/common/solara/surf";
static SURF_HANDOFF_SEQUENCE: AtomicU32 = AtomicU32::new(1);

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
    Ok(path)
}

async fn remove_solara_handoff(path: &str) {
    if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
        let _ = crate::r::fs::trueosfs::file_delete_async(disk, path).await;
    }
}

fn solara_surf_launch_script(path: &str, source_url: &str) -> Result<String, &'static str> {
    if source_url.is_empty() {
        return Err("source URL is empty");
    }
    if source_url
        .bytes()
        .any(|byte| matches!(byte, b'\r' | b'\n' | 0))
    {
        return Err("source URL contains a control character");
    }
    Ok(alloc::format!("fs-scope trueosfs\nopen {source_url}\nsource {path}\n"))
}

#[trueos_executor::task(pool_size = 4)]
async fn launch_solara_task(target: MatrixTarget, html: html_shack::Html) {
    let path = match persist_solara_handoff(&html).await {
        Ok(path) => path,
        Err(error) => {
            print_matrix_target_system_line(
                &target,
                alloc::format!("surf: Solara handoff failed: {error}").as_str(),
            );
            return;
        }
    };
    let launch_script = match solara_surf_launch_script(path.as_str(), html.url.as_str()) {
        Ok(script) => script,
        Err(error) => {
            remove_solara_handoff(path.as_str()).await;
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
                alloc::format!("surf: Solara open queued url={} source={source}", html.url)
                    .as_str(),
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
                remove_solara_handoff(path.as_str()).await;
                print_matrix_target_system_line(
                    &target,
                    "surf: Solara online launch task unavailable",
                );
            }
        }
        Err(error) => {
            remove_solara_handoff(path.as_str()).await;
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

pub(crate) fn prepare_call_with_url(spawner: &Spawner, io: &'static dyn ShellBackend2, url: &str) {
    let trimmed = strip_wrapping_quotes(url.trim());

    if trimmed.len() > 256 {
        print_shell_line(io, "surf: url too long (max 256 chars)");
        return;
    }

    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        print_shell_line(io, "surf: expected one URL");
        return;
    }

    let url = if has_http_scheme(trimmed) {
        String::from(trimmed)
    } else if trimmed.contains("://") {
        print_shell_line(io, "surf: only http and https URLs are supported");
        return;
    } else {
        alloc::format!("https://{trimmed}")
    };

    let road = if url
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
    let _ = html_shack::with_html_shack(|shack| shack.get_ready(&url, road, Some(callback)));
    print_shell_line(io, "surf: fetch queued");
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

#[cfg(test)]
mod tests {
    use super::solara_surf_launch_script;

    #[test]
    fn launch_script_grants_scope_and_names_url_and_source() {
        assert_eq!(
            solara_surf_launch_script(
                "apps/common/solara/surf/surf-1.html",
                "https://example.com/"
            )
            .unwrap(),
            "fs-scope trueosfs\nopen https://example.com/\nsource apps/common/solara/surf/surf-1.html\n"
        );
    }
}
