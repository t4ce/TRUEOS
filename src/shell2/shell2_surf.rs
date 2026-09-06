//! Shell2 only launches the tab. Solara owns all document fetch/navigation in
//! its existing VMX context once that tab is running.
use super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line, submit_online_launch_script_to_target,
};
use alloc::string::String;
use trueos_executor::Spawner;

const SOLARA_APP: &str = "solara";
const SOLARA_ARCHIVE: &str = "solara.bp";

fn launch_script(input: &str) -> Result<String, &'static str> {
    let input = input.trim();
    let input = if input.len() >= 2
        && ((input.starts_with('"') && input.ends_with('"'))
            || (input.starts_with('\'') && input.ends_with('\'')))
    {
        input[1..input.len() - 1].trim()
    } else {
        input
    };
    if input.is_empty() {
        return Ok(String::from("home\n"));
    }
    if input.len() > 8192 || input.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("expected one URL (maximum 8192 bytes)");
    }
    let web_scheme = input
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("http://"))
        || input
            .get(..8)
            .is_some_and(|s| s.eq_ignore_ascii_case("https://"));
    if input.contains("://") && !web_scheme {
        return Err("only http and https URLs are supported");
    }
    let url = if web_scheme {
        String::from(input)
    } else {
        alloc::format!("https://{input}")
    };
    Ok(alloc::format!("open {url}\n"))
}

#[trueos_executor::task(pool_size = 4)]
async fn launch_solara_task(target: MatrixTarget, script: String) {
    match super::cmds::run::submit_archive_name_to_target_from_app_db_with_launch_script_async(
        target.clone(),
        SOLARA_ARCHIVE,
        script.clone(),
    )
    .await
    {
        Ok(_) => {}
        Err(error) if error == "archive not found" => {
            let spawner = unsafe { Spawner::for_current_executor().await };
            if submit_online_launch_script_to_target(&spawner, target.clone(), SOLARA_APP, &script)
                .is_err()
            {
                print_matrix_target_system_line(&target, "surf: Solara launch task unavailable");
            }
        }
        Err(error) => print_matrix_target_system_line(
            &target,
            alloc::format!("surf: could not launch {SOLARA_ARCHIVE}: {error}").as_str(),
        ),
    }
}

pub(crate) fn prepare_call_with_url(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    input: &str,
) {
    let script = match launch_script(input) {
        Ok(script) => script,
        Err(error) => {
            print_shell_line(io, alloc::format!("surf: {error}").as_str());
            return;
        }
    };
    match launch_solara_task(matrix_target_for_backend(io), script) {
        Ok(token) => {
            spawner.spawn(token);
        }
        Err(_) => print_shell_line(io, "surf: Solara launch busy"),
    }
}

#[cfg(test)]
mod tests {
    use super::launch_script;
    #[test]
    fn home_and_url_launch_without_fetch_or_staged_files() {
        assert_eq!(launch_script("").unwrap(), "home\n");
        assert_eq!(launch_script("example.com/a?q=b").unwrap(), "open https://example.com/a?q=b\n");
        assert_eq!(
            launch_script("\"http://localhost:8080/\"").unwrap(),
            "open http://localhost:8080/\n"
        );
        assert!(launch_script("file:///tmp/a").is_err());
        assert!(launch_script("https://example.com/\nsource forged").is_err());
    }
}
