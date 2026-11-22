//! Exact cut results. Result-local topology is never an original model reference.
use crate::{HatchEdge, HatchLoop, model_projection::Projection};
use cadcraft_geom::Bounds2;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plane {
    pub origin: [f64; 3],
    /// Requests may use a nonunit normal; decoded results always use a unit normal.
    pub normal: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeptSide {
    Positive,
    Negative,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SectionRequest {
    pub plane: Plane,
    pub kept_side: KeptSide,
    /// Sight ray into the retained half-space, perpendicular to the cutting plane.
    pub direction: [f64; 3],
    pub up: [f64; 3],
    /// Optional native cap-XY unit axis for exact material-region symmetry analysis.
    pub symmetry_axis: Option<[f64; 2]>,
    /// Original loaded body indices omitted entirely, not uncut assembly exceptions.
    pub exclude_bodies: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionStatus {
    Empty,
    Nonempty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyStatus {
    Excluded,
    Removed,
    Retained,
    Cut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SectionTopology {
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub shells: usize,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BodyDisposition {
    pub source_body: usize,
    pub source_placement: usize,
    pub result_body: Option<usize>,
    pub status: BodyStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topology: Option<SectionTopology>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SectionCap {
    pub source_body: usize,
    pub source_placement: usize,
    pub result_body: usize,
    pub face: usize,
    /// Native directed edge paths: one CCW outer boundary followed by CW holes.
    /// All points share the projection's world-origin screen-millimetre basis.
    pub loops: Vec<HatchLoop>,
    /// Boolean history and planar recognition/subtraction audit, without geometry duplication.
    pub provenance: Value,
}

impl SectionCap {
    /// Conservative native outer bounds, without tessellation or geometry allocation.
    /// Unsupported or malformed boundaries have no trustworthy bounds.
    pub fn conservative_bounds(&self) -> Option<Bounds2> {
        let outer = self.loops.first().filter(|ring| ring.outer && ring.vertices.is_empty() && !ring.edges.is_empty())?;
        let mut bounds = Bounds2::EMPTY;
        for edge in &outer.edges {
            if !edge.is_valid() {
                return None;
            }
            let edge_bounds = match edge {
                HatchEdge::Segment(segment) => segment.bounds(),
                HatchEdge::Spline(s) => {
                    if s.weights.is_empty() || s.weights.iter().all(|w| *w > 0.0) {
                        Bounds2::from_points(s.control.iter().copied())
                    } else {
                        // Negative conic weights invalidate the ordinary control hull.
                        // Homogeneous subdivision at one half gives two positive-weight
                        // hulls for the supported nonpolar rational quadratic.
                        let [a, p, b] = s.control.as_slice() else { return None };
                        let [wa, w, wb] = s.weights.as_slice() else { return None };
                        if s.degree != 2
                            || *wa != 1.0
                            || *wb != 1.0
                            || *w <= -1.0
                            || *w > 0.0
                            || s.knots.len() != 6
                            || s.knots[..3].iter().any(|k| *k != s.knots[0])
                            || s.knots[3..].iter().any(|k| *k != s.knots[5])
                        {
                            return None;
                        }
                        let denominator = 1.0 + *w;
                        Bounds2::from_points([
                            *a,
                            (*a + *p * *w) / denominator,
                            (*a + *p * (2.0 * *w) + *b) / (2.0 * denominator),
                            (*p * *w + *b) / denominator,
                            *b,
                        ])
                    }
                }
                _ => return None,
            };
            if edge_bounds.is_empty() || !edge_bounds.min.is_finite() || !edge_bounds.max.is_finite() {
                return None;
            }
            bounds = bounds.union(&edge_bounds);
        }
        Some(bounds)
    }
}

/// Native numeric comparison of the entire cap, including holes, about the requested axis.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SectionSymmetry {
    pub axis: [f64; 2],
    pub center: [f64; 2],
    pub symmetric: bool,
    pub tolerance_mm: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub projection: Projection,
    pub plane: Plane,
    pub kept_side: KeptSide,
    pub status: SectionStatus,
    /// One entry per original loaded body, including excluded and removed bodies.
    pub bodies: Vec<BodyDisposition>,
    pub caps: Vec<SectionCap>,
    /// Present exactly when an axis was requested and the result contains one cap.
    pub symmetry: Option<SectionSymmetry>,
}
