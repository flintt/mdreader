mod app;
mod document;
mod tree;

pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        centered: true,
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_app_id("com.mdreader.desktop")
            .with_title("MD Reader")
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([680.0, 480.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "MD Reader",
        options,
        Box::new(|context| Ok(Box::new(app::NativeApp::new(context)))),
    )
}
