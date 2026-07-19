use std::collections::VecDeque;
use std::f32::consts::PI;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, ComboBox, Context, DragValue, Frame, Key, Layout, RichText, ScrollArea,
    Stroke, Ui, Vec2,
};
use trueos_draw3d::{CameraOrbit, Command, Rgba8, SceneStats, Vec3, ViewCamera};

use crate::client::{
    ClientCommand, ClientEvent, ConnectionState, ENDPOINT, NetworkHandle, ping_request,
    stats_request,
};
use crate::scripts::{SceneScript, ScriptEvent, ScriptRunner, ScriptStream, discover_scripts};

const ACCENT: Color32 = Color32::from_rgb(78, 156, 255);
const SUCCESS: Color32 = Color32::from_rgb(85, 214, 148);
const WARNING: Color32 = Color32::from_rgb(255, 184, 92);
const DANGER: Color32 = Color32::from_rgb(244, 105, 119);
const PANEL: Color32 = Color32::from_rgb(18, 24, 35);
const CARD: Color32 = Color32::from_rgb(24, 32, 45);
const MUTED: Color32 = Color32::from_rgb(142, 157, 178);

#[derive(Clone, Copy, Debug)]
enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
    Script,
}

struct LogEntry {
    elapsed: Duration,
    level: LogLevel,
    source: &'static str,
    message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackgroundMode {
    Transparent,
    Solid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CameraMode {
    LookAt,
    Orbit,
    Fly,
}

struct CameraSettings {
    mode: CameraMode,
    position: [f32; 3],
    target: [f32; 3],
    fov_degrees: f32,
    near_plane: f32,
    far_plane: f32,
    orbit_look_at: [f32; 3],
    orbit_radii: [f32; 2],
    orbit_rotation_degrees: [f32; 3],
    orbit_speed_degrees: f32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            mode: CameraMode::LookAt,
            position: [8.0, 5.5, 13.0],
            target: [0.0, 2.0, 0.0],
            fov_degrees: 50.0,
            near_plane: 0.1,
            far_plane: 1_000.0,
            orbit_look_at: [0.0, 2.0, 0.0],
            orbit_radii: [14.0, 9.0],
            orbit_rotation_degrees: [-8.0, 0.0, 3.0],
            orbit_speed_degrees: 8.0,
        }
    }
}

struct FlySettings {
    armed: bool,
    position: [f32; 3],
    yaw_degrees: f32,
    pitch_degrees: f32,
    movement_speed: f32,
    turn_speed: f32,
}

impl Default for FlySettings {
    fn default() -> Self {
        Self {
            armed: false,
            position: [8.0, 5.5, 13.0],
            yaw_degrees: -31.6,
            pitch_degrees: -12.9,
            movement_speed: 8.0,
            turn_speed: 72.0,
        }
    }
}

pub struct ControlPlaneApp {
    network: NetworkHandle,
    connection: ConnectionState,
    scripts: Vec<SceneScript>,
    selected_script: usize,
    script_arguments: String,
    script_runner: ScriptRunner,
    stats: Option<SceneStats>,
    auto_refresh_stats: bool,
    last_stats_request: Instant,
    background_mode: BackgroundMode,
    background_color: Color32,
    camera: CameraSettings,
    fly: FlySettings,
    last_fly_tick: Instant,
    last_fly_send: Instant,
    logs: VecDeque<LogEntry>,
    started: Instant,
}

impl ControlPlaneApp {
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        configure_style(&creation_context.egui_ctx);
        let started = Instant::now();
        let network = NetworkHandle::spawn();
        network.send(ClientCommand::Connect);

