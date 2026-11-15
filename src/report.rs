//! Paginated report documents (Phase 8, L7).
//!
//! A report is a sequence of fixed-size pages containing layout objects. Pages
//! carry headers/footers with live FJML values and automatic page numbers, and
//! can embed figures and tables built from the layout or table editors.

use std::collections::BTreeMap;

use crate::layout::{
    FjmlContext, LayoutObject, LayoutOptions, LayoutTable, LayoutText, ObjectBounds, TextAlign,
    layout_object_svg, resolve_fjml,
};

/// Page dimensions in SVG points (L7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}

impl PageSize {
    /// US Letter at 96 dpi.
    pub const LETTER: Self = Self {
        width: 816.0,
        height: 1056.0,
    };
    /// ISO A4 at 96 dpi.
    pub const A4: Self = Self {
        width: 794.0,
        height: 1123.0,
    };

    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

impl Default for PageSize {
    fn default() -> Self {
        Self::LETTER
    }
}

/// Page margins in SVG points (L7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageMargins {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl PageMargins {
    pub fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }
}

impl Default for PageMargins {
    fn default() -> Self {
        Self {
            top: 48.0,
            right: 48.0,
            bottom: 48.0,
            left: 48.0,
        }
    }
}

/// One report page: layout objects plus optional header/footer overrides
/// (L7).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReportPage {
    pub objects: Vec<LayoutObject>,
    /// Page-specific header (falls back to the document header).
    pub header: Option<String>,
    /// Page-specific footer (falls back to the document footer).
    pub footer: Option<String>,
}

impl ReportPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a figure/table/text object and return its index.
    pub fn add(&mut self, object: LayoutObject) -> usize {
        self.objects.push(object);
        self.objects.len() - 1
    }

    pub fn with_header(mut self, header: impl Into<String>) -> Self {
        self.header = Some(header.into());
        self
    }

    pub fn with_footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = Some(footer.into());
        self
    }
}

/// A paginated report document (L7).
#[derive(Debug, Clone, PartialEq)]
pub struct ReportDocument {
    pub title: String,
    pub size: PageSize,
    pub margins: PageMargins,
    pub pages: Vec<ReportPage>,
    /// Live header template applied to pages without an override.
    pub header: String,
    /// Live footer template applied to pages without an override.
    pub footer: String,
    /// Append `Page N of M` to the footer.
    pub show_page_numbers: bool,
}

