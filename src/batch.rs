//! Batch report export (Phase 8, L9).
//!
//! Paginated reports are rendered to PDF, PowerPoint (PPTX), HTML/web pages,
//! print-ready HTML and animated SVG. PDF and PPTX pages are rasterised with
//! [`crate::raster::rasterize_svg`]; HTML and animation output keep the vector
//! page SVGs.

use std::io::Write;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{ColorType, DynamicImage, ImageEncoder, RgbaImage};

use crate::layout::{FjmlContext, LayoutOptions};
use crate::raster::rasterize_svg;
use crate::report::{IterationReport, ReportDocument};

/// Batch export formats (L9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchExportFormat {
    /// Multi-page PDF with one rasterised page per report page.
    Pdf,
    /// PowerPoint deck with one picture slide per report page.
    PowerPoint,
    /// Standalone web page with inline SVG pages.
    Html,
    /// Print-ready HTML with page breaks and print CSS.
    Printer,
    /// Animated SVG cycling through the report pages.
    Animation,
}

impl BatchExportFormat {
    /// Canonical file extension.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::PowerPoint => "pptx",
            Self::Html | Self::Printer => "html",
            Self::Animation => "svg",
        }
    }

    /// MIME type of the exported document.
    pub fn mime(self) -> &'static str {
        match self {
            Self::Pdf => "application/pdf",
            Self::PowerPoint => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            Self::Html | Self::Printer => "text/html",
            Self::Animation => "image/svg+xml",
        }
    }

    /// Human-readable name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF",
            Self::PowerPoint => "PowerPoint",
            Self::Html => "HTML",
            Self::Printer => "Printer",
            Self::Animation => "Animation",
        }
    }
}

/// Options shared by all batch export formats (L9).
#[derive(Debug, Clone, PartialEq)]
pub struct BatchExportOptions {
    /// Document title; empty falls back to the report title.
    pub title: String,
    /// Rasterisation scale for PDF / PowerPoint output.
    pub scale: f32,
    /// Per-frame delay for animation output.
    pub frame_delay_ms: u32,
}

impl Default for BatchExportOptions {
    fn default() -> Self {
        Self {
            title: String::new(),
            scale: 1.0,
            frame_delay_ms: 800,
        }
    }
}

/// The bytes and metadata produced by a batch export (L9).
#[derive(Debug, Clone, PartialEq)]
pub struct BatchExport {
    pub format: BatchExportFormat,
    pub bytes: Vec<u8>,
    pub extension: &'static str,
    pub mime: &'static str,
}

impl BatchExport {
    /// Write the exported document to `path`.
    pub fn write_to(&self, path: &Path) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        file.write_all(&self.bytes)
    }
}

/// Export a paginated report to the requested format (L9).
pub fn export_report(
    report: &ReportDocument,
    ctx: &FjmlContext,
    layout: &LayoutOptions,
    format: BatchExportFormat,
    options: &BatchExportOptions,
) -> Result<BatchExport, String> {
    let title = if options.title.is_empty() {
        report.title.clone()
    } else {
        options.title.clone()
    };
    let pages = report.render_paginated_svg(ctx, layout);
    if pages.is_empty() {
        return Err("report has no pages".to_string());
    }
    let width = report.size.width;
    let height = report.size.height;

    let bytes = match format {
        BatchExportFormat::Pdf => pdf_bytes(&pages, width, height, options.scale)?,
        BatchExportFormat::PowerPoint => {
            let images = rasterize_pages(&pages, width, height, options.scale)?;
            pptx_bytes(&images, width, height)?
        }
        BatchExportFormat::Html => html_document(&pages, &title, false).into_bytes(),
        BatchExportFormat::Printer => html_document(&pages, &title, true).into_bytes(),
        BatchExportFormat::Animation => {
            animated_svg(&pages, width, height, options.frame_delay_ms).into_bytes()
        }
    };

    Ok(BatchExport {
        format,
        bytes,
        extension: format.extension(),
        mime: format.mime(),
    })
}

/// Export an iteration/batch report after flattening it into pages (L9).
pub fn export_iteration_report(
    report: &IterationReport,
    layout: &LayoutOptions,
    format: BatchExportFormat,
    options: &BatchExportOptions,
) -> Result<BatchExport, String> {
    let document = report.to_document();
    export_report(&document, &FjmlContext::default(), layout, format, options)
}

fn rasterize_pages(
    pages: &[String],
    width: f32,
    height: f32,
    scale: f32,
) -> Result<Vec<RgbaImage>, String> {
    let width = width.round().max(1.0) as u32;
    let height = height.round().max(1.0) as u32;
    pages
        .iter()
        .map(|svg| rasterize_svg(svg, width, height, scale))
        .collect()
}

