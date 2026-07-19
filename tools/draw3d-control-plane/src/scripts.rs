use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::client::HOST;

pub const TOOLS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
pub const REPO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

#[derive(Clone, Debug)]
pub struct SceneScript {
    pub file_name: String,
    pub display_name: String,
    pub description: String,
    pub path: PathBuf,
}

fn display_name(file_name: &str) -> String {
    file_name
        .trim_start_matches("draw3d_")
        .trim_end_matches(".py")
        .split('_')
        .map(|word| match word {
            "3d" => "3D".to_owned(),
            "ui" => "UI".to_owned(),
            other => {
                let mut chars = other.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_description(path: &Path) -> String {
    let Ok(source) = fs::read_to_string(path) else {
        return "Draw3D scene script".to_owned();
    };
    let Some(start) = source.find("\"\"\"") else {
        return "Draw3D scene script".to_owned();
    };
    let remainder = &source[start + 3..];
    let Some(end) = remainder.find("\"\"\"") else {
        return "Draw3D scene script".to_owned();
    };
    let description = remainder[..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if description.is_empty() {
        "Draw3D scene script".to_owned()
    } else {
        description
    }
}

pub fn discover_scripts() -> Result<Vec<SceneScript>, String> {
    let entries =
        fs::read_dir(TOOLS_DIR).map_err(|error| format!("could not scan {TOOLS_DIR}: {error}"))?;
    let mut scripts = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not read tools entry: {error}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.starts_with("draw3d_") || !file_name.ends_with(".py") {
            continue;
        }
        scripts.push(SceneScript {
            file_name: file_name.to_owned(),
            display_name: display_name(file_name),
            description: extract_description(&path),
            path,
        });
    }
    scripts.sort_by(|left, right| left.display_name.cmp(&right.display_name));
    Ok(scripts)
}

#[derive(Clone, Copy, Debug)]
pub enum ScriptStream {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub enum ScriptEvent {
    Started {
        name: String,
        pid: u32,
    },
    Line {
        stream: ScriptStream,
        text: String,
    },
    Finished {
        name: String,
        code: Option<i32>,
        canceled: bool,
    },
    SpawnFailed {
        name: String,
        message: String,
    },
}

struct ActiveScript {
    name: String,
    started: Instant,
    cancel: Arc<AtomicBool>,
}

pub struct ScriptRunner {
    sender: Sender<ScriptEvent>,
    receiver: Receiver<ScriptEvent>,
    active: Option<ActiveScript>,
}

impl ScriptRunner {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            active: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.active.is_some()
    }

    pub fn active_name(&self) -> Option<&str> {
        self.active.as_ref().map(|active| active.name.as_str())
    }

    pub fn elapsed(&self) -> Option<Duration> {
        self.active.as_ref().map(|active| active.started.elapsed())
    }

    pub fn start(&mut self, script: SceneScript, extra_arguments: &str) -> Result<(), String> {
        if self.active.is_some() {
            return Err("another scene script is already running".to_owned());
        }
        let extra_arguments = shlex::split(extra_arguments)
            .ok_or_else(|| "extra arguments contain an unterminated quote".to_owned())?;
        let cancel = Arc::new(AtomicBool::new(false));
        self.active = Some(ActiveScript {
            name: script.display_name.clone(),
            started: Instant::now(),
            cancel: Arc::clone(&cancel),
        });

        let events = self.sender.clone();
        thread::Builder::new()
            .name("draw3d-scene-script".to_owned())
            .spawn(move || run_script(script, extra_arguments, cancel, events))
            .map_err(|error| {
                self.active = None;
                format!("could not spawn script worker: {error}")
            })?;
        Ok(())
    }

    pub fn cancel(&self) {
        if let Some(active) = &self.active {
            active.cancel.store(true, Ordering::Release);
        }
    }

    pub fn drain_events(&mut self) -> Vec<ScriptEvent> {
        let events: Vec<_> = self.receiver.try_iter().collect();
        if events.iter().any(|event| {
            matches!(event, ScriptEvent::Finished { .. } | ScriptEvent::SpawnFailed { .. })
        }) {
            self.active = None;
        }
        events
    }
}

fn forward_lines<R: std::io::Read + Send + 'static>(
    reader: R,
    stream: ScriptStream,
    events: Sender<ScriptEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            match line {
                Ok(text) => {
                    let _ = events.send(ScriptEvent::Line { stream, text });
                }
                Err(error) => {
                    let _ = events.send(ScriptEvent::Line {
                        stream: ScriptStream::Stderr,
                        text: format!("could not read script output: {error}"),
                    });
                    break;
                }
            }
        }
    })
}

fn run_script(
    script: SceneScript,
    extra_arguments: Vec<String>,
    cancel: Arc<AtomicBool>,
    events: Sender<ScriptEvent>,
) {
    let mut command = Command::new("python3");
    command
        .arg("-u")
        .arg(&script.path)
        .arg("--host")
        .arg(HOST)
        .args(extra_arguments)
        .current_dir(REPO_ROOT)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = events.send(ScriptEvent::SpawnFailed {
                name: script.display_name,
                message: error.to_string(),
            });
            return;
        }
    };
    let _ = events.send(ScriptEvent::Started {
        name: script.display_name.clone(),
        pid: child.id(),
    });

    let stdout = child
        .stdout
        .take()
        .map(|reader| forward_lines(reader, ScriptStream::Stdout, events.clone()));
    let stderr = child
        .stderr
        .take()
        .map(|reader| forward_lines(reader, ScriptStream::Stderr, events.clone()));

    let mut canceled = false;
    let status = loop {
        if cancel.load(Ordering::Acquire) {
            canceled = true;
            let _ = child.kill();
        }
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => thread::sleep(Duration::from_millis(40)),
            Err(error) => {
                let _ = events.send(ScriptEvent::Line {
                    stream: ScriptStream::Stderr,
                    text: format!("could not wait for script: {error}"),
                });
                break None;
            }
        }
    };

    if let Some(handle) = stdout {
        let _ = handle.join();
    }
    if let Some(handle) = stderr {
        let _ = handle.join();
    }
    let _ = events.send(ScriptEvent::Finished {
        name: script.display_name,
        code: status.and_then(|status| status.code()),
        canceled,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_name_is_human_readable() {
        assert_eq!(display_name("draw3d_grid_world.py"), "Grid World");
        assert_eq!(display_name("draw3d_ui_test.py"), "UI Test");
    }

    #[test]
    fn discovers_house_and_grid_scripts() {
        let scripts = discover_scripts().unwrap();
        assert!(
            scripts
                .iter()
                .any(|script| script.file_name == "draw3d_house_demo.py")
        );
        assert!(
            scripts
                .iter()
                .any(|script| script.file_name == "draw3d_grid_world.py")
        );
    }
}
