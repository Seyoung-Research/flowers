#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{Builder, WebviewUrl, WebviewWindowBuilder};

fn main() {
    let _ = Builder::default().setup(|app| {
        WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
            .title("flowers — flow cytometry analysis")
            .inner_size(1400.0, 900.0)
            .build()?;
        Ok(())
    }).run(tauri::generate_context!());
}