//! Exact half-space section consumption. No cap is sampled, repaired or replaced by an
//! uncut drawing. Source faces and result-local topology deliberately occupy separate scopes.
use crate::model_projection::{self, Error, MAX_CONTROLS, MAX_SEGMENTS, add_count};
use cadcraft_doc::{
    EntityKind, HatchEdge, HatchLoop,
    model_projection::SourceReference,
    model_section::{
        BodyDisposition, BodyStatus, KeptSide, Plane, Section, SectionCap, SectionRequest, SectionStatus, SectionSymmetry, SectionTopology,
    },
};
use cadcraft_geom::{Arc, Line, Segment, Vec2};
use serde::Deserialize;
use serde_json::Value;
use std::f64::consts::TAU;

pub const MAX_CAP_SEGMENTS: usize = 10_000;
const MAX_TOPOLOGY: usize = 10_000;
const MAX_COORD: f64 = 1_000_000.0;
const SOURCE_MAPPING: &str = "drawing placement/body and edge/face indices are SECTION-RESULT-LOCAL; section.bodies maps result_body to original v1.load source_body/source_placement, including removed/excluded bodies; never use cut indices with v1.references or v1.resolve_point";

fn invalid(name: &'static str) -> Error {
    Error::Invalid(name)
}
fn field<'a>(v: &'a Value, key: &'static str) -> Result<&'a Value, Error> {
    v.get(key).ok_or(invalid(key))
}
fn array(v: &Value) -> Result<&[Value], Error> {
    v.as_array().map(Vec::as_slice).ok_or(invalid("section array"))
}
fn index(v: &Value) -> Result<usize, Error> {
    v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or(invalid("section index"))
}
fn number(v: &Value) -> Result<f64, Error> {
    v.as_f64().filter(|n| n.is_finite()).ok_or(invalid("section finite number"))
}
fn typed<'a, T: Deserialize<'a>>(v: &'a Value) -> Result<T, Error> {
    T::deserialize(v).map_err(|_| invalid("section record"))
}
fn keys(v: &Value, expected: &[&str]) -> Result<(), Error> {
    if v.as_object().is_none_or(|o| o.len() != expected.len() || o.keys().any(|k| !expected.contains(&k.as_str()))) {
        return Err(invalid("section geometry fields"));
    }
    Ok(())
}
fn constant(v: &Value, key: &'static str, expected: &str) -> Result<(), Error> {
    if field(v, key)?.as_str() == Some(expected) { Ok(()) } else { Err(invalid(key)) }
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(v: [f64; 3]) -> Result<[f64; 3], Error> {
    if v.iter().any(|n| !n.is_finite() || n.abs() > MAX_COORD) {
        return Err(invalid("section vector"));
    }
    let len = dot(v, v).sqrt();
    if len == 0.0 || !len.is_finite() {
        return Err(invalid("section nonzero vector"));
    }
    Ok(v.map(|n| n / len))
}
fn close3(a: [f64; 3], b: [f64; 3], tolerance: f64) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() <= tolerance)
}
fn unit_axis(axis: [f64; 2]) -> bool {
    axis.iter().all(|n| n.is_finite()) && (axis[0].hypot(axis[1]) - 1.0).abs() <= 1e-9
}

/// Validate before JSON serialization (which otherwise turns nonfinite numbers into null).
/// Body existence is checked separately against the selected immutable snapshot.
pub fn validate_request(request: &SectionRequest) -> Result<(), Error> {
    if request.plane.origin.iter().any(|n| !n.is_finite() || n.abs() > MAX_COORD) {
        return Err(invalid("section plane origin"));
    }
    let normal = unit(request.plane.normal)?;
    let direction = unit(request.direction)?;
    let up = unit(request.up)?;
    unit(cross(direction, up))?;
    let sign = if request.kept_side == KeptSide::Positive { 1.0 } else { -1.0 };
    let perpendicular = cross(normal, direction);
    if dot(normal, direction) * sign <= 0.0 || dot(perpendicular, perpendicular).sqrt() > 1e-9 {
        return Err(invalid("section view must look normally into the kept side"));
    }
    if request.symmetry_axis.is_some_and(|axis| !unit_axis(axis)) {
        return Err(invalid("section finite unit symmetry axis"));
    }
    if request.exclude_bodies.len() > 32 {
        return Err(Error::Limit("section exclusions"));
    }
    for (i, body) in request.exclude_bodies.iter().enumerate() {
        if *body >= 32 || request.exclude_bodies[..i].contains(body) {
            return Err(invalid("section distinct original body exclusions"));
        }
    }
    Ok(())
}