impl ReportDocument {
    /// Create a report with a single empty page.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            size: PageSize::default(),
            margins: PageMargins::default(),
            pages: vec![ReportPage::new()],
            header: String::new(),
            footer: String::new(),
            show_page_numbers: true,
        }
    }

    pub fn with_page_size(mut self, size: PageSize) -> Self {
        self.size = size;
        self
    }

    pub fn with_margins(mut self, margins: PageMargins) -> Self {
        self.margins = margins;
        self
    }

    pub fn with_header(mut self, header: impl Into<String>) -> Self {
        self.header = header.into();
        self
    }

    pub fn with_footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = footer.into();
        self
    }

    pub fn with_page_numbers(mut self, show: bool) -> Self {
        self.show_page_numbers = show;
        self
    }

    /// Append a page and return its index.
    pub fn add_page(&mut self) -> usize {
        self.pages.push(ReportPage::new());
        self.pages.len() - 1
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// `Page N of M` for a zero-based page index (L7).
    pub fn page_number_text(&self, index: usize) -> String {
        format!("Page {} of {}", index + 1, self.pages.len().max(1))
    }

    /// Resolve live FJML values and `{page}` / `{pages}` tokens in a
    /// header/footer template (L7).
    ///
    /// Page tokens are substituted before FJML resolution so they are not
    /// mistaken for unknown FJML fields.
    pub fn resolve_page_text(&self, text: &str, ctx: &FjmlContext, index: usize) -> String {
        let pages = self.pages.len().max(1).to_string();
        let page = (index + 1).to_string();
        let with_pages = text
            .replace("{{pages}}", &pages)
            .replace("{{page}}", &page)
            .replace("{pages}", &pages)
            .replace("{page}", &page);
        resolve_fjml(&with_pages, ctx)
    }

    fn page_header(&self, index: usize) -> String {
        let page = &self.pages[index];
        let template = page.header.as_deref().unwrap_or(&self.header);
        template.to_string()
    }

    fn page_footer(&self, index: usize) -> String {
        let page = &self.pages[index];
        let template = page.footer.as_deref().unwrap_or(&self.footer);
        template.to_string()
    }

    /// Flow objects into pages of the configured size, creating new pages when
    /// the content area overflows (L7 pagination).
    ///
    /// Objects are positioned relative to the page content area (inside the
    /// margins); tall objects that exceed a page are still placed so nothing
    /// is lost.
    pub fn paginate(&mut self, objects: Vec<LayoutObject>) -> usize {
        if self.pages.is_empty() {
            self.add_page();
        }
        let content_height =
            (self.size.height - self.margins.top - self.margins.bottom - 32.0).max(1.0);
        let mut page_index = self.pages.len() - 1;
        let mut cursor = self.pages[page_index]
            .objects
            .iter()
            .filter_map(|object| object.bounds())
            .map(|bounds| bounds.bottom())
            .fold(0.0_f32, f32::max);
        for mut object in objects {
            let height = object
                .bounds()
                .map(|bounds| bounds.height)
                .unwrap_or(20.0)
                .max(1.0);
            if !self.pages[page_index].objects.is_empty() && cursor + height > content_height {
                page_index = self.add_page();
                cursor = 0.0;
            }
            if let Some(bounds) = object.bounds() {
                object.translate(-bounds.x, cursor - bounds.y);
            }
            self.pages[page_index].objects.push(object);
            cursor += height + 12.0;
        }
        self.pages.len()
    }

    /// Build a document and paginate the supplied objects in one call (L7).
    pub fn from_objects(title: impl Into<String>, objects: Vec<LayoutObject>) -> Self {
        let mut document = Self::new(title);
        document.paginate(objects);
        document
    }

    /// Render one page as an SVG document (L7).
    pub fn render_page_svg(
        &self,
        index: usize,
        ctx: &FjmlContext,
        opts: &LayoutOptions,
    ) -> Option<String> {
        let page = self.pages.get(index)?;
        let header = self.resolve_page_text(&self.page_header(index), ctx, index);
        let mut footer = self.resolve_page_text(&self.page_footer(index), ctx, index);
        if self.show_page_numbers {
            let number = self.page_number_text(index);
            if footer.is_empty() {
                footer = number;
            } else if !footer.contains(&number) {
                footer = format!("{footer} · {number}");
            }
        }
        let width = self.size.width;
        let height = self.size.height;
        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">"
        );
        svg.push_str(&format!(
            "<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"#ffffff\"/>"
        ));
        if !self.title.is_empty() {
            svg.push_str(&format!("<title>{}</title>", svg_escape(&self.title)));
        }
        if !header.is_empty() {
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"12\" fill=\"#222222\">{}</text>",
                self.margins.left,
                (self.margins.top / 2.0).max(14.0),
                opts.font_family,
                svg_escape(&header)
            ));
        }
        if !footer.is_empty() {
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"11\" fill=\"#444444\">{}</text>",
                self.margins.left,
                height - (self.margins.bottom / 2.0).max(12.0),
                opts.font_family,
                svg_escape(&footer)
            ));
        }
        for object in &page.objects {
            svg.push_str(&format!(
                "<g transform=\"translate({:.1} {:.1})\">",
                self.margins.left, self.margins.top
            ));
            svg.push_str(&layout_object_svg(object, ctx, opts));
            svg.push_str("</g>");
        }
        svg.push_str("</svg>");
        Some(svg)
    }

    /// Render every page as a separate SVG document (L7).
    pub fn render_paginated_svg(&self, ctx: &FjmlContext, opts: &LayoutOptions) -> Vec<String> {
        (0..self.pages.len())
            .filter_map(|index| self.render_page_svg(index, ctx, opts))
            .collect()
    }

    /// Embed a table layout object on a page (L7 embedded tables).
    pub fn embed_table(&mut self, page: usize, table: &LayoutTable) -> bool {
        if let Some(page) = self.pages.get_mut(page) {
            page.objects.push(LayoutObject::Table(table.clone()));
            true
        } else {
            false
        }
    }
}

