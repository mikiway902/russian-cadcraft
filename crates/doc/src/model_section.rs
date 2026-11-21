//! Exact cut results. Result-local topology is never an original model reference.
use crate::{HatchLoop, model_projection::Projection};
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

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub projection: Projection,
    pub plane: Plane,
    pub kept_side: KeptSide,
    pub status: SectionStatus,
    /// One entry per original loaded body, including excluded and removed bodies.
    pub bodies: Vec<BodyDisposition>,
    pub caps: Vec<SectionCap>,
}