/// Decode against the request and immutable loaded source. `source_faces` contains original
/// v1.load face counts in placement order, used ONLY for boolean ancestry, never section HLR.
/// HLR edge/face references are validated against the returned result-local topology instead.
pub fn decode(result: &Value, request: &SectionRequest, sha: &str, bytes: u64, source_faces: &[usize]) -> Result<Section, Error> {
    validate_request(request)?;
    if source_faces.is_empty() || source_faces.len() > 32 || request.exclude_bodies.iter().any(|i| *i >= source_faces.len()) {
        return Err(invalid("section original body count/exclusions"));
    }
    let (drawing_segments, mut controls) = model_projection::preflight(result)?;
    constant(result, "protocol", "cadcraft-section/2")?;
    constant(result, "source_mapping", SOURCE_MAPPING)?;
    constant(result, "visibility_contract", "raw_uncertain_and_covered_preserved_no_mesh_fallback")?;
    if result.pointer("/source/sha256").and_then(Value::as_str) != Some(sha) || result.pointer("/source/bytes").and_then(Value::as_u64) != Some(bytes)
    {
        return Err(invalid("section source snapshot"));
    }
    let section = field(result, "section")?;
    for (key, expected) in [
        ("topology_identity", "section_result_local_only"),
        ("cap_coordinates", "world_origin_screen_mm_same_basis_and_scale_as_drawing"),
        ("cap_contract", "outer_ccw_holes_cw_exact_segments_no_polygonization_no_paper_hatch_spacing"),
        ("atomicity", "all_selected_bodies_and_caps_or_error_source_snapshot_unchanged"),
    ] {
        constant(section, key, expected)?;
    }
    let plane: Plane = typed(field(section, "plane")?)?;
    if !close3(plane.origin, request.plane.origin, 0.0)
        || !close3(plane.normal, unit(request.plane.normal)?, 1e-9)
        || (dot(plane.normal, plane.normal) - 1.0).abs() > 1e-9
    {
        return Err(invalid("normalized section plane"));
    }
    let kept_side: KeptSide = typed(field(section, "kept_side")?)?;
    if kept_side != request.kept_side {
        return Err(invalid("section kept side"));
    }
    let status: SectionStatus = typed(field(section, "status")?)?;
    let body_values = array(field(section, "bodies")?)?;
    if body_values.len() != source_faces.len() {
        return Err(invalid("section body disposition count"));
    }
    let mut bodies = Vec::with_capacity(body_values.len());
    let (mut result_count, mut topology_count) = (0, 0);
    let mut result_topology: [Option<SectionTopology>; 32] = [None; 32];
    for (i, value) in body_values.iter().enumerate() {
        let body: BodyDisposition = typed(value)?;
        if body.source_body != i || body.source_placement != i {
            return Err(invalid("section original body order"));
        }
        let excluded = request.exclude_bodies.contains(&i);
        if excluded != (body.status == BodyStatus::Excluded) {
            return Err(invalid("section excluded body disposition"));
        }
        match body.status {
            BodyStatus::Excluded | BodyStatus::Removed => {
                if body.result_body.is_some() || body.topology.is_some() {
                    return Err(invalid("section absent body topology"));
                }
            }
            BodyStatus::Cut | BodyStatus::Retained => {
                if body.result_body != Some(result_count) {
                    return Err(invalid("section compact result body order"));
                }
                let t = body.topology.ok_or(invalid("section missing result topology"))?;
                if t.faces == 0 || t.shells == 0 {
                    return Err(invalid("section nonempty exact result topology"));
                }
                for count in [t.faces, t.edges, t.vertices, t.shells] {
                    add_count(&mut topology_count, count, MAX_TOPOLOGY, "section topology")?;
                }
                *result_topology.get_mut(result_count).ok_or(invalid("section result body count"))? = Some(t);
                result_count += 1;
            }
        }
        bodies.push(body);
    }
    if (status == SectionStatus::Empty) != (result_count == 0) {
        return Err(invalid("section empty status"));
    }
    let cap_values = array(field(section, "caps")?)?;
    if cap_values.len() > MAX_CAP_SEGMENTS {
        return Err(Error::Limit("section caps"));
    }
    let (mut cap_segments, mut total_segments, mut loops_count) = (0, drawing_segments, 0);
    // Bound all cap work before allocating native paths. The projection preflight already
    // bounded JSON depth/nodes/text for the entire response, including these caps.
    for cap in cap_values {
        let region = field(cap, "region")?;
        keys(region, &["outer", "holes"])?;
        let holes = array(field(region, "holes")?)?;
        add_count(&mut loops_count, holes.len() + 1, MAX_CAP_SEGMENTS, "section cap loops")?;
        for ring in std::iter::once(field(region, "outer")?).chain(holes.iter()) {
            keys(ring, &["segs"])?;
            let segs = array(field(ring, "segs")?)?;
            if segs.is_empty() {
                return Err(invalid("empty section boundary"));
            }
            add_count(&mut cap_segments, segs.len(), MAX_CAP_SEGMENTS, "section cap segments")?;
            add_count(&mut total_segments, segs.len(), MAX_SEGMENTS, "aggregate section segments")?;
            for segment in segs {
                add_count(&mut controls, model_projection::segment_control_count(segment)?, MAX_CONTROLS, "aggregate section controls")?;
            }
        }
    }
    let projection = model_projection::decode_preflighted(result, drawing_segments)?;
    let direction = unit(request.direction)?;
    let x = unit(cross(direction, unit(request.up)?))?;
    if !close3(projection.basis.direction, direction, 1e-9)
        || !close3(projection.basis.x, x, 1e-9)
        || !close3(projection.basis.y, cross(x, direction), 1e-9)
    {
        return Err(invalid("section request/view basis mismatch"));
    }
    if projection.prepare_reports.as_array().map(Vec::len) != Some(result_count) {
        return Err(invalid("section result body report count"));
    }
    let topology_for = |placement: usize| result_topology.get(placement).copied().flatten();
    for line in &projection.lines {
        let t = topology_for(line.placement).ok_or(invalid("section HLR result placement"))?;
        let valid = match line.source {
            SourceReference::Edge { edge, .. } => edge < t.edges,
            SourceReference::Silhouette { face } => face < t.faces,
        };
        if !valid {
            return Err(invalid("section HLR result-local topology"));
        }
    }
    for face in &projection.unsupported {
        if topology_for(face.placement).is_none_or(|t| face.face >= t.faces) {
            return Err(invalid("section unsupported result-local face"));
        }
    }
    let mut caps = Vec::with_capacity(cap_values.len());
    let mut cut = [false; 32];
    for cap in cap_values {
        let source_body = index(field(cap, "source_body")?)?;
        let body = bodies.get(source_body).ok_or(invalid("section cap original body"))?;
        let source_placement = index(field(cap, "source_placement")?)?;
        let result_body = index(field(cap, "result_body")?)?;
        let face = index(field(cap, "face")?)?;
        if source_placement != body.source_placement
            || body.result_body != Some(result_body)
            || body.status != BodyStatus::Cut
            || body.topology.is_none_or(|t| face >= t.faces)
        {
            return Err(invalid("section cap face/body mapping"));
        }
        let provenance = field(cap, "provenance")?;
        let tolerance = provenance_check(provenance, *source_faces.get(source_body).ok_or(invalid("section cap source body"))?)?;
        let region = field(cap, "region")?;
        let holes = array(field(region, "holes")?)?;
        let mut loops = Vec::with_capacity(holes.len() + 1);
        for (i, ring) in std::iter::once(field(region, "outer")?).chain(holes.iter()).enumerate() {
            loops.push(decode_loop(ring, i == 0, tolerance)?);
        }
        *cut.get_mut(source_body).ok_or(invalid("section cap source body"))? = true;
        caps.push(SectionCap { source_body, source_placement, result_body, face, loops, provenance: provenance.clone() });
    }
    if bodies.iter().zip(cut).any(|(b, has_cap)| (b.status == BodyStatus::Cut) != has_cap) {
        return Err(invalid("section cut/cap consistency"));
    }
    if result_count == 0 && (!projection.lines.is_empty() || !projection.unsupported.is_empty() || !caps.is_empty()) {
        return Err(invalid("section empty geometry consistency"));
    }
    let symmetry = decode_symmetry(field(section, "symmetry")?, request.symmetry_axis, &caps)?;
    Ok(Section { projection, plane, kept_side, status, bodies, caps, symmetry })
}

