//! Exact, bounded conversion of the isolated worker's `v1.view` result.
//!
//! Coordinates stay in model millimetres. Placement, visibility policy, entity handles and
//! database mutation belong to the caller. Analytic describes the producer's representation,
//! not a certificate of topology or of complete hidden-line removal.

use std::f64::consts::TAU;

#[cfg(test)]
use cadcraft_doc::model_projection::SourceReference;
use cadcraft_doc::model_projection::{
    Accuracy, ArcParameters, Basis, LineProvenance, PieceProvenance, ProjectedEntity, Projection, Quality, UnsupportedFace, Visibility,
};
use cadcraft_doc::{Arc, Circle, EntityKind, Line};
use cadcraft_geom::{Spline, Vec2, Vec3};
use serde::Deserialize;
use serde_json::Value;

/// Limits apply to aggregate input counts, before output/control-vector allocation.
pub const MAX_LINES: usize = 4096;
pub const MAX_PIECES: usize = 100_000;
pub const MAX_SEGMENTS: usize = 100_000;
pub const MAX_CONTROLS: usize = 1_000_000;
const MAX_NODES: usize = 2_000_000;
const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_NUMBER: f64 = 1e12;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("invalid worker projection: {0}")]
    Invalid(&'static str),
    #[error("worker projection resource limit: {0}")]
    Limit(&'static str),
    #[error("unsupported worker segment type: {0}")]
    UnknownSegment(String),
}

/// Decode atomically: any malformed/unknown segment fails the whole result. No geometry is
/// fitted, sampled, repaired, translated or scaled. Covered and uncertain pieces survive.
pub fn decode(result: &Value) -> Result<Projection, Error> {
    let segment_count = preflight(result)?;
    let source = field(result, "source")?;
    let sha256 = text(field(source, "sha256")?)?;
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Invalid("source SHA-256"));
    }
    let source_bytes = field(source, "bytes")?.as_u64().filter(|&n| n <= 8 * 1024 * 1024).ok_or(Error::Invalid("source byte count"))?;
    let units = field(result, "units")?;
    for (key, expected) in [("length", "mm"), ("area", "mm2"), ("volume", "mm3"), ("angle", "radian"), ("coordinate_system", "right_handed_z_up")] {
        if text(field(units, key)?)? != expected {
            return Err(Error::Invalid("normalized millimetre units"));
        }
    }
    if number(field(result, "scale")?)? != 1.0 {
        return Err(Error::Invalid("scale must be one"));
    }
    let basis = read_basis(field(result, "basis")?)?;
    let quality: Quality = typed(field(result, "quality")?, "quality")?;
    let reports = field(result, "prepare_reports")?;
    array(reports)?;
    let drawing = field(result, "drawing")?;
    let stats = field(drawing, "stats")?;
    if !stats.is_object() {
        return Err(Error::Invalid("drawing stats"));
    }
    let unsupported: Vec<UnsupportedFace> = typed(field(drawing, "unsupported")?, "unsupported faces")?;
    let input_lines = array(field(drawing, "lines")?)?;
    let mut lines = Vec::with_capacity(input_lines.len());
    let mut entities = Vec::with_capacity(segment_count);
    let (mut uncertain, mut bounded, mut sampled) = (0, 0, 0);
    for (line_index, line) in input_lines.iter().enumerate() {
        let placement = index(field(line, "placement")?)?;
        let source = typed(field(line, "source")?, "source reference")?;
        let input_pieces = array(field(line, "pieces")?)?;
        let mut pieces = Vec::with_capacity(input_pieces.len());
        for (piece_index, piece) in input_pieces.iter().enumerate() {
            let t = numbers::<2>(field(piece, "t")?)?;
            if t[0] > t[1] {
                return Err(Error::Invalid("source parameter interval"));
            }
            let visibility = typed(field(piece, "visibility")?, "visibility")?;
            let covered = field(piece, "covered")?.as_bool().ok_or(Error::Invalid("covered flag"))?;
            let accuracy = read_accuracy(field(piece, "accuracy")?)?;
            uncertain += usize::from(visibility == Visibility::Uncertain);
            match accuracy {
                Accuracy::Analytic => {}
                Accuracy::Bounded { .. } => bounded += 1,
                Accuracy::Sampled { .. } => sampled += 1,
            }
            let geometry = array(field(piece, "geometry")?)?;
            for (segment_index, segment) in geometry.iter().enumerate() {
                let (kind, source_arc) = convert_segment(segment)?;
                entities.push(ProjectedEntity {
                    kind,
                    line_index,
                    piece_index,
                    segment_index,
                    t,
                    visibility,
                    covered,
                    accuracy,
                    parameter_reversed: source_arc.is_some_and(|arc| arc.sweep < 0.0),
                    source_arc,
                });
            }
            pieces.push(PieceProvenance { t, visibility, covered, accuracy, segment_count: geometry.len() });
        }
        lines.push(LineProvenance { placement, source, pieces });
    }
    let complete = unsupported.is_empty()
        && uncertain == 0
        && bounded == 0
        && sampled == 0
        && quality.unsupported_trims == 0
        && quality.import_issues == 0
        && quality.open_bodies == 0;
    if quality.unsupported_faces != unsupported.len()
        || quality.uncertain_pieces != uncertain
        || quality.bounded_pieces != bounded
        || quality.sampled_pieces != sampled
        || quality.analytic_complete != complete
    {
        return Err(Error::Invalid("inconsistent quality declaration"));
    }
    Ok(Projection {
        entities,
        source_sha256: sha256.to_owned(),
        source_bytes,
        basis,
        quality,
        lines,
        unsupported,
        prepare_reports: reports.clone(),
        drawing_stats: stats.clone(),
        source_mapping: text(field(result, "source_mapping")?)?.to_owned(),
        visibility_contract: text(field(result, "visibility_contract")?)?.to_owned(),
    })
}

