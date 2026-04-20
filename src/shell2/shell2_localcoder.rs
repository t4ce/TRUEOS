use alloc::string::String;

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

use super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    set_matrix_target_active,
};

#[derive(Clone)]
pub(crate) enum LocalcoderResumeTarget {
    New,
    ContinueLatest,
    ResumeId(String),
}

#[derive(Clone)]
struct LocalcoderRequest {
    target: MatrixTarget,
    resume_target: LocalcoderResumeTarget,
    prompt: Option<String>,
}

pub(crate) fn submit(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    resume_target: LocalcoderResumeTarget,
    prompt: Option<String>,
) -> bool {
    let target = matrix_target_for_backend(io);
    set_matrix_target_active(&target, true);

    let request = LocalcoderRequest {
        target: target.clone(),
        resume_target,
        prompt,
    };

    match localcoder_command_task(request) {
        Ok(token) => {
            spawner.spawn(token);
            true
        }
        Err(_) => {
            set_matrix_target_active(&target, false);
            false
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn localcoder_command_task(request: LocalcoderRequest) {
    let target = request.target.clone();
    let task_target = target.clone();
    async move {
        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };

        let resume_text = match &request.resume_target {
            LocalcoderResumeTarget::New => "new",
            LocalcoderResumeTarget::ContinueLatest => "continue-latest",
            LocalcoderResumeTarget::ResumeId(_) => "resume-id",
        };

        log("lc: dedicated shell2 localcoder task started");
        log(alloc::format!("lc: mode={}", resume_text).as_str());

        if let LocalcoderResumeTarget::ResumeId(session_id) = &request.resume_target {
            log(alloc::format!("lc: session_id={}", session_id).as_str());
        }

        match request.prompt.as_deref() {
            Some(prompt) if !prompt.trim().is_empty() => {
                log(alloc::format!("lc: prompt={}", prompt).as_str());
            }
            _ => {
                log("lc: interactive launch requested");
            }
        }

        log("lc: this path is separate from classic shell2 AI mode");
        log("lc: vendored localcoder runtime lift is still pending");
        log("lc: next step is a callable kernel-facing entrypoint in vendor/localcoder");
    }
    .await;

    set_matrix_target_active(&target, false);
}
