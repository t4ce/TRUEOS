//! Native Shell3 controls for a RAM-backed vHDA stream.
use super::{
    MatrixTarget,
    helper::{self, Screen},
    keys::{Event, Input},
    tui::{self, Frontend},
};
use crate::{
    intel::hda::vhda::{Clip, FileStream},
    r::{
        fs::{trueosfs as fs, volatile_files},
        path::FsPath,
    },
};
use alloc::{format, string::String, sync::Arc, vec::Vec};
use trueos_time::{Duration, Timer};

pub(super) fn start(frontend: Frontend) -> Result<(), String> {
    let Some((target, spawner)) = helper::admit("audio", frontend)? else {
        return Ok(());
    };
    let task = player_task(target.clone()).map_err(|_| {
        tui::cancel_native_attach(&target);
        super::MatrixSlots::drop_slot(Some("audio"));
        String::from("Audio player task is busy.")
    })?;
    spawner.spawn(task);
    Ok(())
}

struct Player {
    stream: FileStream,
    path: String,
    frames: u64,
    speed: u32,
    paused: bool,
}

impl Player {
    fn open(clip: Clip, path: String) -> Result<Self, String> {
        let frames = clip.frames();
        Ok(Self {
            stream: FileStream::open(clip)
                .map_err(|error| format!("Playback failed ({error:?})"))?,
            path,
            frames,
            speed: 1000,
            paused: false,
        })
    }

    fn status(&self) -> Result<String, String> {
        let position = self
            .stream
            .position()
            .map_err(|error| format!("{error:?}"))?;
        let state = if position == self.frames {
            "Ended"
        } else if self.paused {
            "Paused"
        } else {
            "Playing"
        };
        Ok(format!(
            "{state} · frame {position}/{} · {}‰",
            self.frames, self.speed
        ))
    }
}

async fn load_wav(path: &str) -> Result<(Clip, String), String> {
    let path = FsPath::parse(path, false)
        .map_err(|error| format!("Invalid path ({error:?})"))?
        .to_relative_string();
    let disk = volatile_files::selected_disk(&path).ok_or("No mounted audio file volume")?;
    let handle = fs::authorized::open_read(fs::authorized::Actor::Public, disk, &path)
        .await
        .map_err(|error| format!("Open failed ({error:?})"))?
        .ok_or("WAV file not found")?;
    let length = usize::try_from(handle.data_len()).map_err(|_| "WAV file is too large")?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| "Insufficient memory for the WAV file")?;
    bytes.resize(length, 0);
    const READ_BYTES: usize = 128 * 1024;
    for (index, chunk) in bytes.chunks_mut(READ_BYTES).enumerate() {
        let offset = (index * READ_BYTES) as u64;
        let read = fs::file_read_handle_range_async(handle, offset, chunk)
            .await
            .map_err(|error| format!("Read failed ({error:?})"))?;
        if read != Some(chunk.len()) {
            return Err("Incomplete WAV read".into());
        }
    }
    let clip =
        Clip::from_wav(Arc::new(bytes)).map_err(|error| format!("WAV failed ({error:?})"))?;
    Ok((clip, path))
}

fn control(player: &mut Option<Player>, action: &str, argument: &str) -> Result<String, String> {
    if action == "stop" && argument.is_empty() {
        *player = None;
        return Ok("Stopped".into());
    }
    if action == "status" && argument.is_empty() {
        return player
            .as_ref()
            .map_or(Ok("No WAV loaded".into()), Player::status);
    }
    let player = player.as_mut().ok_or("Load a WAV with play <path>")?;
    match (action, argument) {
        ("seek", frame) => {
            let frame = frame.parse().map_err(|_| "Use seek <frame>")?;
            player
                .stream
                .seek(frame)
                .map_err(|error| format!("{error:?}"))?;
        }
        ("speed", value) => {
            let speed = value.parse().map_err(|_| "Use speed <permille>")?;
            player
                .stream
                .set_speed(speed)
                .map_err(|error| format!("{error:?}"))?;
            player.speed = speed;
        }
        ("pause" | "resume", "") => {
            let paused = action == "pause";
            player
                .stream
                .set_paused(paused)
                .map_err(|error| format!("{error:?}"))?;
            player.paused = paused;
        }
        _ => return Err("Unknown audio command".into()),
    }
    player.status()
}

fn paint(
    screen: &mut Screen,
    target: &MatrixTarget,
    frontend: Frontend,
    path: &str,
    message: &str,
    input: &Input,
) {
    let mut lines = alloc::vec![
        format!("> {}", input.text()),
        message.into(),
        "play <path>   status   seek <frame>   speed <permille>".into(),
        "pause   resume   stop   Esc return".into(),
    ];
    if !path.is_empty() {
        lines.push(path.into());
    }
    screen.paint(target, &lines, frontend.cols, frontend.rows);
}

#[trueos_executor::task]
async fn player_task(target: MatrixTarget) {
    let mut screen = Screen::default();
    let mut input = Input::default();
    let mut player: Option<Player> = None;
    let mut message = String::from("No WAV loaded");
    let mut last_view: Option<(u64, usize, usize)> = None;
    while let Some((bytes, _)) = tui::native_read(&target) {
        if let Some(frontend) = tui::native_frontend(&target) {
            let view = (frontend.id, frontend.cols, frontend.rows);
            if last_view.map(|previous| previous.0) != Some(frontend.id) {
                input = Input::default();
                screen.invalidate();
            }
            let dirty = last_view != Some(view) || !bytes.is_empty();
            last_view = Some(view);
            let now = crate::chronos::monotonic_nanos();
            let events = input
                .feed(&bytes, now)
                .into_iter()
                .chain(input.timeout(now));
            for event in events {
                let Event::Submit(line) = event else {
                    if event == Event::Quit {
                        tui::native_return(&target);
                        break;
                    }
                    continue;
                };
                let line = line.trim();
                let (action, argument) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
                let argument = argument.trim();
                if action.is_empty() {
                    continue;
                }
                if action == "play" {
                    paint(
                        &mut screen,
                        &target,
                        frontend,
                        argument,
                        "Loading WAV",
                        &input,
                    );
                    let loaded = load_wav(argument).await;
                    if tui::native_frontend(&target).map(|current| current.id) != Some(frontend.id)
                    {
                        break;
                    }
                    message = match loaded.and_then(|(clip, path)| Player::open(clip, path)) {
                        Ok(next) => {
                            player = Some(next);
                            "Playback started".into()
                        }
                        Err(error) => error,
                    };
                } else {
                    message = control(&mut player, action, argument).unwrap_or_else(|error| error);
                }
            }
            if dirty
                && let Some(current) = tui::native_frontend(&target)
                && current.id == frontend.id
            {
                let path = player.as_ref().map_or("", |player| player.path.as_str());
                paint(&mut screen, &target, current, path, &message, &input);
            }
        } else {
            last_view = None;
        }
        Timer::after(Duration::from_millis(20)).await;
    }
}
