use super::super::sections;
use super::*;
use cadcraft_doc::{
    Line,
    model_projection::{Basis, LineProvenance, ProjectedEntity},
};

fn setup(source_scale: f64) -> (Session, String, Handle) {
    let mut s = crate::extensions::new_session();
    let metadata = json!({"protocol":"cadcraft-model-worker/1","path":"/models/section.step",
        "source":{"sha256":"a".repeat(64),"bytes":512},"format":"step",
        "units":{"length":"mm","area":"mm2","volume":"mm3","angle":"radian","coordinate_system":"right_handed_z_up"},
        "metadata":{},"bodies":[{"name":"Part"}],"tree":{},"warnings":[],"report":{}});
    let id = super::super::apply_loaded(&mut s, metadata, None).unwrap()["id"].as_str().unwrap().to_owned();
    let mut projection = sections::tests::fixture().projection;
    projection.basis = Basis { x: [1., 0., 0.], y: [0., 1., 0.], direction: [0., 0., -1.] };
    let entity: ProjectedEntity = projection.entities[0].clone();
    let line: LineProvenance = projection.lines[0].clone();
    projection.entities.clear();
    projection.lines.clear();
    let corners = [Vec2::ZERO, Vec2::new(20., 0.), Vec2::new(20., 20.), Vec2::new(0., 20.)];
    for i in 0..4 {
        let mut e = entity.clone();
        e.line_index = i;
        e.kind = EntityKind::Line(Line { a: corners[i].to3(0.), b: corners[(i + 1) % 4].to3(0.) });
        projection.entities.push(e);
        projection.lines.push(line.clone());
    }
    let out = super::super::apply_view(
        &mut s,
        &id,
        &"a".repeat(64),
        &json!({"id":id,"direction":"top","layout":"GOST1","at":[100,100],"scale":source_scale}),
        projection,
    )
    .unwrap();
    let source = handle(out["handle"].as_str().unwrap()).unwrap();
    s.state_mut().unwrap().space = Space::Paper("GOST1".into());
    (s, id, source)
}
fn parameters(id: &str, source: Handle, presentation: &str) -> Value {
    json!({"id":id,"plane":{"origin":[0,6,0],"normal":[0,1,0]},"kept_side":"positive","presentation":presentation,
        "source_view":source.hex(),"layout":"GOST1","at":[160,100],"scale":2})
}
fn add(s: &mut Session, id: &str, p: &Value) -> (Handle, Value) {
    let mut section = sections::tests::fixture();
    section.plane.origin = [0., 6., 0.];
    let out = sections::apply(s, id, &"a".repeat(64), p, section).unwrap();
    (handle(out["handle"].as_str().unwrap()).unwrap(), out)
}
fn insert(s: &Session, h: Handle) -> &Insert {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("native INSERT required") };
    i
}
fn marks(s: &Session, h: Handle) -> Vec<EntityKind> {
    let d = s.doc().unwrap();
    let i = insert(s, h);
    let block = d.block(&i.block).unwrap();
    let m = Mat3::scale(i.scale.x, i.scale.x).then(Mat3::translate(i.insert.xy() - block.base.xy() * i.scale.x));
    block
        .entities
        .iter()
        .filter(|e| mark_owner(&e.common).unwrap().is_some())
        .map(|e| {
            let mut kind = e.kind.clone();
            kind.transform(&m);
            if let EntityKind::Text(text) = &mut kind
                && let Some(anchor) = text.align_pt
                && text.halign == cadcraft_doc::HAlign::Center
            {
                // Justified DXF TEXT may normalize its unused baseline insert.
                text.insert = anchor;
            }
            kind
        })
        .collect()
}
fn saved(s: &Session, h: Handle) -> SavedView {
    load(s.doc().unwrap()).unwrap().views.into_iter().find(|v| v.handle == h.hex()).unwrap()
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}")
}

#[test]
fn paper_marks_and_indexed_caption_use_actual_independent_scales_and_nonzero_block_base() {
    for scale in [0.25, 2., 10.] {
        let (mut s, id, source) = setup(scale);
        let block = insert(&s, source).block.clone();
        Arc::make_mut(s.doc_mut().unwrap().blocks.get_mut(&block).unwrap()).base = Vec3::new(7., -3., 0.);
        let mut p = parameters(&id, source, "detached");
        p["label"] = json!("А12");
        let (section, out) = add(&mut s, &id, &p);
        assert_eq!(out["section"]["notation"]["source_view"], source.hex());
        assert_eq!(out["section"]["notation"]["source_scale"], 1. / scale);
        let generated = marks(&s, source);
        let strokes: Vec<_> = generated.iter().filter_map(|e| if let EntityKind::Line(l) = e { Some(l) } else { None }).collect();
        assert_eq!(strokes.len(), 4);
        for l in &strokes[..2] {
            near((l.b - l.a).len(), 10.);
            near(l.a.y, 100. + 9. / scale);
            near(l.b.y, l.a.y);
        }
        near(strokes[0].a.x, 100. - 7. / scale - 15.);
        near(strokes[1].b.x, 100. + 13. / scale + 15.);
        let labels: Vec<_> = generated.iter().filter_map(|e| if let EntityKind::MText(t) = e { Some(t) } else { None }).collect();
        assert_eq!(labels.len(), 2);
        for t in labels {
            assert_eq!(t.contents, "А{\\A1;\\S^12;}");
            near(t.height, 7.);
        }
        let d = s.doc().unwrap();
        let i = insert(&s, section);
        let caption =
            d.block(&i.block).unwrap().entities.iter().find_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t) } else { None }).unwrap();
        assert_eq!(caption.contents, "А{\\A1;\\S^12;}—А{\\A1;\\S^12;} (1:2)");
        near(caption.height * i.scale.x, 7.);
        near(caption.insert.y * i.scale.x, 18.);
    }
}

