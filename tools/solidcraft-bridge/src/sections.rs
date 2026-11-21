//! Exact one-plane sections. Result topology is deliberately separate from source references.

use super::{Budget, Error, LoadedBody, MAX_BODIES, MAX_TOPOLOGY, ViewQuery, Worker, ensure_response_size, params, projection, vector};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcore_record::{Loop2, Plane, Region2, Vec3, brep::Loop};
use solidcraft_geom::{Loop2Geom, PlaneGeom, Seg2Geom, Vec3Geom, hlr};
use solidcraft_kernel::{Body, PlanarPiece, history::FaceOrigin, watch};
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
                let PlanarPiece::Seg(segment) = piece else {
                    return Err(Error::Geometry(format!(
                        "cap edge {} is a multi-span spline; exact Seg2 hatch boundary unavailable (no fitting)",
                        used.edge
                    )));
                };
                segment.check(MAX_COORD).map_err(|why| Error::Geometry(format!("invalid cap segment: {why}")))?;
                *count = count.checked_add(1).ok_or(Error::Limit("section cap segment overflow"))?;
                if *count > MAX_CAP_SEGMENTS {
                    return Err(Error::Limit("section cap segments"));
                }
                segs.push(segment);
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
