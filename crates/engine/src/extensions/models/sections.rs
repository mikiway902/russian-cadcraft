//! Frozen native half-space sections. No mesh projection, source re-evaluation or topology association.
use super::{
    Arc, CommandSpec, Common, Drawing, EntityKind, Handle, Result, SectionOverlay, Session, Value, Vec2, Vec3, err, hooks, install_view_options,
    json, keys, load, model, options_for_basis, tag, text, vector,
};
use cadcraft_doc::{
    Hatch, HatchEdge, HatchLoop, Insert,
    library::PatternLine,
    model_projection::{Accuracy, Visibility},
    model_section::{BodyStatus, KeptSide, Plane, Section, SectionRequest, SectionStatus},
};
use cadcraft_geom::Segment;
use std::collections::{BTreeMap, BTreeSet};

// Separate from the linked-view app: clipboard identity remapping must retain this small policy.
const STYLE: &str = "CADCRAFTGOST_SECTION_STYLE";
const MAX_COORD: f64 = 1_000_000.;
const MAX_CAP_EDGES: usize = 10_000;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Presentation {
    #[default]
    Cutaway,
    Detached,
    Imposed,
}
impl Presentation {
    pub(super) fn draws_projection(self) -> bool {
        self == Self::Cutaway
    }
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Cutaway => "cutaway",
            Self::Detached => "detached",
            Self::Imposed => "imposed",
        }
    }
    pub(super) fn representation(self) -> &'static str {
        match self {
            Self::Cutaway => "frozen-editable-exact-half-space-section",
            Self::Detached => "frozen-editable-exact-detached-section",
            Self::Imposed => "frozen-editable-exact-imposed-section",
        }
    }
}
fn presentation(p: &Value) -> Result<Presentation> {
    match p.get("presentation").and_then(Value::as_str) {
        None if p.get("presentation").is_none() => Ok(Presentation::Cutaway),
        Some("cutaway") => Ok(Presentation::Cutaway),
        Some("detached") => Ok(Presentation::Detached),
        Some("imposed") => Ok(Presentation::Imposed),
        _ => Err(err("presentation must be cutaway, detached or imposed")),
    }
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("gost.model.section", "Create Exact Model Section", section)
        .params("{id,plane:{origin:[x,y,z],normal:[x,y,z]},kept_side:positive|negative,presentation?:cutaway|detached|imposed,source_view?:HEX,label?:string|null,projection_link?:bool (source-bound default true),up?,layout?,at?,scale?,hidden?,accept_incomplete?,exclude_bodies?}; frozen native geometry; source-linked section placement and MOVE are constrained unless projection_link:false")]
}
fn vec(v: [f64; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}
fn array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
fn unit(v: [f64; 3]) -> Result<Vec3> {
    let v = vec(v);
    if !v.is_finite() || v.len() < 1e-12 {
        return Err(err("nonzero finite section vector required"));
    }
    Ok(v * (1. / v.len()))
}

/// Validate host input and derive a perpendicular sight ray into the retained half-space.
pub fn request(p: &Value) -> Result<SectionRequest> {
    keys(
        p,
        &[
            "id",
            "plane",
            "kept_side",
            "presentation",
            "source_view",
            "label",
            "projection_link",
            "up",
            "layout",
            "at",
            "scale",
            "hidden",
            "accept_incomplete",
            "exclude_bodies",
        ],
    )?;
    presentation(p)?;
    text(p, "id")?;
    if p.get("source_view").is_some() {
        let source = text(p, "source_view")?;
        if source.len() > 16 || Handle::parse_hex(source).is_none_or(|h| h.0 == 0) {
            return Err(err("source_view must be a nonzero native view handle"));
        }
    } else if p.get("label").is_some() {
        return Err(err("section label requires source_view"));
    }
    if p.get("label").is_some_and(|v| !v.is_null() && !v.is_string()) {
        return Err(err("section label must be a designation string or null"));
    }
    if p.get("projection_link").is_some() {
        if p.get("source_view").is_none() {
            return Err(err("section projection_link requires source_view"));
        }
        super::flag(p, "projection_link", true)?;
    }
    let plane_value = p.get("plane").ok_or_else(|| err("section plane required"))?;
    keys(plane_value, &["origin", "normal"])?;
    let origin = vector(plane_value.get("origin").ok_or_else(|| err("section plane origin required"))?)?;
    if origin.iter().any(|v| v.abs() > MAX_COORD) {
        return Err(err("section plane origin exceeds +/-1000000 mm"));
    }
    let normal = unit(vector(plane_value.get("normal").ok_or_else(|| err("section plane normal required"))?)?)?;
    let kept_side = match text(p, "kept_side")? {
        "positive" => KeptSide::Positive,
        "negative" => KeptSide::Negative,
        _ => return Err(err("kept_side must be positive or negative")),
    };
    let direction = normal * if kept_side == KeptSide::Positive { 1. } else { -1. };
    let up = if let Some(up) = p.get("up") {
        unit(vector(up)?)?
    } else if direction.z.abs() < 0.9 {
        Vec3::new(0., 0., 1.)
    } else {
        Vec3::new(0., 1., 0.)
    };
    let x = direction.cross(up);
    if x.len() < 1e-8 {
        return Err(err("section up must not be parallel to the plane normal"));
    }
    let up = (x * (1. / x.len())).cross(direction);
    if let Some(v) = p.get("scale")
        && !v.as_f64().is_some_and(|v| v.is_finite() && (1e-6..=1e6).contains(&v))
    {
        return Err(err("scale must be 1e-6..1e6 model mm per paper mm"));
    }
    if let Some(at) = p.get("at")
        && !at.as_array().is_some_and(|a| a.len() == 2 && a.iter().all(|v| v.as_f64().is_some_and(|v| v.is_finite() && v.abs() <= 1e9)))
    {
        return Err(err("at must contain two finite coordinates within +/-1e9"));
    }
    if p.get("layout").is_some() {
        text(p, "layout")?;
    }
    super::flag(p, "hidden", true)?;
    super::flag(p, "accept_incomplete", false)?;
    let mut exclude_bodies = Vec::new();
    if let Some(indices) = p.get("exclude_bodies") {
        let indices =
            indices.as_array().filter(|v| v.len() <= 32).ok_or_else(|| err("exclude_bodies must be an array of at most 32 original body indices"))?;
        for value in indices {
            let index = value.as_u64().filter(|v| *v < 32).ok_or_else(|| err("excluded body index must be 0..31"))? as usize;
            if exclude_bodies.contains(&index) {
                return Err(err("duplicate excluded body index"));
            }
            exclude_bodies.push(index);
        }
        exclude_bodies.sort_unstable();
    }
    Ok(SectionRequest { plane: Plane { origin, normal: array(normal) }, kept_side, direction: array(direction), up: array(up), exclude_bodies })
}
/// Resolve drawing-dependent source orientation before host work. The returned
/// parameters are the request sent to the worker and checked again at completion.
pub fn prepare_request(s: &Session, p: &Value) -> Result<Value> {
    let store = load(s.doc()?)?;
    Ok(canonical_request(s, p, &store)?.0)
}
fn canonical_request(s: &Session, p: &Value, store: &super::Store) -> Result<(Value, SectionRequest, super::Options)> {
    let mut request = request(p)?;
    let mut opts = options_for_basis(s, p, request.direction, request.up)?;
    model(store, text(p, "id")?)?;
    let source = super::section_notation::select(s.doc()?, store, p, &request, &opts)?;
    let mut canonical = p.clone();
    if let Some(source) = source {
        let enabled = super::flag(p, "projection_link", true)?;
        canonical["projection_link"] = json!(enabled);
        if enabled {
            let up = source.canonical_up(&request, &opts, p.get("up").is_some())?;
            canonical["up"] = json!(up);
            request.up = up;
            opts.up = up;
        }
    }
    Ok((canonical, request, opts))
}

fn section(s: &mut Session, p: &Value) -> Result<Value> {
    let store = load(s.doc()?)?;
    let (canonical, request, _) = canonical_request(s, p, &store)?;
    let m = model(&store, text(&canonical, "id")?)?;
    let result = (hooks()?.section)(&m.path, &m.source.sha256, &request).map_err(err)?;
    install(s, &m.id, &m.source.sha256, &canonical, result)
}

/// Complete host work through the normal guarded transaction, including Undo/Redo.
pub fn apply(s: &mut Session, id: &str, expected_sha: &str, p: &Value, section: Section) -> Result<Value> {
    s.execute_prepared("gost.model.section", p, |s| install(s, id, expected_sha, p, section))
}

fn ends(edge: &HatchEdge) -> [Vec2; 2] {
    match edge {
        HatchEdge::Segment(s) => [s.start(), s.end()],
        HatchEdge::Ellipse { ellipse, ccw } => {
            let mut ends = [ellipse.at_param(ellipse.start), ellipse.at_param(ellipse.end)];
            if !ccw {
                ends.swap(0, 1);
            }
            ends
        }
        HatchEdge::Spline(s) => {
            let (lo, hi) = s.domain();
            [s.eval(lo), s.eval(hi)]
        }
    }
}
fn check_loop(ring: &HatchLoop, outer: bool, tolerance: f64, budget: &mut usize) -> Result<()> {
    *budget = budget.checked_add(ring.edges.len()).ok_or_else(|| err("section cap edge limit"))?;
    if *budget > MAX_CAP_EDGES || ring.edges.is_empty() || !ring.vertices.is_empty() || !ring.sources.is_empty() || ring.outer != outer {
        return Err(err("section requires exact native edge paths, one outer boundary followed by bore holes"));
    }
    for (i, edge) in ring.edges.iter().enumerate() {
        if !edge.is_valid() {
            return Err(err("invalid native section cap edge"));
        }
        let [a, b] = ends(edge);
        if !a.is_finite() || !b.is_finite() || (b - ends(&ring.edges[(i + 1) % ring.edges.len()])[0]).len() > tolerance {
            return Err(err("open or nonfinite native section cap boundary"));
        }
    }
    let bounds = ring.bounds();
    if [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y].iter().any(|v| !v.is_finite() || v.abs() > 2. * MAX_COORD) {
        return Err(err("section cap coordinate limit"));
    }
    let (area, perimeter) = ring.area_and_perimeter();
    if !area.is_finite() || !perimeter.is_finite() || perimeter <= 0. || (outer && area <= 0.) || (!outer && area >= 0.) {
        return Err(err("section cap orientation must be outer CCW and holes CW"));
    }
    Ok(())
}

// Select once per original body, not per cap. Parallel outlines at 45 degrees use the
// GOST 2.306 alternative 30/60 degree families; adjacent source bodies alternate sign.
fn hatch_angle(section: &Section, source_body: usize) -> f64 {
    let sign = if source_body.is_multiple_of(2) { 1. } else { -1. };
    let conflict = |angle: f64| {
        section.caps.iter().filter(|c| c.source_body == source_body).flat_map(|c| &c.loops).flat_map(|r| &r.edges).any(|e| {
            if let HatchEdge::Segment(Segment::Line(line)) = e {
                let delta = line.b - line.a;
                let length = delta.len();
                length > 1e-9 && (delta.x * angle.sin() - delta.y * angle.cos()).abs() / length < (1_f64.to_radians()).sin()
            } else {
                false
            }
        })
    };
    [45_f64, 30., 60.].into_iter().map(|d| sign * d.to_radians()).find(|a| !conflict(*a)).unwrap_or(sign * 30_f64.to_radians())
}

fn outline(edge: &HatchEdge) -> EntityKind {
    match edge {
        HatchEdge::Segment(Segment::Line(line)) => EntityKind::Line(cadcraft_doc::Line { a: line.a.to3(0.), b: line.b.to3(0.) }),
        HatchEdge::Segment(Segment::Arc { arc, .. }) if arc.sweep() >= cadcraft_geom::TAU => {
            EntityKind::Circle(cadcraft_doc::Circle { center: arc.center.to3(0.), radius: arc.radius })
        }
        HatchEdge::Segment(Segment::Arc { arc, .. }) => {
            EntityKind::Arc(cadcraft_doc::Arc { center: arc.center.to3(0.), radius: arc.radius, start: arc.start, end: arc.end })
        }
        HatchEdge::Ellipse { ellipse, .. } => EntityKind::Ellipse(cadcraft_doc::Ellipse {
            center: ellipse.center.to3(0.),
            major: ellipse.major.to3(0.),
            ratio: ellipse.ratio,
            start: ellipse.start,
            end: ellipse.end,
        }),
        HatchEdge::Spline(spline) => EntityKind::Spline(spline.clone()),
    }
}

struct CircularAxes {
    center: Vec2,
    radius: f64,
    extension: f64,
}

fn circular_centers(section: &Section) -> BTreeMap<(u64, u64), CircularAxes> {
    let mut centers = BTreeMap::new();
    for cap in &section.caps {
        let Some(ring) = cap.loops.first() else { continue };
        let [HatchEdge::Segment(Segment::Arc { arc, .. })] = ring.edges.as_slice() else { continue };
        if arc.sweep() < cadcraft_geom::TAU {
            continue;
        }
        // Native full circles only; no ellipse/spline fitting or symmetry claim.
        // Canonical zero prevents +0/-0 from duplicating concentric feature axes.
        let bits = |v: f64| if v == 0.0 { 0 } else { v.to_bits() };
        centers
            .entry((bits(arc.center.x), bits(arc.center.y)))
            .and_modify(|group: &mut CircularAxes| group.radius = group.radius.max(arc.radius))
            .or_insert(CircularAxes { center: arc.center, radius: arc.radius, extension: 3. });
    }
    centers
}

fn install(s: &mut Session, id: &str, expected_sha: &str, p: &Value, mut section: Section) -> Result<Value> {
    let store = load(s.doc()?)?;
    let (mut canonical, request, mut opts) = canonical_request(s, p, &store)?;
    let p = &canonical;
    let presentation = presentation(p)?;
    if text(p, "id")? != id {
        return Err(err("section completion model ID mismatch"));
    }
    if s.doc()?.layer("GOST-HATCH").is_none() {
        return Err(err("GOST-HATCH layer required; run gost.setup first"));
    }
    let m = model(&store, id)?;
    if m.source.sha256 != expected_sha || section.projection.source_sha256 != expected_sha || section.projection.source_bytes != m.source.bytes {
        return Err(err("stale_source: exact section source does not match the linked revision"));
    }
    let normal = unit(section.plane.normal)?;
    if section.plane.origin.iter().any(|n| !n.is_finite())
        || (vec(section.plane.origin) - vec(request.plane.origin)).len() > 1e-8
        || (normal - vec(request.plane.normal)).len() > 1e-8
        || section.kept_side != request.kept_side
    {
        return Err(err("section result plane or kept side differs from request"));
    }
    section.plane = request.plane;
    let basis = section.projection.basis;
    let d = vec(request.direction);
    let x = d.cross(vec(request.up));
    let x = x * (1. / x.len());
    let y = x.cross(d);
    if !crate::finite::is_finite(&basis)
        || (vec(basis.direction) - d).len() > 1e-8
        || (vec(basis.x) - x).len() > 1e-8
        || (vec(basis.y) - y).len() > 1e-8
    {
        return Err(err("section result basis differs from requested plane/up"));
    }
    let count = m.metadata["bodies"].as_array().ok_or_else(|| err("source body metadata missing"))?.len();
    if section.bodies.len() != count || request.exclude_bodies.iter().any(|i| *i >= count) || section.caps.len() > super::MAX_ENTITIES {
        return Err(err("section original body mapping/count mismatch"));
    }
    let mut result_count = 0;
    for (i, body) in section.bodies.iter().enumerate() {
        let excluded = request.exclude_bodies.contains(&i);
        if body.source_body != i || body.source_placement != i || excluded != (body.status == BodyStatus::Excluded) {
            return Err(err("section body mapping differs from original bodies or exclusions"));
        }
        let retained = matches!(body.status, BodyStatus::Retained | BodyStatus::Cut);
        if body.result_body != retained.then_some(result_count) || (!retained && body.topology.is_some()) {
            return Err(err("invalid section result-local body mapping"));
        }
        if retained {
            result_count += 1;
        }
        if (body.status == BodyStatus::Cut) != section.caps.iter().any(|c| c.source_body == i) {
            return Err(err("section cut body/cap mapping mismatch"));
        }
    }
    if (section.status == SectionStatus::Empty) != (result_count == 0) || section.projection.lines.iter().any(|l| l.placement >= result_count) {
        return Err(err("section status or projected body mapping mismatch"));
    }
    let mut edge_budget = 0;
    for cap in &section.caps {
        let body = section.bodies.get(cap.source_body).ok_or_else(|| err("section cap original body missing"))?;
        if body.status != BodyStatus::Cut
            || cap.source_placement != body.source_placement
            || Some(cap.result_body) != body.result_body
            || body.topology.is_some_and(|t| cap.face >= t.faces)
            || cap.loops.is_empty()
        {
            return Err(err("section cap result-local face/body mapping mismatch"));
        }
        let provenance = &cap.provenance;
        if provenance["kind"] != "half_space_boolean_history"
            || provenance["topology_identity"] != "section_result_local_only"
            || provenance["boundary_operation"] != "exact_planar_difference_from_original_coplanar_faces"
            || !provenance["face_origins"].as_array().is_some_and(|v| !v.is_empty())
            || !provenance["planar_read_tolerance_mm"].as_f64().is_some_and(|v| v.is_finite() && v > 0.)
            || provenance["curve_contract"] != "upstream_exact_native_planar_read_with_reported_recognition_tolerance_not_symbolic_certificate"
        {
            return Err(err("section cap lacks exact boolean/planar recognition provenance"));
        }
        let tolerance = provenance["planar_read_tolerance_mm"].as_f64().ok_or_else(|| err("section recognition tolerance missing"))?;
        for (i, ring) in cap.loops.iter().enumerate() {
            check_loop(ring, i == 0, tolerance, &mut edge_budget)?;
        }
    }
    if section.status == SectionStatus::Empty || section.caps.is_empty() {
        let projection = &section.projection;
        let q = projection.quality;
        let mut uncertain = 0;
        let mut covered = 0;
        for piece in projection.lines.iter().flat_map(|l| &l.pieces).filter(|p| p.visibility == Visibility::Uncertain) {
            uncertain += 1;
            covered += usize::from(piece.covered);
        }
        let complete = if q.analytic_complete {
            uncertain == 0
        } else {
            uncertain > 0
                && uncertain == covered
                && uncertain == q.uncertain_pieces
                && projection.visibility_contract == "raw_uncertain_and_covered_preserved_no_mesh_fallback"
        };
        if !opts.accept
            && (!complete
                || uncertain != q.uncertain_pieces
                || uncertain != covered
                || q.unsupported_faces > 0
                || q.bounded_pieces > 0
                || q.sampled_pieces > 0
                || q.unsupported_trims > 0
                || q.import_issues > 0
                || q.open_bodies > 0
                || !projection.unsupported.is_empty()
                || projection
                    .entities
                    .iter()
                    .any(|e| (e.visibility == Visibility::Uncertain && !e.covered) || !matches!(e.accuracy, Accuracy::Analytic)))
        {
            return Err(err("incomplete or uncertain exact section; explicit accept_incomplete:true and manual engineering review required"));
        }
        if section.status == SectionStatus::Empty && (!section.projection.entities.is_empty() || !section.projection.lines.is_empty()) {
            return Err(err("empty section unexpectedly contains projection geometry"));
        }
        return Ok(json!({"id":id,"status":if section.status == SectionStatus::Empty {"empty"} else {"no_intersection"},
            "drawing_mutated":false,"plane":section.plane,"kept_side":section.kept_side,"bodies":section.bodies,
            "quality":section.projection.quality,"topology_identity":"section_result_local_only"}));
    }
    if !presentation.draws_projection() {
        for body in &section.bodies {
            // Adjacent caps are not yet unioned and axial-hole conventions can join a
            // geometric split: do not classify every multi-cap payload as clause 7.10.
            if section.caps.iter().filter(|cap| cap.source_body == body.source_body).count() > 1 {
                return Err(err("multiple cross-section cap regions need component/convention review (GOST 2.305-2008 7.9-7.10); use cutaway"));
            }
        }
    }
    let mut notation = super::section_notation::prepare(s.doc()?, &store, p, &request, &opts)?;
    let mut angles = BTreeMap::new();
    for cap in &section.caps {
        angles.entry(cap.source_body).or_insert_with(|| hatch_angle(&section, cap.source_body));
    }
    let audit_caps: Vec<_> = section
        .caps
        .iter()
        .map(|c| {
            let mut provenance = serde_json::Map::new();
            for key in [
                "kind",
                "operands",
                "face_origins",
                "change",
                "excluded_original_coplanar_faces",
                "boundary_operation",
                "planar_read_tolerance_mm",
                "curve_contract",
                "topology_identity",
            ] {
                if let Some(value) = c.provenance.get(key) {
                    provenance.insert(key.into(), value.clone());
                }
            }
            json!({"source_body":c.source_body,"source_placement":c.source_placement,"result_body":c.result_body,
            "face":c.face,"loops":c.loops.len(),"hatch_angle":angles[&c.source_body],"provenance":provenance})
        })
        .collect();
    let mut centers = circular_centers(&section);
    let axis_count = centers.values_mut().try_fold(0_usize, |sum, group| {
        group.extension = crate::extensions::gost::axes::default_extension(group.radius, opts.scale)?;
        let count = crate::extensions::gost::axes::entity_count(group.radius, opts.scale, group.extension)?;
        sum.checked_add(count).filter(|n| *n <= super::MAX_ENTITIES).ok_or_else(|| err("section axis entity limit"))
    })?;
    let mut audit = json!({"plane":section.plane,"kept_side":section.kept_side,"exclude_bodies":request.exclude_bodies,
        "bodies":section.bodies,"caps":audit_caps,"topology_identity":"section_result_local_only",
        "association":"frozen_editable_not_associative_topology","hatch_spacing_paper_mm":3.,"model_units_per_paper_mm":opts.scale,
        "presentation":presentation.name(),
        "visible_geometry":if presentation.draws_projection() {"cut_caps_and_retained_body_projection"}else{"intersection_regions_only"},
        "circular_center_axes":{"groups":centers.len(),"entities":axis_count,"extension_policy":"automatic_3_to_4_5_paper_mm","association":"frozen_with_view"}});
    let caption = if let Some(notation) = &mut notation {
        let mut bounds = cadcraft_geom::Bounds2::EMPTY;
        for edge in section.caps.iter().flat_map(|cap| &cap.loops).flat_map(|ring| &ring.edges) {
            bounds = bounds.union(&super::projections::curve_bounds(&outline(edge))?);
        }
        if presentation.draws_projection() {
            for entity in &section.projection.entities {
                if entity.visibility != Visibility::Hidden || opts.hidden {
                    bounds = bounds.union(&super::projections::curve_bounds(&entity.kind)?);
                }
            }
        }
        notation.align(bounds, &mut opts)?;
        audit["notation"] = notation.audit();
        notation.caption(s.doc()?, bounds, opts.scale)?
    } else {
        None
    };
    let outline_count = if presentation.draws_projection() { 0 } else { edge_budget };
    let entity_count = section
        .caps
        .len()
        .checked_add(outline_count)
        .and_then(|n| n.checked_add(axis_count))
        .and_then(|n| n.checked_add(usize::from(caption.is_some())))
        .filter(|n| *n <= super::MAX_ENTITIES)
        .ok_or_else(|| err("section native entity count limit"))?;
    let outline_layer = if presentation == Presentation::Imposed { "GOST-THIN" } else { "GOST-OUTLINE" };
    if !presentation.draws_projection() && s.doc()?.layer(outline_layer).is_none() {
        return Err(err("section outline layer required; run gost.setup first"));
    }
    let mut entities = Vec::with_capacity(entity_count);
    for cap in section.caps {
        if !presentation.draws_projection() {
            for edge in cap.loops.iter().flat_map(|ring| &ring.edges) {
                entities.push((Common { layer: outline_layer.into(), ..Common::default() }, outline(edge)));
            }
        }
        entities.push((
            Common { layer: "GOST-HATCH".into(), ..Common::default() },
            EntityKind::Hatch(Hatch {
                pattern: "GOST-METAL".into(),
                solid: false,
                loops: cap.loops,
                scale: 1.,
                angle: angles[&cap.source_body],
                associative: false,
                style: 0,
                elevation: 0.,
                gradient: None,
                origin: Vec2::ZERO,
                background: None,
                pattern_lines: vec![PatternLine { angle: 0., origin: (0., 0.), delta: (0., 3. * opts.scale), dashes: Vec::new() }],
            }),
        ));
    }
    for group in centers.into_values() {
        let first = entities.len();
        crate::extensions::gost::axes::geometry(&mut entities, group.center, group.radius, opts.scale, group.extension)?;
        if let Some((common, _)) = entities.get(first)
            && s.doc()?.layer(&common.layer).is_none()
        {
            return Err(err("section centre-axis layer required; run gost.setup first"));
        }
    }
    if let Some(caption) = caption {
        entities.push(caption);
    }
    let scale = 1. / opts.scale;
    let at = json!([opts.at.x, opts.at.y]);
    if notation.is_some() {
        canonical["at"] = at.clone();
    }
    let mut out =
        install_view_options(s, id, expected_sha, &canonical, section.projection, opts, Some(SectionOverlay { entities, audit, presentation }))?;
    let handle = Handle::parse_hex(text(&out, "handle")?).ok_or_else(|| err("section insertion handle missing"))?;
    s.doc_mut()?.modify_entity(handle, |e| {
        e.common.xdata.extend([tag(1001, STYLE), tag(1070, "1"), tag(1040, scale.to_string())]);
    })?;
    if let Some(notation) = notation {
        super::section_notation::install(s.doc_mut()?, handle, notation)?;
    }
    out["status"] = json!("created");
    out["at"] = at;
    Ok(out)
}

fn style(common: &Common) -> Result<Option<f64>> {
    let Some(start) = common.xdata.iter().position(|t| t.code == 1001 && t.value == STYLE) else { return Ok(None) };
    let rest = &common.xdata[start + 1..];
    if rest.first().is_none_or(|t| t.code != 1070 || t.value != "1") {
        return Err(err("unsupported managed section style marker"));
    }
    let scale = rest
        .get(1)
        .filter(|t| t.code == 1040)
        .and_then(|t| t.value.parse::<f64>().ok())
        .filter(|v| v.is_finite() && (1e-6..=1e6).contains(v))
        .ok_or_else(|| err("invalid managed section style scale"))?;
    Ok(Some(scale))
}
fn identity_style(i: &Insert, scale: f64) -> bool {
    let angle = i.rotation.sin().abs() + (i.rotation.cos() - 1.).abs();
    i.rotation.is_finite()
        && angle <= 1e-10
        && i.scale.is_finite()
        && [i.scale.x, i.scale.y, i.scale.z].iter().all(|v| (*v - scale).abs() <= scale * 1e-10)
}
fn policy_error() -> crate::EngineError {
    err(
        "managed section paper hatch spacing/direction cannot be scaled, rotated, mirrored or transformed nonuniformly, including through nested blocks; recreate the section using its scale/up parameters (MOVE, copy and Undo remain available)",
    )
}

/// Preserve the original paper-style contract without loading the linked-model Store.
/// The command transaction restores the entire drawing if this callback refuses an edit.
pub fn after_command(s: &mut Session, before: Option<&Arc<Drawing>>, uid: Option<u64>) -> Result<()> {
    let Some(before) = before else { return Ok(()) };
    let state = s.state()?;
    if uid.is_some_and(|uid| uid != state.uid) || Arc::ptr_eq(before, &state.doc) {
        return Ok(());
    }
    let d = s.doc()?;
    let mut old_styles = BTreeMap::new();
    for e in super::entities(before) {
        if let Some(scale) = style(&e.common)? {
            old_styles.insert(e.handle, scale);
        }
    }
    let mut marked_blocks = BTreeSet::new();
    let mut parents: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut any = false;
    for e in super::entities(d) {
        let marker = style(&e.common)?;
        // Deletion is allowed; a live insert must not silently detach or rewrite its policy.
        if let Some(old) = old_styles.get(&e.handle)
            && marker != Some(*old)
        {
            return Err(policy_error());
        }
        if let Some(scale) = marker {
            any = true;
            let EntityKind::Insert(i) = &e.kind else { return Err(policy_error()) };
            if !identity_style(i, scale) {
                return Err(policy_error());
            }
        }
    }
    if !any {
        return Ok(());
    }
    for (name, block) in &d.blocks {
        for e in block.entities.iter() {
            if style(&e.common)?.is_some() {
                marked_blocks.insert(name.as_str());
            }
            if let EntityKind::Insert(i) = &e.kind {
                parents.entry(i.block.as_str()).or_default().push(name.as_str());
            }
        }
    }
    let mut queue: Vec<_> = marked_blocks.iter().copied().collect();
    while let Some(child) = queue.pop() {
        if let Some(names) = parents.get(child) {
            for name in names {
                if marked_blocks.insert(*name) {
                    queue.push(*name);
                }
            }
        }
    }
    for e in super::entities(d) {
        if let EntityKind::Insert(i) = &e.kind
            && marked_blocks.contains(i.block.as_str())
            && !identity_style(i, 1.)
        {
            return Err(policy_error());
        }
    }
    Ok(())
}

/// Original source anchors are permitted only on retained bodies and in the kept half-space.
/// A 1e-6 source-mm tolerance covers roundoff at the plane, never a result-local cap identity.
pub(super) fn validate_source_anchor(audit: &Value, placement: u64, point: [f64; 3]) -> Result<()> {
    if audit["topology_identity"] != "section_result_local_only" {
        return Err(err("section topology scope missing"));
    }
    let plane = audit.get("plane").ok_or_else(|| err("section plane audit missing"))?;
    let origin = vector(plane.get("origin").ok_or_else(|| err("section origin audit missing"))?)?;
    let normal = vector(plane.get("normal").ok_or_else(|| err("section normal audit missing"))?)?;
    let presentation = presentation(audit)?;
    if (vec(normal).len() - 1.).abs() > 1e-8 {
        return Err(err("section audit plane is not normalized"));
    }
    let sign = match audit["kept_side"].as_str() {
        Some("positive") => 1.,
        Some("negative") => -1.,
        _ => return Err(err("section kept-side audit missing")),
    };
    let bodies = audit["bodies"].as_array().filter(|b| !b.is_empty() && b.len() <= 32).ok_or_else(|| err("section original body mapping missing"))?;
    let mut target = None;
    let mut results = 0_u64;
    for (i, body) in bodies.iter().enumerate() {
        if body["source_body"].as_u64() != Some(i as u64) || body["source_placement"].as_u64() != Some(i as u64) {
            return Err(err("section original body mapping is absent, duplicated or malformed"));
        }
        let retained = match body["status"].as_str() {
            Some("retained" | "cut") => true,
            Some("excluded" | "removed") => false,
            _ => return Err(err("section body disposition missing")),
        };
        let result = body.get("result_body").ok_or_else(|| err("section result body mapping missing"))?;
        if retained {
            if result.as_u64() != Some(results) {
                return Err(err("section result-local body mapping malformed"));
            }
            results += 1;
        } else if !result.is_null() {
            return Err(err("removed section body has result-local topology"));
        }
        if i as u64 == placement {
            target = Some(retained && (presentation.draws_projection() || body["status"] == "cut"));
        }
    }
    if target != Some(true) {
        return Err(err("source anchor body is removed, excluded or absent from the section"));
    }
    let distance = (vec(point) - vec(origin)).dot(vec(normal));
    if !vec(point).is_finite() || distance * sign < -1e-6 {
        return Err(err("source anchor lies in the removed section half-space"));
    }
    if !presentation.draws_projection() && distance.abs() > 1e-6 {
        return Err(err("source anchor is outside the visible cross-section plane"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "sections_tests.rs"]
pub(super) mod tests;
