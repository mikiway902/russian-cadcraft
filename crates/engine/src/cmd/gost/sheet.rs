//! Native paper-space sheets, constructed from GOST R 2.104-2023, appendix B,
//! forms 1 and 2a (September 2024 corrected edition). Coordinates are paper mm.
//! Conditional dashed boxes 24–25, 27–30, 33–34 and organisation-defined 36–41
//! are deliberately not implied by this preset. It does not certify a document.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use cadcraft_doc::{Attrib, Block, Common, Entity, EntityKind, HAlign, Insert, Layout, Line, PageSetup, Space, Text, VAlign};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use crate::cmd::{CommandSpec, bad};
use crate::{Result, Session};

const CMD: &str = "gost.sheet";
const BLOCK_PREFIX: &str = "GOST-SHEET-";
const REV_COLS: [f64; 6] = [0.0, 7.0, 17.0, 40.0, 55.0, 65.0];
const REV_KEYS: [&str; 5] = ["CHANGE", "SHEET", "DOCUMENT", "SIGNATURE", "DATE"];
const REV_LABELS: [&str; 5] = ["Изм.", "Лист", "№ докум.", "Подп.", "Дата"];
const ROLES: [(&str, &str); 6] = [
    ("DEVELOPER", "Разраб."),
    ("CHECKER", "Пров."),
    ("TECH_CONTROL", "Т. контр."),
    ("EXTRA_ROLE", ""),
    ("NORM_CONTROL", "Н. контр."),
    ("APPROVER", "Утв."),
];

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new(CMD, "GOST Drawing Sheet", run)
        .menu(&["Format", "GOST", "GOST Drawing Sheet"])
        .params("{layout?: unique name (default GOST1, GOST2, …), format?: A4|A3|A2|A1|A0 (A4), landscape?: bool (false; A4 portrait only), form?: \"1\"|\"2a\" (1), fields?: {ATTRIBUTE_TAG: string} (case-insensitive; see returned fields), font?, scale?} → {layout, block, insert, width, height, form, fields, warnings, limitations}")]
}

struct SheetParams {
    layout: String,
    block: String,
    format: String,
    portrait: (f64, f64),
    landscape: bool,
    first: bool,
    values: BTreeMap<String, String>,
}

fn string_param<'a>(p: &'a Value, key: &str, default: &'a str) -> Result<&'a str> {
    match p.get(key) {
        None => Ok(default),
        Some(v) => v.as_str().ok_or_else(|| bad(CMD, format!("`{key}` must be a string"))),
    }
}