fn jpeg_bytes(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let rgb = DynamicImage::ImageRgba8(image.clone()).to_rgb8();
    let (width, height) = rgb.dimensions();
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 90)
        .encode(rgb.as_raw(), width, height, ColorType::Rgb8)
        .map_err(|error| format!("JPEG encoding failed: {error}"))?;
    Ok(bytes)
}

fn png_bytes(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let (width, height) = image.dimensions();
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(image.as_raw(), width, height, ColorType::Rgba8)
        .map_err(|error| format!("PNG encoding failed: {error}"))?;
    Ok(bytes)
}

/// Build a minimal valid multi-page PDF with one JPEG image per page.
fn pdf_bytes(pages: &[String], width: f32, height: f32, scale: f32) -> Result<Vec<u8>, String> {
    let images = rasterize_pages(pages, width, height, scale)?;
    if images.is_empty() {
        return Err("report has no pages".to_string());
    }
    let jpegs: Vec<Vec<u8>> = images
        .iter()
        .map(jpeg_bytes)
        .collect::<Result<Vec<_>, _>>()?;
    let pixel_sizes: Vec<(u32, u32)> = images.iter().map(|image| image.dimensions()).collect();

    let page_count = jpegs.len();
    let object_count = 2 + page_count * 3;
    let mut offsets = vec![0usize; object_count + 1];
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    fn write_object(out: &mut Vec<u8>, offsets: &mut [usize], id: usize, body: &[u8]) {
        offsets[id] = out.len();
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }

    let kids = (0..page_count)
        .map(|index| format!("{} 0 R", 3 + index * 3))
        .collect::<Vec<_>>()
        .join(" ");
    write_object(
        &mut out,
        &mut offsets,
        1,
        b"<< /Type /Catalog /Pages 2 0 R >>",
    );
    write_object(
        &mut out,
        &mut offsets,
        2,
        format!("<< /Type /Pages /Kids [{kids}] /Count {page_count} >>").as_bytes(),
    );

    for (index, jpeg) in jpegs.iter().enumerate() {
        let page_id = 3 + index * 3;
        let content_id = page_id + 1;
        let image_id = page_id + 2;
        let (pixel_width, pixel_height) = pixel_sizes[index];
        let page = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width:.2} {height:.2}] \
             /Resources << /XObject << /Im0 {image_id} 0 R >> >> /Contents {content_id} 0 R >>"
        );
        write_object(&mut out, &mut offsets, page_id, page.as_bytes());

        let content = format!("q\n{width:.2} 0 0 {height:.2} 0 0 cm\n/Im0 Do\nQ\n",);
        let content_body = format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            content
        );
        write_object(&mut out, &mut offsets, content_id, content_body.as_bytes());

        let mut image_body = format!(
            "<< /Type /XObject /Subtype /Image /Width {pixel_width} /Height {pixel_height} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            jpeg.len()
        )
        .into_bytes();
        image_body.extend_from_slice(jpeg);
        image_body.extend_from_slice(b"\nendstream");
        write_object(&mut out, &mut offsets, image_id, &image_body);
    }

    let xref_offset = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", object_count + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for &offset in offsets.iter().skip(1).take(object_count) {
        out.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            object_count + 1
        )
        .as_bytes(),
    );
    Ok(out)
}