fn decode_symmetry(value: &Value, requested: Option<[f64; 2]>, caps: &[SectionCap]) -> Result<Option<SectionSymmetry>, Error> {
    let Some(axis) = requested.filter(|_| caps.len() == 1) else {
        return if value.is_null() { Ok(None) } else { Err(invalid("spurious section symmetry")) };
    };
    let report: SectionSymmetry = typed(value)?;
    if !unit_axis(report.axis) || report.axis.iter().zip(axis).any(|(a, b)| (a - b).abs() > 1e-9) {
        return Err(invalid("section symmetry axis mismatch"));
    }
    if report.center.iter().any(|n| !n.is_finite() || n.abs() > MAX_COORD)
        || !report.tolerance_mm.is_finite()
        || !(1e-9..=1e-6).contains(&report.tolerance_mm)
    {
        return Err(invalid("section symmetry center/tolerance"));
    }
    let bounds = caps[0].conservative_bounds().ok_or(invalid("section symmetry cap boundary"))?;
    let [x, y] = report.center;
    if x < bounds.min.x || x > bounds.max.x || y < bounds.min.y || y > bounds.max.y {
        return Err(invalid("section symmetry center outside cap"));
    }
    Ok(Some(report))
}

fn provenance_check(v: &Value, source_faces: usize) -> Result<f64, Error> {
    for (key, expected) in [
        ("kind", "half_space_boolean_history"),
        ("topology_identity", "section_result_local_only"),
        ("boundary_operation", "exact_planar_difference_from_original_coplanar_faces"),
        ("curve_contract", "upstream_exact_native_planar_read_with_reported_recognition_tolerance_not_symbolic_certificate"),
    ] {
        constant(v, key, expected)?;
    }
    let operands = array(field(v, "operands")?)?;
    if operands.len() != 2 || operands[0].as_str() != Some("original_source_body") || operands[1].as_str() != Some("half_space_tool") {
        return Err(invalid("section boolean operands"));
    }
    if !matches!(field(v, "change")?.as_str(), Some("Kept" | "Modified" | "Split" | "Merged" | "Generated")) {
        return Err(invalid("section boolean change"));
    }
    let origins = array(field(v, "face_origins")?)?;
    if origins.is_empty() {
        return Err(invalid("section missing boolean origins"));
    }
    for origin in origins {
        if origin.as_object().is_none_or(|o| o.len() != 1) {
            return Err(invalid("section face origin"));
        }
        let face = field(origin, "Face")?;
        let operand = index(field(face, "operand")?)?;
        let face = index(field(face, "face")?)?;
        if operand > 1 || (operand == 0 && face >= source_faces) || (operand == 1 && face >= MAX_TOPOLOGY) {
            return Err(invalid("section operand-scoped face origin"));
        }
    }
    let originals = array(field(v, "excluded_original_coplanar_faces")?)?;
    let mut previous = None;
    for value in originals {
        let face = index(value)?;
        if face >= source_faces || previous.is_some_and(|p| face <= p) {
            return Err(invalid("section original coplanar face mapping"));
        }
        previous = Some(face);
    }
    let tolerance = number(field(v, "planar_read_tolerance_mm")?)?;
    if tolerance <= 0.0 || tolerance > MAX_COORD {
        return Err(invalid("section native read tolerance"));
    }
    Ok(tolerance)
}