        let mut app = Self {
            network,
            connection: ConnectionState::Connecting,
            scripts: Vec::new(),
            selected_script: 0,
            script_arguments: String::new(),
            script_runner: ScriptRunner::new(),
            stats: None,
            auto_refresh_stats: true,
            last_stats_request: Instant::now(),
            background_mode: BackgroundMode::Transparent,
            background_color: Color32::from_rgb(255, 255, 255),
            camera: CameraSettings::default(),
            fly: FlySettings::default(),
            last_fly_tick: Instant::now(),
            last_fly_send: Instant::now() - Duration::from_secs(1),
            logs: VecDeque::new(),
            started,
        };
        app.refresh_scripts();
        if let Some(index) = app
            .scripts
            .iter()
            .position(|script| script.file_name == "draw3d_house_demo.py")
        {
            app.selected_script = index;
        }
        app.log(LogLevel::Info, "APP", format!("target fixed at {ENDPOINT}"));
        app
    }

    fn log(&mut self, level: LogLevel, source: &'static str, message: impl Into<String>) {
        if self.logs.len() >= 600 {
            self.logs.pop_front();
        }
        self.logs.push_back(LogEntry {
            elapsed: self.started.elapsed(),
            level,
            source,
            message: message.into(),
        });
    }

    fn refresh_scripts(&mut self) {
        let previous = self
            .scripts
            .get(self.selected_script)
            .map(|script| script.file_name.clone());
        match discover_scripts() {
            Ok(scripts) => {
                self.scripts = scripts;
                self.selected_script = previous
                    .and_then(|name| {
                        self.scripts
                            .iter()
                            .position(|script| script.file_name == name)
                    })
                    .unwrap_or(0)
                    .min(self.scripts.len().saturating_sub(1));
            }
            Err(error) => self.log(LogLevel::Error, "APP", error),
        }
    }

    fn send(&self, label: &'static str, command: Command) {
        self.network.send(ClientCommand::Request { label, command });
    }

    fn handle_network_events(&mut self) {
        for event in self.network.drain_events() {
            match event {
                ClientEvent::Connection(state) => {
                    match &state {
                        ConnectionState::Connecting => {
                            self.log(LogLevel::Info, "TCP", format!("connecting to {ENDPOINT}"));
                        }
                        ConnectionState::Connected { round_trip } => {
                            self.log(
                                LogLevel::Success,
                                "TCP",
                                format!(
                                    "connected; protocol round trip {:.1} ms",
                                    round_trip.as_secs_f64() * 1_000.0
                                ),
                            );
                            self.network.send(stats_request());
                        }
                        ConnectionState::Disconnected { reason } => {
                            let message = reason
                                .as_deref()
                                .unwrap_or("disconnected by operator")
                                .to_owned();
                            self.log(LogLevel::Warning, "TCP", message);
                        }
                    }
                    self.connection = state;
                }
                ClientEvent::Applied {
                    label,
                    affected,
                    stats,
                } => {
                    self.stats = Some(stats);
                    if label != "fly camera tick" {
                        self.log(
                            LogLevel::Success,
                            "API",
                            format!("{label} applied · affected {affected}"),
                        );
                    }
                }
                ClientEvent::Stats(stats) => self.stats = Some(stats),
                ClientEvent::Pong(elapsed) => self.log(
                    LogLevel::Success,
                    "TCP",
                    format!("pong in {:.1} ms", elapsed.as_secs_f64() * 1_000.0),
                ),
                ClientEvent::Error { label, message } => {
                    self.log(LogLevel::Error, "API", format!("{label}: {message}"));
                }
            }
        }
    }

    fn handle_script_events(&mut self) {
        for event in self.script_runner.drain_events() {
            match event {
                ScriptEvent::Started { name, pid } => {
                    self.log(LogLevel::Success, "PY", format!("started {name} · pid {pid}"))
                }
                ScriptEvent::Line { stream, text } if !text.is_empty() => {
                    let level = match stream {
                        ScriptStream::Stdout => LogLevel::Script,
                        ScriptStream::Stderr => LogLevel::Error,
                    };
                    self.log(level, "PY", text);
                }
                ScriptEvent::Line { .. } => {}
                ScriptEvent::Finished {
                    name,
                    code,
                    canceled,
                } => {
                    let (level, outcome) = if canceled {
                        (LogLevel::Warning, "canceled".to_owned())
                    } else if code == Some(0) {
                        (LogLevel::Success, "completed".to_owned())
                    } else {
                        (LogLevel::Error, format!("exited with {code:?}"))
                    };
                    self.log(level, "PY", format!("{name} {outcome}"));
                    if self.connection.is_connected() {
                        self.network.send(stats_request());
                    }
                }
                ScriptEvent::SpawnFailed { name, message } => {
                    self.log(LogLevel::Error, "PY", format!("could not start {name}: {message}"))
                }
            }
        }
    }

    fn poll_stats(&mut self) {
        if self.connection.is_connected()
            && self.auto_refresh_stats
            && !self.script_runner.is_running()
            && self.last_stats_request.elapsed() >= Duration::from_secs(2)
        {
            self.network.send(stats_request());
            self.last_stats_request = Instant::now();
        }
    }

    fn fly_command(&self) -> Command {
        let yaw = self.fly.yaw_degrees.to_radians();
        let pitch = self.fly.pitch_degrees.to_radians();
        let (sin_yaw, cos_yaw) = yaw.sin_cos();
        let (sin_pitch, cos_pitch) = pitch.sin_cos();
        let direction = Vec3::new(sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch);
        Command::SetViewCamera {
            camera: ViewCamera {
                position: array_vec3(self.fly.position),
                view_direction: direction,
                up_axis: Vec3::new(0.0, 1.0, 0.0),
                near_plane: self.camera.near_plane,
                far_plane: self.camera.far_plane,
                vertical_fov: self.camera.fov_degrees.to_radians(),
            },
            orbit: None,
        }
    }

    fn handle_flycam(&mut self, context: &Context) {
        let now = Instant::now();
        let delta = now
            .duration_since(self.last_fly_tick)
            .as_secs_f32()
            .min(0.1);
        self.last_fly_tick = now;
        if !self.fly.armed || !self.connection.is_connected() {
            return;
        }
        context.request_repaint_after(Duration::from_millis(16));
        if context.egui_wants_keyboard_input() {
            return;
        }

        let input = context.input(|input| {
            (
                input.key_down(Key::W),
                input.key_down(Key::S),
                input.key_down(Key::A),
                input.key_down(Key::D),
                input.key_down(Key::Q),
                input.key_down(Key::E),
                input.key_down(Key::ArrowLeft),
                input.key_down(Key::ArrowRight),
                input.key_down(Key::ArrowUp),
                input.key_down(Key::ArrowDown),
                input.modifiers.shift,
            )
        });
        let (
            forward,
            back,
            left,
            right,
            down,
            up,
            turn_left,
            turn_right,
            turn_up,
            turn_down,
            boost,
        ) = input;
        let mut changed = false;
        let turn = self.fly.turn_speed * delta;
        if turn_left {
            self.fly.yaw_degrees -= turn;
            changed = true;
        }
        if turn_right {
            self.fly.yaw_degrees += turn;
            changed = true;
        }
        if turn_up {
            self.fly.pitch_degrees += turn;
            changed = true;
        }
        if turn_down {
            self.fly.pitch_degrees -= turn;
            changed = true;
        }
        self.fly.pitch_degrees = self.fly.pitch_degrees.clamp(-89.0, 89.0);

        let yaw = self.fly.yaw_degrees.to_radians();
        let pitch = self.fly.pitch_degrees.to_radians();
        let (sin_yaw, cos_yaw) = yaw.sin_cos();
        let (sin_pitch, cos_pitch) = pitch.sin_cos();
        let forward_axis = [sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch];
        let right_axis = [cos_yaw, 0.0, sin_yaw];
        let speed = self.fly.movement_speed * if boost { 4.0 } else { 1.0 } * delta;
        let forward_input = i32::from(forward) as f32 - i32::from(back) as f32;
        let right_input = i32::from(right) as f32 - i32::from(left) as f32;
        let up_input = i32::from(up) as f32 - i32::from(down) as f32;
        if forward_input != 0.0 || right_input != 0.0 || up_input != 0.0 {
            for axis in 0..3 {
                self.fly.position[axis] +=
                    (forward_axis[axis] * forward_input + right_axis[axis] * right_input) * speed;
            }
            self.fly.position[1] += up_input * speed;
            changed = true;
        }

        if changed && self.last_fly_send.elapsed() >= Duration::from_millis(70) {
            self.send("fly camera tick", self.fly_command());
            self.last_fly_send = Instant::now();
        }
    }

    fn top_bar(&mut self, root: &mut Ui) {
        egui::Panel::top("top_bar")
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(18, 11)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("TRUEOS").size(13.0).strong().color(ACCENT));
                    ui.label(RichText::new("DRAW3D CONTROL PLANE").size(18.0).strong());
                    ui.separator();
                    ui.label(RichText::new(ENDPOINT).monospace().color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        match &self.connection {
                            ConnectionState::Connected { round_trip } => {
                                if ui.button("Disconnect").clicked() {
                                    self.network.send(ClientCommand::Disconnect);
                                }
                                ui.label(
                                    RichText::new(format!(
                                        "● CONNECTED  {:.1} ms",
                                        round_trip.as_secs_f64() * 1_000.0
                                    ))
                                    .strong()
                                    .color(SUCCESS),
                                );
                            }
                            ConnectionState::Connecting => {
                                ui.spinner();
                                ui.label(RichText::new("CONNECTING").strong().color(WARNING));
                            }
                            ConnectionState::Disconnected { .. } => {
                                if ui
                                    .add(egui::Button::new("Reconnect").fill(ACCENT))
                                    .clicked()
                                {
                                    self.network.send(ClientCommand::Connect);
                                }
                                ui.label(RichText::new("● OFFLINE").strong().color(DANGER));
                            }
                        }
                    });
                });
            });
    }

    fn scene_scripts_panel(&mut self, root: &mut Ui) {
        egui::Panel::left("scene_scripts")
            .default_size(310.0)
            .min_size(280.0)
            .max_size(390.0)
            .resizable(true)
            .frame(Frame::new().fill(PANEL).inner_margin(egui::Margin::same(16)))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Scene launcher");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("↻ Refresh").clicked() {
                            self.refresh_scripts();
                        }
                    });
                });
                ui.label(
                    RichText::new(format!("{} Python scenes discovered", self.scripts.len()))
                        .small()
                        .color(MUTED),
                );
                ui.add_space(10.0);

                let selected_text = self
                    .scripts
                    .get(self.selected_script)
                    .map(|script| script.display_name.as_str())
                    .unwrap_or("No scripts found");
                ComboBox::from_id_salt("scene_script_selector")
                    .selected_text(selected_text)
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for (index, script) in self.scripts.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.selected_script,
                                index,
                                &script.display_name,
                            );
                        }
                    });

                if let Some(script) = self.scripts.get(self.selected_script) {
                    ui.add_space(10.0);
                    ui.label(RichText::new(&script.description).color(MUTED));
                    ui.add_space(5.0);
                    ui.label(
                        RichText::new(&script.file_name)
                            .small()
                            .monospace()
                            .color(Color32::from_rgb(102, 126, 154)),
                    )
                    .on_hover_text(script.path.display().to_string());
                }

                ui.add_space(14.0);
                ui.label(RichText::new("Extra arguments").small().strong());
                ui.add(
                    egui::TextEdit::singleline(&mut self.script_arguments)
                        .hint_text("e.g. --orbit-speed 0.2")
                        .desired_width(f32::INFINITY),
                );
                ui.label(
                    RichText::new("Arguments are passed directly to the selected script.")
                        .small()
                        .color(MUTED),
                );
                ui.add_space(12.0);

                if self.script_runner.is_running() {
                    let name = self.script_runner.active_name().unwrap_or("scene");
                    let elapsed = self.script_runner.elapsed().unwrap_or_default().as_secs_f32();
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(format!("{name} · {elapsed:.1}s")).strong());
                    });
                    if ui
                        .add_sized(
                            [ui.available_width(), 36.0],
                            egui::Button::new("Cancel script").fill(DANGER),
                        )
                        .clicked()
                    {
                        self.script_runner.cancel();
                    }
                } else {
                    let run = ui.add_enabled(
                        !self.scripts.is_empty(),
                        egui::Button::new(RichText::new("▶  Load selected scene").strong())
                            .fill(ACCENT)
                            .min_size(Vec2::new(ui.available_width(), 38.0)),
                    );
                    if run.clicked()
                        && let Some(script) = self.scripts.get(self.selected_script).cloned()
                    {
                        match self
                            .script_runner
                            .start(script.clone(), &self.script_arguments)
                        {
                            Ok(()) => self.log(
                                LogLevel::Info,
                                "PY",
                                format!("launching {}", script.display_name),
                            ),
                            Err(error) => self.log(LogLevel::Error, "PY", error),
                        }
                    }
                }

                ui.add_space(18.0);
                ui.separator();
                ui.add_space(12.0);
                ui.label(RichText::new("Launcher notes").small().strong().color(MUTED));
                ui.label(
                    RichText::new(
                        "Scripts run with unbuffered Python output and the fixed host. Their stdout and stderr appear in the console below.",
                    )
                    .small()
                    .color(MUTED),
                );
            });
    }

    fn lifecycle_card(&mut self, ui: &mut Ui) {
        card(ui, |ui| {
            ui.heading("Scene lifecycle");
            ui.label(
                RichText::new("Direct protocol controls")
                    .small()
                    .color(MUTED),
            );
            ui.add_space(9.0);
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.background_mode,
                    BackgroundMode::Transparent,
                    "Transparent",
                );
                ui.selectable_value(
                    &mut self.background_mode,
                    BackgroundMode::Solid,
                    "Solid clear",
                );
                if self.background_mode == BackgroundMode::Solid {
                    ui.color_edit_button_srgba(&mut self.background_color);
                }
            });
            ui.add_space(8.0);
            let connected = self.connection.is_connected();
            ui.add_enabled_ui(connected, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add(egui::Button::new("▶ Start / resume").fill(ACCENT))
                        .clicked()
                    {
                        let clear = match self.background_mode {
                            BackgroundMode::Transparent => None,
                            BackgroundMode::Solid => {
                                let [r, g, b, a] = self.background_color.to_array();
                                Some(Rgba8::new(r, g, b, a))
                            }
                        };
                        self.send("start scene", Command::StartScene { clear });
                    }
                    if ui.button("Ⅱ Pause").clicked() {
                        self.send("pause scene", Command::StopScene { permanent: false });
                    }
                    if ui
                        .add(egui::Button::new("■ Stop + discard").fill(DANGER))
                        .on_hover_text("Stops the scene and permanently releases resident meshes")
                        .clicked()
                    {
                        self.send("discard scene", Command::StopScene { permanent: true });
                    }
                    if ui
                        .add(egui::Button::new("Clear geometry").fill(WARNING))
                        .clicked()
                    {
                        self.send("clear scene", Command::Clear);
                    }
                });
            });
        });
    }

    fn camera_card(&mut self, ui: &mut Ui) {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Camera control");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.selectable_value(&mut self.camera.mode, CameraMode::Fly, "Fly");
                    ui.selectable_value(&mut self.camera.mode, CameraMode::Orbit, "Orbit");
                    ui.selectable_value(&mut self.camera.mode, CameraMode::LookAt, "Look-at");
                });
            });
            ui.add_space(8.0);

            match self.camera.mode {
                CameraMode::LookAt => {
                    vector_editor(ui, "Position", &mut self.camera.position, 0.1);
                    vector_editor(ui, "Target", &mut self.camera.target, 0.1);
                    ui.label(
                        RichText::new("A static Y-up look-at camera.")
                            .small()
                            .color(MUTED),
                    );
                }
                CameraMode::Orbit => {
                    vector_editor(ui, "Look at", &mut self.camera.orbit_look_at, 0.1);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Radii").strong().color(MUTED));
                        axis_drag(ui, "X", &mut self.camera.orbit_radii[0], 0.1);
                        axis_drag(ui, "Z", &mut self.camera.orbit_radii[1], 0.1);
                    });
                    vector_editor(ui, "Rotation °", &mut self.camera.orbit_rotation_degrees, 0.25);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Speed").strong().color(MUTED));
                        ui.add(
                            DragValue::new(&mut self.camera.orbit_speed_degrees)
                                .speed(0.25)
                                .suffix(" °/s"),
                        );
                    });
                }
                CameraMode::Fly => {
                    vector_editor(ui, "Position", &mut self.fly.position, 0.1);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Heading").strong().color(MUTED));
                        ui.add(
                            DragValue::new(&mut self.fly.yaw_degrees)
                                .speed(0.25)
                                .suffix("° yaw"),
                        );
                        ui.add(
                            DragValue::new(&mut self.fly.pitch_degrees)
                                .speed(0.25)
                                .range(-89.0..=89.0)
                                .suffix("° pitch"),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Motion").strong().color(MUTED));
                        ui.add(
                            DragValue::new(&mut self.fly.movement_speed)
                                .speed(0.25)
                                .range(0.1..=100.0)
                                .suffix(" units/s"),
                        );
                        ui.add(
                            DragValue::new(&mut self.fly.turn_speed)
                                .speed(1.0)
                                .range(1.0..=360.0)
                                .suffix(" °/s turn"),
                        );
                    });
                    ui.checkbox(&mut self.fly.armed, "Arm keyboard flycam");
                    ui.label(
                        RichText::new(
                            "WASD move · Q/E descend/ascend · arrows look · Shift boosts 4×",
                        )
                        .small()
                        .color(if self.fly.armed {
                            SUCCESS
                        } else {
                            MUTED
                        }),
                    );
                }
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Lens").strong().color(MUTED));
                ui.add(
                    DragValue::new(&mut self.camera.fov_degrees)
                        .range(5.0..=170.0)
                        .suffix("° FOV"),
                );
                ui.add(
                    DragValue::new(&mut self.camera.near_plane)
                        .speed(0.01)
                        .range(0.001..=100.0)
                        .prefix("near "),
                );
                ui.add(
                    DragValue::new(&mut self.camera.far_plane)
                        .speed(1.0)
                        .range(1.0..=100_000.0)
                        .prefix("far "),
                );
            });
            ui.add_space(8.0);
            let apply = ui.add_enabled(
                self.connection.is_connected(),
                egui::Button::new(RichText::new("Apply camera").strong())
                    .fill(ACCENT)
                    .min_size(Vec2::new(150.0, 34.0)),
            );
            if apply.clicked() {
                self.send("set camera", self.camera_command());
            }
        });
    }

    fn camera_command(&self) -> Command {
        if self.camera.mode == CameraMode::Fly {
            return self.fly_command();
        }
        let (position, target, orbit) = match self.camera.mode {
            CameraMode::LookAt => (self.camera.position, self.camera.target, None),
            CameraMode::Orbit => {
                let look_at = self.camera.orbit_look_at;
                let position = [
                    look_at[0] + self.camera.orbit_radii[0],
                    look_at[1],
                    look_at[2],
                ];
                let orbit = CameraOrbit::new(
                    array_vec3(look_at),
                    array_vec3(self.camera.orbit_rotation_degrees.map(f32::to_radians)),
                    self.camera.orbit_radii,
                    self.camera.orbit_speed_degrees.to_radians(),
                );
                (position, look_at, Some(orbit))
            }
            CameraMode::Fly => unreachable!(),
        };
        let position = array_vec3(position);
        let target = array_vec3(target);
        Command::SetViewCamera {
            camera: ViewCamera {
                position,
                view_direction: target - position,
                up_axis: Vec3::new(0.0, 1.0, 0.0),
                near_plane: self.camera.near_plane,
                far_plane: self.camera.far_plane,
                vertical_fov: self.camera.fov_degrees * PI / 180.0,
            },
            orbit,
        }
    }

    fn telemetry_card(&mut self, ui: &mut Ui) {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Live scene telemetry");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add_enabled(self.connection.is_connected(), egui::Button::new("Ping"))
                        .clicked()
                    {
                        self.network.send(ping_request());
                    }
                    if ui
                        .add_enabled(self.connection.is_connected(), egui::Button::new("Refresh"))
                        .clicked()
                    {
                        self.network.send(stats_request());
                        self.last_stats_request = Instant::now();
                    }
                    ui.checkbox(&mut self.auto_refresh_stats, "Auto");
                });
            });
            ui.add_space(10.0);
            if let Some(stats) = self.stats {
                ui.columns(3, |columns| {
                    stat_tile(&mut columns[0], "MESHES", stats.mesh_count.to_string());
                    stat_tile(&mut columns[1], "INSTANCES", stats.instance_count.to_string());
                    stat_tile(&mut columns[2], "VERTICES", grouped(stats.vertex_count));
                });
                ui.add_space(8.0);
                ui.columns(3, |columns| {
                    stat_tile(&mut columns[0], "EDGES", grouped(stats.edge_count));
                    stat_tile(&mut columns[1], "FACES", grouped(stats.face_count));
                    stat_tile(&mut columns[2], "MESH MEMORY", human_bytes(stats.mesh_bytes));
                });
            } else {
                ui.label(RichText::new("No telemetry received yet.").color(MUTED));
            }
        });
    }

    fn console_panel(&mut self, root: &mut Ui) {
        egui::Panel::bottom("console")
            .default_size(215.0)
            .min_size(110.0)
            .max_size(420.0)
            .resizable(true)
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(12, 17, 26))
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("EVENT CONSOLE").small().strong().color(MUTED));
                    ui.label(
                        RichText::new(format!("{} entries", self.logs.len()))
                            .small()
                            .color(Color32::from_rgb(91, 108, 130)),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("Clear").clicked() {
                            self.logs.clear();
                        }
                    });
                });
                ui.separator();
                ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for entry in &self.logs {
                            let color = match entry.level {
                                LogLevel::Info => MUTED,
                                LogLevel::Success => SUCCESS,
                                LogLevel::Warning => WARNING,
                                LogLevel::Error => DANGER,
                                LogLevel::Script => Color32::from_rgb(187, 205, 229),
                            };
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{:>7.2}s", entry.elapsed.as_secs_f32()))
                                        .monospace()
                                        .small()
                                        .color(Color32::from_rgb(81, 96, 117)),
                                );
                                ui.label(
                                    RichText::new(format!("{:<4}", entry.source))
                                        .monospace()
                                        .small()
                                        .strong()
                                        .color(color),
                                );
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&entry.message)
                                            .monospace()
                                            .small()
                                            .color(color),
                                    )
                                    .selectable(true),
                                );
                            });
                        }
                    });
            });
    }

    fn central_panel(&mut self, root: &mut Ui) {
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(14, 20, 30))
                    .inner_margin(egui::Margin::same(18)),
            )
            .show(root, |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    self.telemetry_card(ui);
                    ui.add_space(12.0);
                    self.lifecycle_card(ui);
                    ui.add_space(12.0);
                    self.camera_card(ui);
                    ui.add_space(16.0);
                });
            });
    }
}