/// Build a minimal PPTX deck with one full-slide picture per report page.
fn pptx_bytes(images: &[RgbaImage], width: f32, height: f32) -> Result<Vec<u8>, String> {
    if images.is_empty() {
        return Err("report has no pages".to_string());
    }
    let count = images.len();
    let emu_width = (width * 9525.0).round().max(1.0) as i64;
    let emu_height = (height * 9525.0).round().max(1.0) as i64;
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();

    let slide_overrides = (1..=count)
        .map(|index| {
            format!(
                "<Override PartName=\"/ppt/slides/slide{index}.xml\" \
                 ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
            )
        })
        .collect::<String>();
    let content_types = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Default Extension=\"png\" ContentType=\"image/png\"/>\
         <Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>\
         <Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>\
         <Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>\
         <Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>\
         {slide_overrides}</Types>"
    );
    entries.push((
        "[Content_Types].xml".to_string(),
        content_types.into_bytes(),
    ));
    entries.push((
        "_rels/.rels".to_string(),
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"ppt/presentation.xml\"/>\
         </Relationships>"
            .as_bytes()
            .to_vec(),
    ));

    let slide_ids = (0..count)
        .map(|index| {
            format!(
                "<p:sldId id=\"{}\" r:id=\"rId{}\"/>",
                256 + index,
                index + 2
            )
        })
        .collect::<String>();
    let presentation = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <p:presentation xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
         xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
         <p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>\
         <p:sldIdLst>{slide_ids}</p:sldIdLst>\
         <p:sldSz cx=\"{emu_width}\" cy=\"{emu_height}\"/>\
         <p:notesSz cx=\"6858000\" cy=\"9144000\"/>\
         </p:presentation>"
    );
    entries.push((
        "ppt/presentation.xml".to_string(),
        presentation.into_bytes(),
    ));

    let slide_rels = (1..=count)
        .map(|index| {
            format!(
                "<Relationship Id=\"rId{}\" \
                 Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" \
                 Target=\"slides/slide{index}.xml\"/>",
                index + 1
            )
        })
        .collect::<String>();
    let presentation_rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rId1\" \
         Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" \
         Target=\"slideMasters/slideMaster1.xml\"/>\
         {slide_rels}</Relationships>"
    );
    entries.push((
        "ppt/_rels/presentation.xml.rels".to_string(),
        presentation_rels.into_bytes(),
    ));

    let empty_tree = "<p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
         <p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/>\
         <a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr></p:spTree>";
    let slide_master = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <p:sldMaster xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
         xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
         <p:cSld>{empty_tree}</p:cSld>\
         <p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" \
         accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" \
         accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>\
         <p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst>\
         </p:sldMaster>"
    );
    entries.push((
        "ppt/slideMasters/slideMaster1.xml".to_string(),
        slide_master.into_bytes(),
    ));
    entries.push((
        "ppt/slideMasters/_rels/slideMaster1.xml.rels".to_string(),
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rId1\" \
         Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" \
         Target=\"../slideLayouts/slideLayout1.xml\"/>\
         <Relationship Id=\"rId2\" \
         Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" \
         Target=\"../theme/theme1.xml\"/>\
         </Relationships>"
            .as_bytes()
            .to_vec(),
    ));

    let slide_layout = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <p:sldLayout xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
         xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" \
         type=\"blank\" preserve=\"1\">\
         <p:cSld name=\"Blank\">{empty_tree}</p:cSld>\
         <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"
    );
    entries.push((
        "ppt/slideLayouts/slideLayout1.xml".to_string(),
        slide_layout.into_bytes(),
    ));
    entries.push((
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels".to_string(),
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rId1\" \
         Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" \
         Target=\"../slideMasters/slideMaster1.xml\"/>\
         </Relationships>"
            .as_bytes()
            .to_vec(),
    ));
    entries.push((
        "ppt/theme/theme1.xml".to_string(),
        minimal_theme().into_bytes(),
    ));

    for (index, image) in images.iter().enumerate() {
        let number = index + 1;
        let slide = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
             <p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
             xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
             xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
             <p:cSld><p:spTree>\
             <p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
             <p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/>\
             <a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>\
             <p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"Page {number}\"/>\
             <p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>\
             <p:blipFill><a:blip r:embed=\"rId1\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>\
             <p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{emu_width}\" cy=\"{emu_height}\"/></a:xfrm>\
             <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>\
             </p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"
        );
        entries.push((format!("ppt/slides/slide{number}.xml"), slide.into_bytes()));
        entries.push((
            format!("ppt/slides/_rels/slide{number}.xml.rels"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
                 <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
                 <Relationship Id=\"rId1\" \
                 Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" \
                 Target=\"../media/image{number}.png\"/>\
                 </Relationships>"
            )
            .into_bytes(),
        ));
        entries.push((format!("ppt/media/image{number}.png"), png_bytes(image)?));
    }

    Ok(zip_store(&entries))
}

fn minimal_theme() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
     <a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"Office Theme\">\
     <a:themeElements>\
     <a:clrScheme name=\"Office\">\
     <a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1>\
     <a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1>\
     <a:dk2><a:srgbClr val=\"1F497D\"/></a:dk2><a:lt2><a:srgbClr val=\"EEECE1\"/></a:lt2>\
     <a:accent1><a:srgbClr val=\"4F81BD\"/></a:accent1>\
     <a:accent2><a:srgbClr val=\"C0504D\"/></a:accent2>\
     <a:accent3><a:srgbClr val=\"9BBB59\"/></a:accent3>\
     <a:accent4><a:srgbClr val=\"8064A2\"/></a:accent4>\
     <a:accent5><a:srgbClr val=\"4BACC6\"/></a:accent5>\
     <a:accent6><a:srgbClr val=\"F79646\"/></a:accent6>\
     <a:hlink><a:srgbClr val=\"0000FF\"/></a:hlink>\
     <a:folHlink><a:srgbClr val=\"800080\"/></a:folHlink>\
     </a:clrScheme>\
     <a:fontScheme name=\"Office\">\
     <a:majorFont><a:latin typeface=\"Calibri\"/></a:majorFont>\
     <a:minorFont><a:latin typeface=\"Calibri\"/></a:minorFont>\
     </a:fontScheme>\
     <a:fmtScheme name=\"Office\">\
     <a:fillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:fillStyleLst>\
     <a:lnStyleLst><a:ln w=\"9525\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln></a:lnStyleLst>\
     <a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>\
     <a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:bgFillStyleLst>\
     </a:fmtScheme></a:themeElements></a:theme>"
        .to_string()
}

