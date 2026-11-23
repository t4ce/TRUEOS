//! Desktop entry points for the native Shell3 HID and audio routing tools.
//!
//! A retained startup selects the native terminal after its first paint.

use spin::Mutex;
use trueos_time::{Duration, Timer};

use super::Ui4CursorSource;

const SERVICE_POLL_MS: u64 = 16;
static OPEN_REQUESTS: Mutex<LinkOpenRequests> = Mutex::new(LinkOpenRequests {
    hid: None,
    audio: None,
});

#[derive(Default)]
struct LinkOpenRequests {
    hid: Option<LinkOpenRequest>,
    audio: Option<LinkOpenRequest>,
}

#[derive(Copy, Clone)]
struct LinkOpenRequest {
    source: Ui4CursorSource,
    anchor: (u32, u32),
}

/// Coalesce repeated requests for the same tool until the service admits it.
pub(crate) fn request_open(source: Ui4CursorSource, anchor: (u32, u32)) {
    OPEN_REQUESTS.lock().hid = Some(LinkOpenRequest { source, anchor });
}

pub(crate) fn request_audio_open(source: Ui4CursorSource, anchor: (u32, u32)) {
    OPEN_REQUESTS.lock().audio = Some(LinkOpenRequest { source, anchor });
}

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn ui4_link_service_task() {
    crate::log_info!(target: "ui4/link";
        "ui4/link: service online carrier=bsp desktop=normal-shell3-window tools=hid,audio\n"
    );
    loop {
        let requests = core::mem::take(&mut *OPEN_REQUESTS.lock());
        for (request, audio) in [(requests.hid, false), (requests.audio, true)] {
            let Some(LinkOpenRequest { source, anchor }) = request else {
                continue;
            };
            let startup = if audio {
                crate::shell3::startup::Startup::AudioLink { source, anchor }
            } else {
                crate::shell3::startup::Startup::Link { source, anchor }
            };
            match crate::r::services::shell3_font_service::prepare().await {
                Ok(()) => {
                    match crate::shell3::service::request_shell3_with_startup(Some(startup)) {
                        Ok(slot) => crate::log_info!(target: "ui4/link";
                            "ui4/link: terminal requested executor_slot={} cursor={}:{}:{} anchor={},{} tool={}\n",
                            slot, source.controller_id, source.slot_id,
                            source.ep_target, anchor.0, anchor.1, if audio { "audio" } else { "hid" },
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