fn decode_loop(value: &Value, outer: bool, tolerance: f64) -> Result<HatchLoop, Error> {
    let segments = array(field(value, "segs")?)?;
    let mut edges = Vec::with_capacity(segments.len());
    let (mut first, mut last) = (None, None);
    let mut area = 0.0;
    for value in segments {
        let (kind, arc) = model_projection::convert_segment(value)?;
        let (edge, start, end, term) = match kind {
            EntityKind::Line(line) => {
                let a = Vec2::new(line.a.x, line.a.y);
                let b = Vec2::new(line.b.x, line.b.y);
                let origin = first.unwrap_or(a);
                (HatchEdge::Segment(Segment::Line(Line { a, b })), a, b, (a - origin).cross(b - origin))
            }
            EntityKind::Arc(a) => {
                let source = arc.ok_or(invalid("section directed arc"))?;
                let arc = Arc { center: Vec2::new(a.center.x, a.center.y), radius: a.radius, start: a.start, end: a.end };
                if arc.sweep() == TAU && source.sweep.abs() != TAU {
                    return Err(invalid("unrepresentable native section arc sweep"));
                }
                let segment = Segment::Arc { arc, ccw: source.sweep > 0.0 };
                let (a, b) = (segment.start(), segment.end());
                let origin = first.unwrap_or(a);
                (HatchEdge::Segment(segment), a, b, (arc.center - origin).cross(b - a) + arc.radius * arc.radius * source.sweep)
            }
            EntityKind::Circle(c) => {
                let source = arc.ok_or(invalid("section directed circle"))?;
                // Equal native arc angles denote a full turn; retaining the source start
                // also preserves its join point when a circle appears within an edge path.
                let arc = Arc { center: Vec2::new(c.center.x, c.center.y), radius: c.radius, start: source.start, end: source.start };
                let point = arc.start_point();
                (HatchEdge::Segment(Segment::Arc { arc, ccw: source.sweep > 0.0 }), point, point, c.radius * c.radius * source.sweep)
            }
            EntityKind::Spline(spline) => {
                let a = *spline.control.first().ok_or(invalid("section spline controls"))?;
                let b = *spline.control.last().ok_or(invalid("section spline controls"))?;
                let origin = first.unwrap_or(a);
                let term = if spline.weights.is_empty() {
                    bezier_area(&spline.control, origin)
                } else {
                    (a - origin).cross(b - origin) + conic_factor(spline.weights[1]) * (spline.control[1] - a).cross(b - a)
                };
                (HatchEdge::Spline(spline), a, b, term)
            }
            _ => return Err(invalid("unrepresentable exact section segment")),
        };
        let in_range = |p: Vec2| p.is_finite() && p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD;
        let bounded = match &edge {
            HatchEdge::Segment(Segment::Line(l)) => in_range(l.a) && in_range(l.b),
            HatchEdge::Segment(Segment::Arc { arc, .. }) => in_range(arc.center) && arc.radius <= MAX_COORD,
            HatchEdge::Spline(s) => s.control.iter().copied().all(in_range),
            _ => false,
        };
        if !edge.is_valid() || !bounded || !term.is_finite() {
            return Err(invalid("section native boundary geometry"));
        }
        if last.is_some_and(|p: Vec2| p.dist(start) > tolerance) {
            return Err(invalid("open section boundary join"));
        }
        first.get_or_insert(start);
        last = Some(end);
        area += term;
        edges.push(edge);
    }
    if first.zip(last).is_none_or(|(a, b)| a.dist(b) > tolerance) {
        return Err(invalid("open section boundary closure"));
    }
    if !area.is_finite() || area == 0.0 || (area > 0.0) != outer {
        return Err(invalid("section outer/hole winding"));
    }
    Ok(HatchLoop { vertices: Vec::new(), edges, outer, sources: Vec::new() })
}