/// A minimal store-only ZIP writer (no compression) for PPTX packages.
fn zip_store(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central: Vec<(String, u32, u32, u32)> = Vec::new();

    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let name_bytes = name.as_bytes();
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0x0800u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0x0021u16.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(data);
        central.push((name.clone(), crc, data.len() as u32, offset));
    }

    let central_offset = out.len() as u32;
    for (name, crc, size, offset) in &central {
        let name_bytes = name.as_bytes();
        out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0x0800u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0x0021u16.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(name_bytes);
    }
    let central_size = out.len() as u32 - central_offset;

    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(central.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u16).to_le_bytes());
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn html_document(pages: &[String], title: &str, print: bool) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">");
    html.push_str(&format!("<title>{}</title>", escape_html(title)));
    if print {
        html.push_str(
            "<style>@page{margin:0}body{margin:0}.page{break-after:page;page-break-after:always}\
             .page:last-child{break-after:auto;page-break-after:auto}@media print{.page{margin:0}}\
             svg{display:block}</style>",
        );
    } else {
        html.push_str(
            "<style>body{font-family:sans-serif;background:#f4f4f5;margin:0;padding:24px}\
             .toolbar{max-width:900px;margin:0 auto 16px}.pages{display:flex;flex-direction:column;\
             align-items:center;gap:24px}.page{background:#fff;box-shadow:0 2px 8px #0003}\
             .page svg{display:block;max-width:100%;height:auto}</style>",
        );
    }
    html.push_str("</head><body>");
    if !print {
        html.push_str(&format!(
            "<div class=\"toolbar\"><h1>{}</h1><p>{} page(s)</p></div>",
            escape_html(title),
            pages.len()
        ));
    }
    html.push_str("<div class=\"pages\">");
    for (index, svg) in pages.iter().enumerate() {
        html.push_str(&format!(
            "<section class=\"page\" id=\"page-{}\">{svg}</section>",
            index + 1
        ));
    }
    html.push_str("</div></body></html>");
    html
}

fn animated_svg(pages: &[String], width: f32, height: f32, frame_delay_ms: u32) -> String {
    let frame = frame_delay_ms.max(50) as f64 / 1000.0;
    let total = (frame * pages.len().max(1) as f64).max(0.05);
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\">\
         <rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"#ffffff\"/>"
    );
    if pages.len() <= 1 {
        if let Some(page) = pages.first() {
            svg.push_str(svg_inner(page));
        }
    } else {
        for (index, page) in pages.iter().enumerate() {
            let start = index as f64 * frame / total;
            let end = ((index + 1) as f64 * frame / total).min(1.0);
            let initial = if index == 0 { 1 } else { 0 };
            let (values, key_times) = if index == 0 {
                ("1;0".to_string(), format!("0;{end:.6}"))
            } else if index + 1 == pages.len() {
                ("0;1".to_string(), format!("{start:.6};1"))
            } else {
                ("0;1;0".to_string(), format!("0;{start:.6};{end:.6}"))
            };
            svg.push_str(&format!("<g opacity=\"{initial}\">"));
            svg.push_str(&format!(
                "<animate attributeName=\"opacity\" calcMode=\"discrete\" dur=\"{total:.3}s\" \
                 repeatCount=\"indefinite\" values=\"{values}\" keyTimes=\"{key_times}\"/>"
            ));
            svg.push_str(svg_inner(page));
            svg.push_str("</g>");
        }
    }
    svg.push_str("</svg>");
    svg
}

