//! Exact one-plane sections. Result topology is deliberately separate from source references.

use super::{Budget, Error, LoadedBody, MAX_BODIES, MAX_TOPOLOGY, ViewQuery, Worker, ensure_response_size, params, projection, vector};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcore_record::{Loop2, MAX_BEZIER_CTRL, MAX_BEZIER_DEGREE, Plane, Region2, Seg2, Vec2, Vec3, brep::Loop};
use solidcraft_geom::{
    Loop2Geom, PlaneGeom, Seg2Geom, Vec3Geom,
    bspline::{HPoint, NurbsCurve},
    hlr,
};
use solidcraft_kernel::{Body, PlanarPiece, PlanarSpline, history::FaceOrigin, watch};
use std::{collections::BTreeSet, sync::Arc};

pub(super) const MAX_COORD: f64 = 1_000_000.0;
pub(super) const MAX_CAP_SEGMENTS: usize = 10_000;
const FACING: f64 = 1.0 - 1e-9;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CutPlane {
    origin: [f64; 3],
    normal: [f64; 3],
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum KeptSide {
    Positive,
    Negative,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SectionQuery {
    source_sha256: String,
    plane: CutPlane,
    kept_side: KeptSide,
    direction: [f64; 3],
    up: [f64; 3],
    #[serde(default)]
    require_analytic: bool,
    #[serde(default)]
    exclude_bodies: Vec<usize>,
}

impl Worker {
    pub(super) fn section(&self, parameters: Value) -> Result<Value, Error> {
        let request: SectionQuery = params(parameters)?;
        let model = self.model.as_ref().ok_or(Error::NoModel)?;
        if request.source_sha256 != model.fingerprint.sha256 {
            return Err(Error::StaleModel);
        }
        let plane = Plane::from_normal(vector(request.plane.origin)?, vector(request.plane.normal)?)
            .ok_or_else(|| Error::Parameters("section plane normal must be nonzero".into()))?;
        let kept = match request.kept_side {
            KeptSide::Positive => plane,
            KeptSide::Negative => plane.flipped(false),
        };
        let view = hlr::View::new(vector(request.direction)?, vector(request.up)?).map_err(|e| Error::Parameters(e.to_string()))?;
        // Cap regions are real plane regions, not a foreshortened image masquerading as one.
        if view.d.dot(kept.normal()) <= 0.0 || view.d.cross(kept.normal()).len() > 1e-9 {
            return Err(Error::Parameters("section view must be normal to the cutting plane and look into the kept side".into()));
        }
        if request.exclude_bodies.len() > MAX_BODIES {
            return Err(Error::Limit("section excluded body indices"));
        }
        let mut excluded = BTreeSet::new();
        for index in request.exclude_bodies {
            if index >= model.bodies.len() || !excluded.insert(index) {
                return Err(Error::Parameters("exclude_bodies must contain distinct original loaded body indices".into()));
            }
        }
        let budget = Arc::new(Budget::new());
        // Plane origin zero gives exactly the same world-origin screen coordinates as raw HLR.
        let basis = Plane { origin: Vec3::ZERO, x: view.x, y: view.y };
        let mut bodies = Vec::with_capacity(model.bodies.len());
        let mut dispositions = Vec::with_capacity(model.bodies.len());
        let mut caps = Vec::new();
        let mut segments = 0usize;
        let mut topology = 0usize;
        for (source_body, source) in model.bodies.iter().enumerate() {
            budget.check()?;
            if excluded.contains(&source_body) {
                dispositions.push(json!({"source_body":source_body,"source_placement":source_body,"result_body":null,"status":"excluded"}));
                continue;
            }
            if !source.closed || source.refused || source.body.is_mesh() {
                return Err(Error::Geometry(format!("section source body {source_body} is open, refused or mesh-only")));
            }
            let bounds = source.body.exact_bounds().map_err(|e| Error::Geometry(format!("section source body {source_body} bounds: {e}")))?;
            if !bounds.min.is_finite()
                || !bounds.max.is_finite()
                || [bounds.min.x, bounds.min.y, bounds.min.z, bounds.max.x, bounds.max.y, bounds.max.z].iter().any(|v| v.abs() > MAX_COORD)
            {
                return Err(Error::Limit("section source world coordinates"));
            }
            let (part, tool) = watch::watching(budget.clone(), || solidcraft_kernel::half_space(&source.body, &kept, true)).map_err(|e| {
                if budget.check().is_err() {
                    Error::Limit("section boolean work/deadline")
                } else {
                    Error::Geometry(format!("section source body {source_body} boolean: {e}"))
                }
            })?;
            budget.check()?;
            let Some(body) = part else {
                dispositions.push(json!({"source_body":source_body,"source_placement":source_body,"result_body":null,"status":"removed"}));
                continue;
            };
            let record = body.record();
            for count in [record.faces.len(), record.edges.len(), record.vertices.len(), record.shells.len()] {
                topology = topology.checked_add(count).ok_or(Error::Limit("section topology overflow"))?;
            }
            if topology > MAX_TOPOLOGY {
                return Err(Error::Limit("section result topology items"));
            }
            if body.is_mesh() || record.faces.is_empty() || record.edges.iter().any(|edge| edge.uses.len() != 2) {
                return Err(Error::Geometry(format!("section source body {source_body} did not produce a closed exact B-rep")));
            }
            let result_body = bodies.len();
            let first_cap = caps.len();
            // A box enclosing the entire uncut body cannot extend below this lower support.
            // A body wholly on the kept side merely touches the plane: never hatch its own face.
            let n = kept.normal();
            let half = bounds.size() * 0.5;
            let lowest = kept.height(bounds.center()) - (n.x.abs() * half.x + n.y.abs() * half.y + n.z.abs() * half.z);
            if lowest < -source.body.planar_edges_tolerance() {
                let history = body.history().filter(|h| h.operands == 2);
                if let Some(history) = history {
                    history
                        .check_counts(
                            (body.face_count(), body.edge_count()),
                            &[(source.body.face_count(), source.body.edge_count()), (tool.face_count(), tool.edge_count())],
                        )
                        .map_err(|e| Error::Geometry(format!("section source body {source_body} history: {e}")))?;
                }
                // A boolean may merge the cutter cap with an original coplanar face and
                // report only original-face ancestry for the merged face. History alone
                // cannot separate the material actually crossed by the plane.
                let mut original_faces = Vec::new();
                let mut original_regions = Vec::new();
                for (index, face) in source.body.record().faces.iter().enumerate() {
                    if facing_out_on(&source.body, index, &kept) {
                        original_faces.push(index);
                        original_regions.push(cap_region(&source.body, &face.loops, &basis, &mut segments, &budget)?);
                    }
                }
                for (face_index, face) in record.faces.iter().enumerate() {
                    budget.check()?;
                    if !facing_out_on(&body, face_index, &kept) {
                        continue;
                    }
                    // No geometric guess when original and generated coplanar faces coexist.
                    let history = history.ok_or_else(|| Error::Geometry(format!("section source body {source_body} cap provenance unavailable")))?;
                    let provenance = history.faces.get(face_index).ok_or_else(|| Error::Geometry("section cap history missing face".into()))?;
                    if provenance.origins.is_empty() {
                        return Err(Error::Geometry(format!("section source body {source_body} cap face {face_index} has no origins")));
                    }
                    if !provenance.origins.iter().all(|origin| match origin {
                        FaceOrigin::Face { operand: 0, face } => *face < source.body.face_count(),
                        FaceOrigin::Face { operand: 1, face } => facing_out_on(&tool, *face, &kept),
                        _ => false,
                    }) {
                        return Err(Error::Geometry(format!("section source body {source_body} cap origin is not a source face or half-space cap")));
                    }
                    let region = cap_region(&body, &face.loops, &basis, &mut segments, &budget).map_err(|e| match e {
                        Error::Geometry(why) => Error::Geometry(format!("section source body {source_body} cap face {face_index}: {why}")),
                        other => other,
                    })?;
                    let regions = if original_regions.is_empty() {
                        vec![region]
                    } else {
                        let tolerance = body.planar_edges_tolerance().max(source.body.planar_edges_tolerance());
                        watch::watching(budget.clone(), || {
                            solidcraft_kernel::planar_regions_boolean(&[region], &original_regions, solidcraft_kernel::BoolOp::Cut, tolerance)
                        })
                        .map_err(|e| Error::Geometry(format!("section cap minus original coplanar faces: {e}")))?
                    };
                    budget.check()?;
                    for region in regions {
                        for ring in std::iter::once(&region.outer).chain(&region.holes) {
                            segments = segments.checked_add(ring.segs.len()).ok_or(Error::Limit("section cap segment overflow"))?;
                            if segments > MAX_CAP_SEGMENTS {
                                return Err(Error::Limit("section cap segments"));
                            }
                            if !ring.is_closed(body.planar_edges_tolerance()) || ring.segs.is_empty() {
                                return Err(Error::Geometry("planar cap subtraction returned an open boundary".into()));
                            }
                            for segment in &ring.segs {
                                segment.check(MAX_COORD).map_err(|why| Error::Geometry(format!("invalid subtracted cap segment: {why}")))?;
                            }
                        }
                        caps.push(json!({"source_body":source_body,"source_placement":source_body,"result_body":result_body,
                            "face":face_index,"region":region,
                            "provenance":{"kind":"half_space_boolean_history","operands":["original_source_body","half_space_tool"],
                                "face_origins":provenance.origins,"change":provenance.change,
                                "excluded_original_coplanar_faces":original_faces,
                                "boundary_operation":"exact_planar_difference_from_original_coplanar_faces",
                                "planar_read_tolerance_mm":body.planar_edges_tolerance(),
                                "curve_contract":"upstream_exact_native_planar_read_with_reported_recognition_tolerance_not_symbolic_certificate",
                                "topology_identity":"section_result_local_only"}}));
                    }
                }
            }
            dispositions.push(json!({"source_body":source_body,"source_placement":source_body,"result_body":result_body,
                "status":if caps.len() > first_cap { "cut" } else { "retained" },
                "topology":{"faces":record.faces.len(),"edges":record.edges.len(),"vertices":record.vertices.len(),"shells":record.shells.len()}}));
            bodies.push(LoadedBody { body, closed: true, refused: false });
        }
        budget.check()?;
        let mut result = projection(
            model,
            &bodies,
            ViewQuery {
                source_sha256: request.source_sha256,
                direction: request.direction,
                up: request.up,
                require_analytic: request.require_analytic,
            },
            &budget,
        )
        .map_err(|e| match e {
            Error::Geometry(why) => Error::Geometry(format!("section HLR: {why}")),
            other => other,
        })?;
        result["protocol"] = json!("cadcraft-section/1");
        result["source_mapping"] = json!(
            "drawing placement/body and edge/face indices are SECTION-RESULT-LOCAL; section.bodies maps result_body to original v1.load source_body/source_placement, including removed/excluded bodies; never use cut indices with v1.references or v1.resolve_point"
        );
        result["section"] = json!({"plane":{"origin":plane.origin,"normal":plane.normal()},
            "kept_side":match request.kept_side { KeptSide::Positive => "positive", KeptSide::Negative => "negative" },
            "status":if bodies.is_empty() { "empty" } else { "nonempty" },"bodies":dispositions,"caps":caps,
            "topology_identity":"section_result_local_only",
            "cap_coordinates":"world_origin_screen_mm_same_basis_and_scale_as_drawing",
            "cap_contract":"outer_ccw_holes_cw_exact_segments_no_polygonization_no_paper_hatch_spacing",
            "atomicity":"all_selected_bodies_and_caps_or_error_source_snapshot_unchanged"});
        ensure_response_size(&result)?;
        Ok(result)
    }
}

fn facing_out_on(body: &Body, face: usize, plane: &Plane) -> bool {
    body.planar_face(face)
        .is_some_and(|on| on.normal().dot(plane.normal()) <= -FACING && plane.height(on.origin).abs() <= body.planar_edges_tolerance())
}

fn cap_segment(segment: Seg2, segs: &mut Vec<Seg2>, count: &mut usize) -> Result<(), Error> {
    segment.check(MAX_COORD).map_err(|why| Error::Geometry(format!("invalid cap segment: {why}")))?;
    if *count >= MAX_CAP_SEGMENTS {
        return Err(Error::Limit("section cap segments"));
    }
    *count += 1;
    segs.push(segment);
    Ok(())
}

/// Knot insertion only: no sampled points, curve recognition or fitting.
/// Positive-weight rational quadratics have an exact projective reparameterization
/// with endpoint weights one. Higher rational degrees have no Seg2 representation.
fn cap_spline(spline: PlanarSpline, segs: &mut Vec<Seg2>, count: &mut usize, budget: &Budget) -> Result<(), Error> {
    let n = spline.points.len();
    let degree = spline.degree;
    if !(1..=MAX_BEZIER_DEGREE).contains(&degree) || n > MAX_CAP_SEGMENTS * MAX_BEZIER_DEGREE + 1 || spline.knots.len() != n + degree + 1 {
        return Err(Error::Limit("section cap spline degree or controls"));
    }
    if (!spline.weights.is_empty() && spline.weights.len() != n) || spline.weights.iter().any(|w| !w.is_finite() || *w < f64::MIN_POSITIVE) {
        return Err(Error::Geometry("cap spline requires one positive finite weight per control, or no weights".into()));
    }
    let polynomial = spline.weights.first().is_none_or(|first| spline.weights.iter().all(|w| w == first));
    if !polynomial && degree > 2 {
        return Err(Error::Geometry("cap spline rational degree above two has no exact Seg2 representation (no fitting)".into()));
    }
    let (&lo, &hi) = spline.knots.get(degree).zip(spline.knots.get(n)).ok_or_else(|| Error::Geometry("cap spline domain missing".into()))?;
    let spans = spline.knots.windows(2).filter(|w| w[0] >= lo && w[1] <= hi && w[1] > w[0]).count();
    if spans > MAX_CAP_SEGMENTS.saturating_sub(*count) {
        return Err(Error::Limit("section cap spline spans"));
    }
    let mut controls = Vec::with_capacity(n);
    for (i, p) in spline.points.into_iter().enumerate() {
        budget.check()?;
        let weight = if polynomial { 1. } else { *spline.weights.get(i).ok_or_else(|| Error::Geometry("cap spline weight missing".into()))? };
        controls.push(HPoint::weighted(Vec3::new(p.x, p.y, 0.), weight));
    }
    let curve = NurbsCurve::from_homogeneous(degree, spline.knots, &controls).map_err(|e| Error::Geometry(format!("invalid cap spline: {e}")))?;
    drop(controls);
    budget.check()?;
    let pieces = curve.pieces().map_err(|e| Error::Geometry(format!("cap spline knot insertion: {e}")))?;
    segs.try_reserve(pieces.len()).map_err(|_| Error::Limit("section cap spline allocation"))?;
    for piece in pieces {
        budget.check()?;
        let mut points = [Vec2::ZERO; MAX_BEZIER_CTRL];
        let mut weights = [0.; MAX_BEZIER_CTRL];
        for (i, control) in piece.ctrl().enumerate() {
            let point = control.project().ok_or_else(|| Error::Geometry("nonfinite cap spline control".into()))?;
            let slot = points.get_mut(i).ok_or(Error::Limit("section cap Bezier controls"))?;
            *slot = Vec2::new(point.x, point.y);
            *weights.get_mut(i).ok_or(Error::Limit("section cap Bezier weights"))? = control.w;
        }
        let segment = if polynomial || degree == 1 {
            let points = points.get(..=degree).ok_or(Error::Limit("section cap Bezier degree"))?;
            solidcraft_geom::bezier(points).ok_or_else(|| Error::Geometry("cap polynomial span unavailable".into()))?
        } else {
            Seg2::Conic { a: points[0], apex: points[1], b: points[2], w: (weights[1] / weights[0].sqrt()) / weights[2].sqrt() }
        };
        cap_segment(segment, segs, count)?;
    }
    Ok(())
}

fn cap_region(body: &Body, loops: &[Loop], basis: &Plane, count: &mut usize, budget: &Budget) -> Result<Region2, Error> {
    if loops.len() > MAX_CAP_SEGMENTS {
        return Err(Error::Limit("section cap loops"));
    }
    let mut rings = Vec::with_capacity(loops.len());
    for ring in loops {
        budget.check()?;
        let Loop::Edges { edges } = ring else {
            return Err(Error::Geometry("vertex cap loops cannot be represented as an exact hatch boundary".into()));
        };
        if edges.len() > MAX_CAP_SEGMENTS.saturating_sub(*count) {
            return Err(Error::Limit("section cap edge uses"));
        }
        let mut segs = Vec::new();
        for used in edges {
            budget.check()?;
            let pieces = body
                .planar_edges(basis, &[used.edge as usize])
                .pop()
                .flatten()
                .ok_or_else(|| Error::Geometry(format!("cap edge {} has no exact planar read", used.edge)))?;
            let start = segs.len();
            for piece in pieces {
                match piece {
                    PlanarPiece::Seg(segment) => cap_segment(segment, &mut segs, count)?,
                    PlanarPiece::Spline(spline) => cap_spline(spline, &mut segs, count, budget)?,
                }
            }
            if !used.forward {
                let along = segs.get_mut(start..).ok_or_else(|| Error::Geometry("cap edge traversal range unavailable".into()))?;
                along.reverse();
                for segment in along {
                    *segment = segment.reversed();
                }
            }
        }
        let ring = Loop2 { segs };
        if ring.segs.is_empty() || !ring.is_closed(body.planar_edges_tolerance()) {
            return Err(Error::Geometry("cap boundary is empty or not closed within its native planar-read tolerance".into()));
        }
        let area = ring.signed_area();
        if !area.is_finite() || area == 0.0 {
            return Err(Error::Geometry("cap boundary has non-finite or degenerate area".into()));
        }
        rings.push(ring);
    }
    // Face loops are not ordered outer-first. The outer encloses all the others, hence has
    // the largest absolute area; reversing the view does not change that classification.
    let outline = rings
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.signed_area().abs().total_cmp(&b.signed_area().abs()))
        .map(|(index, _)| index)
        .ok_or_else(|| Error::Geometry("cap has no loops".into()))?;
    let outer = rings.remove(outline);
    if rings.iter().any(|ring| ring.is_ccw() == outer.is_ccw()) {
        return Err(Error::Geometry("cap topology has inconsistent outer/hole loop orientations".into()));
    }
    Ok(Region2 { outer: outer.ccw(), holes: rings.into_iter().map(|ring| ring.ccw().reversed()).collect() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_multi_span_cap_keeps_semicircle_with_nonunit_endpoint_weights() {
        let spline = PlanarSpline {
            degree: 2,
            knots: vec![0., 0., 0., 1., 1., 2., 2., 2.],
            points: vec![Vec2::new(1., 0.), Vec2::new(1., 1.), Vec2::new(0., 1.), Vec2::new(-1., 1.), Vec2::new(-1., 0.)],
            weights: vec![4., 2_f64.sqrt(), 1., 2_f64.sqrt(), 4.],
        };
        let mut segs = Vec::new();
        cap_spline(spline, &mut segs, &mut 0, &Budget::new()).unwrap();
        for segment in &segs {
            assert!(matches!(segment, Seg2::Conic { .. }));
            for t in [0., 0.25, 0.5, 0.75, 1.] {
                let p = segment.point_at(t);
                assert!((p.x * p.x + p.y * p.y - 1.).abs() < 1e-12);
                assert!(p.y >= 0.);
            }
        }
        segs.push(Seg2::Line { a: Vec2::new(-1., 0.), b: Vec2::new(1., 0.) });
        let ring = Loop2 { segs };
        assert!(ring.is_closed(1e-12));
        assert!((ring.signed_area() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    }

    #[test]
    fn polynomial_cap_inserts_knots_without_losing_area_or_native_curvature() {
        let spline = PlanarSpline {
            degree: 2,
            knots: vec![0., 0., 0., 1., 2., 2., 2.],
            points: vec![Vec2::ZERO, Vec2::new(1., 2.), Vec2::new(3., 2.), Vec2::new(4., 0.)],
            weights: vec![8.; 4],
        };
        let mut segs = Vec::new();
        cap_spline(spline, &mut segs, &mut 0, &Budget::new()).unwrap();
        assert!(segs.iter().all(|s| matches!(s, Seg2::Conic { .. })));
        segs.push(Seg2::Line { a: Vec2::new(4., 0.), b: Vec2::ZERO });
        let ring = Loop2 { segs };
        assert!(ring.is_closed(1e-12));
        assert!((ring.signed_area() + 16. / 3.).abs() < 1e-12);
    }
}
