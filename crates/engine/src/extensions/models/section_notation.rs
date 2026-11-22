//! Native source-plane marks: source MOVE carries its own block geometry; captions live
//! in the section block. No model I/O, nearest-object rebinding or symmetry inference.
use super::{
    Common, Drawing, Entity, EntityKind, Handle, Insert, Options, Result, SavedView, Session, Store, Value, Vec2, Vec3, data, err, json, load, model,
    projections, save, tag, vector,
};
use crate::extensions::gost::{
    annotations::{self, CuttingPlane},
    standards,
};
use cadcraft_doc::{
    Space,
    model_section::{KeptSide, SectionRequest},
};
use cadcraft_geom::{Bounds2, Mat3};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::Arc};

const MARKER: &str = "CADCRAFTGOST_SECTION_NOTATION";
pub(super) const MARK: &str = "section-cutting-mark";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Link {
    version: u32,
    source_view: Option<String>,
    original_source_view: String,
    status: String,
    label: Option<String>,
    source_scale: f64,
}
fn handle(text: &str) -> Result<Handle> {
    if text.is_empty() || text.len() > 16 || !text.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(err("invalid section notation source handle"));
    }
    Handle::parse_hex(text).filter(|h| h.0 != 0).ok_or_else(|| err("invalid section notation source handle"))
}
fn link(view: &SavedView) -> Result<Option<Link>> {
    let Some(value) = view.provenance.get("section").and_then(|s| s.get("notation")) else { return Ok(None) };
    let link = Link::deserialize(value).map_err(|e| err(format!("invalid section notation: {e}")))?;
    if link.version != 1
        || !link.source_scale.is_finite()
        || !(1e-6..=1e6).contains(&link.source_scale)
        || !matches!((link.status.as_str(), link.source_view.as_deref()), ("linked", Some(_)) | ("missing_source", None))
        || link.label.as_ref().is_some_and(|s| s.is_empty() || s.len() > 14)
    {
        return Err(err("invalid section notation version, scale, label or status"));
    }
    handle(&link.original_source_view)?;
    if let Some(source) = &link.source_view {
        handle(source)?;
    }
    Ok(Some(link))
}
pub(super) fn validate_store(store: &Store) -> Result<()> {
    for section in &store.views {
        let Some(link) = link(section)? else { continue };
        if let Some(source) = link.source_view.as_deref() {
            if handle(source)? == handle(&section.handle)? {
                return Err(err("section cannot reference itself as its source view"));
            }
            if let Some(source) = store.views.iter().find(|v| handle(&v.handle).ok() == handle(source).ok())
                && (source.model_id != section.model_id
                    || source.revision != section.revision
                    || source.source != section.source
                    || source.provenance.get("section").is_some())
            {
                return Err(err("section notation source differs in model/revision or is itself a section"));
            }
        }
        let parameter = section.parameters.get("source_view").and_then(Value::as_str);
        if parameter.map(handle).transpose()? != link.source_view.as_deref().map(handle).transpose()? {
            return Err(err("section notation differs from saved source_view parameter"));
        }
    }
    Ok(())
}
pub(super) fn mark_owner(common: &Common) -> Result<Option<Handle>> {
    if !marked(common)? {
        return Ok(None);
    }
    data::load::<Value>(common, MARK)?
        .map(|v| {
            let owner = v.get("section").and_then(Value::as_str).ok_or_else(|| err("section mark owner missing"))?;
            handle(owner)
        })
        .transpose()
}
/// Remove only a verified mark's application records, never its native geometry.
pub(super) fn detach_mark(common: &mut Common) {
    let mut removing = false;
    common.xdata.retain(|tag| {
        if tag.code == 1001 {
            removing = tag.value.eq_ignore_ascii_case("CADCRAFTGOST") || tag.value.eq_ignore_ascii_case(MARKER);
        }
        !removing
    });
}
fn marked(common: &Common) -> Result<bool> {
    let mut starts = common.xdata.iter().enumerate().filter(|(_, t)| t.code == 1001 && t.value.eq_ignore_ascii_case(MARKER));
    let Some((start, _)) = starts.next() else { return Ok(false) };
    if starts.next().is_some() {
        return Err(err("duplicate section notation marker"));
    }
    let tags = &common.xdata[start + 1..];
    let end = tags.iter().position(|t| t.code == 1001).unwrap_or(tags.len());
    if end != 1 || tags[0].code != 1070 || tags[0].value != "1" {
        return Err(err("invalid section notation marker"));
    }
    Ok(true)
}
fn policy() -> crate::EngineError {
    err(
        "source-linked section marks require an unrotated paper view at its original scale and block; explode or delete the dependent section before transforming the source view",
    )
}
fn unique_source<'a>(d: &'a Drawing, source: &SavedView) -> Result<(&'a Insert, Vec2)> {
    let (insert, _, origin) = projections::placement(d, source)?;
    let source_handle = handle(&source.handle)?;
    for entity in super::entities(d) {
        if let EntityKind::Insert(other) = &entity.kind
            && other.block.eq_ignore_ascii_case(&insert.block)
            && entity.handle != source_handle
        {
            return Err(err("section notation source block has another occurrence or nested wrapper"));
        }
    }
    Ok((insert, origin))
}

