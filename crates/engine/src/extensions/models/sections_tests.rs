use super::*;
use cadcraft_doc::{
    Block, Entity, Line, Space,
    model_projection::{Basis, LineProvenance, PieceProvenance, ProjectedEntity, Projection, Quality, SourceReference},
    model_section::{BodyDisposition, SectionCap, SectionTopology},
};
use cadcraft_geom::PolyVertex;

fn parameters(id: &str) -> Value {
    json!({"id":id,"plane":{"origin":[0,0,0],"normal":[0,2,0]},"kept_side":"positive","layout":"GOST1","scale":2})
}
fn setup() -> (Session, String) {
    let mut s = crate::extensions::new_session();
    let metadata = json!({"protocol":"cadcraft-model-worker/1","path":"/models/section.step",
        "source":{"sha256":"a".repeat(64),"bytes":512},"format":"step",
        "units":{"length":"mm","area":"mm2","volume":"mm3","angle":"radian","coordinate_system":"right_handed_z_up"},
        "metadata":{},"bodies":[{"name":"Part"}],"tree":{},"warnings":[],"report":{}});
    let id = super::super::apply_loaded(&mut s, metadata, None).unwrap()["id"].as_str().unwrap().to_owned();
    (s, id)
}
fn ring(points: &[Vec2]) -> HatchLoop {
    HatchLoop {
        vertices: vec![],
        edges: (0..points.len())
            .map(|i| HatchEdge::Segment(Segment::Line(cadcraft_geom::Line { a: points[i], b: points[(i + 1) % points.len()] })))
            .collect(),
        outer: true,
        sources: vec![],
    }
}
fn fixture() -> Section {
    let outer = ring(&[Vec2::new(0., 0.), Vec2::new(20., 0.), Vec2::new(20., 20.), Vec2::new(0., 20.)]);
    let bore = HatchLoop {
        vertices: vec![],
        edges: vec![HatchEdge::Segment(Segment::Arc {
            arc: cadcraft_geom::Arc { center: Vec2::new(10., 10.), radius: 3., start: 0., end: 0. },
            ccw: false,
        })],
        outer: false,
        sources: vec![],
    };
    Section {
        projection: Projection {
            source_sha256: "a".repeat(64),
            source_bytes: 512,
            basis: Basis { x: [1., 0., 0.], y: [0., 0., 1.], direction: [0., 1., 0.] },
            quality: Quality {
                unsupported_faces: 0,
                uncertain_pieces: 0,
                bounded_pieces: 0,
                sampled_pieces: 0,
                unsupported_trims: 0,
                import_issues: 0,
                open_bodies: 0,
                analytic_complete: true,
            },
            entities: vec![ProjectedEntity {
                kind: EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(20., 0., 0.) }),
                line_index: 0,
                piece_index: 0,
                segment_index: 0,
                t: [0., 1.],
                visibility: Visibility::Visible,
                covered: false,
                accuracy: Accuracy::Analytic,
                parameter_reversed: false,
                source_arc: None,
            }],
            lines: vec![LineProvenance {
                placement: 0,
                source: SourceReference::Edge { edge: 0, tangent: false },
                pieces: vec![PieceProvenance {
                    t: [0., 1.],
                    visibility: Visibility::Visible,
                    covered: false,
                    accuracy: Accuracy::Analytic,
                    segment_count: 1,
                }],
            }],
            unsupported: vec![],
            prepare_reports: json!([]),
            drawing_stats: json!({}),
            source_mapping: "SECTION-RESULT-LOCAL".into(),
            visibility_contract: "exact_hlr".into(),
        },
        plane: Plane { origin: [0., 0., 0.], normal: [0., 1., 0.] },
        kept_side: KeptSide::Positive,
        status: SectionStatus::Nonempty,
        bodies: vec![BodyDisposition {
            source_body: 0,
            source_placement: 0,
            result_body: Some(0),
            status: BodyStatus::Cut,
            topology: Some(SectionTopology { faces: 6, edges: 12, vertices: 8, shells: 1 }),
        }],
        caps: vec![SectionCap {
            source_body: 0,
            source_placement: 0,
            result_body: 0,
            face: 5,
            loops: vec![outer, bore],
            provenance: json!({"kind":"half_space_boolean_history","operands":["original_source_body","half_space_tool"],
                "face_origins":[{"Face":{"operand":1,"face":0}}],"change":"Generated",
                "excluded_original_coplanar_faces":[],"boundary_operation":"exact_planar_difference_from_original_coplanar_faces",
                "planar_read_tolerance_mm":1e-7,
                "curve_contract":"upstream_exact_native_planar_read_with_reported_recognition_tolerance_not_symbolic_certificate",
                "topology_identity":"section_result_local_only"}),
        }],
    }
}
fn add(s: &mut Session, id: &str) -> (Handle, Value) {
    let out = apply(s, id, &"a".repeat(64), &parameters(id), fixture()).unwrap();
    (Handle::parse_hex(out["handle"].as_str().unwrap()).unwrap(), out)
}
fn insert(s: &Session, h: Handle) -> &Insert {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("native view insert required") };
    i
}
fn hatch(s: &Session, h: Handle) -> &Hatch {
    let block = s.doc().unwrap().block(&insert(s, h).block).unwrap();
    let EntityKind::Hatch(hatch) = &block.entities.iter().next().unwrap().kind else { panic!("native HATCH must precede outlines") };
    hatch
}