fn field<'a>(value: &'a Value, key: &'static str) -> Result<&'a Value, Error> {
    value.get(key).ok_or(Error::Invalid(key))
}

fn array(value: &Value) -> Result<&[Value], Error> {
    value.as_array().map(Vec::as_slice).ok_or(Error::Invalid("array"))
}

fn text(value: &Value) -> Result<&str, Error> {
    value.as_str().ok_or(Error::Invalid("string"))
}

fn number(value: &Value) -> Result<f64, Error> {
    value.as_f64().filter(|n| n.is_finite()).ok_or(Error::Invalid("finite number"))
}

fn index(value: &Value) -> Result<usize, Error> {
    value.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or(Error::Invalid("record index"))
}

fn typed<'a, T: Deserialize<'a>>(value: &'a Value, name: &'static str) -> Result<T, Error> {
    T::deserialize(value).map_err(|_| Error::Invalid(name))
}

fn numbers<const N: usize>(value: &Value) -> Result<[f64; N], Error> {
    let input = array(value)?;
    if input.len() != N {
        return Err(Error::Invalid("coordinate count"));
    }
    let mut output = [0.0; N];
    for (out, item) in output.iter_mut().zip(input) {
        *out = number(item)?;
    }
    Ok(output)
}

fn point(value: &Value) -> Result<Vec2, Error> {
    let [x, y] = numbers(value)?;
    if x.abs() > MAX_NUMBER || y.abs() > MAX_NUMBER {
        return Err(Error::Invalid("coordinate magnitude"));
    }
    Ok(Vec2::new(x, y))
}

fn point3(value: &Value) -> Result<Vec3, Error> {
    let p = point(value)?;
    Ok(Vec3::new(p.x, p.y, 0.0))
}

fn read_accuracy(value: &Value) -> Result<Accuracy, Error> {
    let accuracy = typed(value, "accuracy")?;
    let deviation = match accuracy {
        Accuracy::Analytic => return Ok(accuracy),
        Accuracy::Bounded { max_deviation_mm } => max_deviation_mm,
        Accuracy::Sampled { max_sample_deviation_mm } => max_sample_deviation_mm,
    };
    if !deviation.is_finite() || !(0.0..=MAX_NUMBER).contains(&deviation) {
        return Err(Error::Invalid("accuracy deviation"));
    }
    Ok(accuracy)
}

fn read_basis(value: &Value) -> Result<Basis, Error> {
    let basis = Basis { x: numbers(field(value, "x")?)?, y: numbers(field(value, "y")?)?, direction: numbers(field(value, "direction")?)? };
    let dot = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
    for v in [basis.x, basis.y, basis.direction] {
        if (dot(v, v) - 1.0).abs() > 1e-8 {
            return Err(Error::Invalid("unit view basis"));
        }
    }
    for (a, b) in [(basis.x, basis.y), (basis.x, basis.direction), (basis.y, basis.direction)] {
        if dot(a, b).abs() > 1e-8 {
            return Err(Error::Invalid("orthogonal view basis"));
        }
    }
    let [x, y, z] = basis.x;
    let [a, b, c] = basis.y;
    let cross = [y * c - z * b, z * a - x * c, x * b - y * a];
    if cross.iter().zip(basis.direction).any(|(a, b)| (a + b).abs() > 1e-8) {
        return Err(Error::Invalid("view basis handedness"));
    }
    Ok(basis)
}