/// The dimension along which a batch report iterates (L8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IterationAxis {
    Sample,
    Panel,
    Keyword(String),
}

impl IterationAxis {
    /// The keyword driving this axis, when it is keyword-based.
    pub fn keyword(&self) -> Option<&str> {
        match self {
            Self::Keyword(keyword) => Some(keyword),
            _ => None,
        }
    }

    /// Human-readable axis name.
    pub fn label(&self) -> &str {
        match self {
            Self::Sample => "sample",
            Self::Panel => "panel",
            Self::Keyword(_) => "keyword",
        }
    }
}

/// One iteration item: the sample/panel/keyword value plus its report objects
/// (L8).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IterationItem {
    pub label: String,
    /// Sample name used for live FJML sample fields (defaults to `label`).
    pub sample_name: Option<String>,
    /// Population name exposed through `{{stat:...}}`.
    pub population: Option<String>,
    /// Keyword values carried by this item (used for keyword iteration).
    pub keywords: BTreeMap<String, String>,
    pub objects: Vec<LayoutObject>,
}

impl IterationItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..Self::default()
        }
    }

    pub fn with_objects(mut self, objects: Vec<LayoutObject>) -> Self {
        self.objects = objects;
        self
    }

    pub fn with_sample_name(mut self, sample: impl Into<String>) -> Self {
        self.sample_name = Some(sample.into());
        self
    }

    pub fn with_population(mut self, population: impl Into<String>) -> Self {
        self.population = Some(population.into());
        self
    }

    pub fn with_keyword(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.keywords.insert(key.into(), value.into());
        self
    }

    /// Case-insensitive keyword lookup.
    pub fn keyword(&self, key: &str) -> Option<&str> {
        self.keywords.get(key).map(String::as_str).or_else(|| {
            self.keywords
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, value)| value.as_str())
        })
    }

    /// Union of all object bounds.
    pub fn bounds(&self) -> Option<ObjectBounds> {
        let mut bounds: Option<ObjectBounds> = None;
        for object in &self.objects {
            let Some(next) = object.bounds() else {
                continue;
            };
            bounds = Some(match bounds {
                Some(current) => {
                    let x = current.x.min(next.x);
                    let y = current.y.min(next.y);
                    let right = current.right().max(next.right());
                    let bottom = current.bottom().max(next.bottom());
                    ObjectBounds {
                        x,
                        y,
                        width: right - x,
                        height: bottom - y,
                    }
                }
                None => next,
            });
        }
        bounds
    }
}

/// Merge iteration items that share a keyword value (L8).
///
/// Items sharing a value are merged in iteration order; the resulting groups
/// are sorted by value for deterministic reports. Items without the keyword
/// are grouped under `<none>`.
pub fn group_items_by_keyword(items: Vec<IterationItem>, keyword: &str) -> Vec<IterationItem> {
    let mut groups: BTreeMap<String, IterationItem> = BTreeMap::new();
    for item in items {
        let value = item.keyword(keyword).unwrap_or("<none>").to_string();
        let group = groups
            .entry(value.clone())
            .or_insert_with(|| IterationItem::new(value.clone()));
        let IterationItem {
            label: _,
            sample_name,
            population,
            keywords,
            objects,
        } = item;
        if group.sample_name.is_none() {
            group.sample_name = sample_name;
        }
        if group.population.is_none() {
            group.population = population;
        }
        for (key, value) in keywords {
            group.keywords.entry(key).or_insert(value);
        }
        group.keywords.insert(keyword.to_string(), value);
        group.objects.extend(objects);
    }
    groups.into_values().collect()
}