#[test]
fn native_hatch_preserves_bore_and_paper_spacing_and_roundtrips() {
    let (mut s, id) = setup();
    let (h, out) = add(&mut s, &id);
    assert_eq!(out["status"], "created");
    assert_eq!(out["representation"], "frozen-editable-exact-half-space-section");
    assert_eq!(out["section"]["plane"]["normal"], json!([0., 1., 0.]));
    assert_eq!(out["section"]["topology_identity"], "section_result_local_only");
    assert!(out["section"]["caps"][0].get("geometry").is_none());
    assert!(out["section"]["caps"][0].get("region").is_none());
    let i = insert(&s, h);
    let hatch = hatch(&s, h);
    assert_eq!(hatch.pattern, "GOST-METAL");
    assert!(!hatch.solid && !hatch.associative);
    assert_eq!(hatch.style, 0);
    assert_eq!(hatch.loops.len(), 2);
    assert!(hatch.loops[0].outer && !hatch.loops[1].outer);
    assert!(hatch.loops.iter().all(|l| l.vertices.is_empty() && !l.edges.is_empty()));
    assert!(matches!(hatch.loops[1].edges[0], HatchEdge::Segment(Segment::Arc { ccw: false, .. })));
    assert!((hatch.loops[1].area_and_perimeter().0 + std::f64::consts::PI * 9.).abs() < 1e-8);
    assert!((hatch.pattern_lines[0].delta.1 * i.scale.x - 3.).abs() < 1e-12);
    assert_eq!(s.doc().unwrap().block(&i.block).unwrap().entities.iter().next().unwrap().common.layer, "GOST-HATCH");
    let original = hatch.clone();
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(reopened, "section DXF roundtrip", None);
    let mut reopened_hatch = self::hatch(&s, h).clone();
    assert_eq!(reopened_hatch.pattern_lines.len(), original.pattern_lines.len());
    for (actual, expected) in reopened_hatch.pattern_lines.iter_mut().zip(&original.pattern_lines) {
        // DXF stores a rotated world offset; inverse trigonometry can differ by an ULP.
        assert!((actual.delta.0 - expected.delta.0).abs() < 1e-12);
        assert!((actual.delta.1 - expected.delta.1).abs() < 1e-12);
        actual.delta = expected.delta;
    }
    assert_eq!(reopened_hatch, original);
    assert_eq!(style(&s.doc().unwrap().entity(h).unwrap().common).unwrap(), Some(0.5));
}

