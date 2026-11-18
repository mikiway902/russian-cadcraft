//! GOST 2.106 specification: authoritative records, native paginated tables and position leaders.
//! A4 portrait, 185 mm grid (6/6/8/70/63/10/22), 15 mm header, minimum 8 mm rows.
//! Text-document title forms 2/2a follow GOST R 2.104-2023.
use super::data;
use crate::{
    Result, Session,
    cmd::{CommandSpec, bad},
};
use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, HAlign, Handle, Insert, Layout, Line, PageSetup, Space, TableStyle, Text, VAlign};
use cadcraft_geom::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const CMD: &str = "gost.specification";
const META: &str = "specification";
const PART: &str = "specification-part";
const PAGE: &str = "specification-page";
const POSITION: &str = "specification-position";
const MAX_ENTITIES: usize = 20_000;
const MAX_SOURCES: usize = 256;
const MAX_ROWS: usize = 128;
const MAX_SPECS: usize = 8;
const MAX_POSITIONS: usize = 256;
const SECTIONS: [&str; 9] = [
    "Документация",
    "Комплексы",
    "Сборочные единицы",
    "Детали",
    "Программные изделия и базы данных",
    "Стандартные изделия",
    "Прочие изделия",
    "Материалы",
    "Комплекты",
];
const WIDTHS: [f64; 7] = [6.0, 6.0, 8.0, 70.0, 63.0, 10.0, 22.0];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    section: String,
    designation: String,
    name: String,
    #[serde(default)]
    material: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manual {
    id: String,
    #[serde(flatten)]
    fields: Fields,
    quantity: u32,
    #[serde(default)]
    note: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Linked {
    id: String,
    last: Fields,
    sources: Vec<Handle>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    owner: Handle,
    layout: String,
    linked: Vec<Linked>,
    manual: Vec<Manual>,
    pages: Vec<Handle>,
    #[serde(default)]
    fields: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Part {
    id: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    owner: Handle,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Position {
    specification: Handle,
    target: Handle,
    height: f64,
    shelf: f64,
}
#[derive(Clone, Debug, Serialize)]
struct Row {
    id: String,
    fields: Fields,
    quantity: u32,
    position: Option<u32>,
    sources: Vec<Handle>,
    note: String,
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new(CMD, "GOST Specification", create).menu(&["Format","GOST","GOST Specification"]).params("{layout:string,sources?:[hex handle],manual?:[{id,section,designation,name,material?,quantity,note?}],fields?:{title-block tag:string}}; A4 forms 1/1a, title forms 2/2a; source INSERT attributes DESIGNATION,NAME,SECTION,MATERIAL"),
        CommandSpec::new("gost.specification.edit", "Edit GOST Specification", edit).menu(&["Format","GOST","Edit GOST Specification"]).params("{handle,sources?:[hex handle] (replace links),manual?:[manual row] (replace manual rows),fields?:{title-block tag:string}}"),
        CommandSpec::new("gost.specification.inspect", "Inspect GOST Specification", inspect).params("{handle} → {rows,findings,pages,record,sections}").noundo(),
        CommandSpec::new("gost.position", "GOST Position", position).menu(&["Format","GOST","GOST Position"]).params("{specification:hex,target:source INSERT hex,at:[x,y],height?:5,shelf?:12}; number above horizontal shelf, associative leader"),
        CommandSpec::new("gost.position.edit", "Edit GOST Position", edit_position).menu(&["Format","GOST","Edit GOST Position"]).params("{handle,target?:hex,at?:[x,y],height?:5,shelf?:12}"),
        CommandSpec::new("gost.position.inspect","Inspect GOST Position",inspect_position).params("{handle}").noundo(),
    ]
}
fn err(message: impl Into<String>) -> crate::EngineError {
    bad(CMD, message)
}
fn keys(p: &Value, allowed: &[&str]) -> Result<()> {
    let o = p.as_object().ok_or_else(|| err("parameters must be an object"))?;
    if o.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(err("unknown parameter"));
    }
    Ok(())
}
fn handle(p: &Value, key: &str) -> Result<Handle> {
    p.get(key)
        .and_then(Value::as_str)
        .and_then(Handle::parse_hex)
        .filter(|h| h.0 != 0)
        .ok_or_else(|| err(format!("{key}: nonzero hexadecimal handle required")))
}
fn bounded_text(s: &str, max: usize) -> Result<()> {
    if s.chars().count() > max || s.chars().any(|c| c.is_control() || matches!(c, '\\' | '{' | '}')) {
        return Err(err("text exceeds its limit or contains control/formatting characters"));
    }
    Ok(())
}
fn section(s: &str) -> Result<usize> {
    SECTIONS.iter().position(|v| *v == s).ok_or_else(|| err("unknown specification section"))
}
fn validate_fields(f: &Fields) -> Result<()> {
    section(&f.section)?;
    bounded_text(&f.designation, 40)?;
    bounded_text(&f.name, 80)?;
    bounded_text(&f.material, 40)?;
    if f.name.trim().is_empty() {
        return Err(err("part name is required"));
    }
    Ok(())
}
fn entities(d: &Drawing) -> impl Iterator<Item = &Arc<Entity>> {
    d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
}
fn bound_drawing(d: &Drawing) -> Result<()> {
    if entities(d).take(MAX_ENTITIES + 1).count() > MAX_ENTITIES || d.layouts.len() > 256 || d.blocks.len() > 20_000 {
        return Err(err("drawing exceeds specification work limits (20000 entities/blocks, 256 layouts)"));
    }
    Ok(())
}
fn source_fields(d: &Drawing, e: &Entity) -> Result<Fields> {
    let EntityKind::Insert(i) = &e.kind else {
        return Err(err("source must be a block reference"));
    };
    let b = d.block(&i.block).ok_or_else(|| err("source block definition is unresolved"))?;
    if b.anonymous || i.block.starts_with('*') || i.block.starts_with("GOST-") || b.description.starts_with("GOST ") || i.rows != 1 || i.cols != 1 {
        return Err(err("drawing frames, GOST annotations, anonymous and array INSERTs are not parts"));
    }
    if !i.insert.x.is_finite() || !i.insert.y.is_finite() || !i.insert.z.is_finite() || i.insert.x.abs() > 1e9 || i.insert.y.abs() > 1e9 {
        return Err(err("source coordinates are not finite/bounded"));
    }
    if i.attribs.len() > 128 || b.entities.len() > 2048 {
        return Err(err("source attribute/definition work limit exceeded"));
    }
    let mut values = BTreeMap::<String, String>::new();
    for a in &i.attribs {
        if ["DESIGNATION", "NAME", "MATERIAL", "SECTION"].iter().any(|tag| a.tag.eq_ignore_ascii_case(tag))
            && values.insert(a.tag.to_ascii_uppercase(), a.text.value.clone()).is_some()
        {
            return Err(err("duplicate part attribute"));
        }
    }
    for e in b.entities.iter() {
        if let EntityKind::AttDef(a) = &e.kind
            && a.constant
            && ["DESIGNATION", "NAME", "MATERIAL", "SECTION"].iter().any(|tag| a.tag.eq_ignore_ascii_case(tag))
        {
            values.entry(a.tag.to_ascii_uppercase()).or_insert_with(|| a.text.value.clone());
        }
    }
    let get = |tag: &str| values.get(tag).cloned().ok_or_else(|| err(format!("source requires {tag} attribute")));
    let fields = Fields {
        designation: get("DESIGNATION")?,
        name: get("NAME")?,
        section: get("SECTION")?,
        material: values.get("MATERIAL").cloned().unwrap_or_default(),
    };
    validate_fields(&fields)?;
    Ok(fields)
}
fn manual(p: &Value) -> Result<Vec<Manual>> {
    let Some(v) = p.get("manual") else { return Ok(Vec::new()) };
    let a = v.as_array().ok_or_else(|| err("manual must be an array"))?;
    if a.len() > MAX_ROWS {
        return Err(err("at most 128 manual rows"));
    }
    let mut result = Vec::new();
    let mut ids = BTreeSet::new();
    for v in a {
        // Validate the flattened input explicitly: serde flatten and deny_unknown_fields conflict.
        keys(v, &["id", "section", "designation", "name", "material", "quantity", "note"])?;
        let row: Manual = serde_json::from_value(v.clone()).map_err(|e| err(e.to_string()))?;
        validate_manual(&row)?;
        if !ids.insert(row.id.clone()) {
            return Err(err("manual row IDs must be unique"));
        }
        result.push(row);
    }
    Ok(result)
}
fn validate_manual(r: &Manual) -> Result<()> {
    bounded_text(&r.id, 40)?;
    bounded_text(&r.note, 60)?;
    validate_fields(&r.fields)?;
    if r.id.is_empty() || r.quantity == 0 || r.quantity > 999_999 {
        return Err(err("manual ID and quantity 1..999999 required"));
    }
    Ok(())
}
fn load_record(d: &Drawing, h: Handle) -> Result<Record> {
    let e = d.entity(h).ok_or_else(|| err("specification handle is unresolved"))?;
    let r: Record = data::load(&e.common, META)?.ok_or_else(|| err("not a specification"))?;
    if r.owner != h || r.linked.len() > MAX_ROWS || r.manual.len() > MAX_ROWS || r.pages.len() > 16 || r.pages.first() != Some(&h) {
        return Err(err("invalid/copied specification owner or record limits"));
    }
    bounded_text(&r.layout, 80)?;
    if r.fields.len() > 64 {
        return Err(err("at most 64 title-block fields"));
    }
    for (k, v) in &r.fields {
        bounded_text(k, 64)?;
        bounded_text(v, 255)?;
    }
    if r.layout.is_empty() {
        return Err(err("layout name is empty"));
    }
    let mut ids = BTreeSet::new();
    for link in &r.linked {
        bounded_text(&link.id, 64)?;
        validate_fields(&link.last)?;
        if link.id.is_empty() || link.sources.len() > MAX_SOURCES || !ids.insert(link.id.as_str()) {
            return Err(err("invalid/duplicate linked identity"));
        }
    }
    let mut manual_ids = BTreeSet::new();
    for m in &r.manual {
        validate_manual(m)?;
        if !manual_ids.insert(&m.id) {
            return Err(err("duplicate manual identity"));
        }
    }
    Ok(r)
}
fn bind(d: &mut Drawing, p: &Value) -> Result<Vec<Linked>> {
    let a = p.get("sources").and_then(Value::as_array).ok_or_else(|| err("sources must be an explicit array of block-reference handles"))?;
    if a.len() > MAX_SOURCES {
        return Err(err("at most 256 source references"));
    }
    let mut seen = BTreeSet::new();
    let mut groups = BTreeMap::<Fields, Linked>::new();
    for v in a {
        let h = v.as_str().and_then(Handle::parse_hex).ok_or_else(|| err("source handles must be hexadecimal strings"))?;
        if !seen.insert(h) {
            return Err(err("duplicate source handle"));
        }
        let e = d.entity(h).ok_or_else(|| err("source handle is unresolved"))?;
        let f = source_fields(d, e)?;
        let existing: Option<Part> = data::load(&e.common, PART)?;
        // Other GOST kinds cannot share this app's authoritative metadata section.
        if existing.is_none() && e.common.xdata.iter().any(|t| t.code == 1001 && t.value.eq_ignore_ascii_case("CADCRAFTGOST")) {
            return Err(err("source already owns other GOST metadata"));
        }
        let id = existing.map(|v| v.id).unwrap_or_else(|| format!("part-{}", h.hex()));
        bounded_text(&id, 64)?;
        // Same attributes in the initial selection are one item, independent of block names.
        let group = groups.entry(f.clone()).or_insert_with(|| Linked { id, last: f, sources: Vec::new() });
        group.sources.push(h);
    }
    if groups.len() > MAX_ROWS {
        return Err(err("at most 128 linked identities"));
    }
    let links: Vec<_> = groups.into_values().collect();
    // Never change the identity of an already-linked source to merge unrelated records.
    for l in &links {
        for h in &l.sources {
            let e = d.entity(*h).ok_or_else(|| err("source disappeared"))?;
            if let Some(old) = data::load::<Part>(&e.common, PART)?
                && old.id != l.id
            {
                return Err(err("equal attributes have distinct existing identities; bind each identity in separate specifications"));
            }
            let mut common = e.common.clone();
            data::save(&mut common, PART, &Part { id: l.id.clone() })?;
            d.modify_entity(*h, |e| e.common = common)?;
        }
    }
    Ok(links)
}
fn rows(d: &Drawing, r: &Record) -> Result<(Vec<Row>, Vec<String>)> {
    let ids: BTreeSet<_> = r.linked.iter().map(|l| l.id.as_str()).collect();
    let mut grouped = BTreeMap::<(String, Fields), Vec<Handle>>::new();
    let mut findings = Vec::new();
    let mut count = 0usize;
    for e in entities(d) {
        let Some(part) = data::load::<Part>(&e.common, PART)? else { continue };
        if !ids.contains(part.id.as_str()) {
            continue;
        }
        count += 1;
        if count > MAX_SOURCES {
            return Err(err("linked source count exceeds 256"));
        }
        match source_fields(d, e) {
            Ok(f) => grouped.entry((part.id, f)).or_default().push(e.handle),
            Err(error) => findings.push(format!("Source {} unresolved: {error}", e.handle.hex())),
        }
    }
    let mut out = Vec::new();
    for l in &r.linked {
        let mut found = false;
        for ((id, f), handles) in &grouped {
            if id != &l.id {
                continue;
            }
            found = true;
            out.push(Row {
                id: id.clone(),
                fields: f.clone(),
                quantity: handles.len() as u32,
                position: None,
                sources: handles.clone(),
                note: String::new(),
            });
        }
        if !found {
            findings.push(format!(
                "Part {} has no resolved source (previous handles: {})",
                l.id,
                l.sources.iter().map(|h| h.hex()).collect::<Vec<_>>().join(",")
            ));
            out.push(Row {
                id: l.id.clone(), fields: l.last.clone(), quantity: 0, position: None, sources: Vec::new(), note: "НЕ РАЗРЕШЕНО".into()
            });
        }
    }
    for m in &r.manual {
        out.push(Row {
            id: format!("manual:{}", m.id),
            fields: m.fields.clone(),
            quantity: m.quantity,
            position: None,
            sources: Vec::new(),
            note: m.note.clone(),
        });
    }
    if out.len() > MAX_ROWS {
        return Err(err("at most 128 resulting rows, including attribute variants"));
    }
    out.sort_by(|a, b| {
        let a_section = SECTIONS.iter().position(|v| *v == a.fields.section).unwrap_or(usize::MAX);
        let b_section = SECTIONS.iter().position(|v| *v == b.fields.section).unwrap_or(usize::MAX);
        (a_section, &a.fields.designation, &a.fields.name, &a.fields.material, &a.id).cmp(&(
            b_section,
            &b.fields.designation,
            &b.fields.name,
            &b.fields.material,
            &b.id,
        ))
    });
    let mut number = 0u32;
    for row in &mut out {
        if row.quantity > 0 && row.fields.section != SECTIONS[0] {
            number += 1;
            row.position = Some(number);
        }
    }
    Ok((out, findings))
}
fn text(value: String, at: Vec2, height: f64, rotation: f64) -> Text {
    Text {
        insert: at.to3(0.0),
        align_pt: Some(at.to3(0.0)),
        height,
        value,
        rotation,
        width_factor: 1.0,
        oblique: 0.0,
        style: "GOST".into(),
        halign: HAlign::Center,
        valign: VAlign::Middle,
    }
}
fn block(d: &mut Drawing, name: &str, kinds: Vec<(Common, EntityKind)>) {
    if d.block(name).is_some_and(|b| {
        b.units == 4
            && b.entities.len() == kinds.len()
            && b.entities.iter().zip(&kinds).all(|(e, (common, kind))| e.common == *common && e.kind == *kind)
    }) {
        return;
    }
    let mut b = Block::new(name);
    b.units = 4;
    b.description = "GOST specification native editable geometry".into();
    for (common, kind) in kinds {
        b.entities.push(Entity { handle: d.new_handle(), common, kind });
    }
    d.blocks.insert(name.to_string(), Arc::new(b));
}
fn table_style() -> TableStyle {
    let mut style = TableStyle { name: "GOST-SPECIFICATION".into(), text_height: 3.5, margin: 0.5, ..Default::default() };
    for (name, align) in [("Data", 4), ("Header", 5), ("Centered", 5)] {
        style.set_cell_style(cadcraft_doc::CellStyle {
            name: name.into(),
            text_style: "GOST".into(),
            text_height: 3.5,
            margin_h: 0.5,
            margin_v: 0.5,
            align,
            ..Default::default()
        });
    }
    style
}
fn table_page(d: &Drawing, cells: &[Vec<String>], first: bool) -> Vec<(Common, EntityKind)> {
    let style = table_style();
    let mut t =
        crate::cmd::table::make_table(&style, Vec2::new(20.0, 292.0), cells.len(), 7, Some(8.0), None, None, Some(vec![String::new(); 7]), cells);
    t.col_widths = WIDTHS.to_vec();
    if let Some(h) = t.row_heights.first_mut() {
        *h = 15.0;
    }
    if !first && let Some(h) = t.row_heights.last_mut() {
        *h += 1.0;
    }
    for row in &mut t.cells {
        for cell in row {
            cell.text_height = Some(3.5);
        }
    }
    let headers = ["Формат", "Зона", "Поз.", "Обозначение", "Наименование", "Кол.", "Примечание"];
    let mut result = Vec::new();
    let mut x = 20.0;
    for (n, (header, width)) in headers.into_iter().zip(WIDTHS).enumerate() {
        if n == 6 {
            result.push(EntityKind::Text(text("Приме-".into(), Vec2::new(x + width / 2.0, 287.0), 3.5, 0.0)));
            result.push(EntityKind::Text(text("чание".into(), Vec2::new(x + width / 2.0, 282.0), 3.5, 0.0)));
        } else {
            result.push(EntityKind::Text(text(
                header.into(),
                Vec2::new(x + width / 2.0, 284.5),
                3.5,
                if matches!(n, 0 | 1 | 2 | 5) { std::f64::consts::FRAC_PI_2 } else { 0.0 },
            )));
        }
        x += width;
    }
    let font = d.text_style("GOST").map(cadcraft_render::style_font).unwrap_or_else(|| cadcraft_fonts::TextFont::resolve(cadcraft_fonts::GOST_FONT));
    for (row, cells) in cells.iter().enumerate() {
        if cells.iter().enumerate().all(|(i, s)| i == 4 || s.is_empty())
            && let Some(name) = cells.get(4).filter(|s| !s.is_empty())
        {
            if let Some(cell) = t.cells.get_mut(row + 1).and_then(|r| r.get_mut(4)) {
                cell.style = "Centered".into();
            }
            let w = cadcraft_fonts::text_width(&font, name, 3.5, 1.0).min(61.0);
            let y = 277.0 - row as f64 * 8.0 - 6.0;
            result.push(EntityKind::Line(Line { a: Vec3::new(141.5 - w * 0.5, y, 0.0), b: Vec3::new(141.5 + w * 0.5, y, 0.0) }));
        }
        for column in [2, 5] {
            if let Some(cell) = t.cells.get_mut(row + 1).and_then(|r| r.get_mut(column)) {
                cell.style = "Centered".into();
            }
        }
    }
    result.insert(0, EntityKind::Table(t));
    result
        .into_iter()
        .map(|kind| {
            (Common { layer: if matches!(kind, EntityKind::Text(_)) { "GOST-TEXT" } else { "GOST-THIN" }.into(), ..Default::default() }, kind)
        })
        .collect()
}
fn fitted_lines(value: &str, width: f64, font: &cadcraft_fonts::TextFont) -> Vec<String> {
    if value.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, c) in value.char_indices() {
        let end = index + c.len_utf8();
        if cadcraft_fonts::text_width(font, value.get(start..end).unwrap_or(""), 3.5, 1.0) > width && index > start {
            lines.push(value.get(start..index).unwrap_or("").to_string());
            start = index;
        }
    }
    lines.push(value.get(start..).unwrap_or("").to_string());
    lines
}
fn paginated(d: &Drawing, rows: &[Row]) -> Result<Vec<Vec<Vec<String>>>> {
    let mut pages: Vec<Vec<Vec<String>>> = vec![Vec::new()];
    let mut current_section = String::new();
    let font = d.text_style("GOST").map(cadcraft_render::style_font).unwrap_or_else(|| cadcraft_fonts::TextFont::resolve(cadcraft_fonts::GOST_FONT));
    for row in rows {
        // Match the actual native table font; never squeeze labels to fit a character estimate.
        let designation = fitted_lines(&row.fields.designation, 68.0, &font);
        let name = fitted_lines(&row.fields.name, 61.0, &font);
        let note_text = if row.fields.material.is_empty() {
            row.note.clone()
        } else if row.note.is_empty() {
            row.fields.material.clone()
        } else {
            format!("{}; {}", row.fields.material, row.note)
        };
        let notes = fitted_lines(&note_text, 20.0, &font);
        let lines = designation.len().max(name.len()).max(notes.len());
        let heading = row.fields.section != current_section;
        let headings = fitted_lines(&row.fields.section, 61.0, &font);
        let needed = lines + if heading { headings.len() } else { 0 };
        let limit = if pages.len() == 1 { 29 } else { 32 };
        if needed > limit {
            return Err(err("a specification record cannot fit one page without truncation"));
        }
        if pages.last().is_some_and(|p| p.len() + needed > limit) {
            pages.push(Vec::new());
            current_section.clear();
            if lines + headings.len() > 32 {
                return Err(err("a specification record cannot fit one page without truncation"));
            }
        }
        if let Some(page) = pages.last_mut() {
            if row.fields.section != current_section {
                for heading in &headings {
                    let mut line = vec![String::new(); 7];
                    if let Some(c) = line.get_mut(4) {
                        *c = heading.clone();
                    }
                    page.push(line);
                }
                current_section = row.fields.section.clone();
            }
            for n in 0..lines {
                page.push(vec![
                    String::new(),
                    String::new(),
                    if n == 0 { row.position.map(|n| n.to_string()).unwrap_or_default() } else { String::new() },
                    designation.get(n).cloned().unwrap_or_default(),
                    name.get(n).cloned().unwrap_or_default(),
                    if n == 0 && row.fields.section != "Документация" { row.quantity.to_string() } else { String::new() },
                    notes.get(n).cloned().unwrap_or_default(),
                ]);
            }
        }
    }
    for (index, page) in pages.iter_mut().enumerate() {
        let limit = if index == 0 { 29 } else { 32 };
        if page.len() > limit {
            return Err(err("specification page overflow"));
        }
        page.resize_with(limit, || vec![String::new(); 7]);
    }
    Ok(pages)
}
fn ensure_layout(d: &mut Drawing, name: &str) -> Result<()> {
    if d.layout(name).is_some() {
        return Err(err(format!("layout {name} already exists")));
    }
    if d.layouts.len() >= 256 {
        return Err(err("layout count limit exceeded"));
    }
    let order = d.layouts.iter().map(|l| l.tab_order).max().unwrap_or(0).checked_add(1).ok_or_else(|| err("layout tab order overflow"))?;
    let mut l = Layout::new(name, order);
    l.page = PageSetup {
        paper: "A4".into(),
        width_mm: 210.0,
        height_mm: 297.0,
        margins_mm: [0.0; 4],
        landscape: false,
        scale_to_fit: false,
        scale: 1.0,
        center: false,
        paper_mm: true,
        plot_area: "layout".into(),
        ..PageSetup::default()
    };
    l.limits = Some((Vec2::ZERO, Vec2::new(210.0, 297.0)));
    l.view = Some((Vec2::new(105.0, 148.5), 310.0));
    d.layouts.push(l);
    Ok(())
}
fn insert_page(d: &mut Drawing, owner: Option<Handle>, layout: &str) -> Result<Handle> {
    ensure_layout(d, layout)?;
    let mut c = Common::default();
    if let Some(owner) = owner {
        data::save(&mut c, PAGE, &Page { owner })?;
    }
    Ok(d.add(&Space::Paper(layout.into()), c, EntityKind::Insert(Insert::default()))?)
}
fn save_record(d: &mut Drawing, r: &Record) -> Result<()> {
    let mut common = d.entity(r.owner).ok_or_else(|| err("specification owner missing"))?.common.clone();
    data::save(&mut common, META, r)?;
    d.modify_entity(r.owner, |e| e.common = common)?;
    Ok(())
}
fn rebuild(d: &mut Drawing, r: &mut Record) -> Result<Vec<String>> {
    let (rows, findings) = rows(d, r)?;
    if rows.is_empty() {
        return Err(err("specification must contain at least one linked or manual row"));
    }
    let style = table_style();
    if let Some(existing) = d.table_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&style.name)) {
        if existing != &style {
            return Err(err("GOST-SPECIFICATION table style was changed; restore it or use another document"));
        }
    } else {
        d.table_styles.push(style);
    }
    let pages = paginated(d, &rows)?;
    if pages.len() > 16 {
        return Err(err("at most 16 specification pages"));
    }
    while r.pages.len() < pages.len() {
        let n = r.pages.len() + 1;
        let base = format!("{}-{n}", r.layout);
        let name = if d.layout(&base).is_none() {
            base
        } else {
            (1..=256)
                .map(|suffix| format!("{base}-{suffix}"))
                .find(|name| d.layout(name).is_none())
                .ok_or_else(|| err("no free specification page name"))?
        };
        let h = insert_page(d, Some(r.owner), &name)?;
        r.pages.push(h);
    }
    // Remove only generated surplus inserts; never discard user geometry on those tabs.
    while r.pages.len() > pages.len() {
        let h = r.pages.pop().ok_or_else(|| err("missing surplus page"))?;
        let layout = d.space_of(h).and_then(|s| s.layout().map(str::to_string));
        let block = d.entity(h).and_then(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.block.clone()) } else { None });
        d.remove_entity(h);
        if let Some(block) = block
            && block == format!("GOST-SPEC-{}", h.hex())
            && !entities(d).any(|e| matches!(&e.kind,EntityKind::Insert(i) if i.block==block))
        {
            d.blocks.remove(&block);
        }
        if let Some(layout) = layout
            && d.layout(&layout).is_some_and(|l| l.entities.is_empty())
        {
            d.layouts.retain(|l| l.name != layout);
        }
    }
    for (n, h) in r.pages.iter().enumerate() {
        let e = d.entity(*h).ok_or_else(|| err("specification page was deleted; restore it or recreate the specification"))?;
        let EntityKind::Insert(i) = &e.kind else {
            return Err(err("specification page must remain an INSERT"));
        };
        if i.insert != Vec3::ZERO || i.scale != Vec3::new(1.0, 1.0, 1.0) || i.rotation != 0.0 {
            return Err(err("specification paper grid cannot be transformed off its A4 page"));
        }
        if n > 0 && data::load::<Page>(&e.common, PAGE)?.is_none_or(|p| p.owner != r.owner) {
            return Err(err("specification page ownership is invalid"));
        }
        let sp = d.space_of(*h).ok_or_else(|| err("specification page has no space"))?;
        let Space::Paper(name) = sp else {
            return Err(err("specifications require paper layouts"));
        };
        let l = d.layout(&name).ok_or_else(|| err("specification layout is missing"))?;
        if l.page.width_mm != 210.0 || l.page.height_mm != 297.0 || l.page.landscape || !l.page.paper_mm || l.page.scale != 1.0 || l.page.scale_to_fit
        {
            return Err(err("specification layout must remain A4 portrait, paper millimetres at 1:1"));
        }
        let name = format!("GOST-SPEC-{}", h.hex());
        let mut kinds = pages.get(n).map(|c| table_page(d, c, n == 0)).unwrap_or_default();
        if n < pages.len() {
            kinds.extend(super::sheet::text_document(d, &r.fields, n + 1, pages.len())?);
        }
        // Replacing only this owned block is copy-on-write and leaves all unrelated blocks intact.
        block(d, &name, kinds);
        d.modify_entity(*h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.block = name
            }
        })?;
    }
    for l in &mut r.linked {
        let current: Vec<_> = rows.iter().filter(|row| row.id == l.id && row.quantity > 0).collect();
        if let Some(row) = current.first() {
            l.last = row.fields.clone();
            l.sources = current.iter().flat_map(|r| r.sources.iter().copied()).take(MAX_SOURCES + 1).collect();
            if l.sources.len() > MAX_SOURCES {
                return Err(err("at most 256 linked instances per part identity"));
            }
        }
    }
    save_record(d, r)?;
    Ok(findings)
}
fn atomic(s: &mut Session, f: impl FnOnce(&mut Drawing) -> Result<Value>) -> Result<Value> {
    let before = s.state()?.doc.clone();
    let result = f(s.doc_mut()?);
    if result.is_err() {
        s.state_mut()?.doc = before;
    }
    result
}
fn spec_fields(p: &Value) -> Result<Option<BTreeMap<String, String>>> {
    p.get("fields")
        .map(|v| {
            let o = v.as_object().filter(|o| o.len() <= 64).ok_or_else(|| err("fields must have at most 64 text attributes"))?;
            o.iter()
                .map(|(k, v)| {
                    let v = v.as_str().ok_or_else(|| err("field values must be strings"))?;
                    bounded_text(k, 64)?;
                    bounded_text(v, 255)?;
                    Ok((k.to_ascii_uppercase(), v.to_string()))
                })
                .collect()
        })
        .transpose()
}
fn create(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["layout", "sources", "manual", "fields"])?;
    bound_drawing(s.doc()?)?;
    let layout = p.get("layout").and_then(Value::as_str).ok_or_else(|| err("layout name required"))?.to_string();
    bounded_text(&layout, 80)?;
    if layout.trim().is_empty() || layout.starts_with('*') {
        return Err(err("invalid layout name"));
    }
    let m = manual(p)?;
    let fields = match spec_fields(p)? {
        Some(fields) => fields,
        None => super::book::document_fields(s.doc()?)?,
    };
    let count =
        entities(s.doc()?).try_fold(0usize, |n, e| Ok::<_, crate::EngineError>(n + usize::from(data::load::<Record>(&e.common, META)?.is_some())))?;
    if count >= MAX_SPECS {
        return Err(err("at most eight specifications"));
    }
    atomic(s, |d| {
        let linked = if p.get("sources").is_some() { bind(d, p)? } else { Vec::new() };
        if linked.is_empty() && m.is_empty() {
            return Err(err("explicit source references or manual rows required"));
        }
        let h = insert_page(d, None, &layout)?;
        let mut r = Record { owner: h, layout, linked, manual: m, pages: vec![h], fields };
        rebuild(d, &mut r)?;
        inspect_value(d, h)
    })
}
fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["handle", "sources", "manual", "fields"])?;
    let h = handle(p, "handle")?;
    bound_drawing(s.doc()?)?;
    let mut r = load_record(s.doc()?, h)?;
    if let Some(fields) = spec_fields(p)? {
        r.fields = fields;
    }
    let m = if p.get("manual").is_some() { Some(manual(p)?) } else { None };
    atomic(s, |d| {
        if let Some(m) = m {
            r.manual = m;
        }
        if p.get("sources").is_some() {
            r.linked = bind(d, p)?;
        }
        rebuild(d, &mut r)?;
        refresh_positions(d)?;
        inspect_value(d, h)
    })
}
fn inspect_value(d: &Drawing, h: Handle) -> Result<Value> {
    let r = load_record(d, h)?;
    let (rows, mut findings) = rows(d, &r)?;
    for e in entities(d) {
        if let Some(p) = data::load::<Position>(&e.common, POSITION)?
            && p.specification == h
            && let Some(f) = position_finding(d, &p, &rows)
        {
            findings.push(format!("Position {}: {f}", e.handle.hex()));
        }
    }
    Ok(
        json!({"handle":h.hex(),"rows":rows,"findings":findings,"pages":r.pages.iter().map(|h|h.hex()).collect::<Vec<_>>(),"record":r,"sections":SECTIONS,"columnWidths":WIDTHS}),
    )
}
fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["handle"])?;
    bound_drawing(s.doc()?)?;
    inspect_value(s.doc()?, handle(p, "handle")?)
}
fn point(p: &Value, key: &str) -> Result<Vec2> {
    let a = p.get(key).and_then(Value::as_array).filter(|a| a.len() == 2).ok_or_else(|| err(format!("{key}: [x,y] required")))?;
    let number = |n: usize| {
        a.get(n).and_then(Value::as_f64).filter(|n| n.is_finite() && n.abs() <= 1e9).ok_or_else(|| err("finite coordinates within +/-1e9 required"))
    };
    Ok(Vec2::new(number(0)?, number(1)?))
}
fn size(p: &Value, key: &str, default: f64, min: f64, max: f64) -> Result<f64> {
    match p.get(key) {
        None => Ok(default),
        Some(v) => v.as_f64().filter(|n| n.is_finite() && *n >= min && *n <= max).ok_or_else(|| err(format!("{key}: finite {min}..{max} required"))),
    }
}
fn validate_position(p: &Position) -> Result<()> {
    if !p.height.is_finite() || !p.shelf.is_finite() || ![5.0, 7.0, 10.0, 14.0].contains(&p.height) || !(8.0..=60.0).contains(&p.shelf) {
        return Err(err("position height must be 5,7,10,14; shelf 8..60 mm"));
    }
    Ok(())
}
fn position_finding(d: &Drawing, p: &Position, rows: &[Row]) -> Option<String> {
    if d.entity(p.target).is_none() {
        return Some(format!("target {} deleted/unresolved", p.target.hex()));
    }
    if !rows.iter().any(|r| r.sources.contains(&p.target) && r.position.is_some()) {
        return Some(format!("target {} has no numbered specification row", p.target.hex()));
    }
    None
}
fn render_position(d: &mut Drawing, h: Handle, p: &Position, rows: &[Row]) -> Result<Option<String>> {
    validate_position(p)?;
    let e = d.entity(h).ok_or_else(|| err("position entity missing"))?;
    let EntityKind::Insert(i) = &e.kind else {
        return Err(err("position must remain an INSERT"));
    };
    if !i.insert.x.is_finite()
        || !i.insert.y.is_finite()
        || i.insert.x.abs() > 1e9
        || i.insert.y.abs() > 1e9
        || !i.rotation.is_finite()
        || !i.scale.x.is_finite()
        || !i.scale.y.is_finite()
        || i.scale.x.abs() < 1e-9
        || i.scale.y.abs() < 1e-9
        || i.scale.x.abs() > 1e6
        || i.scale.y.abs() > 1e6
    {
        return Err(err("invalid position transform"));
    }
    let mut finding = position_finding(d, p, rows);
    if d.entity(p.target).is_some() && d.space_of(h) != d.space_of(p.target) {
        finding = Some("target and leader are in different spaces".into());
    }
    let label = if finding.is_none() {
        rows.iter().find(|r| r.sources.contains(&p.target)).and_then(|r| r.position).map(|n| n.to_string()).unwrap_or_else(|| "?".into())
    } else {
        "?".into()
    };
    let font = d.text_style("GOST").map(cadcraft_render::style_font).unwrap_or_else(|| cadcraft_fonts::TextFont::resolve(cadcraft_fonts::GOST_FONT));
    if cadcraft_fonts::text_width(&font, &label, p.height, 1.0) > p.shelf - 2.0 {
        return Err(err("position label does not fit its shelf"));
    }
    let mut kinds = vec![
        EntityKind::Line(Line { a: Vec3::new(-p.shelf * 0.5, 0.0, 0.0), b: Vec3::new(p.shelf * 0.5, 0.0, 0.0) }),
        EntityKind::Text(text(label, Vec2::new(0.0, 1.0 + p.height * 0.5), p.height, 0.0)),
    ];
    if finding.is_none()
        && let Some(source) = d.entity(p.target)
        && let EntityKind::Insert(target) = &source.kind
    {
        let relative = (target.insert.xy() - i.insert.xy()).rotate(-i.rotation);
        let local = Vec2::new(relative.x / i.scale.x, relative.y / i.scale.y);
        if !local.x.is_finite() || !local.y.is_finite() || local.x.abs() > 1e12 || local.y.abs() > 1e12 {
            return Err(err("position leader exceeds coordinate limits"));
        }
        kinds.push(EntityKind::Line(Line { a: local.to3(0.0), b: Vec3::new(-p.shelf * 0.5, 0.0, 0.0) }));
        kinds.push(EntityKind::Hatch(cadcraft_doc::Hatch {
            pattern: "SOLID".into(),
            solid: true,
            loops: vec![cadcraft_doc::HatchLoop {
                vertices: vec![
                    cadcraft_geom::PolyVertex::with_bulge(local - Vec2::new(0.5, 0.0), 1.0),
                    cadcraft_geom::PolyVertex::with_bulge(local + Vec2::new(0.5, 0.0), 1.0),
                ],
                outer: true,
                sources: Vec::new(),
            }],
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style: 0,
            elevation: 0.0,
            gradient: None,
            origin: local,
            background: None,
            pattern_lines: Vec::new(),
        }));
    }
    let name = format!("GOST-POS-{}", h.hex());
    block(
        d,
        &name,
        kinds
            .into_iter()
            .map(|kind| {
                (Common { layer: if matches!(kind, EntityKind::Text(_)) { "GOST-TEXT" } else { "GOST-THIN" }.into(), ..Default::default() }, kind)
            })
            .collect(),
    );
    let mut common = d.entity(h).ok_or_else(|| err("position missing"))?.common.clone();
    data::save(&mut common, POSITION, p)?;
    d.modify_entity(h, |e| {
        e.common = common;
        if let EntityKind::Insert(i) = &mut e.kind {
            i.block = name;
        }
    })?;
    Ok(finding)
}
fn inspect_position(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["handle"])?;
    let h = handle(p, "handle")?;
    let e = s.doc()?.entity(h).ok_or_else(|| err("position missing"))?;
    let r: Position = data::load(&e.common, POSITION)?.ok_or_else(|| err("not a position"))?;
    validate_position(&r)?;
    let EntityKind::Insert(i) = &e.kind else { return Err(err("position must be a block reference")) };
    Ok(
        json!({"handle":h.hex(),"specification":r.specification.hex(),"target":r.target.hex(),"height":r.height,"shelf":r.shelf,"at":[i.insert.x,i.insert.y]}),
    )
}
fn position(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["specification", "target", "at", "height", "shelf"])?;
    bound_drawing(s.doc()?)?;
    let at = point(p, "at")?;
    let q = Position {
        specification: handle(p, "specification")?,
        target: handle(p, "target")?,
        height: size(p, "height", 5.0, 5.0, 14.0)?,
        shelf: size(p, "shelf", 12.0, 8.0, 60.0)?,
    };
    validate_position(&q)?;
    let r = load_record(s.doc()?, q.specification)?;
    let (rows, _) = rows(s.doc()?, &r)?;
    if let Some(f) = position_finding(s.doc()?, &q, &rows) {
        return Err(err(f));
    }
    if s.doc()?.space_of(q.target) != Some(s.space()) {
        return Err(err("position and source must share the current editing space"));
    }
    let count = entities(s.doc()?)
        .try_fold(0usize, |n, e| Ok::<_, crate::EngineError>(n + usize::from(data::load::<Position>(&e.common, POSITION)?.is_some())))?;
    if count >= MAX_POSITIONS {
        return Err(err("at most 256 positions"));
    }
    let sp = s.space();
    atomic(s, |d| {
        let h = d.add(&sp, Common::default(), EntityKind::Insert(Insert { insert: at.to3(0.0), ..Insert::default() }))?;
        render_position(d, h, &q, &rows)?;
        Ok(json!({"handle":h.hex(),"parameters":q,"findings":[]}))
    })
}
fn edit_position(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["handle", "target", "at", "height", "shelf"])?;
    bound_drawing(s.doc()?)?;
    let h = handle(p, "handle")?;
    let mut q: Position =
        data::load(&s.doc()?.entity(h).ok_or_else(|| err("position unresolved"))?.common, POSITION)?.ok_or_else(|| err("not a position"))?;
    if p.get("target").is_some() {
        q.target = handle(p, "target")?;
    }
    q.height = size(p, "height", q.height, 5.0, 14.0)?;
    q.shelf = size(p, "shelf", q.shelf, 8.0, 60.0)?;
    validate_position(&q)?;
    let at = if p.get("at").is_some() { Some(point(p, "at")?) } else { None };
    let r = load_record(s.doc()?, q.specification)?;
    let (rows, _) = rows(s.doc()?, &r)?;
    if p.get("target").is_some()
        && let Some(f) = position_finding(s.doc()?, &q, &rows)
    {
        return Err(err(f));
    }
    atomic(s, |d| {
        if let Some(at) = at {
            d.modify_entity(h, |e| {
                if let EntityKind::Insert(i) = &mut e.kind {
                    i.insert = at.to3(0.0)
                }
            })?;
        }
        let finding = render_position(d, h, &q, &rows)?;
        Ok(json!({"handle":h.hex(),"parameters":q,"findings":finding.into_iter().collect::<Vec<_>>()}))
    })
}
fn refresh_positions(d: &mut Drawing) -> Result<Vec<String>> {
    let mut positions = Vec::new();
    for e in entities(d) {
        if let Some(p) = data::load::<Position>(&e.common, POSITION)? {
            positions.push((e.handle, p));
            if positions.len() > MAX_POSITIONS {
                return Err(err("position count exceeds 256"));
            }
        }
    }
    let mut cache = BTreeMap::new();
    let mut findings = Vec::new();
    for (h, p) in positions {
        if let std::collections::btree_map::Entry::Vacant(entry) = cache.entry(p.specification) {
            let rows = if d.entity(p.specification).is_some() {
                let r = load_record(d, p.specification)?;
                rows(d, &r)?.0
            } else {
                Vec::new()
            };
            entry.insert(rows);
        }
        let rows = cache.get(&p.specification).ok_or_else(|| err("position rows unavailable"))?;
        if let Some(f) = render_position(d, h, &p, rows)? {
            findings.push(format!("Position {}: {f}", h.hex()));
        }
    }
    Ok(findings)
}
/// Copy a part+callout as a new pair. A callout copied alone keeps its original source.
/// Resolve only an unambiguous same-command translation, never nearest geometry.
fn remap_copied_positions(d: &mut Drawing, before: &Drawing) -> Result<()> {
    let mut changes = Vec::new();
    for e in entities(d) {
        if before.entity(e.handle).is_some() {
            continue;
        }
        let EntityKind::Insert(insert) = &e.kind else { continue };
        let Some(old) = insert.block.strip_prefix("GOST-POS-").and_then(Handle::parse_hex).and_then(|h| before.entity(h)) else { continue };
        let EntityKind::Insert(old_insert) = &old.kind else { continue };
        let Some(mut position) = data::load::<Position>(&e.common, POSITION)? else { continue };
        let Some(source) = before.entity(position.target) else { continue };
        let EntityKind::Insert(source_insert) = &source.kind else { continue };
        let Some(part) = data::load::<Part>(&source.common, PART)? else { continue };
        let delta = insert.insert - old_insert.insert;
        let mut target = None;
        for candidate in entities(d) {
            if before.entity(candidate.handle).is_some() {
                continue;
            }
            let EntityKind::Insert(i) = &candidate.kind else { continue };
            if i.block != source_insert.block
                || i.rotation != source_insert.rotation
                || i.scale != source_insert.scale
                || (i.insert - source_insert.insert - delta).len() > 1e-8
                || d.space_of(candidate.handle) != d.space_of(e.handle)
            {
                continue;
            }
            if data::load::<Part>(&candidate.common, PART)?.is_some_and(|p| p.id == part.id) && target.replace(candidate.handle).is_some() {
                return Err(err("copied position has ambiguous copied source parts"));
            }
        }
        if let Some(target) = target {
            position.target = target;
            let mut common = e.common.clone();
            data::save(&mut common, POSITION, &position)?;
            changes.push((e.handle, common));
        }
        if changes.len() > MAX_POSITIONS {
            return Err(err("copied position count exceeds 256"));
        }
    }
    for (h, common) in changes {
        d.modify_entity(h, |e| e.common = common)?;
    }
    Ok(())
}

