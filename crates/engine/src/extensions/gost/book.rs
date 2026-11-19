//! Managed drawing books. Metadata rides on each native frame so deleting the first sheet
//! cannot discard common or temporarily hidden first-form fields.
use super::{data, sheet};
use crate::{
    Result, Session,
    cmd::{CommandSpec, bad},
};
use cadcraft_doc::{Common, Drawing, EntityKind, Handle};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

const KIND: &str = "sheet";
const MAX_SHEETS: usize = 1000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Record {
    automatic: bool,
    fields: BTreeMap<String, String>,
}

struct Frame {
    layout: String,
    handle: Handle,
    common: Common,
    format: String,
    landscape: bool,
    first: bool,
    stored: Option<Record>,
    record: Record,
    actual: BTreeMap<String, String>,
}

struct Change {
    layout: String,
    handle: Handle,
    common: Common,
    record: Record,
    format: String,
    landscape: bool,
    first: bool,
    rebuild: bool,
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("gost.document", "GOST Document Properties", document)
            .menu(&["Format", "GOST", "GOST Document Properties"])
            .params("{fields?: {TAG: string}}; get/set common drawing-book properties, synchronizing managed title blocks"),
        CommandSpec::new("gost.sheet.set", "GOST Sheet Properties", set_sheet)
            .menu(&["Format", "GOST", "GOST Sheet Properties"])
            .params("{layout?: current paper tab, format?: A4|A3|A2|A1|A0, landscape?: bool, fields?: {TAG:string}, automatic?: bool}; preserve user geometry and hidden fields"),
        CommandSpec::new("gost.book.sync", "Synchronize GOST Sheets", synchronize_command)
            .menu(&["Format", "GOST", "Synchronize GOST Sheets"])
            .params("{}; synchronize numbering, total, first/continuation forms and common fields"),
        CommandSpec::new("gost.sheet.order", "Reorder GOST Sheet", order_sheet)
            .params("{layout:string, index:positive one-based paper-tab index}; reorder and renumber the managed book"),
    ]
}

pub(super) fn automatic(p: &Value) -> Result<bool> {
    match p.get("automatic") {
        Some(v) => v.as_bool().ok_or_else(|| bad("gost.sheet", "automatic must be boolean")),
        // Explicit forms also support independent sheets/documents sharing a DXF file.
        None => Ok(p.get("form").is_none()),
    }
}

fn shared(tag: &str) -> bool {
    matches!(tag, "DESIGNATION" | "TITLE" | "MATERIAL" | "ORG" | "MASS" | "VERSION" | "LITERA1" | "LITERA2" | "LITERA3")
        || ["DEVELOPER", "CHECKER", "TECH_CONTROL", "EXTRA_ROLE", "NORM_CONTROL", "APPROVER"]
            .iter()
            .any(|r| tag == *r || tag.strip_prefix(r).is_some_and(|s| s == "_SIGNATURE" || s == "_DATE"))
}

fn fields(p: &Value) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    if let Some(v) = p.get("fields") {
        let object = v.as_object().ok_or_else(|| bad("gost.document", "fields must be an object"))?;
        if object.len() > 128 {
            return Err(bad("gost.document", "at most 128 fields"));
        }
        for (tag, v) in object {
            let value = v.as_str().ok_or_else(|| bad("gost.document", "field values must be strings"))?;
            if tag.len() > 64 || value.chars().count() > 255 || value.chars().any(char::is_control) {
                return Err(bad("gost.document", "field names or values exceed limits, or contain control characters"));
            }
            if result.insert(tag.to_ascii_uppercase(), value.to_string()).is_some() {
                return Err(bad("gost.document", "duplicate case-insensitive field"));
            }
        }
    }
    Ok(result)
}