pub(super) struct Prepared {
    source: Handle,
    block: String,
    link: Link,
    marks: Vec<(Common, EntityKind)>,
}
impl Prepared {
    pub(super) fn audit(&self) -> Value {
        json!(self.link)
    }
    pub(super) fn caption(&self, d: &Drawing, bounds: Bounds2, scale: f64) -> Result<Option<(Common, EntityKind)>> {
        let Some(label) = &self.link.label else { return Ok(None) };
        let at = Vec2::new((bounds.min.x + bounds.max.x) * 0.5, bounds.max.y + 8.0 * scale);
        let value = annotations::view_caption(label, true, scale);
        Ok(Some((
            Common { layer: "GOST-TEXT".into(), ..Common::default() },
            annotations::designation_text(at, value, standards::designation_height(d)? * scale),
        )))
    }
}

pub(super) struct Selected<'a> {
    view: &'a SavedView,
    insert: &'a Insert,
    origin: Vec2,
    normal: Vec3,
    x: Vec3,
    y: Vec3,
    imposed: bool,
}
pub(super) fn select<'a>(d: &'a Drawing, store: &'a Store, p: &Value, request: &SectionRequest, opts: &Options) -> Result<Option<Selected<'a>>> {
    let Some(source_value) = p.get("source_view") else {
        if p.get("label").is_some() {
            return Err(err("section label requires source_view"));
        }
        return Ok(None);
    };
    let source_handle = handle(source_value.as_str().ok_or_else(|| err("source_view must be a native view handle"))?)?;
    let source = store.views.iter().find(|v| handle(&v.handle).ok() == Some(source_handle)).ok_or_else(|| err("section source view not found"))?;
    let id = super::text(p, "id")?;
    let m = model(store, id)?;
    if source.model_id != id || source.revision != m.revision || source.source != m.source || source.provenance.get("section").is_some() {
        return Err(err("section requires an ordinary source view of the current linked model revision"));
    }
    let (insert, origin) = unique_source(d, source)?;
    let source_space = d.space_of(source_handle).ok_or_else(policy)?;
    if opts.space != source_space {
        return Err(err("section and source notation must share the same paper layout"));
    }
    let Space::Paper(layout) = &opts.space else { return Err(err("source-linked section notation requires paper space")) };
    let layout = d.layout(layout).ok_or_else(|| err("section paper layout missing"))?;
    if cadcraft_render::paper::page_unit_mm(d, &layout.page) != 1.0 {
        return Err(err("section notation requires millimetre paper units"));
    }
    if d.text_style("GOST").is_none() {
        return Err(err("GOST text style required; run gost.setup first"));
    }
    let normal = Vec3::new(request.plane.normal[0], request.plane.normal[1], request.plane.normal[2]);
    let read = |key| -> Result<Vec3> {
        let [x, y, z] = vector(&source.basis[key])?;
        Ok(Vec3::new(x, y, z))
    };
    let (x, y, sight) = (read("x")?, read("y")?, read("direction")?);
    if normal.dot(sight).abs() > 1e-9 {
        return Err(err("cutting plane is not edge-on in the selected source view"));
    }
    let imposed = p.get("presentation").and_then(Value::as_str) == Some("imposed");
    if imposed {
        if p.get("label").is_some_and(|v| !v.is_null()) {
            return Err(err("imposed source cutting-plane marks use arrows without letters; omit label or use null"));
        }
    } else if let Some(label) = p.get("label") {
        standards::validate_letter(
            d,
            label.as_str().ok_or_else(|| err("detached/cutaway source notation requires a lettered designation"))?,
            "gost.model.section",
        )?;
    }
    for layer in ["GOST-SECTION", "GOST-THIN"].into_iter().chain((!imposed).then_some("GOST-TEXT")) {
        if !d.layer(layer).is_some_and(|l| !l.locked && l.visible() && l.plot) {
            return Err(err(format!("{layer} must be unlocked, visible and printable")));
        }
    }
    Ok(Some(Selected { view: source, insert, origin, normal, x, y, imposed }))
}