/// How iteration items are arranged on report pages (L8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IterationLayout {
    /// One item per page.
    SeparatePages,
    /// A grid of items per page.
    Tiled { columns: usize, rows: usize },
}

impl IterationLayout {
    /// Items per page (1 for separate pages).
    pub fn capacity(&self) -> usize {
        match self {
            Self::SeparatePages => 1,
            Self::Tiled { columns, rows } => (*columns).max(1) * (*rows).max(1),
        }
    }
}

/// A batch report that iterates over samples, panels or keyword values (L8).
#[derive(Debug, Clone, PartialEq)]
pub struct IterationReport {
    pub axis: IterationAxis,
    pub layout: IterationLayout,
    pub items: Vec<IterationItem>,
    pub title: String,
    pub size: PageSize,
    pub margins: PageMargins,
    pub header: String,
    pub footer: String,
    pub show_page_numbers: bool,
}

impl IterationReport {
    pub fn new(axis: IterationAxis, layout: IterationLayout) -> Self {
        Self {
            axis,
            layout,
            items: Vec::new(),
            title: "Report".to_string(),
            size: PageSize::default(),
            margins: PageMargins::default(),
            header: String::new(),
            footer: String::new(),
            show_page_numbers: true,
        }
    }

    /// Iterate over samples, one page per sample.
    pub fn by_sample() -> Self {
        Self::new(IterationAxis::Sample, IterationLayout::SeparatePages)
    }

    /// Iterate over panels, one page per panel.
    pub fn by_panel() -> Self {
        Self::new(IterationAxis::Panel, IterationLayout::SeparatePages)
    }

    /// Iterate over values of `keyword`, one page per value.
    pub fn by_keyword(keyword: impl Into<String>) -> Self {
        Self::new(
            IterationAxis::Keyword(keyword.into()),
            IterationLayout::SeparatePages,
        )
    }

    pub fn with_items(mut self, items: Vec<IterationItem>) -> Self {
        self.items = items;
        self
    }

    pub fn with_layout(mut self, layout: IterationLayout) -> Self {
        self.layout = layout;
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn with_page_size(mut self, size: PageSize) -> Self {
        self.size = size;
        self
    }

    pub fn with_margins(mut self, margins: PageMargins) -> Self {
        self.margins = margins;
        self
    }

    pub fn with_header(mut self, header: impl Into<String>) -> Self {
        self.header = header.into();
        self
    }

    pub fn with_footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = footer.into();
        self
    }

    pub fn with_page_numbers(mut self, show: bool) -> Self {
        self.show_page_numbers = show;
        self
    }

