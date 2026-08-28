use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use sha2::{Digest, Sha256};
use trueos_executor::{SpawnError, Spawner};
use trueos_time::Duration as EmbassyDuration;

use super::cmds::run;
use super::cmds::tlb_helper::TlbTable;
use super::{
    MatrixTarget, ShellBackend2, line_width_for_backend, matrix_target_for_backend,
    print_matrix_target_system_line as print_matrix_target_line, print_shell_line,
    set_matrix_target_active,
};

#[derive(Clone)]
struct OnlineApp {
    name: String,
    archive_name: String,
    sha256: String,
    url: String,
}

const ONLINE_APPS_URL: &str = "https://trueos.eu/apps";
const ONLINE_LIST_MAX_BYTES: usize = 1024 * 1024;
const ONLINE_APP_MAX_BYTES: usize = 64 * 1024 * 1024;
const ONLINE_FETCH_TIMEOUT_MS: u32 = 45_000;
const ONLINE_APP_HASH_SEPARATOR: &str = "§§";
const SHA256_HEX_LEN: usize = 64;
const ONLINE_HEADERS: &[&str; 6] = &["id", "app", "sha", "id", "app", "sha"];

async fn fetch_url_bytes(url: String, max_bytes: usize) -> Result<Vec<u8>, String> {
    crate::surfer::html_shack::fetch_bytes_via_pool(url, ONLINE_FETCH_TIMEOUT_MS as u64, max_bytes)
        .await
        .map(|fetch| fetch.bytes)
}

async fn fetch_online_apps_html() -> Result<Vec<u8>, String> {
    fetch_url_bytes(String::from(ONLINE_APPS_URL), ONLINE_LIST_MAX_BYTES).await
}

fn absolutize_online_url(href: &str) -> String {
    if href.contains("://") {
        String::from(href)
    } else if href.starts_with('/') {
        alloc::format!("https://trueos.eu{}", href)
    } else {
        alloc::format!("https://trueos.eu/apps/{}", href)
    }
}

fn parse_attr_value<'a>(text: &'a str, attr: &str) -> Option<&'a str> {
    let pos = text.find(attr)?;
    let rest = &text[pos + attr.len()..];
    let quote = rest.as_bytes().first().copied()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let rest = &rest[1..];
    let end = rest.as_bytes().iter().position(|&b| b == quote)?;
    Some(&rest[..end])
}

fn published_app_name_parts(value: &str) -> Option<(&str, &str)> {
    if let Some((archive_name, sha256)) = value.rsplit_once(ONLINE_APP_HASH_SEPARATOR) {
        if archive_name.to_ascii_lowercase().ends_with(".bp")
            && sha256.len() == SHA256_HEX_LEN
            && sha256.as_bytes().iter().all(u8::is_ascii_hexdigit)
        {
            return Some((archive_name, sha256));
        }
        return None;
    }

    value
        .to_ascii_lowercase()
        .ends_with(".bp")
        .then_some((value, "-"))
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_nibble(bytes[index + 1]), hex_nibble(bytes[index + 2]))
        {
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(decoded.as_slice()).into_owned()
}

fn published_link_parts(link_text: &str, href: &str) -> Option<(String, String)> {
    let text_parts = published_app_name_parts(link_text);
    let encoded_href_name = href
        .split(['?', '#'])
        .next()
        .unwrap_or(href)
        .rsplit('/')
        .next()
        .unwrap_or(href);
    let href_name = percent_decode(encoded_href_name);
    let href_parts = published_app_name_parts(href_name.as_str());
    match (text_parts, href_parts) {
        (Some(parts), _) if parts.1 != "-" => Some((parts.0.to_string(), parts.1.to_string())),
        (_, Some(parts)) if parts.1 != "-" => Some((parts.0.to_string(), parts.1.to_string())),
        (Some(parts), _) => Some((parts.0.to_string(), parts.1.to_string())),
        (None, Some(parts)) => Some((parts.0.to_string(), parts.1.to_string())),
        (None, None) => None,
    }
}

fn clean_archive_name(value: &str) -> Option<&str> {
    let name = value.rsplit('/').next()?.rsplit('\\').next()?;
    if name.is_empty()
        || !name.to_ascii_lowercase().ends_with(".bp")
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return None;
    }
    Some(name)
}