fn frames(d: &Drawing) -> Result<Vec<Frame>> {
    let mut layouts: Vec<_> = d.layouts.iter().collect();
    layouts.sort_by_key(|l| l.tab_order);
    let mut result = Vec::new();
    for l in layouts {
        let mut found = 0;
        for e in l.entities.iter() {
            let EntityKind::Insert(i) = &e.kind else { continue };
            if !i.block.starts_with("GOST-SHEET-") {
                continue;
            }
            let b = d.block(&i.block).ok_or_else(|| bad("gost.book.sync", "native frame block is missing"))?;
            if !b.description.starts_with("GOST R 2.104-2023 form ") {
                continue;
            }
            found += 1;
            if found > 1 {
                return Err(bad("gost.book.sync", "a managed sheet must contain exactly one native frame"));
            }
            if result.len() >= MAX_SHEETS {
                return Err(bad("gost.book.sync", "at most 1000 managed or independent sheets"));
            }
            let mut actual = BTreeMap::new();
            for a in &i.attribs {
                let key = a.tag.to_ascii_uppercase();
                if actual.insert(key, a.text.value.clone()).is_some() {
                    return Err(bad("gost.book.sync", "duplicate native frame attribute"));
                }
            }
            let stored: Option<Record> = data::load(&e.common, KIND)?;
            let record = stored.clone().unwrap_or_else(|| Record { automatic: true, fields: actual.clone() });
            let format = [("A4", 210.0, 297.0), ("A3", 297.0, 420.0), ("A2", 420.0, 594.0), ("A1", 594.0, 841.0), ("A0", 841.0, 1189.0)]
                .into_iter()
                .find(|(_, w, h)| (l.page.width_mm - w).abs() < 1e-6 && (l.page.height_mm - h).abs() < 1e-6)
                .map(|(name, _, _)| name.to_string())
                .ok_or_else(|| bad("gost.book.sync", "managed sheets require a basic A4–A0 paper format"))?;
            if format == "A4" && l.page.landscape {
                return Err(bad("gost.book.sync", "A4 must be portrait"));
            }
            result.push(Frame {
                layout: l.name.clone(),
                handle: e.handle,
                common: e.common.clone(),
                format,
                landscape: l.page.landscape,
                first: b.description.starts_with("GOST R 2.104-2023 form 1,"),
                stored,
                record,
                actual,
            });
        }
    }
    Ok(result)
}

pub(super) fn managed_count(d: &Drawing) -> Result<usize> {
    Ok(frames(d)?.iter().filter(|f| f.record.automatic).count())
}