impl eframe::App for ControlPlaneApp {
    fn logic(&mut self, context: &Context, _frame: &mut eframe::Frame) {
        self.handle_network_events();
        self.handle_script_events();
        self.poll_stats();
        self.handle_flycam(context);

        if self.script_runner.is_running()
            || matches!(self.connection, ConnectionState::Connecting)
            || self.auto_refresh_stats
        {
            context.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.top_bar(ui);
        self.console_panel(ui);
        self.scene_scripts_panel(ui);
        self.central_panel(ui);
    }
}

fn configure_style(context: &Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.faint_bg_color = CARD;
    visuals.extreme_bg_color = Color32::from_rgb(10, 15, 23);
    visuals.selection.bg_fill = ACCENT;
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(32, 43, 59);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 61, 82);
    visuals.widgets.active.bg_fill = Color32::from_rgb(57, 114, 184);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(42, 54, 72));
    context.set_visuals(visuals);

    context.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 7.0);
        style.spacing.button_padding = Vec2::new(11.0, 7.0);
    });
}

fn card(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, Color32::from_rgb(39, 51, 68)))
        .corner_radius(10)
        .inner_margin(egui::Margin::same(16))
        .show(ui, content);
}

fn vector_editor(ui: &mut Ui, label: &str, value: &mut [f32; 3], speed: f64) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).strong().color(MUTED));
        axis_drag(ui, "X", &mut value[0], speed);
        axis_drag(ui, "Y", &mut value[1], speed);
        axis_drag(ui, "Z", &mut value[2], speed);
    });
}

