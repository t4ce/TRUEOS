//! Desktop LINK entry point for the native Shell3 HID bucket tool.
//!
//! Opening LINK admits a normal UI4 Shell3 window. Its retained startup selects
//! the same native terminal helper used by the System command after first paint.

use spin::Mutex;
use trueos_time::{Duration, Timer};

use super::Ui4CursorSource;

const SERVICE_POLL_MS: u64 = 16;
static OPEN_REQUEST: Mutex<Option<LinkOpenRequest>> = Mutex::new(None);

#[derive(Copy, Clone)]
struct LinkOpenRequest {
    source: Ui4CursorSource,
    anchor: (u32, u32),
}

/// Coalesce desktop requests until the service admits the next Shell3 window.
pub(crate) fn request_open(source: Ui4CursorSource, anchor: (u32, u32)) {
    *OPEN_REQUEST.lock() = Some(LinkOpenRequest { source, anchor });
}

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn ui4_link_service_task() {
    crate::log_info!(target: "ui4/link";
        "ui4/link: service online carrier=bsp desktop=normal-shell3-window startup=native-system-link\n"
    );
    loop {
        let request = OPEN_REQUEST.lock().take();
        if let Some(request) = request {
            match crate::r::services::shell3_font_service::prepare().await {
                Ok(()) => {
                    let startup = crate::shell3::startup::Startup::Link {
                        source: request.source,
                        anchor: request.anchor,
                    };
                    match crate::shell3::service::request_shell3_with_startup(Some(startup)) {
                        Ok(slot) => crate::log_info!(target: "ui4/link";
                            "ui4/link: terminal requested executor_slot={} cursor={}:{}:{} anchor={},{} startup=link\n",
                            slot, request.source.controller_id, request.source.slot_id,
                            request.source.ep_target, request.anchor.0, request.anchor.1,
                        ),
                        Err(error) => crate::log_warn!(target: "ui4/link";
                            "ui4/link: terminal request rejected error={:?}\n", error,
                        ),
                    }
                }
                Err(reason) => crate::log_warn!(target: "ui4/link";
                    "ui4/link: terminal request rejected reason={}\n", reason,
                ),
            }
        }
        Timer::after(Duration::from_millis(SERVICE_POLL_MS)).await;
    }
}
