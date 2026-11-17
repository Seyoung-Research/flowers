//! Regenerate the packaging icons (`icons/`) from the project logo.
//!
//! Run with:
//!
//! ```sh
//! cargo run --release --example generate_icons
//! ```
//!
//! The generated files are committed, so this only needs to run when the logo
//! artwork changes. `cargo-packager` (`.msi` / `.dmg` / `.app` bundles) needs a
//! `.ico` for Windows and an `.icns` for macOS; both are assembled here from the
//! same source image so every platform gets identical artwork.

use image::{ImageFormat, RgbaImage, imageops};
use std::io::Cursor;
use std::path::Path;

/// Source artwork, embedded so the example works from any directory.
const LOGO_JPEG: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/logo.jpeg"));

/// Icon sizes that Windows asks for (`.ico`), including the 256 px Vista size.
const ICO_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

/// Icon sizes bundled into the macOS `.icns`.
const ICNS_SIZES: [(u32, &[u8; 4]); 7] = [
    (16, b"icp4"),
    (32, b"icp5"),
    (64, b"icp6"),
    (128, b"ic07"),
    (256, b"ic08"),
    (512, b"ic09"),
    (1024, b"ic10"),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("icons");
    std::fs::create_dir_all(&out_dir)?;

    let source = image::load_from_memory(LOGO_JPEG)?.to_rgba8();
    println!("source logo: {}x{}", source.width(), source.height());

    let scaled = |size: u32| -> RgbaImage {
        imageops::resize(&source, size, size, imageops::FilterType::Lanczos3)
    };

    // Plain PNGs: used by the desktop entry on Linux and by docs/tests.
    for size in [32u32, 64, 128, 256, 512, 1024] {
        let path = out_dir.join(match size {
            256 => "128x128@2x.png".to_string(),
            other => format!("{other}x{other}.png"),
        });
        scaled(size).save_with_format(&path, ImageFormat::Png)?;
        println!("wrote {}", path.display());
    }
    scaled(512).save_with_format(out_dir.join("icon.png"), ImageFormat::Png)?;
    println!("wrote {}", out_dir.join("icon.png").display());

    // Windows icon: uncompressed 32-bit DIB entries for maximum compatibility
    // (some WiX / Installer paths reject PNG-compressed ICO entries).
    let mut ico = IcoWriter::default();
    for size in ICO_SIZES {
        ico.push(&scaled(size));
    }
    std::fs::write(out_dir.join("icon.ico"), ico.finish())?;
    println!("wrote {}", out_dir.join("icon.ico").display());

    // macOS icon: PNG payloads in an Apple icon container.
    let mut icns: Vec<u8> = Vec::new();
    for (size, kind) in ICNS_SIZES {
        icns.extend_from_slice(kind);
        let png = png_bytes(&scaled(size))?;
        icns.extend_from_slice(&((png.len() + 8) as u32).to_be_bytes());
        icns.extend_from_slice(&png);
    }
    let mut file: Vec<u8> = Vec::with_capacity(icns.len() + 8);
    file.extend_from_slice(b"icns");
    file.extend_from_slice(&((icns.len() + 8) as u32).to_be_bytes());
    file.extend_from_slice(&icns);
    std::fs::write(out_dir.join("icon.icns"), file)?;
    println!("wrote {}", out_dir.join("icon.icns").display());

    Ok(())
}

fn png_bytes(image: &RgbaImage) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut buf = Vec::new();
    image.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
    Ok(buf)
}

/// Minimal multi-resolution `.ico` writer (BITMAPINFOHEADER entries).
#[derive(Default)]
struct IcoWriter {
    entries: Vec<(u32, Vec<u8>)>,
}

impl IcoWriter {
    fn push(&mut self, image: &RgbaImage) {
        let size = image.width();
        let mask_stride = size.div_ceil(32) * 4;
        let pixel_bytes = (size * size * 4) as usize;
        let mask_bytes = (mask_stride * size) as usize;

        let mut dib = Vec::with_capacity(40 + pixel_bytes + mask_bytes);
        dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
        dib.extend_from_slice(&(size as i32).to_le_bytes()); // biWidth
        dib.extend_from_slice(&((size as i32) * 2).to_le_bytes()); // biHeight (XOR + AND)
        dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
        dib.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
        dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
        dib.extend_from_slice(&(pixel_bytes as u32).to_le_bytes()); // biSizeImage
        for _ in 0..4 {
            dib.extend_from_slice(&0u32.to_le_bytes()); // ppm, colours, important
        }

        // Bottom-up BGRA rows.
        for y in (0..size).rev() {
            for x in 0..size {
                let p = image.get_pixel(x, y).0;
                dib.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
            }
        }
        // AND mask: fully transparent, alpha lives in the BGRA pixels.
        dib.resize(dib.len() + mask_bytes, 0);

        self.entries.push((size, dib));
    }

    fn finish(self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_le_bytes()); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // type: icon
        out.extend_from_slice(&(self.entries.len() as u16).to_le_bytes());

        let mut offset = 6 + 16 * self.entries.len() as u32;
        for (size, dib) in &self.entries {
            let dimension = if *size >= 256 { 0 } else { *size as u8 };
            out.push(dimension); // width
            out.push(dimension); // height
            out.push(0); // palette colours
            out.push(0); // reserved
            out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
            out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
            out.extend_from_slice(&(dib.len() as u32).to_le_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
            offset += dib.len() as u32;
        }
        for (_, dib) in &self.entries {
            out.extend_from_slice(dib);
        }
        out
    }
}
