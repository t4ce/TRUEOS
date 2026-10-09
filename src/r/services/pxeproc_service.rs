//! Cold disk boots run the ordinary OS UI in a regular, visible kernel shell.

use alloc::string::String;
use trueos_time::{Duration, Timer};

pub(crate) const LAUNCH_SCRIPT: &str = "key down\nkey down\nkey enter\nkey down\nkey enter\n";

pub(crate) async fn autostart() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_V4_CONFIGURED).await;
    loop {
        let startup = crate::shell3::startup::Startup {
            launch_script: String::from(LAUNCH_SCRIPT),
        };
        match crate::shell3::service::request_shell3_with_startup(Some(startup)) {
            Ok(_) => return,
            Err(crate::shell3::Shell3Error::NoExecutor) => {
                Timer::after(Duration::from_millis(100)).await
            }
            Err(error) => {
                crate::log_warn!(target: "service"; "pxeproc: Shell3 launch failed error={error:?}\n");
                return;
            }
        }
    }
}