fn common_fields(fs: &[Frame]) -> BTreeMap<String, String> {
    fs.iter()
        .find(|f| f.record.automatic)
        .map(|f| f.record.fields.iter().filter(|(k, _)| shared(k)).map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}
pub(in crate::extensions) fn document_fields(d: &Drawing) -> Result<BTreeMap<String, String>> {
    Ok(common_fields(&frames(d)?))
}

fn frame_fits(d: &Drawing, handle: Handle, landscape: bool, format: &str) -> bool {
    let Some(e) = d.entity(handle) else { return false };
    let EntityKind::Insert(i) = &e.kind else { return false };
    let Some(block) = d.block(&i.block) else { return false };
    let Some((_, w, h)) = [("A4", 210.0, 297.0), ("A3", 297.0, 420.0), ("A2", 420.0, 594.0), ("A1", 594.0, 841.0), ("A0", 841.0, 1189.0)]
        .into_iter()
        .find(|(name, _, _)| *name == format)
    else {
        return false;
    };
    let (w, h) = if landscape { (h, w) } else { (w, h) };
    let corners = [(20.0, 5.0), (w - 5.0, 5.0), (w - 5.0, h - 5.0), (20.0, h - 5.0)];
    let near = |p: cadcraft_geom::Vec3, q: (f64, f64)| (p.x - q.0).abs() < 1e-6 && (p.y - q.1).abs() < 1e-6;
    corners.iter().zip(corners.iter().cycle().skip(1)).all(|(&a, &b)| {
        block.entities.iter().any(|e| {
            e.common.layer == "GOST-FRAME" && matches!(&e.kind, EntityKind::Line(l) if (near(l.a,a)&&near(l.b,b))||(near(l.a,b)&&near(l.b,a)))
        })
    })
}
fn plan(d: &Drawing, updates: &BTreeMap<String, String>) -> Result<Vec<Change>> {
    let fs = frames(d)?;
    let mut common = common_fields(&fs);
    common.extend(updates.iter().map(|(k, v)| (k.clone(), v.clone())));
    let total = fs.iter().filter(|f| f.record.automatic).count().to_string();
    let mut number = 0usize;
    let mut changes = Vec::new();
    for f in fs {
        if !f.record.automatic {
            continue;
        }
        number += 1;
        let mut record = f.record;
        record.fields.extend(f.actual.iter().map(|(k, v)| (k.clone(), v.clone())));
        record.fields.extend(common.iter().map(|(k, v)| (k.clone(), v.clone())));
        record.fields.insert("SHEET".into(), number.to_string());
        record.fields.insert("SHEETS".into(), total.clone());
        record.fields.insert("FORMAT".into(), f.format.clone());
        if let Some(value) = record.fields.get("DESIGNATION").cloned() {
            record.fields.insert("DESIGNATION_TOP".into(), value);
        }
        let first = number == 1;
        let description_matches = d
            .entity(f.handle)
            .and_then(|e| if let EntityKind::Insert(i) = &e.kind { d.block(&i.block) } else { None })
            .is_some_and(|b| b.description == format!("GOST R 2.104-2023 form {}, {}", if first { "1" } else { "2a" }, f.format));
        let rebuild = first != f.first
            || !description_matches
            || !frame_fits(d, f.handle, f.landscape, &f.format)
            || f.actual.iter().any(|(k, v)| record.fields.get(k) != Some(v));
        if !rebuild && f.stored.as_ref() == Some(&record) {
            continue;
        }
        let mut common = f.common;
        data::save(&mut common, KIND, &record)?;
        changes.push(Change { layout: f.layout, handle: f.handle, common, record, format: f.format, landscape: f.landscape, first, rebuild });
    }
    Ok(changes)
}

fn apply(d: &mut Drawing, changes: Vec<Change>) -> Result<()> {
    for c in changes {
        if c.rebuild {
            sheet::rebuild(d, &c.layout, c.handle, c.record.fields, c.first, &c.format, c.landscape)?;
        }
        d.modify_entity(c.handle, |e| e.common = c.common)?;
    }
    Ok(())
}

fn sync(s: &mut Session, updates: &BTreeMap<String, String>) -> Result<()> {
    let changes = plan(s.doc()?, updates)?;
    if !changes.is_empty() {
        apply(s.doc_mut()?, changes)?;
    }
    Ok(())
}

fn atomic(s: &mut Session, f: impl FnOnce(&mut Session) -> Result<Value>) -> Result<Value> {
    let before = s.state()?.doc.clone();
    let result = f(s);
    if result.is_err() {
        s.state_mut()?.doc = before;
    }
    result
}

pub(super) fn created(s: &mut Session, handle: Handle, p: &Value) -> Result<()> {
    let e = s.doc()?.entity(handle).ok_or_else(|| bad("gost.sheet", "frame missing"))?;
    let EntityKind::Insert(i) = &e.kind else { return Err(bad("gost.sheet", "frame is not an insert")) };
    let record = Record { automatic: automatic(p)?, fields: i.attribs.iter().map(|a| (a.tag.to_ascii_uppercase(), a.text.value.clone())).collect() };
    let mut common = e.common.clone();
    data::save(&mut common, KIND, &record)?;
    s.doc_mut()?.modify_entity(handle, |e| e.common = common)?;
    let updates = if record.automatic { fields(p)?.into_iter().filter(|(k, _)| shared(k)).collect() } else { BTreeMap::new() };
    sync(s, &updates)
}

fn inspect(d: &Drawing) -> Result<Value> {
    let fs = frames(d)?;
    Ok(json!({"fields":common_fields(&fs),"sheets":fs.iter().map(|f|json!({
        "layout":f.layout,"handle":f.handle.hex(),"format":f.format,"landscape":f.landscape,
        "form":if f.first {"1"} else {"2a"},"automatic":f.record.automatic,"fields":f.actual
    })).collect::<Vec<_>>()}))
}

fn document(s: &mut Session, p: &Value) -> Result<Value> {
    let object = p.as_object().ok_or_else(|| bad("gost.document", "parameters must be an object"))?;
    if object.keys().any(|k| k != "fields") {
        return Err(bad("gost.document", "unknown parameter"));
    }
    let updates = fields(p)?;
    if updates.keys().any(|k| !shared(k)) {
        return Err(bad("gost.document", "only common document fields can be changed here"));
    }
    if p.get("fields").is_none() {
        return inspect(s.doc()?);
    }
    atomic(s, |s| {
        if managed_count(s.doc()?)? == 0 {
            return Err(bad("gost.document", "no automatic native GOST sheets"));
        }
        sync(s, &updates)?;
        inspect(s.doc()?)
    })
}

fn synchronize_command(s: &mut Session, p: &Value) -> Result<Value> {
    if !p.as_object().is_some_and(|o| o.is_empty()) {
        return Err(bad("gost.book.sync", "parameters must be an empty object"));
    }
    atomic(s, |s| {
        sync(s, &BTreeMap::new())?;
        inspect(s.doc()?)
    })
}

fn order_sheet(s: &mut Session, p: &Value) -> Result<Value> {
    let o = p.as_object().ok_or_else(|| bad("gost.sheet.order", "parameters must be an object"))?;
    if o.keys().any(|k| !matches!(k.as_str(), "layout" | "index")) {
        return Err(bad("gost.sheet.order", "unknown parameter"));
    }
    let name = p.get("layout").and_then(Value::as_str).ok_or_else(|| bad("gost.sheet.order", "layout required"))?;
    if s.doc()?.layouts.len() > MAX_SHEETS {
        return Err(bad("gost.sheet.order", "at most 1000 paper tabs"));
    }
    let mut layouts: Vec<_> = s.doc()?.layouts.iter().collect();
    layouts.sort_by_key(|l| l.tab_order);
    let from = layouts.iter().position(|l| l.name.eq_ignore_ascii_case(name)).ok_or_else(|| bad("gost.sheet.order", "layout not found"))?;
    let index = p
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|i| usize::try_from(i).ok())
        .filter(|i| *i > 0 && *i <= layouts.len())
        .ok_or_else(|| bad("gost.sheet.order", "index must address an existing paper tab"))?;
    let mut names: Vec<_> = layouts.iter().map(|l| l.name.clone()).collect();
    let name = names.remove(from);
    names.insert(index - 1, name);
    atomic(s, move |s| {
        for (i, name) in names.iter().enumerate() {
            if let Some(l) = s.doc_mut()?.layouts.iter_mut().find(|l| l.name == *name) {
                l.tab_order = u32::try_from(i + 1).map_err(|_| bad("gost.sheet.order", "tab order exceeds supported range"))?;
            }
        }
        sync(s, &BTreeMap::new())?;
        inspect(s.doc()?)
    })
}