pub(super) fn prepare(d: &Drawing, store: &Store, p: &Value, request: &SectionRequest, opts: &Options) -> Result<Option<Prepared>> {
    let Some(Selected { view, insert, origin, normal, x, y, imposed }) = select(d, store, p, request, opts)? else { return Ok(None) };
    let source_handle = handle(&view.handle)?;
    let label = if imposed {
        None
    } else {
        let label = match p.get("label") {
            None => standards::next_letter(d, "gost.model.section")?,
            Some(Value::String(label)) => label.clone(),
            _ => return Err(err("detached/cutaway source notation requires a lettered designation")),
        };
        Some(label)
    };
    let bounds = projections::view_bounds(d, insert)?;
    let plane_origin = Vec3::new(request.plane.origin[0], request.plane.origin[1], request.plane.origin[2]);
    let on = Vec2::new(plane_origin.dot(x), plane_origin.dot(y)) * insert.scale.x + origin;
    let into = Vec2::new(normal.dot(x), normal.dot(y)) * if request.kept_side == KeptSide::Positive { 1.0 } else { -1.0 };
    let tangent = Vec2::new(into.y, -into.x) * (1.0 / into.len());
    let corners = [bounds.min, Vec2::new(bounds.min.x, bounds.max.y), bounds.max, Vec2::new(bounds.max.x, bounds.min.y)];
    let (low, high) = corners.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), p| {
        let t = (*p - on).dot(tangent);
        (low.min(t), high.max(t))
    });
    let first = on + tangent * (low - 15.0);
    let last = on + tangent * (high + 15.0);
    let height = if label.is_some() { standards::designation_height(d)? } else { 1.0 };
    let local = Mat3::translate(-origin).then(Mat3::scale(1.0 / insert.scale.x, 1.0 / insert.scale.x));
    let mut marks = Vec::with_capacity(if label.is_some() { 8 } else { 6 });
    annotations::cutting_plane(
        CuttingPlane { first, last, lettering: label.as_deref().map(|label| (label, height)), scale: 1.0, reverse: false },
        &mut |mut kind, layer| {
            kind.transform(&local);
            marks.push((Common { layer: layer.into(), ..Common::default() }, kind));
            Ok(())
        },
    )?;
    Ok(Some(Prepared {
        source: source_handle,
        block: d.block(&insert.block).ok_or_else(|| err("section source block missing"))?.name.clone(),
        link: Link {
            version: 1,
            source_view: Some(source_handle.hex()),
            original_source_view: source_handle.hex(),
            status: "linked".into(),
            label,
            source_scale: insert.scale.x,
        },
        marks,
    }))
}

pub(super) fn install(d: &mut Drawing, section: Handle, prepared: Prepared) -> Result<()> {
    let mut added = Vec::with_capacity(prepared.marks.len());
    for (mut common, kind) in prepared.marks {
        data::save(&mut common, MARK, &json!({"section":section.hex()}))?;
        common.xdata.extend([tag(1001, MARKER), tag(1070, "1")]);
        added.push(Entity { handle: d.new_handle(), common, kind });
    }
    let block = d.blocks.get_mut(&prepared.block).ok_or_else(|| err("source view block disappeared during section notation install"))?;
    let block = Arc::make_mut(block);
    if block.entities.len().checked_add(added.len()).is_none_or(|n| n > super::MAX_ENTITIES) {
        return Err(err("source view section mark entity budget exceeded"));
    }
    for entity in added {
        block.entities.push(entity);
    }
    for target in [prepared.source, section] {
        let entity = d.entity(target).ok_or_else(|| err("section notation owner disappeared"))?;
        if !marked(&entity.common)? {
            d.modify_entity(target, |e| e.common.xdata.extend([tag(1001, MARKER), tag(1070, "1")]))?;
        }
    }
    Ok(())
}

