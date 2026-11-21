use super::*;
use cadcraft_doc::{
    Line,
    model_projection::{Basis, LineProvenance, PieceProvenance, ProjectedEntity, SourceReference},
};

fn metadata(sha: char) -> Value {
    json!({"protocol":"cadcraft-model-worker/1","path":"/models/деталь.step","source":{"sha256":sha.to_string().repeat(64),"bytes":512},
        "format":"step","units":{"length":"mm","area":"mm2","volume":"mm3","angle":"radian","coordinate_system":"right_handed_z_up"},
        "metadata":{"part_number":null,"engineering_material":null,"description":"Деталь".repeat(100)},
        "bodies":[{"name":"Тело","source_item":17}],"tree":{"name":"Сборка","children":[]},"warnings":[],"report":{}})
}
fn projection(sha: char) -> Projection {
    Projection {
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
        source_sha256: sha.to_string().repeat(64),
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
        lines: vec![LineProvenance {
            placement: 0,
            source: SourceReference::Edge { edge: 7, tangent: false },
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
        source_mapping: "payload_scoped".into(),
        visibility_contract: "exact_hlr".into(),
    }
}
fn linked(s: &mut Session) -> String {
    apply_loaded(s, metadata('a'), None).unwrap()["id"].as_str().unwrap().into()
}
fn params(id: &str) -> Value {
    json!({"id":id,"layout":"GOST1","scale":2})
}

#[test]
fn malformed_or_unknown_records_are_never_silently_overwritten() {
    for text in ["{", r#"{"version":2,"next_id":1,"models":[],"views":[]}"#, r#"{"version":1,"next_id":0,"models":[],"views":[]}"#] {
        let mut s = crate::extensions::new_session();
        let d = s.doc_mut().unwrap();
        let h = d.new_handle();
        let foreign = Arc::make_mut(&mut d.foreign);
        foreign.dictionary.push((RECORD.into(), h));
        foreign.objects.push(ForeignObject { handle: h, kind: "XRECORD".into(), tags: vec![tag(1, text)] });
        let before = s.doc().unwrap().clone();
        assert!(s.execute("gost.model.list", &json!({})).is_err());
        assert!(apply_loaded(&mut s, metadata('a'), None).is_err());
        assert_eq!(s.doc().unwrap(), &before);
    }
}

#[test]
fn source_mismatch_and_superseded_background_refresh_are_atomic() {
    let mut s = crate::extensions::new_session();
    let id = linked(&mut s);
    let before = s.doc().unwrap().clone();
    assert!(apply_view(&mut s, &id, &"a".repeat(64), &params(&id), projection('b')).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    assert!(apply_loaded(&mut s, metadata('b'), Some((&id, &"c".repeat(64)))).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    // A failure after native insertion exercises the shared prepared-command rollback guard.
    let mut nonfinite = projection('a');
    if let EntityKind::Line(line) = &mut nonfinite.entities[0].kind {
        line.b.x = f64::NAN;
    }
    assert!(apply_view(&mut s, &id, &"a".repeat(64), &params(&id), nonfinite).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn refresh_retains_geometry_marks_views_stale_and_refuses_linked_deletion() {
    let mut s = crate::extensions::new_session();
    let id = linked(&mut s);
    let view = apply_view(&mut s, &id, &"a".repeat(64), &params(&id), projection('a')).unwrap();
    let h = Handle::parse_hex(view["handle"].as_str().unwrap()).unwrap();
    let before_view = s.doc().unwrap().entity(h).unwrap().clone();
    let EntityKind::Insert(insert) = &before_view.kind else { panic!() };
    assert_eq!(insert.scale, Vec3::new(0.5, 0.5, 0.5));
    let block = s.doc().unwrap().block(&insert.block).unwrap().clone();
    let EntityKind::Line(line) = &block.entities.iter().next().unwrap().kind else { panic!() };
    assert_eq!(line.b.x, 20.); // Native geometry is model-mm, not paper-scaled coordinates.
    apply_loaded(&mut s, metadata('b'), Some((&id, &"a".repeat(64)))).unwrap();
    let inspected = s.execute("gost.model.inspect", &json!({"id":id})).unwrap();
    assert_eq!(inspected["views"][0]["stale"], true);
    assert_eq!(inspected["views"][0]["source"]["sha256"], "a".repeat(64));
    assert_eq!(s.doc().unwrap().entity(h).unwrap(), &before_view);
    assert_eq!(s.doc().unwrap().block(&insert.block).unwrap(), &block);
    let before_remove = s.doc().unwrap().clone();
    assert!(s.execute("gost.model.remove", &json!({"id":id})).is_err());
    assert_eq!(s.doc().unwrap(), &before_remove);
    s.undo().unwrap();
    assert_eq!(s.execute("gost.model.inspect", &json!({"id":id})).unwrap()["views"][0]["stale"], false);
    s.redo().unwrap();
    assert_eq!(s.execute("gost.model.inspect", &json!({"id":id})).unwrap()["views"][0]["stale"], true);
}

#[test]
fn unicode_xrecord_and_saved_provenance_roundtrip_without_losing_quality() {
    let mut s = crate::extensions::new_session();
    let id = linked(&mut s);
    let mut incomplete = projection('a');
    incomplete.quality.uncertain_pieces = 1;
    incomplete.quality.analytic_complete = false;
    incomplete.entities[0].visibility = Visibility::Uncertain;
    incomplete.lines[0].pieces[0].visibility = Visibility::Uncertain;
    let mut p = params(&id);
    assert!(apply_view(&mut s, &id, &"a".repeat(64), &p, incomplete.clone()).is_err());
    p["accept_incomplete"] = json!(true);
    let view = apply_view(&mut s, &id, &"a".repeat(64), &p, incomplete).unwrap();
    let h = Handle::parse_hex(view["handle"].as_str().unwrap()).unwrap();
    let EntityKind::Insert(insert) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
    let block = s.doc().unwrap().block(&insert.block).unwrap();
    assert_eq!(block.entities.iter().next().unwrap().common.color, cadcraft_color::Color::Index(1));
    assert!(record(s.doc().unwrap()).unwrap().unwrap().tags.iter().filter(|t| t.code == 1).all(|t| t.value.len() <= 240));
    let expected = s.execute("gost.model.inspect", &json!({"id":id})).unwrap();
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(reopened, "linked model roundtrip", None);
    let actual = s.execute("gost.model.inspect", &json!({"id":id})).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual["views"][0]["quality"]["uncertain_pieces"], 1);
    assert_eq!(actual["views"][0]["provenance"]["lines"][0]["source"]["edge"], 7);
    assert!(!actual["views"][0]["warnings"].as_array().unwrap().is_empty());
}

#[test]
fn named_views_do_not_mirror_world_x_or_reverse_top_and_bottom() {
    use cadcraft_geom::drafting3d::Frame;
    let point = Vec3::new(1., 2., 3.);
    for (name, screen) in [
        ("front", Vec2::new(1., 3.)),
        ("rear", Vec2::new(-1., 3.)),
        ("top", Vec2::new(1., 2.)),
        ("bottom", Vec2::new(1., -2.)),
        ("left", Vec2::new(-2., 3.)),
        ("right", Vec2::new(2., 3.)),
    ] {
        let (direction, up) = view_vectors(&json!({"direction":name})).unwrap();
        let camera = Vec3::new(-direction[0], -direction[1], -direction[2]);
        let frame = Frame::new(camera, Vec3::new(up[0], up[1], up[2])).unwrap();
        assert!((frame.point(point) - screen).len() < 1e-12, "{name} mirrored or reversed");
    }
}

#[test]
fn covered_uncertainty_keeps_visible_geometry_but_uncovered_uncertainty_is_rejected() {
    let mut s = crate::extensions::new_session();
    let id = linked(&mut s);
    let mut p = projection('a');
    let mut duplicate = p.entities[0].clone();
    duplicate.covered = true;
    duplicate.visibility = Visibility::Uncertain;
    duplicate.piece_index = 1;
    p.lines[0].pieces.push(PieceProvenance {
        t: duplicate.t,
        visibility: duplicate.visibility,
        covered: true,
        accuracy: Accuracy::Analytic,
        segment_count: 1,
    });
    p.entities.push(duplicate);
    p.quality.analytic_complete = false;
    p.quality.uncertain_pieces = 1;
    p.visibility_contract = "raw_uncertain_and_covered_preserved_no_mesh_fallback".into();
    let accepted = apply_view(&mut s, &id, &"a".repeat(64), &params(&id), p.clone()).unwrap();
    let h = Handle::parse_hex(accepted["handle"].as_str().unwrap()).unwrap();
    let EntityKind::Insert(insert) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
    assert_eq!(s.doc().unwrap().block(&insert.block).unwrap().entities.len(), 1);
    let report = s.execute("gost.model.inspect", &json!({"id":id})).unwrap();
    assert_eq!(report["views"][0]["quality"]["analytic_complete"], false);
    assert_eq!(report["views"][0]["quality"]["uncertain_pieces"], 1);
    assert_eq!(report["views"][0]["provenance"]["lines"][0]["pieces"][1]["covered"], true);
    p.entities[1].covered = false;
    p.lines[0].pieces[1].covered = false;
    let before = s.doc().unwrap().clone();
    assert!(apply_view(&mut s, &id, &"a".repeat(64), &params(&id), p).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}