fn axis_drag(ui: &mut Ui, axis: &str, value: &mut f32, speed: f64) {
    ui.add(
        DragValue::new(value)
            .speed(speed)
            .prefix(format!("{axis} "))
            .max_decimals(3),
    );
}

fn stat_tile(ui: &mut Ui, label: &str, value: String) {
    Frame::new()
        .fill(Color32::from_rgb(19, 27, 39))
        .corner_radius(7)
        .inner_margin(egui::Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(10.0).strong().color(MUTED));
            ui.label(RichText::new(value).size(19.0).strong().monospace());
        });
}

fn array_vec3(value: [f32; 3]) -> Vec3 {
    Vec3::new(value[0], value[1], value[2])
}

fn grouped(value: u64) -> String {
    let text = value.to_string();
    let mut result = String::with_capacity(text.len() + text.len() / 3);
    for (index, character) in text.chars().enumerate() {
        if index != 0 && (text.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(character);
    }
    result
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_large_values_for_telemetry() {
        assert_eq!(grouped(46_624), "46,624");
        assert_eq!(human_bytes(1_048_576), "1.0 MiB");
    }

    #[test]
    fn default_camera_is_valid_and_finite() {
        let settings = CameraSettings::default();
        assert!(settings.fov_degrees > 0.0 && settings.fov_degrees < 180.0);
        assert!(settings.near_plane > 0.0);
        assert!(settings.far_plane > settings.near_plane);
    }
}