impl SheetParams {
    fn parse(s: &Session, p: &Value) -> Result<Self> {
        let object = p.as_object().ok_or_else(|| bad(CMD, "parameters must be an object"))?;
        for key in object.keys() {
            if !matches!(key.as_str(), "layout" | "format" | "landscape" | "form" | "fields" | "font" | "scale") {
                return Err(bad(CMD, format!("unknown parameter `{key}`")));
            }
        }
        let format = string_param(p, "format", "A4")?.to_ascii_uppercase();
        let portrait = match format.as_str() {
            "A4" => (210.0, 297.0),
            "A3" => (297.0, 420.0),
            "A2" => (420.0, 594.0),
            "A1" => (594.0, 841.0),
            "A0" => (841.0, 1189.0),
            _ => return Err(bad(CMD, "`format` must be A4, A3, A2, A1 or A0")),
        };
        let landscape = match p.get("landscape") {
            None => false,
            Some(v) => v.as_bool().ok_or_else(|| bad(CMD, "`landscape` must be a boolean"))?,
        };
        if format == "A4" && landscape {
            return Err(bad(CMD, "GOST drawing sheets of format A4 must be portrait"));
        }
        let first = match string_param(p, "form", "1")? {
            "1" => true,
            "2a" => false,
            _ => return Err(bad(CMD, "`form` must be \"1\" or \"2a\"")),
        };
        let d = s.doc()?;
        let layout = match p.get("layout") {
            Some(v) => v.as_str().ok_or_else(|| bad(CMD, "`layout` must be a string"))?.trim().to_string(),
            None => (1..=10_000)
                .map(|i| format!("GOST{i}"))
                .find(|n| d.layout(n).is_none() && d.block(&format!("{BLOCK_PREFIX}{n}")).is_none())
                .ok_or_else(|| bad(CMD, "no free GOST layout name"))?,
        };
        let block = format!("{BLOCK_PREFIX}{layout}");
        if !crate::cmd::blocks::valid_block_name(&layout) || layout.chars().any(char::is_control) || layout.eq_ignore_ascii_case("Model") {
            return Err(bad(CMD, "invalid or reserved layout name"));
        }
        if !crate::cmd::blocks::valid_block_name(&block) {
            return Err(bad(CMD, "layout name is too long for its GOST sheet block name"));
        }
        if d.layout(&layout).is_some() || d.block(&block).is_some() {
            return Err(bad(CMD, "layout or GOST sheet block already exists; choose a new layout name"));
        }
        let mut values = BTreeMap::new();
        if let Some(fields) = p.get("fields") {
            let fields = fields.as_object().ok_or_else(|| bad(CMD, "`fields` must be an object of attribute strings"))?;
            for (tag, v) in fields {
                let value = v.as_str().ok_or_else(|| bad(CMD, format!("field `{tag}` must be a string")))?;
                if value.chars().count() > 255 || value.chars().any(char::is_control) {
                    return Err(bad(CMD, format!("field `{tag}` must be a single line of at most 255 characters")));
                }
                if values.insert(tag.to_ascii_uppercase(), value.to_string()).is_some() {
                    return Err(bad(CMD, format!("duplicate case-insensitive field `{tag}`")));
                }
            }
        }
        // FORMAT is derived from actual paper size, never allowed to contradict it.
        if values.get("FORMAT").is_some_and(|v| v != &format) {
            return Err(bad(CMD, "FORMAT field must match the sheet's format"));
        }
        Ok(Self { layout, block, format, portrait, landscape, first, values })
    }

    fn size(&self) -> (f64, f64) {
        if self.landscape { (self.portrait.1, self.portrait.0) } else { self.portrait }
    }
}

/// Geometry is prepared, including the complete field vocabulary, before touching the drawing.
struct SheetGeometry {
    entities: Vec<(Common, EntityKind)>,
    tags: BTreeSet<String>,
}

impl SheetGeometry {
    fn new() -> Self {
        Self { entities: Vec::new(), tags: BTreeSet::new() }
    }

    fn push(&mut self, layer: &str, kind: EntityKind) {
        self.entities.push((Common { layer: layer.into(), ..Common::default() }, kind));
    }

