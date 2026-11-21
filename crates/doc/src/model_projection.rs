//! Exact projected curves and their payload-scoped provenance, shared by native I/O and engine.
//! Display tessellation is deliberately a different type and cannot supply these curves.
use crate::EntityKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Debug, PartialEq)]
pub struct Projection {
    pub entities: Vec<ProjectedEntity>,
    pub source_sha256: String,
    pub source_bytes: u64,
    pub basis: Basis,
    pub quality: Quality,
    /// Entity `line_index` indexes this provenance table. Empty pieces are retained too.
    pub lines: Vec<LineProvenance>,
    pub unsupported: Vec<UnsupportedFace>,
    pub prepare_reports: Value,
    pub drawing_stats: Value,
    pub source_mapping: String,
    pub visibility_contract: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Basis {
    pub x: [f64; 3],
    pub y: [f64; 3],
    /// Sight direction; the worker screen normal is **minus** this vector.
    pub direction: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Quality {
    pub unsupported_faces: usize,
    pub uncertain_pieces: usize,
    pub bounded_pieces: usize,
    pub sampled_pieces: usize,
    pub unsupported_trims: usize,
    pub import_issues: usize,
    pub open_bodies: usize,
    pub analytic_complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Visible,
    Hidden,
    Uncertain,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Accuracy {
    Analytic,
    Bounded { max_deviation_mm: f64 },
    Sampled { max_sample_deviation_mm: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceReference {
    Edge { edge: usize, tangent: bool },
    Silhouette { face: usize },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LineProvenance {
    pub placement: usize,
    pub source: SourceReference,
    pub pieces: Vec<PieceProvenance>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PieceProvenance {
    /// Original source-curve interval, NOT an inferred per-segment interval.
    pub t: [f64; 2],
    pub visibility: Visibility,
    pub covered: bool,
    pub accuracy: Accuracy,
    pub segment_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectedEntity {
    pub kind: EntityKind,
    pub line_index: usize,
    pub piece_index: usize,
    pub segment_index: usize,
    pub t: [f64; 2],
    pub visibility: Visibility,
    pub covered: bool,
    pub accuracy: Accuracy,
    /// Native arcs are CCW; true means their traversal reverses the original source segment.
    pub parameter_reversed: bool,
    /// Retains the original signed angular parametrization, including a circle's seam.
    pub source_arc: Option<ArcParameters>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ArcParameters {
    pub start: f64,
    pub sweep: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UnsupportedFace {
    pub placement: usize,
    pub face: usize,
    pub reason: String,
}
