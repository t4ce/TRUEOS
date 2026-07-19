mod app;
mod client;
mod scripts;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("TRUEOS Draw3D Control Plane")
            .with_inner_size([1360.0, 900.0])
            .with_min_inner_size([980.0, 680.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "TRUEOS Draw3D Control Plane",
        options,
        Box::new(|creation_context| Ok(Box::new(app::ControlPlaneApp::new(creation_context)))),
    )
}