fn set_sheet(s: &mut Session, p: &Value) -> Result<Value> {
    let o = p.as_object().ok_or_else(|| bad("gost.sheet.set", "parameters must be an object"))?;
    if o.keys().any(|k| !matches!(k.as_str(), "layout" | "format" | "landscape" | "fields" | "automatic")) {
        return Err(bad("gost.sheet.set", "unknown parameter"));
    }
    let name = match p.get("layout") {
        Some(v) => v.as_str().ok_or_else(|| bad("gost.sheet.set", "layout must be a string"))?.to_string(),
        None => s.space().layout().ok_or_else(|| bad("gost.sheet.set", "layout required in Model space"))?.to_string(),
    };
    let fs = frames(s.doc()?)?;
    let mut frame = fs.into_iter().find(|f| f.layout.eq_ignore_ascii_case(&name)).ok_or_else(|| bad("gost.sheet.set", "native sheet not found"))?;
    let updates = fields(p)?;
    if let Some(v) = p.get("automatic") {
        frame.record.automatic = v.as_bool().ok_or_else(|| bad("gost.sheet.set", "automatic must be boolean"))?;
    }
    for tag in updates.keys() {
        if frame.record.automatic && matches!(tag.as_str(), "SHEET" | "SHEETS" | "FORMAT" | "DESIGNATION_TOP") {
            return Err(bad("gost.sheet.set", "numbering, format and top designation are derived in automatic mode"));
        }
        if !shared(tag) && !frame.record.fields.contains_key(tag) && !frame.actual.contains_key(tag) {
            return Err(bad("gost.sheet.set", format!("unknown field {tag}")));
        }
    }
    let format = match p.get("format") {
        Some(v) => v.as_str().ok_or_else(|| bad("gost.sheet.set", "format must be a string"))?.to_ascii_uppercase(),
        None => frame.format.clone(),
    };
    let landscape = match p.get("landscape") {
        Some(v) => v.as_bool().ok_or_else(|| bad("gost.sheet.set", "landscape must be boolean"))?,
        None => frame.landscape,
    };
    atomic(s, move |s| {
        frame.record.fields.extend(frame.actual);
        frame.record.fields.extend(updates.clone());
        frame.record.fields.insert("FORMAT".into(), format.clone());
        let common_updates = if frame.record.automatic { updates.into_iter().filter(|(k, _)| shared(k)).collect() } else { BTreeMap::new() };
        sheet::rebuild(s.doc_mut()?, &frame.layout, frame.handle, frame.record.fields.clone(), frame.first, &format, landscape)?;
        let mut common = frame.common;
        data::save(&mut common, KIND, &frame.record)?;
        s.doc_mut()?.modify_entity(frame.handle, |e| e.common = common)?;
        sync(s, &common_updates)?;
        inspect(s.doc()?)
    })
}

