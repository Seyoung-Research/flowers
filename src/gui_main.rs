use eframe::egui;
use flowers::gui::FlowersApp;
use std::path::PathBuf;

/// Build an egui window icon from the project `logo.jpeg`.
///
/// The image is decoded to straight (unmultiplied) RGBA, converted to the
/// premultiplied representation egui expects, resized to a multiple of four
/// pixels per side (an egui requirement), and returned for the native window.
fn logo_icon() -> egui::IconData {
    let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/logo.jpeg"));
    let dyn_img = image::open(path).expect("failed to open logo.jpeg");
    let img = if !dyn_img.width().is_multiple_of(4) || !dyn_img.height().is_multiple_of(4) {
        dyn_img
            .resize(512, 512, image::imageops::FilterType::Lanczos3)
            .to_rgba8()
    } else {
        dyn_img.to_rgba8()
    };
    let (w, h) = img.dimensions();
    let raw: &[u8] = img.as_raw();
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for px in raw.as_chunks::<4>().0 {
        let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
        rgba.push(((r as u16 * a as u16) / 255) as u8);
        rgba.push(((g as u16 * a as u16) / 255) as u8);
        rgba.push(((b as u16 * a as u16) / 255) as u8);
        rgba.push(a);
    }
    egui::IconData {
        rgba,
        width: w,
        height: h,
    }
}

fn main() -> eframe::Result {
    env_logger::init();

    // Any paths given on the command line are loaded at startup.
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("flowers — flow cytometry analysis")
            .with_icon(logo_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "flowers-gui",
        options,
        Box::new(move |cc| {
            // Register an egui image loader that can decode `bytes://` images so
            // the window icon / header logo actually render.
            cc.egui_ctx
                .add_image_loader(std::sync::Arc::new(flowers::gui::FlowersImageLoader));
            let mut app = FlowersApp::default();
            app.open_paths(&paths);
            Ok(Box::new(app))
        }),
    )
}