fn parse_online_apps(html: &str) -> Vec<OnlineApp> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(li_start) = rest.find("<li") {
        rest = &rest[li_start + 3..];
        let li_end = rest.find("</li>").unwrap_or(rest.len());
        let item = &rest[..li_end];
        let Some(a_start) = item.find("<a") else {
            rest = &rest[li_end..];
            continue;
        };
        let link = &item[a_start..];
        let Some(tag_end) = link.find('>') else {
            rest = &rest[li_end..];
            continue;
        };
        let tag = &link[..tag_end];
        let Some(href) = parse_attr_value(tag, "href=") else {
            rest = &rest[li_end..];
            continue;
        };
        let Some(text_end) = crate::r::pat::find_str(&link[tag_end + 1..], "</a>") else {
            rest = &rest[li_end..];
            continue;
        };
        let published_name = link[tag_end + 1..tag_end + 1 + text_end].trim();
        let Some((published_archive_name, sha256)) = published_link_parts(published_name, href)
        else {
            rest = &rest[li_end..];
            continue;
        };
        let Some(archive_name) = clean_archive_name(published_archive_name.as_str()) else {
            rest = &rest[li_end..];
            continue;
        };
        let url = absolutize_online_url(href);
        out.push(OnlineApp {
            name: trim_bp_suffix(archive_name).to_string(),
            archive_name: archive_name.to_string(),
            sha256,
            url,
        });
        rest = &rest[li_end..];
    }
    out
}

async fn online_apps() -> Result<Vec<OnlineApp>, String> {
    let html = fetch_online_apps_html().await?;
    let text = core::str::from_utf8(html.as_slice())
        .map_err(|_| String::from("online apps list is not UTF-8"))?;
    Ok(parse_online_apps(text))
}

fn short_sha256(value: &str) -> String {
    if value.len() != SHA256_HEX_LEN {
        return value.to_string();
    }

    alloc::format!("{}…{}", &value[..4], &value[value.len() - 4..])
}

fn print_online_apps_target(target: &MatrixTarget, width: usize, apps: &[OnlineApp], prefix: &str) {
    if apps.is_empty() {
        print_matrix_target_line(
            target,
            alloc::format!("{}: online list is empty", prefix).as_str(),
        );
        return;
    }
    let id_width = apps
        .len()
        .saturating_sub(1)
        .to_string()
        .len()
        .max(ONLINE_HEADERS[0].len());
    let sha_width = 9;
    let table = TlbTable::with_width(ONLINE_HEADERS, width.saturating_sub(2))
        .with_max_col_widths(&[id_width, 0, sha_width, id_width, 0, sha_width]);
    table.emit_header(|text| print_matrix_target_line(target, text));
    for left_idx in (0..apps.len()).step_by(2) {
        let left_app = &apps[left_idx];
        let left_id = alloc::format!("{}", left_idx);
        let left_sha = short_sha256(left_app.sha256.as_str());

        let right_idx = left_idx + 1;
        let right_app = apps.get(right_idx);
        let right_id = right_app
            .map(|_| alloc::format!("{}", right_idx))
            .unwrap_or_default();
        let right_name = right_app.map(|app| app.name.as_str()).unwrap_or("");
        let right_sha = right_app
            .map(|app| short_sha256(app.sha256.as_str()))
            .unwrap_or_default();
        let row = [
            left_id.as_str(),
            left_app.name.as_str(),
            left_sha.as_str(),
            right_id.as_str(),
            right_name,
            right_sha.as_str(),
        ];
        table.emit_row(&row, |text| print_matrix_target_line(target, text));
    }
    table.emit_footer(|text| print_matrix_target_line(target, text));
}

fn trim_bp_suffix(value: &str) -> &str {
    let suffix_at = value.len().saturating_sub(3);
    if value.is_char_boundary(suffix_at)
        && value
            .get(suffix_at..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".bp"))
    {
        &value[..suffix_at]
    } else {
        value
    }
}

fn online_app_match_key(value: &str) -> &str {
    let value = value.trim();
    let end = value
        .char_indices()
        .find_map(|(idx, ch)| matches!(ch, '?' | '#').then_some(idx))
        .unwrap_or(value.len());
    let value = &value[..end];
    let value = value.rsplit('/').next().unwrap_or(value);
    let value = value
        .rsplit_once(ONLINE_APP_HASH_SEPARATOR)
        .map(|(archive_name, _)| archive_name)
        .unwrap_or(value);
    trim_bp_suffix(value)
}