fn relevant_changes(before: &Drawing, d: &Drawing) -> Result<bool> {
    let mut any = false;
    for drawing in [before, d] {
        for e in super::entities(drawing) {
            if !matches!(e.kind, EntityKind::Insert(_)) || !marked(&e.common)? {
                continue;
            }
            any = true;
            match (before.entity(e.handle), d.entity(e.handle)) {
                (Some(a), Some(b)) => {
                    let blocks_unchanged = match (&a.kind, &b.kind) {
                        (EntityKind::Insert(a), EntityKind::Insert(b)) => {
                            matches!((before.block(&a.block),d.block(&b.block)),(Some(a),Some(b)) if Arc::ptr_eq(a,b))
                        }
                        _ => false,
                    };
                    if a.kind != b.kind
                        || a.common.xdata != b.common.xdata
                        || a.common.extrusion != b.common.extrusion
                        || before.space_of(e.handle) != d.space_of(e.handle)
                        || !blocks_unchanged
                    {
                        return Ok(true);
                    }
                }
                _ => return Ok(true),
            }
        }
    }
    if !any {
        return Ok(false);
    }
    // An alias may lack our marker. Any wrapper necessarily contains a direct
    // source-block reference; inspect changed INSERTs without traversing geometry.
    for drawing in [before, d] {
        for e in super::entities(drawing) {
            let EntityKind::Insert(insert) = &e.kind else { continue };
            if matches!((before.entity(e.handle),d.entity(e.handle)), (Some(a),Some(b)) if a.kind==b.kind) {
                continue;
            }
            for views in [before, d] {
                for candidate in super::entities(views) {
                    if let EntityKind::Insert(view) = &candidate.kind
                        && insert.block.eq_ignore_ascii_case(&view.block)
                        && marked(&candidate.common)?
                    {
                        return Ok(true);
                    }
                }
            }
        }
    }
    Ok(!(Arc::ptr_eq(&before.foreign, &d.foreign) || super::record(before)?.map(|r| &r.tags) == super::record(d)?.map(|r| &r.tags)))
}