// Twice the signed Green area of a polynomial Bezier, integrated in Bernstein form.
// At most degree nine: fixed bounded arithmetic, no tessellation or numerical fitting.
fn choose(n: usize, k: usize) -> f64 {
    (0..k.min(n - k)).fold(1.0, |v, i| v * (n - i) as f64 / (i + 1) as f64)
}
fn bezier_area(p: &[Vec2], origin: Vec2) -> f64 {
    let n = p.len() - 1;
    let mut area = 0.0;
    for i in 0..=n {
        for j in 0..n {
            area += (p[i] - origin).cross(p[j + 1] - p[j]) * choose(n, i) * choose(n - 1, j) / (2.0 * choose(2 * n - 1, i + j));
        }
    }
    area
}
// Analytic rational quadratic Green-area factor. For w near one, evaluate the
// convergent integral series instead of subtracting two almost equal transcendental terms.
fn conic_factor(w: f64) -> f64 {
    let e = w - 1.0;
    if e.abs() < 0.001 {
        let mut beta = 1.0 / 3.0;
        let mut power = 1.0;
        let mut sum = 0.0;
        for k in 0..12 {
            sum += (k + 1) as f64 * power * beta;
            beta *= ((k + 3) * (k + 1)) as f64 / ((2 * k + 5) * (2 * k + 4)) as f64;
            power *= -2.0 * e;
        }
        return 2.0 * w * sum;
    }
    let integral = if w < 1.0 { w.acos() / ((1.0 - w) * (1.0 + w)).sqrt() } else { w.acosh() / ((w - 1.0) * (w + 1.0)).sqrt() };
    w * (integral - w) / ((1.0 - w) * (1.0 + w))
}

#[cfg(test)]
#[path = "model_section_tests.rs"]
mod tests;
