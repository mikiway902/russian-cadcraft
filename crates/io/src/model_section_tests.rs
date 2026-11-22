use super::*;
use serde_json::json;

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
fn request() -> SectionRequest {
    SectionRequest {
        plane: Plane { origin: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 2.0] },
        kept_side: KeptSide::Negative,
        direction: [0.0, 0.0, -3.0],
        up: [0.0, 1.0, 0.0],
        symmetry_axis: None,
        exclude_bodies: Vec::new(),
    }
}
fn circle(radius: f64, sweep: f64) -> Value {
    json!({"type":"arc","center":[0,0],"radius":radius,"start":0.25,"sweep":sweep})
}
fn line(a: [f64; 2], b: [f64; 2]) -> Value {
    json!({"type":"line","a":a,"b":b})
}
fn fixture() -> Value {
    json!({
        "protocol":"cadcraft-section/2","source":{"sha256":SHA,"bytes":42},
        "units":{"length":"mm","area":"mm2","volume":"mm3","angle":"radian","coordinate_system":"right_handed_z_up"},
        "scale":1,"basis":{"x":[1,0,0],"y":[0,1,0],"direction":[0,0,-1]},
        "quality":{"unsupported_faces":0,"uncertain_pieces":0,"bounded_pieces":0,"sampled_pieces":0,
            "unsupported_trims":0,"import_issues":0,"open_bodies":0,"analytic_complete":true},
        "prepare_reports":[{}],"source_mapping":SOURCE_MAPPING,
        "visibility_contract":"raw_uncertain_and_covered_preserved_no_mesh_fallback",
        "drawing":{"unsupported":[],"stats":{},"lines":[{"placement":0,"source":{"kind":"edge","edge":3,"tangent":false},
            "pieces":[{"t":[0,1],"visibility":"visible","covered":false,"accuracy":{"kind":"analytic"},
                "geometry":[{"type":"line","a":[0,0],"b":[1,0]}]}]}]},
        "section":{"plane":{"origin":[0,0,0],"normal":[0,0,1]},"kept_side":"negative","status":"nonempty",
            "topology_identity":"section_result_local_only","cap_coordinates":"world_origin_screen_mm_same_basis_and_scale_as_drawing",
            "cap_contract":"outer_ccw_holes_cw_exact_segments_no_polygonization_no_paper_hatch_spacing",
            "atomicity":"all_selected_bodies_and_caps_or_error_source_snapshot_unchanged",
            "symmetry":null,
            "bodies":[{"source_body":0,"source_placement":0,"result_body":0,"status":"cut",
                "topology":{"faces":4,"edges":4,"vertices":4,"shells":1}}],
            "caps":[{"source_body":0,"source_placement":0,"result_body":0,"face":3,
                "region":{"outer":{"segs":[circle(5.0,TAU)]},"holes":[{"segs":[circle(1.0,-TAU)]}]},
                "provenance":{"kind":"half_space_boolean_history","operands":["original_source_body","half_space_tool"],
                    "face_origins":[{"Face":{"operand":0,"face":0}},{"Face":{"operand":1,"face":0}}],"change":"Merged",
                    "excluded_original_coplanar_faces":[0],"boundary_operation":"exact_planar_difference_from_original_coplanar_faces",
                    "planar_read_tolerance_mm":1e-8,
                    "curve_contract":"upstream_exact_native_planar_read_with_reported_recognition_tolerance_not_symbolic_certificate",
                    "topology_identity":"section_result_local_only"}}]}
    })
}
fn decode_fixture(v: &Value) -> Result<Section, Error> {
    decode(v, &request(), SHA, 42, &[1])
}