    fn line(&mut self, a: (f64, f64), b: (f64, f64), main: bool) {
        self.push(if main { "GOST-FRAME" } else { "GOST-THIN" }, EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) }));
    }

    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.line((x, y), (x + w, y), true);
        self.line((x + w, y), (x + w, y + h), true);
        self.line((x + w, y + h), (x, y + h), true);
        self.line((x, y + h), (x, y), true);
    }

    fn text(value: &str, x: f64, y: f64, height: f64, rotation: f64) -> Text {
        let insert = Vec3::new(x, y, 0.0);
        Text {
            insert,
            align_pt: Some(insert),
            height,
            value: value.into(),
            rotation,
            width_factor: 1.0,
            oblique: 0.0,
            style: "GOST".into(),
            halign: HAlign::Center,
            valign: VAlign::Middle,
        }
    }

    fn label(&mut self, value: &str, x: f64, y: f64) {
        self.push("GOST-TEXT", EntityKind::Text(Self::text(value, x, y, 2.5, 0.0)));
    }

    fn field(&mut self, q: &SheetParams, tag: &str, default: &str, position: (f64, f64), height: f64, rotation: f64) {
        self.tags.insert(tag.into());
        let value = q.values.get(tag).map_or(default, String::as_str);
        self.push(
            "GOST-TEXT",
            EntityKind::AttDef(Attrib {
                tag: tag.into(),
                text: Self::text(value, position.0, position.1, height, rotation),
                invisible: false,
                constant: false,
                prompt: tag.into(),
                props: Default::default(),
                multiline: None,
                modes: Default::default(),
            }),
        );
    }

    fn fit_fields(&mut self, q: &SheetParams, s: &Session, p: &Value) -> Result<Vec<String>> {
        let (font, width_factor) = if let Some(style) = s.doc()?.text_style("GOST") {
            (cadcraft_render::style_font(style), style.width_factor)
        } else {
            (cadcraft_fonts::TextFont::resolve(p.get("font").and_then(Value::as_str).unwrap_or(cadcraft_fonts::GOST_FONT)), 1.0)
        };
        let mut fitted = Vec::new();
        for (_, kind) in &mut self.entities {
            let EntityKind::AttDef(a) = kind else { continue };
            if a.text.value.is_empty() {
                continue;
            }
            let width = field_width(&a.tag, q.first);
            let unit_width = cadcraft_fonts::text_width(&font, &a.text.value, 1.0, width_factor);
            let height = [7.0, 5.0, 3.5, 2.5].into_iter().find(|h| *h <= a.text.height && unit_width * h <= width);
            let Some(height) = height.filter(|_| unit_width.is_finite() && unit_width > 0.0) else {
                return Err(bad(
                    CMD,
                    format!(
                        "attribute {} does not fit its {width} mm cell at supported GOST text heights; shorten it or prepare a multiline title block",
                        a.tag
                    ),
                ));
            };
            if height != a.text.height {
                a.text.height = height;
                fitted.push(a.tag.clone());
            }
        }
        Ok(fitted)
    }

    fn revisions(&mut self, q: &SheetParams, x: f64, y: f64, rows: usize) {
        for col in REV_COLS.into_iter().skip(1) {
            self.line((x + col, y), (x + col, y + (rows + 1) as f64 * 5.0), true);
        }
        for row in 1..=rows {
            self.line((x, y + row as f64 * 5.0), (x + 65.0, y + row as f64 * 5.0), false);
        }
        for ((edges, key), label) in REV_COLS.windows(2).zip(REV_KEYS).zip(REV_LABELS) {
            let cx = x + (edges[0] + edges[1]) / 2.0;
            self.label(label, cx, y + 2.5);
            for row in 1..=rows {
                self.field(q, &format!("REV{row}_{key}"), "", (cx, y + row as f64 * 5.0 + 2.5), 2.5, 0.0);
            }
        }
    }

    fn first(&mut self, q: &SheetParams, x: f64) {
        // Bottom and right edges already belong to the sheet frame.
        self.line((x + 185.0, 60.0), (x, 60.0), true);
        if x > 20.0 {
            self.line((x, 60.0), (x, 5.0), true);
        }
        // Left signature table: widths 17,23,15,10; six rows of 5 mm.
        for col in [17.0, 40.0, 55.0, 65.0] {
            self.line((x + col, 5.0), (x + col, 35.0), true);
        }
        for row in 1..=6 {
            self.line((x, 5.0 + f64::from(row) * 5.0), (x + 65.0, 5.0 + f64::from(row) * 5.0), row == 6);
        }
        for (row, (key, label)) in ROLES.into_iter().enumerate() {
            let cy = 32.5 - row as f64 * 5.0;
            if label.is_empty() {
                self.field(q, "EXTRA_ROLE_LABEL", "", (x + 8.5, cy), 2.5, 0.0);
            } else {
                self.label(label, x + 8.5, cy);
            }
            self.field(q, key, "", (x + 28.5, cy), 2.5, 0.0);
            self.field(q, &format!("{key}_SIGNATURE"), "", (x + 47.5, cy), 2.5, 0.0);
            self.field(q, &format!("{key}_DATE"), "", (x + 60.0, cy), 2.5, 0.0);
        }
        self.revisions(q, x, 35.0, 4);
        self.line((x + 65.0, 20.0), (x + 185.0, 20.0), true);
        self.line((x + 65.0, 45.0), (x + 185.0, 45.0), true);
        self.line((x + 135.0, 5.0), (x + 135.0, 45.0), true);
        self.field(q, "TITLE", "", (x + 100.0, 32.5), 5.0, 0.0);
        self.field(q, "MATERIAL", "", (x + 100.0, 12.5), 3.5, 0.0);
        self.field(q, "DESIGNATION", "", (x + 120.0, 52.5), 5.0, 0.0);
        self.line((x + 175.0, 45.0), (x + 175.0, 60.0), true);
        self.line((x + 175.0, 55.0), (x + 185.0, 55.0), false);
        self.label("Вер.", x + 180.0, 57.5);
        self.field(q, "VERSION", "", (x + 180.0, 50.0), 3.5, 0.0);
        self.line((x + 135.0, 25.0), (x + 185.0, 25.0), true);
        self.line((x + 135.0, 40.0), (x + 185.0, 40.0), false);
        for col in [150.0, 167.0] {
            self.line((x + col, 25.0), (x + col, 45.0), true);
        }
        for col in [140.0, 145.0] {
            self.line((x + col, 25.0), (x + col, 40.0), false);
        }
        self.label("Лит.", x + 142.5, 42.5);
        self.label("Масса", x + 158.5, 42.5);
        self.label("Масштаб", x + 176.0, 42.5);
        for n in 1..=3 {
            self.field(q, &format!("LITERA{n}"), "", (x + 132.5 + f64::from(n) * 5.0, 32.5), 3.5, 0.0);
        }
        self.field(q, "MASS", "", (x + 158.5, 32.5), 3.5, 0.0);
        self.field(q, "SCALE", "1:1", (x + 176.0, 32.5), 3.5, 0.0);
        self.line((x + 155.0, 20.0), (x + 155.0, 25.0), true);
        self.label("Лист", x + 141.0, 22.5);
        self.label("Листов", x + 165.0, 22.5);
        self.field(q, "SHEET", "1", (x + 150.0, 22.5), 2.5, 0.0);
        self.field(q, "SHEETS", "1", (x + 179.0, 22.5), 2.5, 0.0);
        self.field(q, "ORG", "", (x + 160.0, 12.5), 3.5, 0.0);
    }

    fn subsequent(&mut self, q: &SheetParams, x: f64) {
        self.line((x + 185.0, 20.0), (x, 20.0), true);
        if x > 20.0 {
            self.line((x, 20.0), (x, 5.0), true);
        }
        self.revisions(q, x, 5.0, 2);
        self.line((x + 165.0, 5.0), (x + 165.0, 20.0), true);
        self.line((x + 175.0, 5.0), (x + 175.0, 20.0), true);
        self.line((x + 165.0, 15.0), (x + 185.0, 15.0), false);
        self.label("Вер.", x + 170.0, 17.5);
        self.label("Лист", x + 180.0, 17.5);
        self.field(q, "DESIGNATION", "", (x + 115.0, 12.5), 5.0, 0.0);
        self.field(q, "VERSION", "", (x + 170.0, 10.0), 3.5, 0.0);
        self.field(q, "SHEET", "2", (x + 180.0, 10.0), 3.5, 0.0);
    }

    fn supplementary(&mut self, q: &SheetParams, w: f64, h: f64, x: f64) {
        // Fields 19–23: 5-mm caption + 7-mm value columns, rotated 90°.
        let mut y = 5.0;
        self.line((8.0, 5.0), (20.0, 5.0), true);
        self.line((8.0, 5.0), (8.0, 150.0), true);
        self.line((13.0, 5.0), (13.0, 150.0), true);
        for (tag, caption, length) in [
            ("INVENTORY", "Инв. № подл.", 25.0),
            ("INVENTORY_SIGNATURE_DATE", "Подп. и дата", 35.0),
            ("REPLACES_INVENTORY", "Взам. инв. №", 25.0),
            ("DUPLICATE_INVENTORY", "Инв. № дубл.", 25.0),
            ("DUPLICATE_SIGNATURE_DATE", "Подп. и дата", 35.0),
        ] {
            self.line((8.0, y + length), (20.0, y + length), true);
            self.push("GOST-TEXT", EntityKind::Text(Self::text(caption, 10.5, y + length / 2.0, 2.5, std::f64::consts::FRAC_PI_2)));
            self.field(q, tag, "", (16.5, y + length / 2.0), 3.5, std::f64::consts::FRAC_PI_2);
            y += length;
        }
        // Field 26: 70×14, rotated with the short/long-side placement (table 1).
        let designation = q.values.get("DESIGNATION").map_or("", String::as_str);
        if q.format == "A4" || q.landscape {
            self.line((20.0, h - 19.0), (90.0, h - 19.0), true);
            self.line((90.0, h - 19.0), (90.0, h - 5.0), true);
            self.field(q, "DESIGNATION_TOP", designation, (55.0, h - 12.0), 7.0, std::f64::consts::PI);
        } else {
            self.line((w - 19.0, h - 75.0), (w - 5.0, h - 75.0), true);
            self.line((w - 19.0, h - 5.0), (w - 19.0, h - 75.0), true);
            self.field(q, "DESIGNATION_TOP", designation, (w - 12.0, h - 40.0), 7.0, std::f64::consts::FRAC_PI_2);
        }
        // Fields 31 and 32 lie in the 5-mm strip below the frame, as in appendix B.
        self.label("Копировал", x + 100.0, 2.5);
        self.field(q, "COPIED_BY", "", (x + 128.0, 2.5), 2.5, 0.0);
        self.label("Формат", x + 160.0, 2.5);
        self.field(q, "FORMAT", &q.format, (x + 178.0, 2.5), 2.5, 0.0);
    }

    fn build(q: &SheetParams) -> Result<Self> {
        let mut g = Self::new();
        let (w, h) = q.size();
        let x = w - 190.0;
        g.rect(20.0, 5.0, w - 25.0, h - 10.0);
        if q.first {
            g.first(q, x);
        } else {
            g.subsequent(q, x);
        }
        g.supplementary(q, w, h, x);
        for tag in q.values.keys() {
            if !g.tags.contains(tag) {
                return Err(bad(CMD, format!("unknown or inapplicable attribute `{tag}` for this form")));
            }
        }
        Ok(g)
    }
}

