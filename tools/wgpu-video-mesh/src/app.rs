use std::{
    error::Error,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use eframe::{egui, egui_wgpu};

use crate::{
    renderer::{MeshKind, SceneCallback, SceneParameters, SceneRenderResources},
    video::{self, PlaybackStats},
};

pub struct VideoMeshApp {
    mesh: MeshKind,
    playing: bool,
    auto_rotate: bool,
    rotation_speed: f32,
    yaw: f32,
    tilt: f32,
    size: f32,
    camera_distance: f32,
    exposure: f32,
    lighting: f32,
    saturation: f32,
    object_color: [u8; 3],
    background_color: [u8; 3],
    stats: Arc<PlaybackStats>,
    video_path: PathBuf,
    last_update: Instant,
}

impl VideoMeshApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        video_path: PathBuf,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .ok_or_else(|| std::io::Error::other("eframe did not initialize its WGPU renderer"))?;

        let (frames, stats) = video::spawn_decoder(video_path.clone());
        render_state
            .renderer
            .write()
            .callback_resources
            .insert(SceneRenderResources::new(render_state, frames, Arc::clone(&stats)));

        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        Ok(Self {
            mesh: MeshKind::Sphere,
            playing: true,
            auto_rotate: true,
            rotation_speed: 18.0,
            yaw: 0.0,
            tilt: -0.08,
            size: 1.12,
            camera_distance: 3.45,
            exposure: 1.0,
            lighting: 0.32,
            saturation: 1.0,
            object_color: [255, 255, 255],
            background_color: [7, 10, 17],
            stats,
            video_path,
            last_update: Instant::now(),
        })
    }

    fn reset_view(&mut self) {
        self.yaw = 0.0;
        self.tilt = -0.08;
        self.size = 1.12;
        self.camera_distance = 3.45;
    }

    fn choose_video(&mut self, frame: &mut eframe::Frame) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Choose a video texture")
            .add_filter("Video", &["mp4", "mkv", "mov", "webm", "avi", "m4v"]);
        if let Some(directory) = self.video_path.parent() {
            dialog = dialog.set_directory(directory);
        }

        let Some(video_path) = dialog.pick_file() else {
            return;
        };
        let Some(render_state) = frame.wgpu_render_state() else {
            return;
        };

        let (frames, stats) = video::spawn_decoder(video_path.clone());
        let mut renderer = render_state.renderer.write();
        let Some(resources) = renderer
            .callback_resources
            .get_mut::<SceneRenderResources>()
        else {
            return;
        };
        resources.replace_video(&render_state.queue, frames, Arc::clone(&stats));
        self.stats = stats;
        self.video_path = video_path;
        self.playing = true;
    }

    fn controls(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.heading("Video Mesh");
        let source_label = self
            .video_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("video");
        ui.label(egui::RichText::new(source_label).weak())
            .on_hover_text(self.video_path.display().to_string());
        if ui.button("Choose video…").clicked() {
            self.choose_video(frame);
        }
        ui.add_space(10.0);

        ui.label("Geometry");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mesh, MeshKind::Sphere, "Sphere");
            ui.selectable_value(&mut self.mesh, MeshKind::Cube, "Cube");
        });

        ui.add_space(8.0);
        if ui
            .button(if self.playing {
                "Pause video"
            } else {
                "Play video"
            })
            .clicked()
        {
            self.playing = !self.playing;
        }
        ui.checkbox(&mut self.auto_rotate, "Auto rotate");
        ui.add(
            egui::Slider::new(&mut self.rotation_speed, -90.0..=90.0)
                .suffix(" deg/s")
                .text("Spin"),
        );

        ui.separator();
        ui.label("Scene");
        ui.add(egui::Slider::new(&mut self.size, 0.55..=1.55).text("Mesh size"));
        ui.add(egui::Slider::new(&mut self.camera_distance, 2.6..=6.0).text("Camera distance"));
        ui.add(
            egui::Slider::new(&mut self.tilt, -1.2..=1.2)
                .custom_formatter(|value, _| format!("{:.0}°", value.to_degrees()))
                .text("Tilt"),
        );
        if ui.button("Reset view").clicked() {
            self.reset_view();
        }

        ui.separator();
        ui.label("Material");
        ui.add(egui::Slider::new(&mut self.exposure, 0.25..=2.0).text("Exposure"));
        ui.add(egui::Slider::new(&mut self.lighting, 0.0..=1.0).text("3D lighting"));
        ui.add(egui::Slider::new(&mut self.saturation, 0.0..=1.5).text("Saturation"));

        ui.separator();
        ui.label("Colors");
        ui.horizontal(|ui| {
            ui.label("Object tint");
            ui.color_edit_button_srgb(&mut self.object_color);
        });
        ui.horizontal(|ui| {
            ui.label("Background");
            ui.color_edit_button_srgb(&mut self.background_color);
        });

        ui.separator();
        if let Some(error) = self.stats.error() {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        } else if self.stats.uploaded_frames() == 0 {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Starting FFmpeg…");
            });
        } else {
            ui.colored_label(egui::Color32::LIGHT_GREEN, "Looping video");
            ui.label(format!(
                "Decoded {} • uploaded {}",
                self.stats.decoded_frames(),
                self.stats.uploaded_frames()
            ));
        }
        ui.label(format!(
            "GPU texture: {}×{} @ {} fps",
            video::VIDEO_WIDTH,
            video::VIDEO_HEIGHT,
            video::VIDEO_FPS
        ));

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.label(egui::RichText::new("Drag the mesh to orbit • wheel to zoom").weak());
        });
    }

    fn scene(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size().max(egui::vec2(1.0, 1.0));
        let (rect, response) = ui.allocate_exact_size(available, egui::Sense::drag());

        let drag = response.drag_motion();
        self.yaw += drag.x * 0.008;
        self.tilt = (self.tilt + drag.y * 0.008).clamp(-1.45, 1.45);

        if response.hovered() {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            self.camera_distance = (self.camera_distance - scroll * 0.004).clamp(2.6, 6.0);
        }

        ui.painter().rect_filled(
            rect,
            0.0,
            egui::Color32::from_rgb(
                self.background_color[0],
                self.background_color[1],
                self.background_color[2],
            ),
        );
        let tint = egui::Rgba::from_srgba_unmultiplied(
            self.object_color[0],
            self.object_color[1],
            self.object_color[2],
            255,
        )
        .to_array();
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            rect,
            SceneCallback {
                mesh: self.mesh,
                parameters: SceneParameters {
                    yaw: self.yaw,
                    tilt: self.tilt,
                    size: self.size,
                    camera_distance: self.camera_distance,
                    exposure: self.exposure,
                    lighting: self.lighting,
                    saturation: self.saturation,
                    object_tint: [tint[0], tint[1], tint[2]],
                    aspect_ratio: rect.width() / rect.height().max(1.0),
                },
                playing: self.playing,
            },
        ));
    }
}

impl eframe::App for VideoMeshApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f32().min(0.1);
        self.last_update = now;
        if self.auto_rotate {
            self.yaw += self.rotation_speed.to_radians() * elapsed;
        }

        if ctx.input(|input| input.key_pressed(egui::Key::Space))
            && !ctx.egui_wants_keyboard_input()
        {
            self.playing = !self.playing;
        }

        if self.playing || self.auto_rotate {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::Panel::left("controls")
            .resizable(false)
            .exact_size(280.0)
            .show(ui, |ui| self.controls(ui, frame));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.scene(ui));
    }
}