fn bezier(control: Vec<Vec2>, weights: Vec<f64>) -> EntityKind {
    let degree = control.len() - 1;
    let mut knots = Vec::with_capacity(2 * (degree + 1));
    knots.resize(degree + 1, 0.0);
    knots.resize(2 * (degree + 1), 1.0);
    EntityKind::Spline(Spline { degree, knots, control, weights, ..Spline::default() })
}

fn convert_segment(value: &Value) -> Result<(EntityKind, Option<ArcParameters>), Error> {
    let kind = match text(field(value, "type")?)? {
        "line" => EntityKind::Line(Line { a: point3(field(value, "a")?)?, b: point3(field(value, "b")?)? }),
        "arc" => return convert_arc(value),
        "cubic" => bezier(
            vec![point(field(value, "p0")?)?, point(field(value, "p1")?)?, point(field(value, "p2")?)?, point(field(value, "p3")?)?],
            Vec::new(),
        ),
        "conic" => {
            let w = number(field(value, "w")?)?;
            // D(t)=(1-t)^2+2*w*t*(1-t)+t^2 has no zero on [0,1] iff w > -1.
            // Zero and negative non-polar weights remain exact rational curves, not chords.
            if w <= -1.0 || w > MAX_NUMBER {
                return Err(Error::Invalid("rational conic weight/pole"));
            }
            bezier(vec![point(field(value, "a")?)?, point(field(value, "apex")?)?, point(field(value, "b")?)?], vec![1.0, w, 1.0])
        }
        "bezier" => {
            let n = index(field(value, "n")?)?;
            if !(4..=9).contains(&n) {
                return Err(Error::Invalid("Bezier degree"));
            }
            let input = array(field(value, "p")?)?;
            if input.len() != 10 {
                return Err(Error::Invalid("Bezier control count"));
            }
            // Validate the unused serialized tail too; never pass it to the native spline.
            let mut control = Vec::with_capacity(n + 1);
            for (i, value) in input.iter().enumerate() {
                let p = point(value)?;
                if i <= n {
                    control.push(p);
                }
            }
            bezier(control, Vec::new())
        }
        other => return Err(Error::UnknownSegment(other.to_owned())),
    };
    Ok((kind, None))
}

fn convert_arc(value: &Value) -> Result<(EntityKind, Option<ArcParameters>), Error> {
    let center = point3(field(value, "center")?)?;
    let radius = number(field(value, "radius")?)?;
    let start = number(field(value, "start")?)?;
    let sweep = number(field(value, "sweep")?)?;
    if radius <= 0.0 || radius > MAX_NUMBER || start.abs() > 1e6 || sweep == 0.0 || sweep.abs() > TAU {
        return Err(Error::Invalid("arc radius/angles"));
    }
    let kind = if sweep.abs() == TAU {
        EntityKind::Circle(Circle { center, radius })
    } else {
        // Keep the start near zero and compute long arcs through their *small gap*.
        // Adding a nearly complete turn directly can round a real gap away.
        let mut native_start = start % TAU;
        if native_start > TAU * 0.5 {
            native_start -= TAU;
        }
        if native_start < -TAU * 0.5 {
            native_start += TAU;
        }
        let end = if sweep > TAU * 0.5 {
            native_start - (TAU - sweep)
        } else if sweep < -TAU * 0.5 {
            native_start + (TAU + sweep)
        } else {
            native_start + sweep
        };
        if end == native_start {
            return Err(Error::Invalid("unrepresentable arc endpoints"));
        }
        let (start, end) = if sweep < 0.0 { (end, native_start) } else { (native_start, end) };
        EntityKind::Arc(Arc { center, radius, start, end })
    };
    Ok((kind, Some(ArcParameters { start, sweep })))
}

fn add_count(total: &mut usize, count: usize, maximum: usize, name: &'static str) -> Result<(), Error> {
    *total = total.checked_add(count).filter(|n| *n <= maximum).ok_or(Error::Limit(name))?;
    Ok(())
}

