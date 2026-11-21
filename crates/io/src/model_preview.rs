//! Bounded, display-only consumption of the isolated worker's `v1.preview` result.
//!
//! Positions are millimetres in world coordinates, already placed: do not transform again.
//! These approximate triangles are never a source for exact projections, dimensions or entities.

use cadcraft_doc::model_preview::{Preview, PreviewBody};
use cadcraft_geom::Vec3;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

pub const MAX_BODIES: usize = 32;
pub const MAX_TRIANGLES: usize = 100_000;
pub const MAX_VERTICES: usize = 200_000;
const MAX_TOPOLOGY: usize = 10_000;
const MAX_NUMBER: f64 = 1e12;
const MAX_NODES: usize = 2_000_000;
const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PreviewError {
    #[error("invalid worker preview: {0}")]
    Invalid(&'static str),
    #[error("worker preview resource limit: {0}")]
    Limit(&'static str),
    #[error("worker preview belongs to another source")]
    StaleSource,
    #[error("unsupported worker preview contract: {0}")]
    Unsupported(&'static str),
}

/// Consume a worker result atomically, validating all counts before allocating typed meshes.
/// No fitting, repair, scaling, placement, persistent entity or exact-geometry conversion occurs.
pub fn from_worker(mut value: Value, expected_sha256: &str) -> Result<Preview, PreviewError> {
    preflight(&value, expected_sha256)?;
    let root = value.as_object_mut().ok_or(PreviewError::Invalid("result object"))?;
    let mut source = take(root, "source")?;
    let source = source.as_object_mut().ok_or(PreviewError::Invalid("source"))?;
    let source_sha256 = typed(take(source, "sha256")?, "source SHA-256")?;
    let source_bytes = typed(take(source, "bytes")?, "source byte count")?;
    let detail = typed(take(root, "detail")?, "detail")?;
    let quality = take(root, "quality")?;
    let input_bodies = into_array(take(root, "bodies")?)?;
    let mut bodies = Vec::with_capacity(input_bodies.len());
    for body in input_bodies {
        let mut body = match body {
            Value::Object(body) => body,
            _ => return Err(PreviewError::Invalid("body object")),
        };
        let input_positions = into_array(take(&mut body, "positions")?)?;
        let mut positions = Vec::with_capacity(input_positions.len());
        for position in input_positions {
            let [x, y, z]: [f64; 3] = typed(position, "position")?;
            positions.push(Vec3::new(x, y, z));
        }
        bodies.push(PreviewBody {
            placement: typed(take(&mut body, "placement")?, "placement")?,
            positions,
            triangles: typed(take(&mut body, "triangles")?, "triangles")?,
            tri_face: typed(take(&mut body, "tri_face")?, "triangle faces")?,
            missing_faces: typed(take(&mut body, "missing_faces")?, "missing faces")?,
            refused: into_array(take(&mut body, "refused")?)?,
            requested_chord_mm: typed(take(&mut body, "requested_chord_mm")?, "requested chord")?,
            effective_chord_mm: typed(take(&mut body, "effective_chord_mm")?, "effective chord")?,
            display_angle_radians: typed(take(&mut body, "display_angle_radians")?, "display angle")?,
            metadata: body,
        });
    }
    Ok(Preview { source_sha256, source_bytes, detail, bodies, quality })
}

fn field<'a>(value: &'a Value, key: &'static str) -> Result<&'a Value, PreviewError> {
    value.get(key).ok_or(PreviewError::Invalid(key))
}

fn array(value: &Value) -> Result<&[Value], PreviewError> {
    value.as_array().map(Vec::as_slice).ok_or(PreviewError::Invalid("array"))
}

fn index(value: &Value) -> Result<usize, PreviewError> {
    value.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or(PreviewError::Invalid("index/count"))
}

fn number(value: &Value) -> Result<f64, PreviewError> {
    value.as_f64().filter(|n| n.is_finite() && n.abs() <= MAX_NUMBER).ok_or(PreviewError::Invalid("finite bounded number"))
}

fn take(object: &mut Map<String, Value>, key: &'static str) -> Result<Value, PreviewError> {
    object.remove(key).ok_or(PreviewError::Invalid(key))
}

fn into_array(value: Value) -> Result<Vec<Value>, PreviewError> {
    match value {
        Value::Array(items) => Ok(items),
        _ => Err(PreviewError::Invalid("array")),
    }
}

fn typed<T: DeserializeOwned>(value: Value, name: &'static str) -> Result<T, PreviewError> {
    serde_json::from_value(value).map_err(|_| PreviewError::Invalid(name))
}

fn add_count(total: &mut usize, count: usize, limit: usize, name: &'static str) -> Result<(), PreviewError> {
    *total = total.checked_add(count).filter(|n| *n <= limit).ok_or(PreviewError::Limit(name))?;
    Ok(())
}

fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn preflight(value: &Value, expected_sha256: &str) -> Result<(), PreviewError> {
    let source = field(value, "source")?;
    let sha256 = field(source, "sha256")?.as_str().ok_or(PreviewError::Invalid("source SHA-256"))?;
    if !digest(sha256) || !digest(expected_sha256) {
        return Err(PreviewError::Invalid("source SHA-256"));
    }
    if sha256 != expected_sha256 {
        return Err(PreviewError::StaleSource);
    }
    field(source, "bytes")?.as_u64().filter(|n| *n <= 8 * 1024 * 1024).ok_or(PreviewError::Invalid("source byte count"))?;
    for (key, expected) in [
        ("purpose", "display_only_not_measurement_or_projection"),
        ("placement_convention", "world_coordinates_already_placed_do_not_transform_again"),
    ] {
        if field(value, key)?.as_str() != Some(expected) {
            return Err(PreviewError::Unsupported(key));
        }
    }
    let units = field(value, "units")?;
    for (key, expected) in [
        ("length", "mm"),
        ("area", "mm2"),
        ("volume", "mm3"),
        ("angle", "radian"),
        ("coordinate_system", "right_handed_z_up"),
        ("original_declaration", "not_exposed_by_selected_import_api"),
    ] {
        if field(units, key)?.as_str() != Some(expected) {
            return Err(PreviewError::Unsupported("normalized millimetre units"));
        }
    }
    if !(1..=8).contains(&index(field(value, "detail")?)?) {
        return Err(PreviewError::Invalid("detail"));
    }
    let bodies = array(field(value, "bodies")?)?;
    if bodies.is_empty() {
        return Err(PreviewError::Invalid("empty bodies"));
    }
    if bodies.len() > MAX_BODIES {
        return Err(PreviewError::Limit("bodies"));
    }
    let (mut vertices, mut triangles, mut missing, mut refused) = (0, 0, 0, 0);
    // Admission precedes even number/metadata traversal and all typed output allocation.
    for body in bodies {
        add_count(&mut vertices, array(field(body, "positions")?)?.len(), MAX_VERTICES, "vertices")?;
        let triangle_count = array(field(body, "triangles")?)?.len();
        add_count(&mut triangles, triangle_count, MAX_TRIANGLES, "triangles")?;
        if array(field(body, "tri_face")?)?.len() != triangle_count {
            return Err(PreviewError::Invalid("triangle face count"));
        }
        add_count(&mut missing, array(field(body, "missing_faces")?)?.len(), MAX_TOPOLOGY, "missing faces")?;
        add_count(&mut refused, array(field(body, "refused")?)?.len(), MAX_TOPOLOGY, "refused faces")?;
    }
    let counts = field(value, "counts")?;
    if index(field(counts, "vertices")?)? != vertices || index(field(counts, "triangles")?)? != triangles {
        return Err(PreviewError::Invalid("aggregate counts"));
    }
    let quality = field(value, "quality")?;
    if field(quality, "bounds_certified")?.as_bool() != Some(false) || index(field(quality, "missing_faces")?)? != missing {
        return Err(PreviewError::Invalid("quality declaration"));
    }
    if !field(quality, "notes")?.is_string() {
        return Err(PreviewError::Invalid("quality notes"));
    }
    scan_value(value, 0, &mut 0, &mut 0)?;
    let mut faces = 0;
    for (placement, body) in bodies.iter().enumerate() {
        if index(field(body, "placement")?)? != placement {
            return Err(PreviewError::Invalid("placement sequence"));
        }
        let requested = number(field(body, "requested_chord_mm")?)?;
        let effective = number(field(body, "effective_chord_mm")?)?;
        let angle = number(field(body, "display_angle_radians")?)?;
        if requested < 1e-3 || effective < requested || angle <= 0.0 || angle > std::f64::consts::PI {
            return Err(PreviewError::Invalid("display tolerance"));
        }
        let positions = array(field(body, "positions")?)?;
        for position in positions {
            let components = array(position)?;
            if components.len() != 3 {
                return Err(PreviewError::Invalid("position components"));
            }
            for component in components {
                number(component)?;
            }
        }
        for triangle in array(field(body, "triangles")?)? {
            let indices = array(triangle)?;
            if indices.len() != 3 {
                return Err(PreviewError::Invalid("triangle indices"));
            }
            for i in indices {
                if index(i)? >= positions.len() {
                    return Err(PreviewError::Invalid("triangle vertex index"));
                }
            }
        }
        // The worker has no face-count field. Its present/missing partition must nevertheless
        // cover a contiguous local face range, with at most 10,000 faces across all bodies.
        let mut face_state = [0_u8; MAX_TOPOLOGY];
        let mut face_count = 0;
        for face in array(field(body, "tri_face")?)? {
            let face = face_index(face)?;
            face_state[face] = 1;
            face_count = face_count.max(face + 1);
        }
        for face in array(field(body, "missing_faces")?)? {
            let face = face_index(face)?;
            if face_state[face] != 0 {
                return Err(PreviewError::Invalid("missing face partition"));
            }
            face_state[face] = 2;
            face_count = face_count.max(face + 1);
        }
        if face_state[..face_count].contains(&0) {
            return Err(PreviewError::Invalid("face coverage"));
        }
        add_count(&mut faces, face_count, MAX_TOPOLOGY, "faces")?;
        for refusal in array(field(body, "refused")?)? {
            let face = face_index(field(refusal, "face")?)?;
            if face_state[face] != 2 || !field(refusal, "reason")?.is_string() {
                return Err(PreviewError::Invalid("refused face declaration"));
            }
            face_state[face] = 3;
        }
    }
    Ok(())
}

fn face_index(value: &Value) -> Result<usize, PreviewError> {
    let face = index(value)?;
    if face >= MAX_TOPOLOGY {
        return Err(PreviewError::Invalid("face index"));
    }
    Ok(face)
}

// Bound retained diagnostics as well as geometry; no input-sized scratch vectors or clones.
fn scan_value(value: &Value, depth: usize, nodes: &mut usize, text_bytes: &mut usize) -> Result<(), PreviewError> {
    if depth > 32 {
        return Err(PreviewError::Limit("nesting"));
    }
    add_count(nodes, 1, MAX_NODES, "JSON nodes")?;
    match value {
        Value::Array(items) => {
            if items.len() > MAX_NODES.saturating_sub(*nodes) {
                return Err(PreviewError::Limit("JSON nodes"));
            }
            for item in items {
                scan_value(item, depth + 1, nodes, text_bytes)?;
            }
        }
        Value::Object(items) => {
            for (key, item) in items {
                add_count(text_bytes, key.len(), MAX_TEXT_BYTES, "JSON text")?;
                scan_value(item, depth + 1, nodes, text_bytes)?;
            }
        }
        Value::String(text) => add_count(text_bytes, text.len(), MAX_TEXT_BYTES, "JSON text")?,
        Value::Number(value) if value.as_f64().is_none_or(|n| !n.is_finite()) => return Err(PreviewError::Invalid("finite number")),
        Value::Number(_) | Value::Null | Value::Bool(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn result() -> Value {
        json!({
            "source":{"sha256":SHA,"bytes":42},
            "units":{"length":"mm","area":"mm2","volume":"mm3","angle":"radian",
                "coordinate_system":"right_handed_z_up","original_declaration":"not_exposed_by_selected_import_api"},
            "purpose":"display_only_not_measurement_or_projection",
            "placement_convention":"world_coordinates_already_placed_do_not_transform_again",
            "detail":4,"counts":{"vertices":3,"triangles":1},
            "quality":{"missing_faces":1,"bounds_certified":false,"notes":"Not certified","diagnostic":"retained"},
            "bodies":[{"placement":0,"positions":[[2,3,4],[3,3,4],[2,4,4]],"triangles":[[0,1,2]],"tri_face":[0],
                "missing_faces":[1],"refused":[{"face":1,"reason":"unsupported surface","diagnostic":{"code":7}}],
                "requested_chord_mm":0.01,"effective_chord_mm":0.01,"display_angle_radians":0.3490658503988659,
                "quality":{"note":"retained per body"}}]
        })
    }

    #[test]
    fn consumes_world_positions_and_preserves_missing_refused_and_quality() -> Result<(), PreviewError> {
        let preview = from_worker(result(), SHA)?;
        assert_eq!(preview.source_sha256, SHA);
        assert_eq!(preview.source_bytes, 42);
        assert_eq!(preview.detail, 4);
        let body = &preview.bodies[0];
        assert_eq!(body.positions[0], Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(body.triangles, [[0, 1, 2]]);
        assert_eq!(body.tri_face, [0]);
        assert_eq!(body.missing_faces, [1]);
        assert_eq!(body.refused[0]["reason"], "unsupported surface");
        assert_eq!(body.refused[0]["diagnostic"]["code"], 7);
        assert_eq!(body.metadata["quality"]["note"], "retained per body");
        assert_eq!(preview.quality["diagnostic"], "retained");
        Ok(())
    }

    #[test]
    fn rejects_stale_source_and_unsupported_display_contracts() {
        assert_eq!(from_worker(result(), &"a".repeat(64)), Err(PreviewError::StaleSource));
        for (key, replacement) in [
            ("purpose", json!("measurement")),
            ("placement_convention", json!("apply_transform")),
            ("units", json!({"length":"inch"})),
            ("detail", json!(9)),
        ] {
            let mut input = result();
            input[key] = replacement;
            assert!(from_worker(input, SHA).is_err());
        }
    }

    #[test]
    fn rejects_invalid_vertex_and_face_references_without_partial_meshes() {
        for (key, replacement) in [
            ("triangles", json!([[0, 1, 3]])),
            ("triangles", json!([[0, 1]])),
            ("tri_face", json!([])),
            ("tri_face", json!([10_000])),
            ("missing_faces", json!([0])),
            ("refused", json!([{"face":0,"reason":"not missing"}])),
        ] {
            let mut input = result();
            input["bodies"][0][key] = replacement;
            assert!(from_worker(input, SHA).is_err(), "accepted {key}");
        }
    }

    #[test]
    fn rejects_nonfinite_or_unbounded_coordinates_and_tolerances() {
        for coordinate in [json!(f64::INFINITY), json!(f64::NAN), json!(1e13), json!("NaN")] {
            let mut input = result();
            input["bodies"][0]["positions"][0][0] = coordinate;
            assert!(from_worker(input, SHA).is_err());
        }
        let mut input = result();
        input["bodies"][0]["effective_chord_mm"] = json!(0.001);
        assert_eq!(from_worker(input, SHA), Err(PreviewError::Invalid("display tolerance")));
    }

    #[test]
    fn rejects_false_quality_and_declared_counts() {
        for (section, key, replacement) in [
            ("quality", "bounds_certified", json!(true)),
            ("quality", "missing_faces", json!(0)),
            ("counts", "vertices", json!(4)),
            ("counts", "triangles", json!(0)),
        ] {
            let mut input = result();
            input[section][key] = replacement;
            assert!(from_worker(input, SHA).is_err());
        }
    }

    #[test]
    fn admits_global_counts_before_geometry_conversion() {
        let mut input = result();
        input["bodies"] = Value::Array(vec![json!({}); MAX_BODIES + 1]);
        assert_eq!(from_worker(input, SHA), Err(PreviewError::Limit("bodies")));
        let mut input = result();
        input["bodies"][0]["triangles"] = Value::Array(vec![Value::Null; MAX_TRIANGLES + 1]);
        assert_eq!(from_worker(input, SHA), Err(PreviewError::Limit("triangles")));
        let mut input = result();
        input["bodies"][0]["positions"] = Value::Array(vec![Value::Null; MAX_VERTICES + 1]);
        assert_eq!(from_worker(input, SHA), Err(PreviewError::Limit("vertices")));
        let mut input = result();
        let mut first = input["bodies"][0].clone();
        first["triangles"] = Value::Array(vec![Value::Null; MAX_TRIANGLES / 2 + 1]);
        first["tri_face"] = Value::Array(vec![Value::Null; MAX_TRIANGLES / 2 + 1]);
        input["bodies"] = json!([first, first]);
        assert_eq!(from_worker(input, SHA), Err(PreviewError::Limit("triangles")));
    }
}
