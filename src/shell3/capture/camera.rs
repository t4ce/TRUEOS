//! First HDMI-to-filesystem marker for the frozen-tile FPGA capture ABI.
use super::super::{
    MatrixSlots, MatrixTarget,
    helper::{self, Action, Input, Screen},
    tui::{self, Frontend},
};
use alloc::{format, string::String, sync::Arc};
use spin::Mutex;
use trueos_time::{Duration, Timer};

type ResultCell = Arc<Mutex<Option<Result<String, String>>>>;

pub(super) fn start(frontend: Frontend) -> Result<(), String> {
    let Some((target, spawner)) = helper::admit("cam", frontend)? else {
        return Ok(());
    };
    let token = match menu_task(target.clone()) {
        Ok(token) => token,
        Err(_) => {
            tui::cancel_native_attach(&target);
            MatrixSlots::drop_slot(Some("cam"));
            return Err("cam: helper task pool is full".into());
        }
    };
    spawner.spawn(token);
    Ok(())
}

fn receiver_status() -> String {
    match crate::tga::camera::status() {
        Ok(s) => format!(
            "HDMI: HPD={} PLL={} tile={} input={}x{} frames={} EDID={}",
            s.flags & 1 != 0,
            s.flags & 2 != 0,
            s.flags & 4 != 0,
            s.width,
            s.height,
            s.frames,
            s.edid_reads,
        ),
        Err(error) => format!("HDMI: {error}"),
    }
}

async fn save_tile() -> Result<String, String> {
    let disk = crate::ui4::writable_capture_root_handle()
        .ok_or("No writable TRUEOSFS root is mounted.")?;
    let pixels = crate::tga::camera::read_tile()
        .await
        .map_err(String::from)?;
    let png = crate::graphics::encoder::png::encode_rgba8_png(64, 64, &pixels, 256)
        .map_err(|error| format!("PNG encode failed: {error:?}"))?;
    let wall = crate::chronos::best_effort_unix_time_seconds().unwrap_or(0);
    let path = format!("camera/hdmi-tile-{wall}-{}.png", crate::chronos::monotonic_nanos());
    match crate::r::fs::trueosfs::file_in_typed_async(disk, &path, &png, infer::ContentTypeId::PNG)
        .await
    {
        Ok(true) => {
            crate::log_important!(
                "cam: captured path={} bytes={} source=hdmi-rx tile=64x64\n",
                path,
                png.len()
            );
            Ok(format!("Saved: trueosfs:/{path}"))
        }
        result => Err(format!("Camera save failed: {result:?}")),
    }
}

#[trueos_executor::task(pool_size = 1)]
async fn save_task(target: MatrixTarget, result: ResultCell) {
    let _work = helper::Work::new(&target);
    let saved = save_tile().await;
    let message = match &saved {
        Ok(path) => path.clone(),
        Err(error) => super::error_result(error),
    };
    super::print_line(&target, &message);
    *result.lock() = Some(saved);
    super::super::service::notify_work();
}

#[trueos_executor::task(pool_size = 1)]
async fn menu_task(target: MatrixTarget) {
    let mut input = Input::default();
    let mut screen = Screen::default();
    let mut selected = 0usize;
    let mut pending: Option<ResultCell> = None;
    let mut result = String::new();
    while let Some((bytes, _)) = tui::native_read(&target) {
        let now = crate::chronos::monotonic_nanos();
        let Some(surface) = tui::surface(&target) else {
            break;
        };
        if let Some(cell) = &pending {
            let completed = cell.lock().take();
            if let Some(saved) = completed {
                result = saved.unwrap_or_else(|error| super::error_result(&error));
                pending = None;
            }
        }
        let first = if surface.rows >= 10 { 4 } else { 2 };
        let mut actions = input.feed(&bytes, now);
        if let Some(action) = input.timeout(now) {
            actions.push(action);
        }
        for action in actions {
            match action {
                Action::Up | Action::Down => selected ^= 1,
                Action::Quit => {
                    tui::native_return(&target);
                    break;
                }
                Action::Click(1) => {
                    tui::native_return(&target);
                    break;
                }
                Action::Click(row) if row == first || row == first + 1 => {
                    selected = row - first;
                }
                Action::Choose => {}
                _ => continue,
            }
            if !matches!(action, Action::Choose | Action::Click(_)) {
                continue;
            }
            if selected == 1 {
                tui::native_return(&target);
                break;
            }
            if pending.is_some() {
                continue;
            }
            let cell = Arc::new(Mutex::new(None));
            let admitted = crate::workers::pick_background_spawner()
                .ok_or("No background worker is ready.")
                .and_then(|spawner| {
                    let token = save_task(target.clone(), cell.clone())
                        .map_err(|_| "Camera capture already running.")?;
                    spawner.spawn(token);
                    Ok(())
                });
            match admitted {
                Ok(()) => pending = Some(cell),
                Err(error) => result = super::error_result(error),
            }
        }
        let mut lines = alloc::vec![String::new(); surface.rows as usize];
        if lines.len() >= 5 {
            lines[0] = "CAM  HDMI capture card".into();
            lines[1] = "↑/↓ j/k  Enter choose  Esc/q Return".into();
            if first == 4 {
                lines[2] = receiver_status();
                lines[3] =
                    "First frozen 64×64 tile → PNG; video recording is not available yet.".into();
            }
            lines[first] =
                format!("  {} Save HDMI tile to filesystem", if selected == 0 { "›" } else { " " });
            lines[first + 1] = format!("  {} Return", if selected == 1 { "›" } else { " " });
            let last = lines.len() - 1;
            lines[last] = if pending.is_some() {
                format!("{} Reading HDMI tile and saving…", helper::spinner(now))
            } else {
                result.clone()
            };
        }
        screen.paint(&target, &lines, surface.cols as usize, surface.rows as usize);
        Timer::after(Duration::from_millis(100)).await;
    }
}
