use alloc::string::String;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use localcoder::kernel::{self, BasicPromptRequest};
use localcoder::resume::ResumeTarget as LocalcoderKernelResumeTarget;

use super::{
    MatrixTarget, ShellBackend2, localcoder_service, localcoder_ui2_window,
    matrix_target_for_backend, print_matrix_target_line, set_matrix_target_active,
};

static LOCALCODER_USAGE_HINT_SHOWN: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
struct LocalcoderRequest {
    target: MatrixTarget,
    prompt: Option<String>,
}

pub(crate) fn submit(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    prompt: Option<String>,
) -> bool {
    localcoder_service::ensure_registered(spawner);
    localcoder_ui2_window::ensure_registered();

    let target = matrix_target_for_backend(io);
    set_matrix_target_active(&target, true);

    let request = LocalcoderRequest {
        target: target.clone(),
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

        match request.prompt.as_deref() {
            Some(prompt) if !prompt.trim().is_empty() => {
                let kernel_request = BasicPromptRequest {
                    session_scope: Some(alloc::format!(
                        "matrix-slot-{}",
                        request.target.slot_id.as_str()
                    )),
                    resume_target: LocalcoderKernelResumeTarget::ContinueLatest,
                    prompt: String::from(prompt),
                    max_tokens: 1024,
                };
                match kernel::run_basic_prompt(&kernel_request).await {
                    Ok(response) => {
                        log(alloc::format!(
                            "lc: session {} ({})",
                            response.session_id,
                            response.session_action
                        )
                        .as_str());
                        if response.text.trim().is_empty() {
                            log("lc: empty response");
                        } else {
                            for line in response.text.lines() {
                                log(line);
                            }
                        }
                    }
                    Err(err) => {
                        log(alloc::format!("lc: {}", err).as_str());
                    }
                }
            }
            _ => {
                let first_hint = LOCALCODER_USAGE_HINT_SHOWN
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok();
                if first_hint {
                    log("lc: session mode is ready; use `lc <prompt...>`");
                } else {
                    log("lc: use `lc <prompt...>`");
                }
            }
        }
    }
    .await;

    set_matrix_target_active(&target, false);
}
