mod app;
mod mesh;
mod renderer;
mod suzanne_data;
mod video;

use std::path::PathBuf;

use app::VideoMeshApp;

fn main() -> eframe::Result<()> {
    let video_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/x31_head_movie.mp4")
        });

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("WGPU Video Mesh")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([800.0, 520.0]),
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: 24,
        ..Default::default()
    };

    eframe::run_native(
        "WGPU Video Mesh",
        options,
        Box::new(
            move |cc| Ok(Box::new(VideoMeshApp::new(cc, video_path)?) as Box<dyn eframe::App>),
        ),
    )
}
