//! Transient, display-only model preview data shared by native IO and the renderer.
//!
//! Positions are millimetres in world coordinates, already placed: do not transform again.
//! These approximate triangles are not database entities, exact projections or measurement data.

use cadcraft_geom::Vec3;
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    pub source_sha256: String,
    pub source_bytes: u64,
    pub detail: u8,
    pub bodies: Vec<PreviewBody>,
    /// Uncertified upstream quality declaration, including notes and future diagnostics.
    pub quality: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreviewBody {
    pub placement: usize,
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub tri_face: Vec<u32>,
    pub missing_faces: Vec<usize>,
    /// Original refusal records (face, reason and any additional diagnostic fields).
    pub refused: Vec<Value>,
    pub requested_chord_mm: f64,
    pub effective_chord_mm: f64,
    pub display_angle_radians: f64,
    /// Additional per-body metadata is preserved, not silently discarded.
    pub metadata: Map<String, Value>,
}
