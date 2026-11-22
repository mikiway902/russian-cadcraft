//! Source-bound cutting-plane or symmetry-axis notation. Native geometry follows
//! its owning INSERT; no model I/O or nearest-object rebinding in document events.
use super::{
    Common, Drawing, Entity, EntityKind, Handle, Insert, Options, Result, SavedView, Session, Store, Value, Vec2, Vec3, data, err, json, load, model,
    projections, save, tag, vector,
};
use crate::extensions::gost::{
    annotations::{self, CuttingPlane},
    axes, standards,
};
use cadcraft_doc::{
    Space,
    model_section::{KeptSide, Section, SectionRequest},
};
use cadcraft_geom::{Bounds2, Mat3};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::Arc};

const MARKER: &str = "CADCRAFTGOST_SECTION_NOTATION";
pub(super) const MARK: &str = "section-cutting-mark";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    CuttingPlane,
    SymmetryAxis,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Link {
    pub(super) version: u32,
    pub(super) source_view: Option<String>,
    pub(super) original_source_view: String,
    pub(super) status: String,
    pub(super) label: Option<String>,
    pub(super) source_scale: f64,
    pub(super) projection: Option<super::section_projection::Constraint>,
    pub(super) kind: Kind,
    pub(super) source_marks: usize,
}
fn handle(text: &str) -> Result<Handle> {
    if text.is_empty() || text.len() > 16 || !text.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(err("invalid section notation source handle"));
    }
    Handle::parse_hex(text).filter(|h| h.0 != 0).ok_or_else(|| err("invalid section notation source handle"))
}
pub(super) fn link(view: &SavedView) -> Result<Option<Link>> {
    let Some(value) = view.provenance.get("section").and_then(|s| s.get("notation")) else { return Ok(None) };
    let link = Link::deserialize(value).map_err(|e| err(format!("invalid section notation: {e}")))?;
    if link.version != 3
        || !link.source_scale.is_finite()
        || !(1e-6..=1e6).contains(&link.source_scale)
        || !matches!((link.status.as_str(), link.source_view.as_deref()), ("linked", Some(_)) | ("missing_source", None))
        || link.label.as_ref().is_some_and(|s| s.is_empty() || s.len() > 14)
        || link.source_marks > super::MAX_ENTITIES
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
        let enabled = section
            .parameters
            .get("projection_link")
            .and_then(Value::as_bool)
            .ok_or_else(|| err("saved section projection_link must be explicit boolean"))?;
        if link.projection.is_some() != enabled {
            return Err(err("section projection constraint differs from saved projection_link parameter"));
        }
        if let Some(constraint) = &link.projection {
            super::section_projection::validate(constraint)?;
            let imposed = section.provenance["section"]["presentation"] == "imposed";
            if (constraint.mode == super::section_projection::Mode::Fixed) != imposed {
                return Err(err("section projection constraint mode differs from its presentation"));
            }
        }
        let imposed = section.provenance["section"]["presentation"] == "imposed";
        match link.kind {
            Kind::CuttingPlane if link.source_marks != if link.label.is_some() { 8 } else { 6 } => {
                return Err(err("cutting-plane mark count differs from its lettering"));
            }
            Kind::SymmetryAxis => {
                if !enabled
                    || link.label.is_some()
                    || (!imposed && section.provenance["section"]["presentation"] != "detached")
                    || (imposed && link.source_marks != 0)
                    || (!imposed && (link.source_marks < 2 || link.source_marks % 4 != 2))
                {
                    return Err(err("invalid source-bound symmetry-axis notation"));
                }
                let proof: cadcraft_doc::model_section::SectionSymmetry = serde_json::from_value(section.provenance["section"]["symmetry"].clone())
                    .map_err(|e| err(format!("invalid saved section symmetry: {e}")))?;
                let constraint = link.projection.as_ref().ok_or_else(|| err("symmetry-axis notation requires projection linkage"))?;
                if !proof.symmetric
                    || !crate::finite::is_finite(&proof)
                    || !(1e-9..=1e-6).contains(&proof.tolerance_mm)
                    || proof.center.iter().any(|v| v.abs() > 1e6)
                    || proof.axis.iter().zip(constraint.axis).any(|(a, b)| (a - b).abs() > 1e-9)
                    || !section.parameters["scale"].as_f64().is_some_and(|scale| (scale * link.source_scale - 1.).abs() <= 1e-10)
                {
                    return Err(err("saved symmetry-axis proof differs from its frozen source frame"));
                }
            }
            Kind::CuttingPlane => {}
        }
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
        let parameter = match section.parameters.get("source_view") {
            Some(Value::String(source)) => Some(source.as_str()),
            Some(Value::Null) => None,
            _ => return Err(err("saved section source_view must be a handle string or orphan null")),
        };
        if parameter.map(handle).transpose()? != link.source_view.as_deref().map(handle).transpose()? {
            return Err(err("section notation differs from saved source_view parameter"));
        }
    }
    Ok(())
}
/// Migrate old cutting-plane styles without moving or reclassifying geometry.
/// The next ordinary Store save persists the v3 notation record.
pub(super) fn migrate(store: &mut Store) -> Result<()> {
    for view in &mut store.views {
        let Some(value) = view.provenance.get_mut("section").and_then(|s| s.get_mut("notation")) else { continue };
        let version = value.get("version").and_then(Value::as_u64);
        if !matches!(version, Some(1 | 2)) {
            continue;
        }
        if value.get("kind").is_some() || value.get("source_marks").is_some() {
            return Err(err("legacy section notation cannot carry v3 style fields"));
        }
        if version == Some(1) {
            if value.get("projection").is_some_and(|v| !v.is_null()) || view.parameters.get("projection_link").is_some() {
                return Err(err("legacy section notation cannot carry projection constraints"));
            }
            value["projection"] = Value::Null;
            view.parameters["projection_link"] = json!(false);
        }
        value["version"] = json!(3);
        value["kind"] = json!("cutting_plane");
        value["source_marks"] = json!(if value.get("label").is_some_and(|v| !v.is_null()) { 8 } else { 6 });
        Link::deserialize(&*value).map_err(|e| err(format!("invalid legacy section notation: {e}")))?;
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

struct SymmetryAxes {
    center: Vec2,
    axis: Vec2,
    pattern: axes::AxisPattern,
}
pub(super) struct Prepared {
    source: Handle,
    block: String,
    link: Link,
    marks: Vec<(Common, EntityKind)>,
    symmetry: Option<SymmetryAxes>,
    bounds: Bounds2,
    scale: f64,
}
impl Prepared {
    pub(super) fn audit(&self) -> Value {
        json!(self.link)
    }
    pub(super) fn axis_direction(&self) -> Option<Vec2> {
        self.symmetry.as_ref().map(|s| s.axis)
    }
    pub(super) fn axis_count(&self) -> usize {
        self.symmetry.as_ref().map_or(0, |s| s.pattern.count())
    }
    pub(super) fn append_axis(&self, target: &mut Vec<(Common, EntityKind)>) -> Result<()> {
        if let Some(axis) = &self.symmetry {
            axis.pattern.append(target, axis.center, axis.axis, self.scale)?;
        }
        Ok(())
    }
    pub(super) fn caption(&self, d: &Drawing) -> Result<Option<(Common, EntityKind)>> {
        let Some(label) = &self.link.label else { return Ok(None) };
        let at = Vec2::new((self.bounds.min.x + self.bounds.max.x) * 0.5, self.bounds.max.y + 8.0 * self.scale);
        let value = annotations::view_caption(label, true, self.scale);
        Ok(Some((
            Common { layer: "GOST-TEXT".into(), ..Common::default() },
            annotations::designation_text(at, value, standards::designation_height(d)? * self.scale),
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
    sight: Vec3,
    imposed: bool,
}
impl Selected<'_> {
    /// Fold the section frame about the source-plane trace, preserving the source
    /// trace direction on paper instead of guessing a world-axis up.
    pub(super) fn canonical_up(&self, request: &SectionRequest, opts: &Options, explicit: bool) -> Result<[f64; 3]> {
        if self.imposed && (opts.scale * self.insert.scale.x - 1.0).abs() > 1e-10 {
            return Err(err("projection-linked imposed section must use the source view scale; disable projection_link for free placement"));
        }
        let tangent = self.sight.cross(self.normal);
        let tangent = tangent * (1.0 / tangent.len());
        let screen = Vec2::new(tangent.dot(self.x), tangent.dot(self.y));
        let screen = screen * (1.0 / screen.len());
        let direction = Vec3::new(request.direction[0], request.direction[1], request.direction[2]);
        let up = tangent * screen.y + tangent.cross(direction) * screen.x;
        let up = up * (1.0 / up.len());
        if explicit {
            let actual = Vec3::new(request.up[0], request.up[1], request.up[2]);
            if actual.dot(up) < 1.0 - 1e-10 {
                return Err(err(
                    "projection-linked section up must preserve the source trace orientation; disable projection_link for a free orientation",
                ));
            }
            Ok(request.up)
        } else {
            Ok([up.x, up.y, up.z])
        }
    }
    pub(super) fn symmetry_axis(&self, p: &Value, request: &SectionRequest, opts: &Options) -> Result<Option<[f64; 2]>> {
        if (p["presentation"] != "detached" && !self.imposed)
            || p.get("label").is_some_and(|v| !v.is_null())
            || (opts.scale * self.insert.scale.x - 1.).abs() > 1e-10
        {
            return Ok(None);
        }
        let into = Vec2::new(self.normal.dot(self.x), self.normal.dot(self.y)) * if request.kept_side == KeptSide::Positive { 1. } else { -1. };
        let length = into.len();
        if !length.is_finite() || length <= 0. {
            return Err(err("invalid source cutting trace for symmetry classification"));
        }
        Ok(Some([into.y / length, -into.x / length]))
    }
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
    Ok(Some(Selected { view: source, insert, origin, normal, x, y, sight, imposed }))
}

pub(super) fn prepare(
    d: &Drawing,
    store: &Store,
    p: &Value,
    request: &SectionRequest,
    opts: &mut Options,
    section: &Section,
    circle_radius: Option<f64>,
) -> Result<Option<Prepared>> {
    let Some(Selected { view, insert, origin, normal, x, y, imposed, .. }) = select(d, store, p, request, opts)? else { return Ok(None) };
    let source_handle = handle(&view.handle)?;
    let projection_enabled = super::flag(p, "projection_link", true)?;
    let symmetry = section.symmetry.as_ref().filter(|s| s.symmetric);
    if symmetry.is_some() && (!projection_enabled || request.symmetry_axis.is_none()) {
        return Err(err("symmetry-axis notation requires its canonical projection-linked source request"));
    }
    let kind = if symmetry.is_some() { Kind::SymmetryAxis } else { Kind::CuttingPlane };
    if kind == Kind::CuttingPlane {
        ready(d, "GOST-SECTION")?;
        ready(d, "GOST-THIN")?;
        if !imposed {
            ready(d, "GOST-TEXT")?;
            if d.text_style("GOST").is_none() {
                return Err(err("GOST text style required; run gost.setup first"));
            }
        }
    }
    let label = if imposed || symmetry.is_some() {
        None
    } else {
        Some(match p.get("label") {
            None => standards::next_letter(d, "gost.model.section")?,
            Some(Value::String(label)) => label.clone(),
            _ => return Err(err("detached/cutaway source notation requires a lettered designation")),
        })
    };
    let source_bounds = projections::view_bounds(d, insert)?;
    let bounds = super::sections::bounds(section, p, opts.hidden)?;
    let plane_origin = Vec3::new(request.plane.origin[0], request.plane.origin[1], request.plane.origin[2]);
    let on = Vec2::new(plane_origin.dot(x), plane_origin.dot(y)) * insert.scale.x + origin;
    let into = Vec2::new(normal.dot(x), normal.dot(y)) * if request.kept_side == KeptSide::Positive { 1. } else { -1. };
    let tangent = Vec2::new(into.y, -into.x) * (1. / into.len());
    let trace = super::section_projection::SourceTrace { origin, point: on, axis: tangent };
    let center = symmetry.map_or(bounds.center(), |s| Vec2::new(s.center[0], s.center[1]));
    let projection = if projection_enabled { Some(super::section_projection::place(&trace, insert.scale.x, center, opts, imposed)?) } else { None };
    let local = Mat3::translate(-origin).then(Mat3::scale(1. / insert.scale.x, 1. / insert.scale.x));
    let mut marks = Vec::new();
    let symmetry = if let Some(proof) = symmetry {
        let axis = Vec2::new(proof.axis[0], proof.axis[1]);
        let corners = [bounds.min, Vec2::new(bounds.min.x, bounds.max.y), bounds.max, Vec2::new(bounds.max.x, bounds.min.y)];
        let radius = circle_radius.unwrap_or_else(|| corners.iter().fold(0_f64, |r, p| r.max((*p - center).dot(axis).abs())));
        let extension = axes::default_extension(radius, opts.scale)?;
        let pattern = axes::AxisPattern::new(radius, opts.scale, extension)?;
        ready(d, if pattern.count() == 2 { "GOST-THIN" } else { "GOST-CENTER" })?;
        if !imposed {
            marks.reserve(pattern.count());
            let source_center = on + tangent * (origin + center * insert.scale.x - on).dot(tangent);
            pattern.append(&mut marks, source_center, tangent, 1.)?;
        }
        Some(SymmetryAxes { center, axis, pattern })
    } else {
        let corners = [
            source_bounds.min,
            Vec2::new(source_bounds.min.x, source_bounds.max.y),
            source_bounds.max,
            Vec2::new(source_bounds.max.x, source_bounds.min.y),
        ];
        let (low, high) = corners.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            let t = (*p - on).dot(tangent);
            (lo.min(t), hi.max(t))
        });
        let first = on + tangent * (low - 15.);
        let last = on + tangent * (high + 15.);
        let height = if label.is_some() { standards::designation_height(d)? } else { 1. };
        marks.reserve(if label.is_some() { 8 } else { 6 });
        annotations::cutting_plane(
            CuttingPlane { first, last, lettering: label.as_deref().map(|s| (s, height)), scale: 1., reverse: false },
            &mut |kind, layer| {
                marks.push((Common { layer: layer.into(), ..Common::default() }, kind));
                Ok(())
            },
        )?;
        None
    };
    for (_, geometry) in &mut marks {
        geometry.transform(&local);
    }
    Ok(Some(Prepared {
        source: source_handle,
        block: d.block(&insert.block).ok_or_else(|| err("section source block missing"))?.name.clone(),
        link: Link {
            version: 3,
            source_view: Some(source_handle.hex()),
            original_source_view: source_handle.hex(),
            status: "linked".into(),
            label,
            source_scale: insert.scale.x,
            projection,
            kind,
            source_marks: marks.len(),
        },
        marks,
        symmetry,
        bounds,
        scale: opts.scale,
    }))
}

fn ready(d: &Drawing, layer: &str) -> Result<()> {
    if !d.layer(layer).is_some_and(|l| !l.locked && l.visible() && l.plot) {
        return Err(err(format!("{layer} must be unlocked, visible and printable")));
    }
    Ok(())
}

pub(super) fn install(d: &mut Drawing, section: Handle, prepared: Prepared) -> Result<()> {
    let mut added = Vec::with_capacity(prepared.marks.len());
    for (mut common, kind) in prepared.marks {
        data::save(&mut common, MARK, &json!({"section":section.hex()}))?;
        common.xdata.extend([tag(1001, MARKER), tag(1070, "1")]);
        added.push(Entity { handle: d.new_handle(), common, kind });
    }
    if !added.is_empty() {
        let block = d.blocks.get_mut(&prepared.block).ok_or_else(|| err("source view block disappeared during section notation install"))?;
        let block = Arc::make_mut(block);
        if block.entities.len().checked_add(added.len()).is_none_or(|n| n > super::MAX_ENTITIES) {
            return Err(err("source view section mark entity budget exceeded"));
        }
        for entity in added {
            block.entities.push(entity);
        }
    }
    for target in [prepared.source, section] {
        let entity = d.entity(target).ok_or_else(|| err("section notation owner disappeared"))?;
        if !marked(&entity.common)? {
            d.modify_entity(target, |e| e.common.xdata.extend([tag(1001, MARKER), tag(1070, "1")]))?;
        }
    }
    Ok(())
}

fn paper_unit(d: &Drawing, handle: Handle) -> Option<f64> {
    let space = d.space_of(handle)?;
    let layout = d.layout(space.layout()?)?;
    Some(cadcraft_render::paper::page_unit_mm(d, &layout.page))
}

pub(super) fn relevant_changes(before: &Drawing, d: &Drawing) -> Result<bool> {
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
                        || paper_unit(before, e.handle) != paper_unit(d, e.handle)
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
        if relation.kind == Kind::SymmetryAxis
            && let Some(previous) = store_source(&old, section_handle)
            && before.entity(section_handle).is_some()
        {
            let (was, _, _) = projections::placement(before, previous)?;
            let (now, _, _) = projections::placement(d, section)?;
            let a = before.block(&was.block).ok_or_else(policy)?;
            let b = d.block(&now.block).ok_or_else(policy)?;
            if was.block != now.block || a.base != b.base || a.entities != b.entities {
                return Err(err("symmetry-axis sections refuse edits of verified native geometry; explode the section to edit independently"));
            }
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
        if paper_unit(d, source_handle) != Some(1.0) {
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
        if marks != relation.source_marks {
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