// No input-sized temporary vectors: check all aggregate counts before allocating native output.
fn preflight(result: &Value) -> Result<usize, Error> {
    scan_value(result, 0, &mut 0, &mut 0)?;
    let drawing = field(result, "drawing")?;
    let lines = array(field(drawing, "lines")?)?;
    if lines.len() > MAX_LINES {
        return Err(Error::Limit("lines"));
    }
    if array(field(drawing, "unsupported")?)?.len() > MAX_PIECES {
        return Err(Error::Limit("unsupported faces"));
    }
    let (mut pieces, mut segments, mut controls) = (0, 0, 0);
    for line in lines {
        let input_pieces = array(field(line, "pieces")?)?;
        add_count(&mut pieces, input_pieces.len(), MAX_PIECES, "pieces")?;
        for piece in input_pieces {
            let geometry = array(field(piece, "geometry")?)?;
            add_count(&mut segments, geometry.len(), MAX_SEGMENTS, "segments")?;
            for segment in geometry {
                let count = match text(field(segment, "type")?)? {
                    "line" => 2,
                    "arc" => 1,
                    "cubic" => 4,
                    "conic" => 3,
                    "bezier" => {
                        let input = array(field(segment, "p")?)?;
                        if input.len() != 10 {
                            return Err(Error::Invalid("Bezier control count"));
                        }
                        10
                    }
                    other => return Err(Error::UnknownSegment(other.to_owned())),
                };
                add_count(&mut controls, count, MAX_CONTROLS, "controls")?;
            }
        }
    }
    Ok(segments)
}