fn svg_inner(svg: &str) -> &str {
    let start = svg.find('>').map(|index| index + 1).unwrap_or(0);
    let end = svg.rfind("</svg>").unwrap_or(svg.len());
    if start <= end { &svg[start..end] } else { svg }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{LayoutObject, LayoutShape, ShapeKind};
    use crate::report::{PageMargins, PageSize};

    fn shape(x: f32, y: f32, width: f32, height: f32) -> LayoutObject {
        LayoutObject::Shape(LayoutShape {
            kind: ShapeKind::Rect,
            x,
            y,
            width,
            height,
            fill: None,
            stroke: None,
            stroke_width: 1.0,
            opacity: 1.0,
            shadow: false,
            rotation_deg: 0.0,
        })
    }

    fn sample_report() -> ReportDocument {
        let mut report = ReportDocument::new("Batch <Report>")
            .with_page_size(PageSize::new(200.0, 160.0))
            .with_margins(PageMargins::new(10.0, 10.0, 10.0, 10.0))
            .with_page_numbers(false);
        report.pages[0].add(shape(0.0, 0.0, 60.0, 40.0));
        report.add_page();
        report.pages[1].add(shape(20.0, 20.0, 80.0, 50.0));
        report
    }

    fn export(format: BatchExportFormat) -> BatchExport {
        export_report(
            &sample_report(),
            &FjmlContext::default(),
            &LayoutOptions::default(),
            format,
            &BatchExportOptions::default(),
        )
        .expect("batch export")
    }

    #[test]
    fn html_export_embeds_every_page() {
        let export = export(BatchExportFormat::Html);
        assert_eq!(export.extension, "html");
        assert_eq!(export.mime, "text/html");
        let html = String::from_utf8(export.bytes.clone()).unwrap();
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("Batch &lt;Report&gt;"));
        assert_eq!(html.matches("<section class=\"page\"").count(), 2);
        assert_eq!(html.matches("<svg").count(), 2);
        assert!(html.contains("2 page(s)"));
    }

    #[test]
    fn printer_export_has_page_breaks() {
        let export = export(BatchExportFormat::Printer);
        let html = String::from_utf8(export.bytes).unwrap();
        assert!(html.contains("break-after:page"));
        assert!(html.contains("page-break-after:always"));
        assert!(!html.contains("class=\"toolbar\""));
    }

    #[test]
    fn pdf_export_has_pages_and_xref() {
        let export = export(BatchExportFormat::Pdf);
        assert_eq!(export.extension, "pdf");
        assert!(export.bytes.starts_with(b"%PDF-1.4"));
        let text = String::from_utf8_lossy(&export.bytes);
        assert_eq!(text.matches("/Type /Page ").count(), 2);
        assert!(text.contains("/Count 2"));
        assert!(text.contains("/Filter /DCTDecode"));
        assert!(text.trim_end().ends_with("%%EOF"));
        let index = text.rfind("startxref").unwrap();
        let offset: usize = text[index + "startxref".len()..]
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(export.bytes[offset..].starts_with(b"xref"));
    }

    #[test]
    fn powerpoint_export_is_a_zip_deck() {
        let export = export(BatchExportFormat::PowerPoint);
        assert_eq!(export.extension, "pptx");
        assert_eq!(&export.bytes[..4], b"PK\x03\x04");
        let text = String::from_utf8_lossy(&export.bytes);
        for name in [
            "[Content_Types].xml",
            "ppt/presentation.xml",
            "ppt/slides/slide1.xml",
            "ppt/slides/slide2.xml",
            "ppt/media/image1.png",
            "ppt/media/image2.png",
            "ppt/slideMasters/slideMaster1.xml",
            "ppt/slideLayouts/slideLayout1.xml",
            "ppt/theme/theme1.xml",
        ] {
            assert!(text.contains(name), "missing {name}");
        }
        assert!(text.contains("<p:sldId id=\"256\" r:id=\"rId2\"/>"));
        assert!(text.contains("<p:sldId id=\"257\" r:id=\"rId3\"/>"));
    }

    #[test]
    fn animation_export_cycles_pages() {
        let export = export(BatchExportFormat::Animation);
        assert_eq!(export.extension, "svg");
        let svg = String::from_utf8(export.bytes).unwrap();
        assert!(svg.starts_with("<svg"));
        assert_eq!(svg.matches("<animate").count(), 2);
        assert_eq!(svg.matches("repeatCount=\"indefinite\"").count(), 2);
        assert!(svg.contains("dur=\"1.600s\""));
    }

    #[test]
    fn batch_export_writes_files() {
        let export = export(BatchExportFormat::Html);
        let path = std::env::temp_dir().join(format!(
            "flowers-batch-{}-l9.{}",
            std::process::id(),
            export.extension
        ));
        export.write_to(&path).expect("write export");
        let written = std::fs::read(&path).expect("read export");
        assert_eq!(written, export.bytes);
        let _ = std::fs::remove_file(path);
    }
}