/// Usable widths along the text baseline, allowing for cell borders and captions.
fn field_width(tag: &str, first: bool) -> f64 {
    match tag {
        "DESIGNATION_TOP" => 68.0,
        "DESIGNATION" => {
            if first {
                108.0
            } else {
                98.0
            }
        }
        "TITLE" | "MATERIAL" => 68.0,
        "ORG" => 48.0,
        "MASS" => 15.0,
        "SCALE" => 16.0,
        "VERSION" => 8.0,
        "SHEET" => {
            if first {
                6.0
            } else {
                8.0
            }
        }
        "SHEETS" => 10.0,
        "LITERA1" | "LITERA2" | "LITERA3" => 4.0,
        "INVENTORY" | "REPLACES_INVENTORY" | "DUPLICATE_INVENTORY" => 23.0,
        "INVENTORY_SIGNATURE_DATE" | "DUPLICATE_SIGNATURE_DATE" => 33.0,
        "COPIED_BY" => 30.0,
        "FORMAT" => 10.0,
        "EXTRA_ROLE_LABEL" => 15.0,
        _ if tag.starts_with("REV") => match tag.rsplit('_').next() {
            Some("CHANGE") => 5.0,
            Some("SHEET") => 8.0,
            Some("DOCUMENT") => 21.0,
            Some("SIGNATURE") => 13.0,
            _ => 9.0,
        },
        _ if tag.ends_with("_SIGNATURE") => 13.0,
        _ if tag.ends_with("_DATE") => 9.0,
        _ => 21.0,
    }
}