/// Run within the caller's original command transaction; errors roll the whole mutation back.
pub(super) fn after_command(s: &mut Session, before: Option<&Arc<Drawing>>, uid: Option<u64>) -> Result<()> {
    let Some(before) = before else { return Ok(()) };
    let state = s.state()?;
    if uid.is_some_and(|u| u != state.uid) || Arc::ptr_eq(before, &state.doc) {
        return Ok(());
    }
    // No metadata scans/work for drawings with no managed specifications or leaders.
    let d = s.doc()?;
    let managed = entities(d).any(|e| matches!(&e.kind,EntityKind::Insert(i) if i.block.starts_with("GOST-SPEC-")||i.block.starts_with("GOST-POS-")));
    if !managed {
        return Ok(());
    }
    bound_drawing(d)?;
    // Unrelated drafting edits must not allocate new table blocks or consume handles.
    let mut source_changed = false;
    let mut position_changed = false;
    for e in entities(d) {
        let EntityKind::Insert(i) = &e.kind else { continue };
        let prior = before.entity(e.handle);
        if i.block.starts_with("GOST-SPEC-") && prior.is_none() && data::load::<Record>(&e.common, META)?.is_some() {
            let _ = load_record(d, e.handle)?; // reject copied authoritative owners
        }
        if i.block.starts_with("GOST-POS-") {
            position_changed |= prior.is_none_or(|old| !Arc::ptr_eq(old, e));
        } else if !i.block.starts_with("GOST-") && data::load::<Part>(&e.common, PART)?.is_some() {
            let already_linked = prior.is_some_and(|old| data::load::<Part>(&old.common, PART).is_ok_and(|p| p.is_some()));
            source_changed |= (prior.is_none() || already_linked)
                && (prior.is_none_or(|old| !Arc::ptr_eq(old, e))
                    || match (d.block(&i.block), before.block(&i.block)) {
                        (Some(new), Some(old)) => new != old,
                        _ => true,
                    });
        }
    }
    for e in entities(before) {
        if let EntityKind::Insert(i) = &e.kind
            && !i.block.starts_with("GOST-")
            && d.entity(e.handle).is_none()
            && data::load::<Part>(&e.common, PART)?.is_some()
        {
            source_changed = true;
        }
    }
    if !source_changed && !position_changed {
        return Ok(());
    }
    let mut records = Vec::new();
    for e in entities(d) {
        if data::load::<Record>(&e.common, META)?.is_some() {
            records.push(load_record(d, e.handle)?);
            if records.len() > MAX_SPECS {
                return Err(err("specification count exceeds eight"));
            }
        }
    }
    let mut findings = Vec::new();
    if source_changed {
        for mut r in records {
            if let Some(Space::Paper(name)) = s.doc()?.space_of(r.owner) {
                r.layout = name;
            }
            findings.extend(rebuild(s.doc_mut()?, &mut r)?);
        }
    }
    remap_copied_positions(s.doc_mut()?, before)?;
    findings.extend(refresh_positions(s.doc_mut()?)?);
    findings.sort();
    findings.dedup();
    for finding in findings {
        s.echo(finding);
    }
    Ok(())
}

#[cfg(test)]
#[path = "specification_tests.rs"]
mod tests;