fn scan_value(value: &Value, depth: usize, nodes: &mut usize, text_bytes: &mut usize) -> Result<(), Error> {
    if depth > 64 {
        return Err(Error::Limit("nesting"));
    }
    add_count(nodes, 1, MAX_NODES, "JSON nodes")?;
    match value {
        Value::Array(items) => {
            if items.len() > MAX_NODES.saturating_sub(*nodes) {
                return Err(Error::Limit("JSON nodes"));
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
        Value::Number(_) => {
            number(value)?;
        }
        Value::Null | Value::Bool(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn result(segment: Value) -> Value {
        json!({
            "source": {"sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", "bytes": 42},
            "units": {"length": "mm", "area": "mm2", "volume": "mm3", "angle": "radian", "coordinate_system": "right_handed_z_up"},
            "scale": 1,
            "basis": {"x": [1,0,0], "y": [0,1,0], "direction": [0,0,-1]},
            "quality": {"unsupported_faces": 0, "uncertain_pieces": 0, "bounded_pieces": 0,
                "sampled_pieces": 0, "unsupported_trims": 0, "import_issues": 0, "open_bodies": 0, "analytic_complete": true},
            "prepare_reports": [], "source_mapping": "result-local", "visibility_contract": "raw",
            "drawing": {"unsupported": [], "stats": {}, "lines": [{"placement": 2,
                "source": {"kind": "edge", "edge": 7, "tangent": false},
                "pieces": [{"t": [0.2,0.8], "visibility": "visible", "covered": false,
                    "accuracy": {"kind": "analytic"}, "geometry": [segment]}]}]}
        })
    }

    #[test]
    fn rational_conic_retains_control_weights_midpoint_and_piece_provenance() -> Result<(), Error> {
        let w = 0.5_f64.sqrt();
        let mut input = result(json!({"type":"conic", "a":[1,0], "apex":[1,1], "b":[0,1], "w":w}));
        input["drawing"]["lines"][0]["pieces"][0]["visibility"] = json!("uncertain");
        input["drawing"]["lines"][0]["pieces"][0]["covered"] = json!(true);
        input["drawing"]["lines"][0]["pieces"][0]["accuracy"] = json!({"kind":"sampled", "max_sample_deviation_mm":0.0});
        input["quality"]["uncertain_pieces"] = json!(1);
        input["quality"]["sampled_pieces"] = json!(1);
        input["quality"]["analytic_complete"] = json!(false);
        let projection = decode(&input)?;
        let entity = &projection.entities[0];
        let EntityKind::Spline(spline) = &entity.kind else { panic!("conic was not preserved as a spline") };
        assert_eq!(spline.degree, 2);
        assert_eq!(spline.control, vec![Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(0.0, 1.0)]);
        assert_eq!(spline.weights, vec![1.0, w, 1.0]);
        assert_eq!(spline.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        let midpoint = spline.eval(0.5);
        assert!((midpoint.x - w).abs() < 1e-14 && (midpoint.y - w).abs() < 1e-14);
        assert_eq!(entity.t, [0.2, 0.8]);
        assert_eq!(entity.visibility, Visibility::Uncertain);
        assert!(entity.covered);
        assert_eq!(entity.accuracy, Accuracy::Sampled { max_sample_deviation_mm: 0.0 });
        assert_eq!(projection.lines[entity.line_index].placement, 2);
        assert_eq!(projection.lines[entity.line_index].source, SourceReference::Edge { edge: 7, tangent: false });
        Ok(())
    }

    #[test]
    fn signed_arcs_reverse_only_traversal_and_near_full_arcs_keep_the_gap() -> Result<(), Error> {
        let almost_tau = f64::from_bits(TAU.to_bits() - 1);
        for sweep in [TAU, -TAU, almost_tau, -almost_tau, TAU - 1e-10, -TAU + 1e-10, -1.5] {
            let projection = decode(&result(json!({"type":"arc", "center":[4,5], "radius":3, "start":0.25, "sweep":sweep})))?;
            let entity = &projection.entities[0];
            assert_eq!(entity.source_arc, Some(ArcParameters { start: 0.25, sweep }));
            assert_eq!(entity.parameter_reversed, sweep < 0.0);
            if sweep.abs() == TAU {
                assert!(matches!(entity.kind, EntityKind::Circle(_)));
            } else {
                let EntityKind::Arc(arc) = &entity.kind else { panic!("real gap was closed") };
                let native_sweep = (arc.end - arc.start).rem_euclid(TAU);
                assert!(native_sweep > 0.0 && native_sweep < TAU);
                assert!((native_sweep - sweep.abs()).abs() < 1e-14);
                let (first, last) = if sweep < 0.0 { (arc.end, arc.start) } else { (arc.start, arc.end) };
                assert!((first.cos() - 0.25_f64.cos()).abs() < 1e-14);
                assert!((first.sin() - 0.25_f64.sin()).abs() < 1e-14);
                assert!((last.cos() - (0.25 + sweep).cos()).abs() < 1e-14);
                assert!((last.sin() - (0.25 + sweep).sin()).abs() < 1e-14);
            }
        }
        Ok(())
    }

    #[test]
    fn rejects_invalid_poles_nonfinite_fields_and_oversized_results() {
        for w in [-1.0, -2.0] {
            assert!(matches!(
                decode(&result(json!({"type":"conic", "a":[0,0], "apex":[1,2], "b":[2,0], "w":w}))),
                Err(Error::Invalid("rational conic weight/pole"))
            ));
        }
        let line = json!({"type":"line", "a":[0,0], "b":[1,1]});
        let mut malformed = result(line.clone());
        malformed["drawing"]["lines"][0]["pieces"][0]["geometry"][0]["a"] = json!([f64::INFINITY, 0]);
        assert!(matches!(decode(&malformed), Err(Error::Invalid("finite number"))));
        let mut invalid_basis = result(line.clone());
        invalid_basis["basis"]["direction"] = json!([0, 0, 1]);
        assert!(matches!(decode(&invalid_basis), Err(Error::Invalid("view basis handedness"))));
        let mut oversized = result(line.clone());
        let repeated = oversized["drawing"]["lines"][0].clone();
        oversized["drawing"]["lines"] = Value::Array(vec![repeated; MAX_LINES + 1]);
        assert_eq!(decode(&oversized), Err(Error::Limit("lines")));
        let mut oversized_segments = result(line);
        oversized_segments["drawing"]["lines"][0]["pieces"][0]["geometry"] = Value::Array(vec![json!({"type":"arc"}); MAX_SEGMENTS + 1]);
        assert_eq!(decode(&oversized_segments), Err(Error::Limit("segments")));
        let mut invalid_tail = result(json!({"type":"bezier", "n":4, "p":[[0,0],[1,0],[2,1],[3,1],[4,0],[0,0],[0,0],[0,0],[0,0],[null,0]]}));
        assert!(matches!(decode(&invalid_tail), Err(Error::Invalid("finite number"))));
        invalid_tail["drawing"]["lines"][0]["pieces"][0]["geometry"][0] = json!({"type":"unknown"});
        assert_eq!(decode(&invalid_tail), Err(Error::UnknownSegment("unknown".into())));
    }
}