#[test]
fn native_move_follows_only_the_selected_block_and_undo_restores_every_entity() {
    let (mut s, id, source) = setup(2.);
    let (section, _) = add(&mut s, &id, &parameters(&id, source, "detached"));
    let original = s.doc().unwrap().clone();
    let before = marks(&s, source);
    let section_at = insert(&s, section).insert;
    s.execute("move", &json!({"handles":[source.hex()],"delta":[13,17]})).unwrap();
    assert_eq!(insert(&s, section).insert, section_at);
    let moved = marks(&s, source);
    for (mut expected, actual) in before.into_iter().zip(moved) {
        expected.transform(&Mat3::translate(Vec2::new(13., 17.)));
        assert_eq!(expected, actual);
    }
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap(), &original);
    let source_at = insert(&s, source).insert;
    s.execute("move", &json!({"handles":[section.hex()],"delta":[7,9]})).unwrap();
    assert_eq!(insert(&s, source).insert, source_at);
    assert_eq!(marks(&s, source), marks_from_original(&original, source));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap(), &original);
}
fn marks_from_original(d: &Drawing, h: Handle) -> Vec<EntityKind> {
    let mut s = crate::extensions::new_session();
    s.open_drawing(d.clone(), "comparison", None);
    marks(&s, h)
}

#[test]
fn section_delete_cleans_only_its_marks_and_source_delete_freezes_without_rebinding() {
    let (mut s, id, source) = setup(2.);
    let (a, _) = add(&mut s, &id, &parameters(&id, source, "detached"));
    let (b, out) = add(&mut s, &id, &parameters(&id, source, "detached"));
    assert_eq!(out["section"]["notation"]["label"], "Б");
    assert_eq!(marks(&s, source).len(), 16);
    let before = s.doc().unwrap().clone();
    s.execute("erase", &json!({"handles":[a.hex()]})).unwrap();
    assert_eq!(marks(&s, source).len(), 8);
    assert_eq!(link(&saved(&s, b)).unwrap().unwrap().source_view, Some(source.hex()));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap(), &before);
    s.execute("erase", &json!({"handles":[source.hex()]})).unwrap();
    for h in [a, b] {
        let saved = saved(&s, h);
        let relation = link(&saved).unwrap().unwrap();
        assert_eq!(relation.status, "missing_source");
        assert_eq!(relation.source_view, None);
        assert_eq!(relation.original_source_view, source.hex());
        assert!(saved.parameters["source_view"].is_null());
        assert!(s.doc().unwrap().entity(h).is_some());
    }
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn imposed_arrows_omit_letters_and_do_not_consume_a_designation_or_require_text_layer() {
    let (mut s, id, source) = setup(2.);
    s.execute("layer.set", &json!({"name":"GOST-TEXT","locked":true})).unwrap();
    let (h, out) = add(&mut s, &id, &parameters(&id, source, "imposed"));
    assert!(out["section"]["notation"]["label"].is_null());
    assert_eq!(marks(&s, source).len(), 6);
    assert!(marks(&s, source).iter().all(|e| !matches!(e, EntityKind::Text(_) | EntityKind::MText(_))));
    assert!(
        s.doc().unwrap().block(&insert(&s, h).block).unwrap().entities.iter().all(|e| !matches!(e.kind, EntityKind::Text(_) | EntityKind::MText(_)))
    );
    assert_eq!(standards::next_letter(s.doc().unwrap(), "test").unwrap(), "А");
}