#[test]
fn directed_circles_holes_and_boolean_history_survive_without_vertices() -> Result<(), Error> {
    let value = fixture();
    let result = decode_fixture(&value)?;
    assert_eq!(result.projection.lines[0].source, SourceReference::Edge { edge: 3, tangent: false });
    let cap = &result.caps[0];
    assert_eq!(cap.face, 3); // Result face 3 is not original source face 3 (which does not exist).
    assert_eq!(cap.provenance, value["section"]["caps"][0]["provenance"]);
    assert_eq!(cap.loops.len(), 2);
    for (i, ring) in cap.loops.iter().enumerate() {
        assert!(ring.vertices.is_empty() && ring.sources.is_empty());
        assert_eq!(ring.outer, i == 0);
        assert_eq!(ring.edges.len(), 1);
        let HatchEdge::Segment(Segment::Arc { arc, ccw }) = &ring.edges[0] else {
            return Err(invalid("expected directed circle"));
        };
        assert_eq!(*ccw, i == 0);
        assert_eq!(arc.start, 0.25);
        assert_eq!(arc.start, arc.end);
        assert_eq!(arc.sweep(), TAU);
    }
    Ok(())
}

#[test]
fn rational_and_polynomial_paths_keep_native_controls_and_direction() -> Result<(), Error> {
    let mut value = fixture();
    value["section"]["caps"][0]["region"] = json!({
        "outer":{"segs":[{"type":"cubic","p0":[-2,0],"p1":[-2,-2],"p2":[2,-2],"p3":[2,0]},line([2.0,0.0],[-2.0,0.0])]},
        "holes":[{"segs":[{"type":"conic","a":[1,0],"apex":[0,-1],"b":[-1,0],"w":0.5},line([-1.0,0.0],[1.0,0.0])]}]});
    let result = decode_fixture(&value)?;
    let loops = &result.caps[0].loops;
    let HatchEdge::Spline(outer) = &loops[0].edges[0] else {
        return Err(invalid("expected cubic spline"));
    };
    assert_eq!(outer.degree, 3);
    assert_eq!(outer.control, vec![Vec2::new(-2.0, 0.0), Vec2::new(-2.0, -2.0), Vec2::new(2.0, -2.0), Vec2::new(2.0, 0.0)]);
    let HatchEdge::Spline(hole) = &loops[1].edges[0] else {
        return Err(invalid("expected rational spline"));
    };
    assert_eq!(hole.degree, 2);
    assert_eq!(hole.weights, vec![1.0, 0.5, 1.0]);
    assert_eq!(hole.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(hole.control, vec![Vec2::new(1.0, 0.0), Vec2::new(0.0, -1.0), Vec2::new(-1.0, 0.0)]);
    Ok(())
}

#[test]
fn high_degree_bezier_and_clockwise_partial_arcs_are_not_fitted() -> Result<(), Error> {
    let mut value = fixture();
    value["section"]["caps"][0]["region"] = json!({"outer":{"segs":[
        {"type":"bezier","n":4,"p":[[-2,0],[-2,-2],[0,-2],[2,-2],[2,0],[0,0],[0,0],[0,0],[0,0],[0,0]]},line([2.0,0.0],[-2.0,0.0])]},
        "holes":[{"segs":[{"type":"arc","center":[0,0],"radius":0.5,"start":0,"sweep":-std::f64::consts::PI},
            {"type":"arc","center":[0,0],"radius":0.5,"start":-std::f64::consts::PI,"sweep":-std::f64::consts::PI}]}]});
    let result = decode_fixture(&value)?;
    let HatchEdge::Spline(spline) = &result.caps[0].loops[0].edges[0] else {
        return Err(invalid("expected degree-four spline"));
    };
    assert_eq!(spline.degree, 4);
    assert_eq!(spline.control.len(), 5);
    assert_eq!(spline.knots, vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
    assert!(result.caps[0].loops[1].edges.iter().all(|e| matches!(e, HatchEdge::Segment(Segment::Arc { ccw: false, .. }))));
    Ok(())
}

#[test]
fn tiny_lines_are_not_omitted_and_native_tolerance_is_not_a_repair() -> Result<(), Error> {
    let mut value = fixture();
    value["section"]["caps"][0]["region"] = json!({"outer":{"segs":[line([0.0,0.0],[1e-12,0.0]),line([1e-12,0.0],[1.0,0.0]),
        line([1.0,0.0],[1.0,1.0]),line([1.0,1.0],[0.0,1.0]),line([0.0,1.0],[0.0,0.0])]},"holes":[]});
    let result = decode_fixture(&value)?;
    assert_eq!(result.caps[0].loops[0].edges.len(), 5);
    assert!(result.caps[0].loops[0].vertices.is_empty());
    value["section"]["caps"][0]["region"]["outer"]["segs"][4]["b"] = json!([0, 0.001]);
    assert!(decode_fixture(&value).is_err());
    Ok(())
}

#[test]
fn malformed_mapping_contract_and_winding_refuse_the_entire_result() {
    let mutations: &[(&str, Value)] = &[
        ("/protocol", json!("cadcraft-section/1")),
        ("/source/bytes", json!(43)),
        ("/section/plane/normal", json!([0, 0, 2])),
        ("/basis/x", json!([-1, 0, 0])),
        ("/section/bodies/0/source_placement", json!(1)),
        ("/section/bodies/0/result_body", json!(1)),
        ("/section/bodies/0/status", json!("retained")),
        ("/section/bodies/0/topology", Value::Null),
        ("/section/bodies/0/topology/faces", json!(10001)),
        ("/section/caps/0/source_body", json!(1)),
        ("/section/caps/0/result_body", json!(1)),
        ("/section/caps/0/face", json!(4)),
        ("/drawing/lines/0/source/edge", json!(4)),
        ("/drawing/lines/0/placement", json!(1)),
        ("/section/caps/0/provenance/face_origins/0/Face/face", json!(1)),
        ("/section/caps/0/provenance/face_origins", json!([])),
        ("/section/caps/0/provenance/excluded_original_coplanar_faces", json!([1])),
        ("/section/caps/0/provenance/boundary_operation", json!("guessed")),
        ("/section/caps/0/region/holes/0/segs/0/sweep", json!(TAU)),
        ("/section/caps/0/region/outer/segs/0/sweep", json!(-TAU)),
        ("/section/caps/0/region/outer/segs/0/type", json!("spline")),
        ("/section/status", json!("empty")),
        ("/section/caps", json!([])),
    ];
    for (path, replacement) in mutations {
        let mut value = fixture();
        let Some(slot) = value.pointer_mut(path) else {
            assert!(value.pointer(path).is_some(), "missing test fixture path {path}");
            continue;
        };
        *slot = replacement.clone();
        assert!(decode_fixture(&value).is_err(), "accepted malformed {path}");
    }
}

#[test]
fn excluded_removed_and_empty_are_explicit_and_compact() -> Result<(), Error> {
    let mut value = fixture();
    value["section"]["bodies"] = json!([
        {"source_body":0,"source_placement":0,"result_body":null,"status":"excluded"},
        {"source_body":1,"source_placement":1,"result_body":null,"status":"removed"},
        {"source_body":2,"source_placement":2,"result_body":0,"status":"cut","topology":{"faces":4,"edges":4,"vertices":4,"shells":1}}]);
    value["section"]["caps"][0]["source_body"] = json!(2);
    value["section"]["caps"][0]["source_placement"] = json!(2);
    let mut query = request();
    query.exclude_bodies = vec![0];
    let result = decode(&value, &query, SHA, 42, &[1, 1, 1])?;
    assert_eq!(result.bodies[2].result_body, Some(0));
    assert_eq!(result.caps[0].source_body, 2);
    value["section"]["bodies"] = json!([{"source_body":0,"source_placement":0,"result_body":null,"status":"removed"}]);
    value["section"]["status"] = json!("empty");
    value["section"]["caps"] = json!([]);
    value["prepare_reports"] = json!([]);
    value["drawing"]["lines"] = json!([]);
    let empty = decode_fixture(&value)?;
    assert_eq!(empty.status, SectionStatus::Empty);
    assert!(empty.projection.entities.is_empty() && empty.caps.is_empty());
    value["section"]["bodies"][0]["result_body"] = json!(0);
    assert!(decode_fixture(&value).is_err());
    Ok(())
}

#[test]
fn raw_quality_survives_and_request_or_resource_refusals_have_no_fallback() -> Result<(), Error> {
    let mut value = fixture();
    value["quality"]["import_issues"] = json!(1);
    value["quality"]["analytic_complete"] = json!(false);
    assert!(!decode_fixture(&value)?.projection.quality.analytic_complete);
    let mut query = request();
    query.direction = [0.0, 0.0, 1.0];
    assert!(validate_request(&query).is_err());
    query = request();
    query.plane.origin[0] = f64::NAN;
    assert!(validate_request(&query).is_err());
    query = request();
    query.exclude_bodies = vec![0, 0];
    assert!(validate_request(&query).is_err());
    query = request();
    query.exclude_bodies = vec![1];
    assert!(decode(&value, &query, SHA, 42, &[1]).is_err());
    value["section"]["caps"][0]["region"]["outer"]["segs"] = Value::Array(vec![circle(5.0, TAU); MAX_CAP_SEGMENTS + 1]);
    assert!(decode_fixture(&value).is_err());
    Ok(())
}

#[test]
fn negative_and_zero_nonpolar_conic_weights_are_preserved_not_chords() -> Result<(), Error> {
    for w in [-0.5, 0.0] {
        let mut value = fixture();
        // The remaining three edges give this hole negative area even for w=0.
        value["section"]["caps"][0]["region"]["holes"][0] = json!({"segs":[
            {"type":"conic","a":[-1,0],"apex":[0,-1],"b":[1,0],"w":w},
            line([1.0,0.0],[1.0,-1.0]),line([1.0,-1.0],[-1.0,-1.0]),line([-1.0,-1.0],[-1.0,0.0])]});
        let result = decode_fixture(&value)?;
        let HatchEdge::Spline(s) = &result.caps[0].loops[1].edges[0] else {
            return Err(invalid("expected rational spline"));
        };
        assert_eq!(s.weights, vec![1.0, w, 1.0]);
        assert_eq!(result.caps[0].loops[1].edges.len(), 4);
        value["section"]["caps"][0]["region"]["holes"][0]["segs"][0]["w"] = json!(-1.0);
        assert!(decode_fixture(&value).is_err());
    }
    Ok(())
}

#[test]
fn unrepresentable_tiny_arc_is_refused_instead_of_becoming_a_full_circle() {
    let mut value = fixture();
    value["section"]["caps"][0]["region"]["outer"]["segs"] = json!([
        {"type":"arc","center":[0,0],"radius":5,"start":0,"sweep":1e-12}]);
    assert!(decode_fixture(&value).is_err());
}

fn symmetry_report(symmetric: bool) -> Value {
    json!({"axis":[0,1],"center":[0,0],"symmetric":symmetric,"tolerance_mm":1e-9})
}

#[test]
fn symmetry_classification_keeps_native_material_and_raw_hlr() -> Result<(), Error> {
    let mut query = request();
    query.symmetry_axis = Some([0.0, 1.0]);
    for symmetric in [true, false] {
        let mut value = fixture();
        // A concentric annulus is symmetric about Y; moving only its hole in X is not.
        // The consumer must keep the complete material boundary, not only the outer circle.
        if !symmetric {
            value["section"]["caps"][0]["region"]["holes"][0]["segs"][0]["center"] = json!([2, 0]);
        }
        value["section"]["symmetry"] = symmetry_report(symmetric);
        value["quality"]["analytic_complete"] = json!(false);
        value["quality"]["uncertain_pieces"] = json!(1);
        value["drawing"]["lines"][0]["pieces"][0]["visibility"] = json!("uncertain");
        value["drawing"]["lines"][0]["pieces"][0]["covered"] = json!(true);
        let decoded = decode(&value, &query, SHA, 42, &[1])?;
        assert_eq!(decoded.symmetry, Some(SectionSymmetry { axis: [0.0, 1.0], center: [0.0, 0.0], symmetric, tolerance_mm: 1e-9 }));
        assert!(!decoded.projection.quality.analytic_complete);
        assert_eq!(decoded.projection.quality.uncertain_pieces, 1);
        assert_eq!(decoded.projection.entities.len(), 1);
        let entity = &decoded.projection.entities[0];
        assert_eq!(entity.visibility, cadcraft_doc::model_projection::Visibility::Uncertain);
        assert!(entity.covered);
        assert_eq!(decoded.projection.lines[0].source, SourceReference::Edge { edge: 3, tangent: false });
        assert!(decoded.projection.lines[0].pieces[0].covered);
        assert_eq!(decoded.caps[0].provenance, value["section"]["caps"][0]["provenance"]);
        assert_eq!(decoded.caps[0].loops.len(), 2);
        let HatchEdge::Segment(Segment::Arc { arc, ccw }) = &decoded.caps[0].loops[1].edges[0] else {
            return Err(invalid("expected native symmetry hole"));
        };
        assert!(!*ccw);
        assert_eq!(arc.center, Vec2::new(if symmetric { 0.0 } else { 2.0 }, 0.0));
        assert_eq!(arc.radius, 1.0);
        assert_eq!(arc.sweep(), TAU);
    }
    Ok(())
}

#[test]
fn symmetry_reports_are_required_strict_and_request_bound() {
    let mut query = request();
    query.symmetry_axis = Some([0.0, 1.0]);
    let mutations = [
        ("/section/symmetry", Value::Null),
        ("/section/symmetry", json!({})),
        ("/section/symmetry/axis", json!([1, 0])),
        ("/section/symmetry/axis", json!([0, -1])),
        ("/section/symmetry/axis", json!([0, 2])),
        ("/section/symmetry/axis", json!([null, 1])),
        ("/section/symmetry/axis", json!([0, 1, 0])),
        ("/section/symmetry/center", json!([null, 0])),
        ("/section/symmetry/center", json!([0, 1e100])),
        ("/section/symmetry/center", json!([1_000_001, 0])),
        ("/section/symmetry/center", json!([5.001, 0])),
        ("/section/symmetry/center", json!([0, -5.001])),
        ("/section/symmetry/symmetric", json!("true")),
        ("/section/symmetry/symmetric", Value::Null),
        ("/section/symmetry/tolerance_mm", Value::Null),
        ("/section/symmetry/tolerance_mm", json!(0)),
        ("/section/symmetry/tolerance_mm", json!(1e-10)),
        ("/section/symmetry/tolerance_mm", json!(1.001e-6)),
        ("/section/symmetry/tolerance_mm", json!(1e100)),
    ];
    for (path, replacement) in mutations {
        let mut value = fixture();
        value["section"]["symmetry"] = symmetry_report(true);
        *value.pointer_mut(path).expect("symmetry fixture path") = replacement;
        assert!(decode(&value, &query, SHA, 42, &[1]).is_err(), "accepted malformed {path}");
    }
    let mut value = fixture();
    value["section"].as_object_mut().expect("section object").remove("symmetry");
    assert!(decode(&value, &query, SHA, 42, &[1]).is_err());
    assert!(decode_fixture(&value).is_err());
    for field in ["axis", "center", "symmetric", "tolerance_mm"] {
        let mut value = fixture();
        value["section"]["symmetry"] = symmetry_report(true);
        value["section"]["symmetry"].as_object_mut().expect("report object").remove(field);
        assert!(decode(&value, &query, SHA, 42, &[1]).is_err(), "accepted missing {field}");
    }
    let mut value = fixture();
    value["section"]["symmetry"] = symmetry_report(true);
    value["section"]["symmetry"]["extra"] = json!(true);
    assert!(decode(&value, &query, SHA, 42, &[1]).is_err());
    value["section"]["symmetry"] = symmetry_report(true);
    assert!(decode_fixture(&value).is_err(), "accepted unrequested classification");
}

#[test]
fn symmetry_is_null_for_no_intersection_empty_and_multiple_caps() -> Result<(), Error> {
    let mut query = request();
    query.symmetry_axis = Some([0.0, 1.0]);
    for status in ["retained", "removed", "cut"] {
        let mut value = fixture();
        match status {
            "retained" => {
                value["section"]["bodies"][0]["status"] = json!("retained");
                value["section"]["caps"] = json!([]);
            }
            "removed" => {
                value["section"]["bodies"] = json!([{"source_body":0,"source_placement":0,"result_body":null,"status":"removed"}]);
                value["section"]["status"] = json!("empty");
                value["section"]["caps"] = json!([]);
                value["prepare_reports"] = json!([]);
                value["drawing"]["lines"] = json!([]);
            }
            _ => {
                let mut second = value["section"]["caps"][0].clone();
                second["face"] = json!(2);
                value["section"]["caps"].as_array_mut().expect("caps array").push(second);
            }
        }
        assert!(decode(&value, &query, SHA, 42, &[1])?.symmetry.is_none());
        value["section"]["symmetry"] = symmetry_report(true);
        assert!(decode(&value, &query, SHA, 42, &[1]).is_err(), "accepted spurious {status} report");
    }
    Ok(())
}

#[test]
fn symmetry_axis_rejects_nonfinite_and_nonunit_before_serialization() -> Result<(), Error> {
    let mut query = request();
    for axis in
        [[f64::NAN, 1.0], [0.0, f64::INFINITY], [f64::NEG_INFINITY, 0.0], [f64::MAX, f64::MAX], [0.0, 0.0], [0.0, 2.0], [1.0, 1.0], [0.0, 1.0 + 2e-9]]
    {
        query.symmetry_axis = Some(axis);
        assert!(validate_request(&query).is_err(), "accepted unsafe axis {axis:?}");
    }
    query.symmetry_axis = Some([0.6, 0.8]);
    validate_request(&query)?;
    let mut value = fixture();
    value["section"]["symmetry"] = json!({"axis":[0.6,0.8],"center":[0,0],"symmetric":true,"tolerance_mm":1e-6});
    assert_eq!(decode(&value, &query, SHA, 42, &[1])?.symmetry.expect("requested report").axis, [0.6, 0.8]);
    Ok(())
}

#[test]
fn symmetry_center_uses_native_bounds_including_negative_weight_conics() -> Result<(), Error> {
    let mut query = request();
    query.symmetry_axis = Some([0.0, 1.0]);
    let mut value = fixture();
    // The negative-weight conic bows below its control hull. Its real outer-bounds
    // center is (0,-0.5), not in the controls' Y interval [0,1].
    value["section"]["caps"][0]["region"] = json!({"outer":{"segs":[
        {"type":"conic","a":[-1,0],"apex":[0,1],"b":[1,0],"w":-0.5},
        line([1.0,0.0],[-1.0,0.0])]},"holes":[]});
    value["section"]["symmetry"] = json!({"axis":[0,1],"center":[0,-0.5],"symmetric":true,"tolerance_mm":1e-9});
    let decoded = decode(&value, &query, SHA, 42, &[1])?;
    assert_eq!(decoded.symmetry.expect("conic report").center, [0.0, -0.5]);
    let HatchEdge::Spline(s) = &decoded.caps[0].loops[0].edges[0] else {
        return Err(invalid("expected native conic"));
    };
    assert_eq!(s.weights, vec![1.0, -0.5, 1.0]);
    assert_eq!(s.control, vec![Vec2::new(-1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(1.0, 0.0)]);
    value["section"]["symmetry"]["center"] = json!([0, -2]);
    assert!(decode(&value, &query, SHA, 42, &[1]).is_err());
    Ok(())
}

#[test]
fn shared_cap_bounds_refuse_malformed_native_boundaries() -> Result<(), Error> {
    let mut value = fixture();
    value["section"]["caps"][0]["region"] = json!({"outer":{"segs":[
        {"type":"conic","a":[-1,0],"apex":[0,1],"b":[1,0],"w":-0.5},
        line([1.0,0.0],[-1.0,0.0])]},"holes":[]});
    let mut decoded = decode_fixture(&value)?;
    let cap = &mut decoded.caps[0];
    let bounds = cap.conservative_bounds().expect("native conic bounds");
    assert!(bounds.min.y <= -1.0 && bounds.max.y >= 0.0);
    cap.loops[0].outer = false;
    assert!(cap.conservative_bounds().is_none());
    cap.loops[0].outer = true;
    let HatchEdge::Spline(s) = &mut cap.loops[0].edges[0] else {
        return Err(invalid("expected bounds conic"));
    };
    s.weights[1] = -1.0;
    assert!(cap.conservative_bounds().is_none(), "polar conics have no finite hull");
    let HatchEdge::Spline(s) = &mut cap.loops[0].edges[0] else {
        return Err(invalid("expected bounds conic"));
    };
    s.weights[1] = -0.5;
    s.weights[0] = 0.5;
    assert!(cap.conservative_bounds().is_none(), "unsupported negative-weight spline");
    cap.loops[0].edges.clear();
    assert!(cap.conservative_bounds().is_none());
    cap.loops.clear();
    assert!(cap.conservative_bounds().is_none());
    Ok(())
}