fn resolve_online_app<'a>(apps: &'a [OnlineApp], selector: &str) -> Option<&'a OnlineApp> {
    if let Ok(id) = selector.parse::<usize>() {
        return apps.get(id);
    }

    let requested = online_app_match_key(selector);
    apps.iter().find(|app| {
        online_app_match_key(app.name.as_str()).eq_ignore_ascii_case(requested)
            || online_app_match_key(app.archive_name.as_str()).eq_ignore_ascii_case(requested)
            || online_app_match_key(app.url.as_str()).eq_ignore_ascii_case(requested)
    })
}

fn online_app_sha256_matches(app: &OnlineApp, bytes: &[u8]) -> bool {
    if app.sha256 == "-" {
        return true;
    }

    let digest = Sha256::digest(bytes);
    let mut actual = String::with_capacity(SHA256_HEX_LEN);
    for byte in digest {
        let _ = write!(actual, "{byte:02x}");
    }
    actual.eq_ignore_ascii_case(app.sha256.as_str())
}

async fn wait_for_online_ready() -> bool {
    crate::r::readiness::wait_for_timeout(
        crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TLS_SOCKET_SERVICE_READY,
        EmbassyDuration::from_millis(ONLINE_FETCH_TIMEOUT_MS as u64),
    )
    .await
}

fn write_blueprint(app: &OnlineApp, bytes: &[u8]) -> Result<String, String> {
    crate::app_db::insert_download(app.archive_name.as_str(), bytes)?;
    Ok(alloc::format!("app.db:{}", app.archive_name))
}

#[trueos_executor::task(pool_size = 2)]
async fn online_run_task(
    target: MatrixTarget,
    width: usize,
    mut args: Vec<String>,
    launch_script: Option<String>,
) {
    let log = |text: &str| print_matrix_target_line(&target, text);
    if !wait_for_online_ready().await {
        log("apps: online network unavailable");
        set_matrix_target_active(&target, false);
        return;
    }

    if args.is_empty() {
        log("apps: fetching online app list");
        match online_apps().await {
            Ok(apps) => print_online_apps_target(&target, width, apps.as_slice(), "apps"),
            Err(err) => log(alloc::format!("apps: online list failed: {}", err).as_str()),
        }
        set_matrix_target_active(&target, false);
        return;
    }

    // Host launch is deliberately argument-free.  App configuration belongs
    // to the VMX shell after the Blueprint is running.  `new` remains an
    // internal compatibility transport for named launchers such as GridP.
    let instance = if args.first().is_some_and(|arg| arg == "new") {
        args.remove(0);
        if args.len() < 2 {
            log(
                "apps: launch arguments are not supported; start the app, then configure it in VMX",
            );
            set_matrix_target_active(&target, false);
            return;
        }
        let selector = args.remove(0);
        let name = args.remove(0);
        (selector, crate::hv::BlueprintInstanceRequest::named(name))
    } else {
        let selector = args.remove(0);
        if !args.is_empty() {
            log(
                "apps: launch arguments are not supported; start the app, then configure it in VMX",
            );
            set_matrix_target_active(&target, false);
            return;
        }
        let instance = crate::hv::BlueprintInstanceRequest::default();
        (selector, instance)
    };
    let (selector, instance) = instance;
    let app_args = args;
    let apps = match online_apps().await {
        Ok(apps) => apps,
        Err(err) => {
            log(alloc::format!("apps: online list failed: {}", err).as_str());
            set_matrix_target_active(&target, false);
            return;
        }
    };
    let Some(app) = resolve_online_app(apps.as_slice(), selector.as_str()) else {
        log(alloc::format!("apps: no app with that id or name `{}`", selector).as_str());
        print_online_apps_target(&target, width, apps.as_slice(), "apps");
        set_matrix_target_active(&target, false);
        return;
    };
    log(alloc::format!("apps: fetching {} from {}", app.name, app.url).as_str());
    match fetch_url_bytes(app.url.clone(), ONLINE_APP_MAX_BYTES).await {
        Ok(module_bytes) => {
            if !online_app_sha256_matches(app, module_bytes.as_slice()) {
                log("apps: online app SHA-256 mismatch");
                set_matrix_target_active(&target, false);
                return;
            }
            let _ = run::enqueue_blueprint_bytes_with_instance_and_launch_script(
                target.clone(),
                app.archive_name.clone(),
                module_bytes,
                app_args,
                instance,
                launch_script,
            );
        }
        Err(err) => log(alloc::format!("apps: online fetch failed: {}", err).as_str()),
    }
    set_matrix_target_active(&target, false);
}