/// Run only after a successful mutation of this same document. Ordinary drawing operations
/// do not scan/parse sheet metadata; only changed sheet structure or frame attributes do.
pub(super) fn after_command(s: &mut Session, before: Option<&Arc<Drawing>>, uid: Option<u64>) -> Result<()> {
    let Some(before) = before else { return Ok(()) };
    let st = s.state()?;
    if uid.is_some_and(|u| u != st.uid) || Arc::ptr_eq(before, &st.doc) {
        return Ok(());
    }
    let d = s.doc()?;
    let mut relevant = d.layouts.len() != before.layouts.len();
    let mut updates = BTreeMap::new();
    for l in &d.layouts {
        let old = before.layout(&l.name);
        relevant |= old.is_none_or(|o| o.tab_order != l.tab_order || o.page != l.page);
        relevant |= old.is_some_and(|old| {
            old.entities
                .iter()
                .any(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.starts_with("GOST-SHEET-")) && !l.entities.contains(e.handle))
        });
        for e in l.entities.iter() {
            let EntityKind::Insert(i) = &e.kind else { continue };
            if !i.block.starts_with("GOST-SHEET-") {
                continue;
            }
            let prior = before.entity(e.handle);
            let prior_insert = prior.and_then(|e| if let EntityKind::Insert(i) = &e.kind { Some(i) } else { None });
            let frame_changed = prior_insert.is_none_or(|p| p.attribs != i.attribs || p.block != i.block);
            relevant |= frame_changed;
            if !frame_changed {
                continue;
            }
            if data::load::<Record>(&e.common, KIND)?.is_some_and(|r| !r.automatic) {
                continue;
            }
            if let Some(prior) = prior_insert {
                for a in &i.attribs {
                    let tag = a.tag.to_ascii_uppercase();
                    if shared(&tag)
                        && prior.attribs.iter().find(|x| x.tag.eq_ignore_ascii_case(&a.tag)).is_some_and(|old| old.text.value != a.text.value)
                        && updates.insert(tag, a.text.value.clone()).is_some_and(|v| v != a.text.value)
                    {
                        return Err(bad("gost.book.sync", "conflicting edits to common fields"));
                    }
                }
            }
        }
    }
    if relevant {
        sync(s, &updates)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "book_tests.rs"]
mod tests;