/// Fixed, validated empty A4 form: no parsing, runtime commands or fallible I/O.
pub(super) fn initial(d: &mut cadcraft_doc::Drawing) {
    let q = SheetParams {
        layout: "GOST1".into(),
        block: "GOST-SHEET-GOST1".into(),
        format: "A4".into(),
        portrait: (210.0, 297.0),
        landscape: false,
        first: true,
        values: BTreeMap::new(),
    };
    let mut geometry = SheetGeometry::new();
    geometry.rect(20.0, 5.0, 185.0, 287.0);
    geometry.first(&q, 20.0);
    geometry.supplementary(&q, 210.0, 297.0, 20.0);
    append(d, &q, geometry);
}

fn append(d: &mut cadcraft_doc::Drawing, q: &SheetParams, geometry: SheetGeometry) -> cadcraft_doc::Handle {
    let (w, h) = q.size();
    let page = PageSetup {
        device: "CADCraft PDF".into(),
        paper: q.format.clone(),
        width_mm: q.portrait.0,
        height_mm: q.portrait.1,
        margins_mm: [0.0; 4],
        landscape: q.landscape,
        scale_to_fit: false,
        scale: 1.0,
        center: false,
        paper_mm: true,
        ..PageSetup::default()
    };
    let order = d.layouts.iter().map(|l| l.tab_order).max().unwrap_or(0).saturating_add(1);
    let mut block = Block::new(&q.block);
    block.units = 4; // DXF INSUNITS: mm, independent of the model's units.
    block.description = format!("GOST R 2.104-2023 form {}, {}", if q.first { "1" } else { "2a" }, q.format);
    let mut attribs = Vec::new();
    for (common, kind) in geometry.entities {
        if let EntityKind::AttDef(ad) = &kind {
            let mut a = ad.clone();
            a.prompt.clear();
            attribs.push(a);
        }
        block.entities.push(Entity { handle: d.new_handle(), common, kind });
    }
    d.blocks.insert(q.block.clone(), Arc::new(block));
    let mut layout = Layout { page, limits: Some((Vec2::ZERO, Vec2::new(w, h))), ..Layout::new(&q.layout, order) };
    let handle = d.new_handle();
    layout.entities.push(Entity {
        handle,
        common: Common { layer: "GOST-FRAME".into(), ..Common::default() },
        kind: EntityKind::Insert(Insert { block: q.block.clone(), attribs, ..Insert::default() }),
    });
    d.layouts.push(layout);
    d.active_layout = Some(q.layout.clone());
    handle
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let q = SheetParams::parse(s, p)?;
    let mut geometry = SheetGeometry::build(&q)?;
    let fitted = geometry.fit_fields(&q, s, p)?;
    // Paper installation validates first and preserves the model's headers and units.
    // Session owns the one undo transaction.
    let profile = super::profile::install_paper(s, p)?;
    let (w, h) = q.size();
    let tags = geometry.tags.clone();
    let handle = append(s.doc_mut()?, &q, geometry);
    let st = s.state_mut()?;
    st.space = Space::Paper(q.layout.clone());
    st.mspace = None;
    st.selection.clear();
    s.pending_window = None;
    s.zoom_extents()?;
    let view = s.state()?.view();
    if let Some(layout) = s.doc_mut()?.layouts.iter_mut().find(|l| l.name == q.layout) {
        layout.view = Some((view.center, view.height));
    }
    Ok(json!({
        "layout": q.layout, "block": q.block, "insert": handle.hex(),
        "format": q.format, "width": w, "height": h, "form": if q.first { "1" } else { "2a" },
        "fields": tags, "textFitted": fitted,
        "warnings": profile.get("warnings").cloned().unwrap_or_else(|| json!([])),
        "limitations": [
            "Conditional boxes 24–25, 27–30, 33–34, organisation-defined fields 36–41, zone marks and extended revision tables are not generated.",
            "Attribute text uses the largest fitting standard height down to 2.5 mm without squeezing glyphs. Recheck fit after editing values or changing fonts; DESIGNATION_TOP edits are independent.",
            "Signature fields are editable text, not handwritten or electronic signatures. Required document information must be completed by the author.",
            "SCALE is title text only, not a model viewport scale; no viewport or model associativity is created. PDF has zero margins; physical printers may clip the bottom supplementary strip."
        ]
    }))
}

#[cfg(test)]
#[path = "sheet_tests.rs"]
mod tests;