    /// Append one item and return its index.
    pub fn add_item(&mut self, item: IterationItem) -> usize {
        self.items.push(item);
        self.items.len() - 1
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Replace `items` with one item per distinct keyword value (L8).
    pub fn group_by_keyword(mut self, keyword: &str) -> Self {
        self.items = group_items_by_keyword(self.items, keyword);
        self
    }

    /// Number of report pages produced by this batch.
    pub fn page_count(&self) -> usize {
        let capacity = self.layout.capacity();
        self.items.len().div_ceil(capacity).max(1)
    }

    /// Build a regular [`ReportDocument`] with items placed on pages (L8).
    ///
    /// Live FJML text and headers/footers are resolved per item before the
    /// document is assembled, so tiled pages can mix different samples.
    pub fn to_document(&self) -> ReportDocument {
        let mut document = ReportDocument::new(self.title.clone())
            .with_page_size(self.size)
            .with_margins(self.margins)
            .with_header(self.header.clone())
            .with_footer(self.footer.clone())
            .with_page_numbers(self.show_page_numbers);
        document.pages.clear();

        match self.layout {
            IterationLayout::SeparatePages => {
                for item in &self.items {
                    let ctx = self.item_context(item);
                    let mut page = ReportPage::new();
                    for object in &item.objects {
                        page.add(resolve_object_fjml(object, item, &ctx));
                    }
                    document.pages.push(page);
                }
            }
            IterationLayout::Tiled { columns, rows } => {
                let columns = columns.max(1);
                let rows = rows.max(1);
                let content_width =
                    (self.size.width - self.margins.left - self.margins.right).max(1.0);
                let content_height =
                    (self.size.height - self.margins.top - self.margins.bottom).max(1.0);
                let cell_width = content_width / columns as f32;
                let cell_height = content_height / rows as f32;
                let capacity = columns * rows;
                for chunk in self.items.chunks(capacity) {
                    let mut page = ReportPage::new();
                    for (slot, item) in chunk.iter().enumerate() {
                        let column = slot % columns;
                        let row = slot / columns;
                        let cell_x = column as f32 * cell_width;
                        let cell_y = row as f32 * cell_height;
                        let ctx = self.item_context(item);
                        page.add(LayoutObject::Text(LayoutText {
                            content: item.label.clone(),
                            x: cell_x + 8.0,
                            y: cell_y + 16.0,
                            width: (cell_width - 16.0).max(1.0),
                            height: 18.0,
                            font_size: 12.0,
                            color: "#222222".to_string(),
                            opacity: 1.0,
                            rotation_deg: 0.0,
                            align: TextAlign::Start,
                        }));
                        let bounds = item.bounds();
                        for object in &item.objects {
                            let mut resolved = resolve_object_fjml(object, item, &ctx);
                            let (dx, dy) = match bounds {
                                Some(bounds) => (cell_x + 8.0 - bounds.x, cell_y + 26.0 - bounds.y),
                                None => (cell_x + 8.0, cell_y + 26.0),
                            };
                            resolved.translate(dx, dy);
                            page.add(resolved);
                        }
                    }
                    document.pages.push(page);
                }
            }
        }
        if document.pages.is_empty() {
            document.add_page();
        }

        // Resolve headers/footers against the item that owns each page.
        let page_items = self.page_item_indices();
        let resolved: Vec<(Option<String>, Option<String>)> = (0..document.pages.len())
            .map(|index| {
                let Some(item) = page_items
                    .get(index)
                    .copied()
                    .flatten()
                    .and_then(|item_index| self.items.get(item_index))
                else {
                    return (None, None);
                };
                let ctx = self.item_context(item);
                let header = if document.header.is_empty() {
                    None
                } else {
                    let template = resolve_keyword_fields(&document.header, item);
                    Some(document.resolve_page_text(&template, &ctx, index))
                };
                let footer = if document.footer.is_empty() {
                    None
                } else {
                    let template = resolve_keyword_fields(&document.footer, item);
                    Some(document.resolve_page_text(&template, &ctx, index))
                };
                (header, footer)
            })
            .collect();
        for (index, (header, footer)) in resolved.into_iter().enumerate() {
            let page = &mut document.pages[index];
            if let Some(header) = header.filter(|text| !text.is_empty()) {
                page.header = Some(header);
            }
            if let Some(footer) = footer.filter(|text| !text.is_empty()) {
                page.footer = Some(footer);
            }
        }
        document
    }

    /// Render every page with the per-item FJML context (L8).
    pub fn render_paginated_svg(&self, opts: &LayoutOptions) -> Vec<String> {
        self.to_document()
            .render_paginated_svg(&FjmlContext::default(), opts)
    }

    /// Render one page with the per-item FJML context (L8).
    pub fn render_page_svg(&self, index: usize, opts: &LayoutOptions) -> Option<String> {
        self.to_document()
            .render_page_svg(index, &FjmlContext::default(), opts)
    }

    fn item_context<'a>(&'a self, item: &'a IterationItem) -> FjmlContext<'a> {
        FjmlContext {
            sample_name: item.sample_name.as_deref().unwrap_or(&item.label),
            statistic: item.population.as_deref(),
            keyword: self.axis.keyword().and_then(|key| item.keyword(key)),
            date: item.keyword("$DATE").unwrap_or(""),
        }
    }

    fn page_item_indices(&self) -> Vec<Option<usize>> {
        match self.layout {
            IterationLayout::SeparatePages => (0..self.items.len()).map(Some).collect(),
            IterationLayout::Tiled { .. } => (0..self.items.len())
                .step_by(self.layout.capacity())
                .map(Some)
                .collect(),
        }
    }
}

/// Substitute `{{keyword:KEY}}` fields from an iteration item's keyword map
/// (L8), leaving unknown fields for FJML resolution.
fn resolve_keyword_fields(text: &str, item: &IterationItem) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{keyword:") {
        out.push_str(&rest[..start]);
        let after = start + "{{keyword:".len();
        let Some(end) = rest[after..].find("}}") else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let key = &rest[after..after + end];
        match item.keyword(key) {
            Some(value) => out.push_str(value),
            None => out.push_str(&rest[start..after + end + 2]),
        }
        rest = &rest[after + end + 2..];
    }
    out.push_str(rest);
    out
}