#[test]
fn spacing_tracks_requested_model_units_not_geometry_sampling() {
    for scale in [0.25, 1., 4., 100.] {
        let (mut s, id) = setup();
        let mut p = parameters(&id);
        p["scale"] = json!(scale);
        let out = apply(&mut s, &id, &"a".repeat(64), &p, fixture()).unwrap();
        let h = Handle::parse_hex(out["handle"].as_str().unwrap()).unwrap();
        assert!((hatch(&s, h).pattern_lines[0].delta.1 * insert(&s, h).scale.x - 3.).abs() < 1e-12);
        assert_eq!(hatch(&s, h).loops[0].edges.len(), 4);
    }
}

#[test]
fn mismatches_and_mesh_like_caps_rollback_without_saved_view() {
    let (mut s, id) = setup();
    let before = s.doc().unwrap().clone();
    let mut variants = Vec::new();
    let mut value = fixture();
    value.projection.source_sha256 = "b".repeat(64);
    variants.push(value);
    let mut value = fixture();
    value.plane.origin[1] = 2.;
    variants.push(value);
    let mut value = fixture();
    value.kept_side = KeptSide::Negative;
    variants.push(value);
    let mut value = fixture();
    value.projection.basis.x[0] = -1.;
    variants.push(value);
    let mut value = fixture();
    value.caps[0].source_placement = 1;
    variants.push(value);
    let mut value = fixture();
    value.bodies[0].source_body = 1;
    variants.push(value);
    let mut value = fixture();
    value.caps[0].provenance["kind"] = json!("preview_mesh");
    variants.push(value);
    let mut value = fixture();
    value.caps[0].loops[0].edges.clear();
    value.caps[0].loops[0].vertices = vec![PolyVertex::new(Vec2::ZERO), PolyVertex::new(Vec2::new(1., 0.)), PolyVertex::new(Vec2::new(0., 1.))];
    variants.push(value);
    for value in variants {
        assert!(apply(&mut s, &id, &"a".repeat(64), &parameters(&id), value).is_err());
        assert_eq!(s.doc().unwrap(), &before);
    }
    let mut p = parameters(&id);
    p["exclude_bodies"] = json!([0]);
    assert!(apply(&mut s, &id, &"a".repeat(64), &p, fixture()).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    // A failure after insertion must still roll back the block, record, marker and insert.
    let mut value = fixture();
    if let EntityKind::Line(l) = &mut value.projection.entities[0].kind {
        l.b.x = f64::NAN;
    }
    assert!(apply(&mut s, &id, &"a".repeat(64), &parameters(&id), value).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn removal_and_tangent_or_outside_return_without_uncut_fallback() {
    let (mut s, id) = setup();
    let before = s.doc().unwrap().clone();
    let mut value = fixture();
    value.caps.clear();
    value.bodies[0].status = BodyStatus::Retained;
    let out = apply(&mut s, &id, &"a".repeat(64), &parameters(&id), value.clone()).unwrap();
    assert_eq!(out["status"], "no_intersection");
    assert_eq!(out["drawing_mutated"], false);
    assert!(out.get("handle").is_none());
    assert_eq!(s.doc().unwrap(), &before);
    value.status = SectionStatus::Empty;
    value.bodies[0].status = BodyStatus::Removed;
    value.bodies[0].result_body = None;
    value.bodies[0].topology = None;
    value.projection.lines.clear();
    value.projection.entities.clear();
    let out = apply(&mut s, &id, &"a".repeat(64), &parameters(&id), value.clone()).unwrap();
    assert_eq!(out["status"], "empty");
    assert_eq!(out["drawing_mutated"], false);
    assert_eq!(s.doc().unwrap(), &before);
    let mut p = parameters(&id);
    p["exclude_bodies"] = json!([0]);
    value.bodies[0].status = BodyStatus::Excluded;
    assert_eq!(apply(&mut s, &id, &"a".repeat(64), &p, value).unwrap()["status"], "empty");
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn raw_quality_refuses_section_unless_explicitly_accepted() {
    let (mut s, id) = setup();
    let before = s.doc().unwrap().clone();
    let mut value = fixture();
    value.projection.quality.open_bodies = 1;
    assert!(apply(&mut s, &id, &"a".repeat(64), &parameters(&id), value.clone()).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    let mut p = parameters(&id);
    p["accept_incomplete"] = json!(true);
    let out = apply(&mut s, &id, &"a".repeat(64), &p, value).unwrap();
    assert_eq!(out["quality"]["open_bodies"], 1);
    assert!(!out["warnings"].as_array().unwrap().is_empty());
}

#[test]
fn moves_and_undo_work_but_generic_style_transforms_are_atomic() {
    let (mut s, id) = setup();
    let (h, _) = add(&mut s, &id);
    let original = s.doc().unwrap().clone();
    s.execute("move", &json!({"handles":[h.hex()],"delta":[30,10]})).unwrap();
    assert_eq!(insert(&s, h).insert.xy(), Vec2::new(80., 110.));
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap(), &original);
    s.redo().unwrap();
    for (cmd, p) in [
        ("scale", json!({"handles":[h.hex()],"base":[0,0],"factor":2})),
        ("rotate", json!({"handles":[h.hex()],"base":[0,0],"angle":30})),
        ("mirror", json!({"handles":[h.hex()],"p1":[0,0],"p2":[1,0],"erase":true})),
    ] {
        let before = s.doc().unwrap().clone();
        let message = s.execute(cmd, &p).unwrap_err().to_string();
        assert!(message.contains("scale/up"), "{message}");
        assert_eq!(s.doc().unwrap(), &before);
    }
    // Exercise nonuniform/native prepared mutations through the same command event guard.
    let before = s.doc().unwrap().clone();
    assert!(
        s.execute_prepared("move", &json!({}), |s| {
            s.doc_mut()?.modify_entity(h, |e| {
                if let EntityKind::Insert(i) = &mut e.kind {
                    i.scale.y *= 2.;
                }
            })?;
            Ok(json!({}))
        })
        .is_err()
    );
    assert_eq!(s.doc().unwrap(), &before);
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap(), &original);
    s.undo().unwrap();
    assert!(s.doc().unwrap().entity(h).is_none());
    s.redo().unwrap();
    assert_eq!(s.doc().unwrap(), &original);
}

#[test]
fn copied_and_pasted_section_markers_survive_identity_remapping() {
    let (mut source, id) = setup();
    let (h, _) = add(&mut source, &id);
    let out = source.execute("copy", &json!({"handles":[h.hex()],"delta":[100,0]})).unwrap();
    let copied = Handle::parse_hex(out["handles"][0].as_str().unwrap()).unwrap();
    assert_eq!(style(&source.doc().unwrap().entity(copied).unwrap().common).unwrap(), Some(0.5));
    let before = source.doc().unwrap().clone();
    assert!(source.execute("rotate", &json!({"handles":[copied.hex()],"base":[0,0],"angle":90})).is_err());
    assert_eq!(source.doc().unwrap(), &before);
    source.execute("copybase", &json!({"handles":[h.hex()],"base":[0,0]})).unwrap();
    let mut destination = crate::extensions::new_session();
    destination.clipboard = source.clipboard.clone();
    destination.clipboard_base = source.clipboard_base;
    destination.clipboard_source = source.clipboard_source.clone();
    let out = destination.execute("pasteblock", &json!({"at":[200,0]})).unwrap();
    let wrapper = Handle::parse_hex(out["handle"].as_str().unwrap()).unwrap();
    destination.execute("move", &json!({"handles":[wrapper.hex()],"delta":[10,0]})).unwrap();
    let before = destination.doc().unwrap().clone();
    assert!(destination.execute("scale", &json!({"handles":[wrapper.hex()],"base":[0,0],"factor":2})).is_err());
    assert_eq!(destination.doc().unwrap(), &before);
    assert!(destination.execute("rotate", &json!({"handles":[wrapper.hex()],"base":[0,0],"angle":45})).is_err());
    assert_eq!(destination.doc().unwrap(), &before);
}

#[test]
fn recursive_wrappers_cannot_hide_nonuniform_or_mirror_transforms() {
    let (mut s, id) = setup();
    let (h, _) = add(&mut s, &id);
    let before = s.doc().unwrap().clone();
    assert!(
        s.execute_prepared("move", &json!({}), |s| {
            let d = s.doc_mut()?;
            let mut child = d.entity(h).unwrap().as_ref().clone();
            child.handle = d.new_handle();
            let mut block = Block::new("SECTION-WRAPPER");
            block.entities.push(child);
            d.blocks.insert("SECTION-WRAPPER".into(), Arc::new(block));
            let mut outer = Block::new("SECTION-OUTER");
            outer.entities.push(Entity {
                handle: d.new_handle(),
                common: Common::default(),
                kind: EntityKind::Insert(Insert { block: "SECTION-WRAPPER".into(), scale: Vec3::new(-1., 1., 1.), ..Insert::default() }),
            });
            d.blocks.insert("SECTION-OUTER".into(), Arc::new(outer));
            d.add(&Space::Model, Common::default(), EntityKind::Insert(Insert { block: "SECTION-OUTER".into(), ..Insert::default() }))?;
            Ok(json!({}))
        })
        .is_err()
    );
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn body_stable_angles_avoid_parallel_45_degree_outline() {
    let mut section = fixture();
    section.caps[0].loops[0] = ring(&[Vec2::new(0., 10.), Vec2::new(10., 0.), Vec2::new(20., 10.), Vec2::new(10., 20.)]);
    assert!((hatch_angle(&section, 0) - 30_f64.to_radians()).abs() < 1e-12);
    let mut second = section.caps[0].clone();
    second.source_body = 1;
    section.caps.push(second);
    assert!((hatch_angle(&section, 1) + 30_f64.to_radians()).abs() < 1e-12);
    let mut another = section.caps[0].clone();
    another.face = 4;
    section.caps.push(another);
    assert!((hatch_angle(&section, 0) - 30_f64.to_radians()).abs() < 1e-12);
}

#[test]
fn request_validation_and_source_anchor_half_space_guards() {
    let mut p = parameters("1");
    let r = request(&p).unwrap();
    assert_eq!(r.direction, [0., 1., 0.]);
    assert_eq!(r.plane.normal, [0., 1., 0.]);
    p["kept_side"] = json!("negative");
    assert_eq!(request(&p).unwrap().direction, [0., -1., 0.]);
    p["plane"]["normal"] = json!([0, 0, 1]);
    let r = request(&p).unwrap();
    assert!(vec(r.direction).cross(vec(r.up)).len() > 0.99);
    for (key, value) in
        [("direction", json!([0, 1, 0])), ("exclude_bodies", json!([0, 0])), ("scale", json!(0)), ("hidden", json!("yes")), ("at", json!([1]))]
    {
        let mut p = parameters("1");
        p[key] = value;
        assert!(request(&p).is_err(), "{key}");
    }
    let (mut s, id) = setup();
    let (_, out) = add(&mut s, &id);
    let audit = &out["section"];
    assert!(validate_source_anchor(audit, 0, [1., 2., 3.]).is_ok());
    assert!(validate_source_anchor(audit, 0, [1., -5e-7, 3.]).is_ok());
    assert!(validate_source_anchor(audit, 0, [1., -1., 3.]).is_err());
    assert!(validate_source_anchor(audit, 1, [1., 2., 3.]).is_err());
    let mut invalid = audit.clone();
    invalid["bodies"][0]["status"] = json!("excluded");
    invalid["bodies"][0]["result_body"] = Value::Null;
    assert!(validate_source_anchor(&invalid, 0, [1., 2., 3.]).is_err());
    let mut invalid = audit.clone();
    invalid["bodies"].as_array_mut().unwrap().push(audit["bodies"][0].clone());
    assert!(validate_source_anchor(&invalid, 0, [1., 2., 3.]).is_err());
}

#[test]
fn cross_sections_keep_only_exact_plane_regions_and_preserve_underlying_view() {
    for (mode, layer) in [("detached", "GOST-OUTLINE"), ("imposed", "GOST-THIN")] {
        let (mut s, id) = setup();
        let underlying = s
            .doc_mut()
            .unwrap()
            .add(&Space::Paper("GOST1".into()), Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(20., 20., 0.) }))
            .unwrap();
        let underlying_entity = s.doc().unwrap().entity(underlying).unwrap().clone();
        let mut p = parameters(&id);
        p["presentation"] = json!(mode);
        let mut section = fixture();
        // Geometry behind the section plane must not leak into a cross-section.
        section.projection.entities[0].kind = EntityKind::Line(Line { a: Vec3::new(100., 100., 0.), b: Vec3::new(200., 200., 0.) });
        let raw_lines = json!(section.projection.lines);
        let original_loops = section.caps[0].loops.clone();
        let out = apply(&mut s, &id, &"a".repeat(64), &p, section).unwrap();
        let h = Handle::parse_hex(out["handle"].as_str().unwrap()).unwrap();
        let block = s.doc().unwrap().block(&insert(&s, h).block).unwrap();
        assert_eq!(block.entities.len(), 6);
        assert_eq!(block.entities.iter().filter(|e| e.common.layer == layer).count(), 5);
        let circles: Vec<_> =
            block.entities.iter().filter_map(|e| if let EntityKind::Circle(circle) = &e.kind { Some(circle) } else { None }).collect();
        assert_eq!(circles.len(), 1);
        assert_eq!(circles[0].center, Vec3::new(10., 10., 0.));
        assert_eq!(circles[0].radius, 3.);
        assert!(!block.entities.iter().any(|e| matches!(&e.kind,EntityKind::Line(line) if line.a.x >= 100.)));
        let native_hatch = hatch_in_block(block);
        assert_eq!(native_hatch.loops, original_loops);
        assert!((native_hatch.pattern_lines[0].delta.1 * insert(&s, h).scale.x - 3.).abs() < 1e-12);
        assert_eq!(s.doc().unwrap().entity(underlying).unwrap(), &underlying_entity);
        assert_eq!(out["section"]["presentation"], mode);
        assert_eq!(out["section"]["visible_geometry"], "intersection_regions_only");
        let saved = s.execute("gost.model.inspect", &json!({"id":id})).unwrap();
        assert_eq!(saved["views"][0]["provenance"]["lines"], raw_lines);
        assert!(validate_source_anchor(&out["section"], 0, [1., 0., 3.]).is_ok());
        assert!(validate_source_anchor(&out["section"], 0, [1., 2., 3.]).is_err());
        let created = s.doc().unwrap().clone();
        s.execute("undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().entity(h).is_none());
        assert_eq!(s.doc().unwrap().entity(underlying).unwrap(), &underlying_entity);
        s.execute("redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap(), &created);
        let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
        s.open_drawing(reopened, "cross-section DXF", None);
        assert_eq!(s.execute("gost.model.inspect", &json!({"id":id})).unwrap(), saved);
        assert_eq!(hatch_in_block(s.doc().unwrap().block(&insert(&s, h).block).unwrap()).loops, original_loops);
    }
}

fn hatch_in_block(block: &Block) -> &Hatch {
    block.entities.iter().find_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h) } else { None }).unwrap()
}

#[test]
fn cross_sections_do_not_bypass_raw_quality_or_payload_integrity() {
    for mode in ["detached", "imposed"] {
        for bad in ["quality", "provenance", "disconnected"] {
            let (mut s, id) = setup();
            let mut p = parameters(&id);
            p["presentation"] = json!(mode);
            let mut section = fixture();
            match bad {
                "quality" => section.projection.quality.unsupported_faces = 1,
                "provenance" => section.projection.entities[0].line_index = 99,
                _ => {
                    let mut cap = section.caps[0].clone();
                    cap.face = 4;
                    cap.loops = vec![ring(&[Vec2::new(40., 0.), Vec2::new(60., 0.), Vec2::new(60., 20.), Vec2::new(40., 20.)])];
                    section.caps.push(cap);
                }
            }
            let before = s.doc().unwrap().clone();
            assert!(apply(&mut s, &id, &"a".repeat(64), &p, section).is_err(), "{mode}/{bad}");
            assert_eq!(s.doc().unwrap(), &before);
        }
    }
    for bad in [Value::Null, json!(42), json!("silhouette")] {
        let mut p = parameters("1");
        p["presentation"] = bad;
        assert!(request(&p).is_err());
    }
}