#[trueos_executor::task(pool_size = 2)]
async fn download_task(target: MatrixTarget, width: usize, selector: Option<String>) {
    let log = |text: &str| print_matrix_target_line(&target, text);
    if !wait_for_online_ready().await {
        log("dl: network unavailable");
        set_matrix_target_active(&target, false);
        return;
    }

    log("dl: fetching online app list");
    let apps = match online_apps().await {
        Ok(apps) => apps,
        Err(err) => {
            log(alloc::format!("dl: online list failed: {}", err).as_str());
            set_matrix_target_active(&target, false);
            return;
        }
    };
    let Some(selector) = selector else {
        print_online_apps_target(&target, width, apps.as_slice(), "dl");
        set_matrix_target_active(&target, false);
        return;
    };
    let Some(app) = resolve_online_app(apps.as_slice(), selector.as_str()) else {
        log(alloc::format!("dl: no app with that id or name `{}`", selector).as_str());
        print_online_apps_target(&target, width, apps.as_slice(), "dl");
        set_matrix_target_active(&target, false);
        return;
    };

    log(alloc::format!("dl: fetching {} from {}", app.name, app.url).as_str());
    match fetch_url_bytes(app.url.clone(), ONLINE_APP_MAX_BYTES).await {
        Ok(bytes) => {
            if !online_app_sha256_matches(app, bytes.as_slice()) {
                log("dl: SHA-256 mismatch");
            } else {
                match write_blueprint(app, bytes.as_slice()) {
                    Ok(path) => {
                        log(alloc::format!("dl: saved {} bytes -> {}", bytes.len(), path).as_str())
                    }
                    Err(err) => log(alloc::format!("dl: save failed: {}", err).as_str()),
                }
            }
        }
        Err(err) => log(alloc::format!("dl: fetch failed: {}", err).as_str()),
    }
    set_matrix_target_active(&target, false);
}

pub(crate) fn submit_online_to_target(
    spawner: &Spawner,
    target: MatrixTarget,
    width: usize,
    args: Vec<String>,
) -> Result<(), SpawnError> {
    set_matrix_target_active(&target, true);
    match online_run_task(target.clone(), width, args, None) {
        Ok(token) => {
            spawner.spawn(token);
            Ok(())
        }
        Err(err) => {
            set_matrix_target_active(&target, false);
            Err(err)
        }
    }
}

pub(crate) fn submit_online_launch_script_to_target(
    spawner: &Spawner,
    target: MatrixTarget,
    width: usize,
    selector: String,
    launch_script: String,
) -> Result<(), SpawnError> {
    submit_online_args_with_launch_script_to_target(
        spawner,
        target,
        width,
        alloc::vec![selector],
        launch_script,
    )
}

pub(crate) fn submit_online_args_with_launch_script_to_target(
    spawner: &Spawner,
    target: MatrixTarget,
    width: usize,
    args: Vec<String>,
    launch_script: String,
) -> Result<(), SpawnError> {
    set_matrix_target_active(&target, true);
    match online_run_task(target.clone(), width, args, Some(launch_script)) {
        Ok(token) => {
            spawner.spawn(token);
            Ok(())
        }
        Err(err) => {
            set_matrix_target_active(&target, false);
            Err(err)
        }
    }
}

pub(crate) fn submit_online(spawner: &Spawner, io: &'static dyn ShellBackend2, submitted: &str) {
    let target = matrix_target_for_backend(io);
    let width = line_width_for_backend(io);
    let args = submitted.split_whitespace().map(String::from).collect();
    if submit_online_to_target(spawner, target, width, args).is_err() {
        print_shell_line(io, "apps: online task unavailable");
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn submit_download(spawner: &Spawner, io: &'static dyn ShellBackend2, submitted: &str) {
    submit_download_args(spawner, io, submitted.split_whitespace().map(String::from).collect());
}

pub(crate) fn submit_download_args(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: Vec<String>,
) {
    let target = matrix_target_for_backend(io);
    let width = line_width_for_backend(io);
    set_matrix_target_active(&target, true);
    let selector = args.into_iter().next();
    match download_task(target.clone(), width, selector) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "dl: task unavailable");
        }
    }
}