/// Resolve a text template against an iteration item (L8).
fn resolve_item_text(text: &str, item: &IterationItem, ctx: &FjmlContext) -> String {
    resolve_fjml(&resolve_keyword_fields(text, item), ctx)
}

/// Resolve live FJML fields in text objects before batching (L8).
fn resolve_object_fjml(
    object: &LayoutObject,
    item: &IterationItem,
    ctx: &FjmlContext,
) -> LayoutObject {
    match object {
        LayoutObject::Text(text) => {
            let mut text = text.clone();
            text.content = resolve_item_text(&text.content, item, ctx);
            LayoutObject::Text(text)
        }
        LayoutObject::Group(group) => {
            let mut group = group.clone();
            group.objects = group
                .objects
                .iter()
                .map(|child| resolve_object_fjml(child, item, ctx))
                .collect();
            LayoutObject::Group(group)
        }
        other => other.clone(),
    }
}

fn svg_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{LayoutShape, LayoutTable, ShapeKind};

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

    #[test]
    fn report_paginates_objects_across_pages() {
        let mut report = ReportDocument::new("Report")
            .with_page_size(PageSize::new(400.0, 300.0))
            .with_margins(PageMargins::new(20.0, 20.0, 20.0, 20.0));
        report.show_page_numbers = false;
        let pages = report.paginate(vec![
            shape(0.0, 0.0, 100.0, 100.0),
            shape(0.0, 0.0, 100.0, 100.0),
            shape(0.0, 0.0, 100.0, 100.0),
        ]);
        assert_eq!(pages, 2);
        assert_eq!(report.pages[0].objects.len(), 2);
        assert_eq!(report.pages[1].objects.len(), 1);
        // Pagination moves objects into the content area top-down.
        let first = report.pages[0].objects[0].bounds().unwrap();
        assert_eq!((first.x, first.y), (0.0, 0.0));
        let second = report.pages[0].objects[1].bounds().unwrap();
        assert_eq!(second.y, 112.0);
        let third = report.pages[1].objects[0].bounds().unwrap();
        assert_eq!(third.y, 0.0);
    }

    #[test]
    fn report_headers_footers_resolve_fjml_and_page_numbers() {
        let mut report = ReportDocument::new("Report")
            .with_header("{{sample.name}} / {{stat:population}}")
            .with_footer("Confidential");
        report.add_page();
        let ctx = FjmlContext::new("SampleA", Some("Live"), None, "2026-11-14");
        let first = report
            .render_page_svg(0, &ctx, &LayoutOptions::default())
            .unwrap();
        assert!(first.contains("SampleA / Live"));
        assert!(first.contains("Confidential · Page 1 of 2"));
        let second = report
            .render_page_svg(1, &ctx, &LayoutOptions::default())
            .unwrap();
        assert!(second.contains("Confidential · Page 2 of 2"));
    }

    #[test]
    fn report_footer_page_tokens_are_not_duplicated() {
        let mut report = ReportDocument::new("Report").with_footer("Page {page} of {pages}");
        report.add_page();
        let ctx = FjmlContext::new("SampleA", None, None, "");
        let svg = report
            .render_page_svg(0, &ctx, &LayoutOptions::default())
            .unwrap();
        assert!(svg.contains("Page 1 of 2"));
        assert!(!svg.contains("Page 1 of 2 · Page 1 of 2"));
        assert_eq!(report.resolve_page_text("{{pages}}", &ctx, 0), "2");
    }

    #[test]
    fn report_embeds_tables_and_figures_with_page_overrides() {
        let mut report = ReportDocument::new("Report")
            .with_page_size(PageSize::A4)
            .with_page_numbers(false);
        report.pages[0].header = Some("Override {{sample.name}}".into());
        let table = LayoutTable::new(
            0.0,
            0.0,
            300.0,
            80.0,
            "Statistics",
            vec!["population".into(), "count".into()],
            vec![vec!["Live".into(), "42".into()]],
        );
        assert!(report.embed_table(0, &table));
        assert!(!report.embed_table(4, &table));
        let page = &mut report.pages[0];
        page.add(shape(0.0, 100.0, 20.0, 20.0));
        let ctx = FjmlContext::new("SampleA", None, None, "");
        let svg = report
            .render_page_svg(0, &ctx, &LayoutOptions::default())
            .unwrap();
        assert!(svg.contains("Override SampleA"));
        assert!(svg.contains("Statistics"));
        assert!(svg.contains(">42<"));
        assert!(svg.contains("<rect"));
        assert!(!svg.contains("Page 1 of 1"));
        assert_eq!(
            report
                .render_paginated_svg(&ctx, &LayoutOptions::default())
                .len(),
            1
        );
    }

    #[test]
    fn report_from_objects_creates_pages_and_custom_footers() {
        let objects = vec![
            shape(0.0, 0.0, 10.0, 180.0),
            shape(0.0, 0.0, 10.0, 180.0),
            shape(0.0, 0.0, 10.0, 180.0),
        ];
        // Pagination follows the page size configured before the objects are
        // flowed, so builder settings must come first.
        let mut report = ReportDocument::new("Batch")
            .with_page_size(PageSize::new(300.0, 420.0))
            .with_margins(PageMargins::new(10.0, 10.0, 10.0, 10.0))
            .with_footer("Batch report");
        report.paginate(objects);
        assert!(report.page_count() >= 2);
        let single = ReportDocument::from_objects("Single", vec![shape(0.0, 0.0, 10.0, 10.0)]);
        assert_eq!(single.page_count(), 1);
        report.pages[0].footer = Some("First page".into());
        let ctx = FjmlContext::new("S", None, None, "");
        let pages = report.render_paginated_svg(&ctx, &LayoutOptions::default());
        assert_eq!(pages.len(), report.page_count());
        assert!(pages[0].contains("First page"));
        assert!(pages[0].contains("Page 1 of"));
        assert_eq!(
            report.page_number_text(0),
            format!("Page 1 of {}", report.page_count())
        );
    }

    fn text_object(content: &str, x: f32, y: f32) -> LayoutObject {
        LayoutObject::Text(LayoutText {
            content: content.to_string(),
            x,
            y,
            width: 140.0,
            height: 18.0,
            font_size: 12.0,
            color: "#222222".to_string(),
            opacity: 1.0,
            rotation_deg: 0.0,
            align: TextAlign::Start,
        })
    }

    #[test]
    fn iteration_report_builds_separate_pages_per_sample() {
        let items = vec![
            IterationItem::new("S1")
                .with_keyword("$TOT", "100")
                .with_objects(vec![text_object(
                    "{{sample.name}} ({{keyword:$TOT}})",
                    0.0,
                    0.0,
                )]),
            IterationItem::new("S2")
                .with_keyword("$TOT", "200")
                .with_objects(vec![text_object(
                    "{{sample.name}} ({{keyword:$TOT}})",
                    0.0,
                    0.0,
                )]),
        ];
        let report = IterationReport::by_sample()
            .with_title("Samples")
            .with_header("Sample: {{sample.name}}")
            .with_items(items);
        assert_eq!(report.page_count(), 2);
        assert_eq!(report.to_document().page_count(), 2);
        let pages = report.render_paginated_svg(&LayoutOptions::default());
        assert_eq!(pages.len(), 2);
        assert!(pages[0].contains("Sample: S1"));
        assert!(pages[0].contains("S1 (100)"));
        assert!(pages[1].contains("Sample: S2"));
        assert!(pages[1].contains("S2 (200)"));
    }

    #[test]
    fn iteration_report_tiles_items_with_captions() {
        let items: Vec<IterationItem> = (1..=4)
            .map(|i| {
                IterationItem::new(format!("Sample{i}")).with_objects(vec![
                    shape(0.0, 0.0, 40.0, 30.0),
                    shape(50.0, 0.0, 40.0, 30.0),
                ])
            })
            .collect();
        let report = IterationReport::by_sample()
            .with_layout(IterationLayout::Tiled {
                columns: 2,
                rows: 2,
            })
            .with_page_size(PageSize::new(500.0, 500.0))
            .with_margins(PageMargins::new(10.0, 10.0, 10.0, 10.0))
            .with_items(items);
        assert_eq!(report.page_count(), 1);
        let document = report.to_document();
        assert_eq!(document.page_count(), 1);
        let page = &document.pages[0];
        // Four captions plus two shapes per item.
        assert_eq!(page.objects.len(), 12);
        let shapes: Vec<ObjectBounds> = page
            .objects
            .iter()
            .filter(|object| matches!(object, LayoutObject::Shape(_)))
            .filter_map(|object| object.bounds())
            .collect();
        assert_eq!(shapes.len(), 8);
        assert_eq!(shapes.iter().filter(|b| b.y < 240.0).count(), 4);
        assert_eq!(shapes.iter().filter(|b| b.y >= 240.0).count(), 4);
        assert_eq!(shapes.iter().filter(|b| b.x < 240.0).count(), 4);
        assert_eq!(shapes.iter().filter(|b| b.x >= 240.0).count(), 4);
        let svg = report
            .render_page_svg(0, &LayoutOptions::default())
            .unwrap();
        for label in ["Sample1", "Sample2", "Sample3", "Sample4"] {
            assert!(svg.contains(label));
        }
    }

    #[test]
    fn iteration_report_groups_keyword_values() {
        let items = vec![
            IterationItem::new("S1")
                .with_keyword("panel", "A")
                .with_objects(vec![text_object("one", 0.0, 0.0)]),
            IterationItem::new("S2")
                .with_keyword("panel", "B")
                .with_objects(vec![text_object("two", 0.0, 0.0)]),
            IterationItem::new("S3")
                .with_keyword("panel", "A")
                .with_objects(vec![text_object("three", 0.0, 0.0)]),
        ];
        let grouped = group_items_by_keyword(items.clone(), "panel");
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].label, "A");
        assert_eq!(grouped[0].objects.len(), 2);
        assert_eq!(grouped[1].label, "B");
        assert_eq!(grouped[1].objects.len(), 1);
        assert_eq!(
            group_items_by_keyword(vec![IterationItem::new("S")], "panel")[0].label,
            "<none>"
        );

        let report = IterationReport::by_keyword("panel")
            .with_header("Panel {{keyword:panel}}")
            .with_items(items)
            .group_by_keyword("panel");
        let pages = report.render_paginated_svg(&LayoutOptions::default());
        assert_eq!(pages.len(), 2);
        assert!(pages[0].contains("Panel A"));
        assert!(pages[0].contains("one"));
        assert!(pages[0].contains("three"));
        assert!(pages[1].contains("Panel B"));
        assert!(pages[1].contains("two"));
    }

    #[test]
    fn iteration_report_panel_axis_uses_population_context() {
        let items = vec![
            IterationItem::new("T cells")
                .with_population("CD3+")
                .with_objects(vec![text_object("{{stat:value}}", 0.0, 0.0)]),
        ];
        let report = IterationReport::by_panel()
            .with_header("Panel {{sample.name}}")
            .with_items(items);
        let svg = report
            .render_page_svg(0, &LayoutOptions::default())
            .unwrap();
        assert!(svg.contains("Panel T cells"));
        assert!(svg.contains("CD3+"));
    }
}
