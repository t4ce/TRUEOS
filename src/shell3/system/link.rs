//! Native System LINK: live HID combos presented as mouse-driven buckets.
use super::{
    MatrixTarget, helper,
    tui::{self, Frontend},
};
use crate::usb3::hid::hut::{self, ComboMoveError, HidSourceKind};
use alloc::string::String;
use trueos_time::{Duration, Timer};

#[path = "link_model.rs"]
mod model;
use model::{Command, Input, Screen, Snapshot, View};

pub(super) fn start(frontend: Frontend) -> Result<(), String> {
    let Some((target, spawner)) = helper::admit("link", frontend)? else {
        return Ok(());
    };
    let task = menu_task(target.clone()).map_err(|_| {
        tui::cancel_native_attach(&target);
        String::from("Link menu task pool is full.")
    })?;
    spawner.spawn(task);
    Ok(())
}

fn snapshot() -> Snapshot {
    Snapshot {
        devices: hut::devices_snapshot().into_iter().collect(),
        buckets: hut::combos_snapshot().into_iter().collect(),
    }
}

fn execute(command: Command, view: &mut View, target: &MatrixTarget) {
    match command {
        Command::Return => {
            tui::native_write(target, b"\x1b[?1000l\x1b[?1002l\x1b[?1006l\x1b[?25h\x1b[0m");
            tui::native_return(target);
        }
        Command::NewBucket => {
            view.status = match hut::request_combo(HidSourceKind::Human, "link", None) {
                Some(_) => {
                    String::from("Bucket created. Add a keyboard, mouse or tablet, and gamepad.")
                }
                None => {
                    String::from("Bucket limit reached (32). Remove an empty bucket to make room.")
                }
            };
        }
        Command::RemoveBucket(id) => {
            view.status = if hut::remove_empty_combo(id) {
                String::from("Empty bucket removed.")
            } else {
                String::from("Bucket changed; only empty buckets can be removed.")
            };
        }
        Command::MoveDevice(device, id) => {
            view.status = match hut::move_device_to_combo(device, id) {
                Ok(()) => String::from("Device linked. Each bucket has at most one active cursor."),
                Err(ComboMoveError::CursorOccupied) => {
                    String::from("Bucket already has a cursor. Move it to another bucket first.")
                }
                Err(ComboMoveError::DeviceKindOccupied) => String::from(
                    "Bucket already has this device type. Move it to another bucket first.",
                ),
                Err(ComboMoveError::DeviceMissing) => {
                    String::from("Device disconnected. Drop cancelled.")
                }
                Err(ComboMoveError::ComboMissing) => {
                    String::from("Bucket disappeared. Drop cancelled.")
                }
            };
        }
    }
}

#[trueos_executor::task(pool_size = 2)]
async fn menu_task(target: MatrixTarget) {
    let mut input = Input::default();
    let mut view = View::default();
    let mut screen = Screen::default();
    let mut visible = false;
    while let Some((bytes, notices)) = tui::native_read(&target) {
        if !tui::native_visible(&target) {
            view.cancel_gesture();
            input = Input::default();
            screen = Screen::default();
            visible = false;
            Timer::after(Duration::from_millis(20)).await;
            continue;
        }
        let Some(frontend) = tui::native_frontend(&target) else {
            break;
        };
        if !visible {
            view.cancel_gesture();
            input = Input::default();
            visible = true;
        }
        for notice in notices {
            view.status = notice;
        }
        let now = crate::chronos::monotonic_nanos();
        let mut events = input.feed(&bytes, now);
        if let Some(event) = input.timeout(now) {
            events.push(event);
        }
        let mut data = snapshot();
        let mut frame = view.frame(&data, frontend.cols, frontend.rows);
        for event in events {
            if let Some(command) = view.event(event, &frame) {
                execute(command, &mut view, &target);
                if command == Command::Return {
                    visible = false;
                    input = Input::default();
                    screen = Screen::default();
                }
                data = snapshot();
            }
            frame = view.frame(&data, frontend.cols, frontend.rows);
            if !tui::native_visible(&target) {
                break;
            }
        }
        if tui::native_visible(&target) {
            let bytes = screen.diff(&frame, frontend.cols, frontend.rows);
            if !bytes.is_empty() {
                tui::native_write(&target, bytes.as_bytes());
            }
        }
        Timer::after(Duration::from_millis(20)).await;
    }
}
