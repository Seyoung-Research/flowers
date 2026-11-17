use eframe::egui;
use flowers::gui::FlowersApp;
use std::path::PathBuf;

/// The project logo, embedded at compile time.
///
/// A run-time `std::fs` read of `logo.jpeg` only works from the source tree: a
/// packaged app (`.app` / `.msi`) does not ship the repository next to the
/// executable, so the window icon and header mark silently disappeared.
const LOGO_JPEG: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/logo.jpeg"));

/// Build an egui window icon from the embedded logo.
///
/// The image is decoded to straight (unmultiplied) RGBA, converted to the
/// premultiplied representation egui expects, resized to a multiple of four
/// pixels per side (an egui requirement), and returned for the native window.
/// A corrupt or unsupported logo yields `None` rather than aborting startup.
fn logo_icon() -> Option<egui::IconData> {
    let dyn_img = image::load_from_memory(LOGO_JPEG).ok()?;
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
    Some(egui::IconData {
        rgba,
        width: w,
        height: h,
    })
}

fn main() -> eframe::Result {
    env_logger::init();

    // Any paths given on the command line are loaded at startup.
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();

    let mut viewport = egui::ViewportBuilder::default()
        // Fits a 1280×720 logical desktop (e.g. a 1920×1080 laptop at 150 %
        // scaling). The old 1400×900 default was larger than the usable area,
        // so the side panels and the plot footer were clipped on laptops.
        .with_inner_size([1180.0, 720.0])
        .with_min_inner_size([860.0, 560.0])
        .with_app_id("flowers")
        .with_title("flowers — flow cytometry analysis");
    if let Some(icon) = logo_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
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