#[test]
fn invalid_source_plane_scale_layout_and_letters_refuse_atomically() {
    let (mut s, id, source) = setup(2.);
    let baseline = s.doc().unwrap().clone();
    for change in [
        json!({"source_view":"FFFF"}),
        json!({"source_view":null}),
        json!({"label":"З"}),
        json!({"label":null}),
        json!({"label":"А\\PБ"}),
        json!({"layout":"Model"}),
    ] {
        let mut p = parameters(&id, source, "detached");
        p.as_object_mut().unwrap().extend(change.as_object().unwrap().clone());
        let mut section = sections::tests::fixture();
        section.plane.origin = [0., 6., 0.];
        assert!(sections::apply(&mut s, &id, &"a".repeat(64), &p, section).is_err(), "{p}");
        assert_eq!(s.doc().unwrap(), &baseline);
    }
    let (section, _) = add(&mut s, &id, &parameters(&id, source, "detached"));
    let baseline = s.doc().unwrap().clone();
    for (cmd, p) in [
        ("scale", json!({"handles":[source.hex()],"base":[0,0],"factor":2})),
        ("rotate", json!({"handles":[source.hex()],"base":[0,0],"angle":30})),
        ("mirror", json!({"handles":[source.hex()],"p1":[0,0],"p2":[0,1],"erase":true})),
    ] {
        assert!(s.execute(cmd, &p).is_err(), "{cmd}");
        assert_eq!(s.doc().unwrap(), &baseline);
    }
    assert_eq!(link(&saved(&s, section)).unwrap().unwrap().status, "linked");
}

#[test]
fn dxf_reopen_keeps_native_marks_and_active_source_move() {
    let (mut s, id, source) = setup(2.);
    let (section, _) = add(&mut s, &id, &parameters(&id, source, "detached"));
    let expected = marks(&s, source);
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(reopened, "section notation DXF", None);
    assert_eq!(marks(&s, source), expected);
    assert_eq!(link(&saved(&s, section)).unwrap().unwrap().source_view, Some(source.hex()));
    s.execute("move", &json!({"handles":[source.hex()],"delta":[11,0]})).unwrap();
    for (mut expected, actual) in expected.into_iter().zip(marks(&s, source)) {
        expected.transform(&Mat3::translate(Vec2::new(11., 0.)));
        assert_eq!(expected, actual);
    }
}

#[test]
fn exploding_either_end_detaches_geometry_without_leaving_live_source_marks() {
    for explode_source in [false, true] {
        let (mut s, id, source) = setup(2.);
        let (section, _) = add(&mut s, &id, &parameters(&id, source, "detached"));
        let before = s.doc().unwrap().clone();
        let h = if explode_source { source } else { section };
        let result = s.execute("explode", &json!({"handles":[h.hex()]})).unwrap();
        let created = result["handles"].as_array().unwrap();
        assert!(created.iter().any(|v| {
            let e = s.doc().unwrap().entity(handle(v.as_str().unwrap()).unwrap()).unwrap();
            if explode_source { matches!(e.kind, EntityKind::Solid(_)) } else { matches!(e.kind, EntityKind::Hatch(_)) }
        }));
        for h in created {
            let e = s.doc().unwrap().entity(handle(h.as_str().unwrap()).unwrap()).unwrap();
            assert!(mark_owner(&e.common).unwrap().is_none(), "exploded primitives retain geometry, not an association");
        }
        if explode_source {
            assert_eq!(link(&saved(&s, section)).unwrap().unwrap().status, "missing_source");
            assert!(s.doc().unwrap().entity(section).is_some());
        } else {
            assert!(marks(&s, source).is_empty());
        }
        s.undo().unwrap();
        assert_eq!(s.doc().unwrap(), &before);
    }
}

#[test]
fn untagged_source_block_alias_and_nested_wrapper_refuse_atomically() {
    let (mut s, id, source) = setup(2.);
    add(&mut s, &id, &parameters(&id, source, "detached"));
    let block = insert(&s, source).block.clone();
    let before = s.doc().unwrap().clone();
    let result = s.execute("insert", &json!({"name":block,"at":[250,100]}));
    assert!(result.is_err(), "an untagged source-block alias must not bypass paper notation policy");
    assert_eq!(s.doc().unwrap(), &before);
    let result = s.execute("block", &json!({"name":"SourceWrapper","base":[0,0],"handles":[source.hex()]}));
    assert!(result.is_err(), "an active source must remain top-level, not nested in an arbitrary wrapper");
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn a_face_on_source_view_cannot_describe_the_cutting_plane_trace() {
    let (mut s, id, _) = setup(2.);
    let p = json!({"id":id,"layout":"GOST1","direction":"front","at":[50,50]});
    let source = super::super::apply_view(&mut s, &id, &"a".repeat(64), &p, sections::tests::fixture().projection).unwrap();
    let source = handle(source["handle"].as_str().unwrap()).unwrap();
    let before = s.doc().unwrap().clone();
    let mut section = sections::tests::fixture();
    section.plane.origin = [0., 6., 0.];
    let error = sections::apply(&mut s, &id, &"a".repeat(64), &parameters(&id, source, "detached"), section).unwrap_err();
    assert!(error.to_string().contains("edge-on"), "{error}");
    assert_eq!(s.doc().unwrap(), &before);
}