pub(super) fn after_command(s: &mut Session, before: Option<&Arc<Drawing>>, uid: Option<u64>) -> Result<()> {
    let Some(before) = before else { return Ok(()) };
    let state = s.state()?;
    if uid.is_some_and(|uid| uid != state.uid) || Arc::ptr_eq(before, &state.doc) {
        return Ok(());
    }
    let d = s.doc()?;
    if !relevant_changes(before, d)? {
        return Ok(());
    }
    let mut store = load(d)?;
    let old = load(before)?;
    let mut cleanup = BTreeSet::new();
    let mut updates = Vec::new();
    for (index, section) in store.views.iter().enumerate() {
        let Some(mut relation) = link(section)? else { continue };
        let section_handle = handle(&section.handle)?;
        if let Some(previous) = store_source(&old, section_handle)
            && before.entity(section_handle).is_some()
            && link(previous)?.as_ref() != Some(&relation)
        {
            return Err(err("an existing section notation relation is immutable"));
        }
        if d.entity(section_handle).is_none() {
            cleanup.insert(section_handle);
            continue;
        }
        let Some(source_handle) = relation.source_view.as_deref().map(handle).transpose()? else { continue };
        let source = old.views.iter().find(|v| handle(&v.handle).ok() == Some(source_handle));
        if d.entity(source_handle).is_none() {
            if let Some(source) = source
                && before.entity(source_handle).is_some()
            {
                let (was, _, _) = projections::placement(before, source)?;
                if super::entities(d).any(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(&was.block))) {
                    return Err(policy());
                }
            }
            if super::entities(d).any(|e| e.handle == source_handle) {
                return Err(policy());
            }
            relation.source_view = None;
            relation.status = "missing_source".into();
            updates.push((index, relation));
            cleanup.insert(section_handle);
            continue;
        }
        let current_source = store_source(&store, source_handle).ok_or_else(|| err("saved section notation source view missing"))?;
        for h in [source_handle, section_handle] {
            if !marked(&d.entity(h).ok_or_else(policy)?.common)? {
                return Err(err("an active section notation INSERT lost its policy marker"));
            }
        }
        let (insert, _) = unique_source(d, current_source)?;
        let source_space = d.space_of(source_handle).ok_or_else(policy)?;
        if d.space_of(section_handle).as_ref() != Some(&source_space)
            || before.entity(source_handle).is_some_and(|_| before.space_of(source_handle).as_ref() != Some(&source_space))
        {
            return Err(policy());
        }
        if (insert.scale.x - relation.source_scale).abs() > relation.source_scale * 1e-10 {
            return Err(policy());
        }
        if let Some(old_source) = source
            && before.entity(source_handle).is_some()
        {
            let (was, _, _) = projections::placement(before, old_source)?;
            let old_block = before.block(&was.block).ok_or_else(policy)?;
            let new_block = d.block(&insert.block).ok_or_else(policy)?;
            if was.block != insert.block || old_block.base != new_block.base {
                return Err(policy());
            }
            if Arc::ptr_eq(old_block, new_block) {
                continue;
            }
            let mut a = old_block.entities.iter().filter_map(|e| match mark_owner(&e.common) {
                Ok(None) => Some(Ok(e)),
                Ok(Some(_)) => None,
                Err(e) => Some(Err(e)),
            });
            let mut b = new_block.entities.iter().filter_map(|e| match mark_owner(&e.common) {
                Ok(None) => Some(Ok(e)),
                Ok(Some(_)) => None,
                Err(e) => Some(Err(e)),
            });
            let mut unchanged = true;
            loop {
                match (a.next().transpose()?, b.next().transpose()?) {
                    (None, None) => break,
                    (Some(a), Some(b)) if a == b => {}
                    _ => {
                        unchanged = false;
                        break;
                    }
                }
            }
            if !unchanged {
                return Err(err("source-linked section marks refuse manual edits of their exact source view geometry"));
            }
        }
        let block = d.block(&insert.block).ok_or_else(policy)?;
        let marks = block
            .entities
            .iter()
            .try_fold(0_usize, |count, e| -> Result<usize> { Ok(count + usize::from(mark_owner(&e.common)? == Some(section_handle))) })?;
        if marks != if relation.label.is_some() { 8 } else { 6 } {
            return Err(err("source view cutting-plane marks are missing or duplicated"));
        }
    }
    if !cleanup.is_empty() {
        let mut detached = Vec::new();
        for space in std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)) {
            for e in space.iter() {
                if mark_owner(&e.common)?.is_some_and(|h| cleanup.contains(&h)) {
                    detached.push(e.handle);
                }
            }
        }
        let mut removals = Vec::new();
        for (name, block) in &d.blocks {
            let mut handles = Vec::new();
            for e in block.entities.iter() {
                if mark_owner(&e.common)?.is_some_and(|h| cleanup.contains(&h)) {
                    handles.push(e.handle);
                }
            }
            if !handles.is_empty() {
                removals.push((name.clone(), handles));
            }
        }
        // EXPLODE leaves ordinary top-level geometry: detach, never delete it.
        for h in detached {
            s.doc_mut()?.modify_entity(h, |e| detach_mark(&mut e.common))?;
        }
        for (name, handles) in removals {
            if let Some(block) = s.doc_mut()?.blocks.get_mut(&name) {
                let block = Arc::make_mut(block);
                for h in handles {
                    block.entities.remove(h);
                }
            }
        }
    }
    if !updates.is_empty() {
        for (index, relation) in updates {
            let section = store.views.get_mut(index).ok_or_else(|| err("section notation update target disappeared"))?;
            section.provenance["section"]["notation"] = json!(relation);
            section.parameters["source_view"] = Value::Null;
            section.warnings.push(
                "Section cutting-plane source view is missing; native section and caption are frozen, never rebound by handle collision.".into(),
            );
        }
        save(s.doc_mut()?, &store)?;
    }
    Ok(())
}
fn store_source(store: &Store, h: Handle) -> Option<&SavedView> {
    store.views.iter().find(|v| handle(&v.handle).ok() == Some(h))
}

#[cfg(test)]
#[path = "section_notation_tests.rs"]
mod tests;
