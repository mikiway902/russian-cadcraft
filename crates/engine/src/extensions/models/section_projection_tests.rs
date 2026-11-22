use super::super::{SavedView, Value, Vec3, apply_loaded, apply_view, json, sections};
use super::*;
use cadcraft_doc::{
    Line,
    model_projection::{Basis, Projection},
};
use cadcraft_geom::Mat3;

fn projected(p: &Value) -> Projection {
    let mut projection = sections::tests::fixture().projection;
    let (direction, up) = super::super::view_vectors(p).unwrap();
    let d = Vec3::new(direction[0], direction[1], direction[2]).normalized();
    let u = Vec3::new(up[0], up[1], up[2]);
    let x = d.cross(u).normalized();
    let y = x.cross(d);
    projection.basis = Basis { x: [x.x, x.y, x.z], y: [y.x, y.y, y.z], direction: [d.x, d.y, d.z] };
    let entity = projection.entities[0].clone();
    let line = projection.lines[0].clone();
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
    projection
}

fn setup(direction: &str) -> (Session, String, Handle) {
    let mut s = crate::extensions::new_session();
    let metadata = json!({"protocol":"cadcraft-model-worker/1","path":"/models/section.step",
        "source":{"sha256":"a".repeat(64),"bytes":512},"format":"step",
        "units":{"length":"mm","area":"mm2","volume":"mm3","angle":"radian","coordinate_system":"right_handed_z_up"},
        "metadata":{},"bodies":[{"name":"Part"}],"tree":{},"warnings":[],"report":{}});
    let id = apply_loaded(&mut s, metadata, None).unwrap()["id"].as_str().unwrap().to_owned();
    let p = json!({"id":id,"direction":direction,"layout":"GOST1","at":[100,100],"scale":2});
    let out = apply_view(&mut s, &id, &"a".repeat(64), &p, projected(&p)).unwrap();
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
    let canonical = sections::prepare_request(s, p).unwrap();
    let request = sections::request(&canonical).unwrap();
    section.symmetry = request.symmetry_axis.map(|axis| cadcraft_doc::model_section::SectionSymmetry {
        axis,
        center: [10., 10.],
        symmetric: true,
        tolerance_mm: 1e-9,
    });
    let out = sections::apply(s, id, &"a".repeat(64), p, section).unwrap();
    (handle(out["handle"].as_str().unwrap()).unwrap(), out)
}

fn insert(s: &Session, h: Handle) -> &Insert {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("native INSERT required") };
    i
}

fn saved(s: &Session, h: Handle) -> SavedView {
    load(s.doc().unwrap()).unwrap().views.into_iter().find(|v| v.handle == h.hex()).unwrap()
}

fn relation(s: &Session, h: Handle) -> section_notation::Link {
    section_notation::link(&saved(s, h)).unwrap().unwrap()
}

fn near(actual: Vec2, expected: Vec2) {
    assert!((actual - expected).len() < 1e-8, "{actual:?} != {expected:?}");
}

fn native(s: &Session, h: Handle) -> Vec<EntityKind> {
    let i = insert(s, h);
    let block = s.doc().unwrap().block(&i.block).unwrap();
    let matrix = Mat3::scale(i.scale.x, i.scale.x).then(Mat3::translate(i.insert.xy() - block.base.xy() * i.scale.x));
    block
        .entities
        .iter()
        .map(|e| {
            let mut kind = e.kind.clone();
            kind.transform(&matrix);
            if let EntityKind::Text(text) = &mut kind
                && let Some(anchor) = text.align_pt
                && text.halign == cadcraft_doc::HAlign::Center
            {
                text.insert = anchor;
            }
            kind
        })
        .collect()
}

fn translated(before: &[EntityKind], after: &[EntityKind], delta: Vec2) {
    assert_eq!(before.len(), after.len());
    for (old, actual) in before.iter().zip(after) {
        let mut expected = old.clone();
        expected.transform(&Mat3::translate(delta));
        assert_eq!(&expected, actual);
    }
}

#[test]
fn default_detached_snaps_exact_native_center_and_source_move_carries_axis_once() {
    let (mut s, id, source) = setup("top");
    let (section, out) = add(&mut s, &id, &parameters(&id, source, "detached"));
    near(insert(&s, section).insert.xy(), Vec2::new(160., 98.));
    assert_eq!(out["at"], json!([160., 98.]));
    let constraint = relation(&s, section).projection.unwrap();
    assert_eq!(constraint.mode, Mode::AlongTrace);
    assert_eq!(saved(&s, section).parameters["projection_link"], true);
    let original = s.doc().unwrap().clone();
    let source_geometry = native(&s, source);
    let section_geometry = native(&s, section);
    assert!(!section_geometry.iter().any(|e| matches!(e, EntityKind::Text(_) | EntityKind::MText(_))));
    assert_eq!(relation(&s, section).kind, section_notation::Kind::SymmetryAxis);
    let section_block = s.doc().unwrap().block(&insert(&s, section).block).unwrap().clone();
    s.execute("move", &json!({"handles":[source.hex()],"delta":[13,17]})).unwrap();
    near(insert(&s, section).insert.xy(), Vec2::new(173., 115.));
    translated(&source_geometry, &native(&s, source), Vec2::new(13., 17.));
    translated(&section_geometry, &native(&s, section), Vec2::new(13., 17.));
    assert!(Arc::ptr_eq(&section_block, s.doc().unwrap().block(&insert(&s, section).block).unwrap()));
    assert_eq!(relation(&s, section).projection, Some(constraint));
    let moved = s.doc().unwrap().clone();
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap(), &original);
    s.redo().unwrap();
    assert_eq!(s.doc().unwrap(), &moved);
    s.execute("move", &json!({"handles":[source.hex(),section.hex()],"delta":[3,4]})).unwrap();
    near(insert(&s, section).insert.xy(), Vec2::new(176., 119.));
    translated(&section_geometry, &native(&s, section), Vec2::new(16., 21.));
}

#[test]
fn detached_and_cutaway_allow_along_trace_but_refuse_perpendicular_and_native_transforms_atomically() {
    for presentation in ["detached", "cutaway"] {
        let (mut s, id, source) = setup("top");
        let (section, _) = add(&mut s, &id, &parameters(&id, source, presentation));
        let source_geometry = native(&s, source);
        let target_geometry = native(&s, section);
        s.execute("move", &json!({"handles":[section.hex()],"delta":[9,0]})).unwrap();
        near(insert(&s, section).insert.xy(), Vec2::new(169., 98.));
        assert_eq!(native(&s, source), source_geometry);
        translated(&target_geometry, &native(&s, section), Vec2::new(9., 0.));
        let before = s.doc().unwrap().clone();
        for (command, p) in [
            ("move", json!({"handles":[section.hex()],"delta":[0,1]})),
            ("scale", json!({"handles":[section.hex()],"base":[0,0],"factor":2})),
            ("rotate", json!({"handles":[section.hex()],"base":[0,0],"angle":30})),
            ("mirror", json!({"handles":[section.hex()],"p1":[0,0],"p2":[0,1],"erase":true})),
            ("scale", json!({"handles":[source.hex(),section.hex()],"base":[0,0],"factor":2})),
        ] {
            assert!(s.execute(command, &p).is_err(), "{command}: {p}");
            assert_eq!(s.doc().unwrap(), &before);
        }
        s.execute("move", &json!({"handles":[source.hex()],"delta":[5,7]})).unwrap();
        near(insert(&s, section).insert.xy(), Vec2::new(174., 105.));
    }
}

#[test]
fn imposed_preserves_source_coordinates_and_scale_and_cannot_move_independently() {
    let (mut s, id, source) = setup("top");
    let (section, out) = add(&mut s, &id, &parameters(&id, source, "imposed"));
    near(insert(&s, section).insert.xy(), Vec2::new(100., 98.));
    assert_eq!(out["at"], json!([100., 98.]));
    assert_eq!(insert(&s, source).scale, insert(&s, section).scale);
    assert_eq!(relation(&s, section).projection.unwrap().mode, Mode::Fixed);
    let before = s.doc().unwrap().clone();
    for delta in [[1, 0], [0, 1]] {
        assert!(s.execute("move", &json!({"handles":[section.hex()],"delta":delta})).is_err());
        assert_eq!(s.doc().unwrap(), &before);
    }
    s.execute("move", &json!({"handles":[source.hex()],"delta":[8,11]})).unwrap();
    near(insert(&s, section).insert.xy(), Vec2::new(108., 109.));
    let before = s.doc().unwrap().clone();
    let mut wrong_scale = parameters(&id, source, "imposed");
    wrong_scale["scale"] = json!(4);
    let mut section = sections::tests::fixture();
    section.plane.origin = [0., 6., 0.];
    assert!(sections::apply(&mut s, &id, &"a".repeat(64), &wrong_scale, section).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn off_center_imposed_cap_preserves_world_trace_coordinates_at_actual_scale_and_block_base() {
    for scale in [0.25, 2., 10.] {
        for base in [Vec2::ZERO, Vec2::new(7., -3.)] {
            let (mut s, id, source) = setup("top");
            let block = insert(&s, source).block.clone();
            let d = s.doc_mut().unwrap();
            d.modify_entity(source, |e| {
                let EntityKind::Insert(i) = &mut e.kind else { panic!("source fixture requires INSERT") };
                i.scale = Vec3::new(1. / scale, 1. / scale, 1. / scale);
            })
            .unwrap();
            let block = Arc::make_mut(d.blocks.get_mut(&block).unwrap());
            block.base = base.to3(0.);
            // Source spans X=0..100, while the cut region spans X=30..50.
            // Their bbox centres must not determine the imposed X coordinate.
            for h in block.entities.handles() {
                block.entities.modify(h, |entity| {
                    let EntityKind::Line(line) = &mut entity.kind else { panic!("source fixture requires LINE boundary") };
                    line.a.x *= 5.;
                    line.b.x *= 5.;
                });
            }
            let source_origin = Vec2::new(100., 100.) - base * (1. / scale);
            let mut p = parameters(&id, source, "imposed");
            p["scale"] = json!(scale);
            let mut section = sections::tests::fixture();
            section.plane.origin = [0., 6., 0.];
            for ring in &mut section.caps[0].loops {
                ring.transform(&Mat3::translate(Vec2::new(30., 0.)));
            }
            let canonical = sections::prepare_request(&s, &p).unwrap();
            section.symmetry = sections::request(&canonical).unwrap().symmetry_axis.map(|axis| cadcraft_doc::model_section::SectionSymmetry {
                axis,
                center: [40., 10.],
                symmetric: true,
                tolerance_mm: 1e-9,
            });
            let out = sections::apply(&mut s, &id, &"a".repeat(64), &p, section).unwrap();
            let target = handle(out["handle"].as_str().unwrap()).unwrap();
            let at = insert(&s, target).insert.xy();
            near(at, source_origin + Vec2::new(0., -4. / scale));
            // Each cap X maps to the same paper X as the source-world point.
            for x in [30., 40., 50.] {
                let target_paper = at + Vec2::new(x, 10.) * (1. / scale);
                let source_paper = source_origin + Vec2::new(x, 6.) * (1. / scale);
                near(target_paper, source_paper);
            }
            let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
            s.open_drawing(reopened, "off-centre imposed DXF", None);
            let before = s.doc().unwrap().clone();
            s.execute("move", &json!({"handles":[source.hex(),target.hex()],"delta":[13,17]})).unwrap();
            near(insert(&s, target).insert.xy(), at + Vec2::new(13., 17.));
            s.undo().unwrap();
            assert_eq!(s.doc().unwrap(), &before);
        }
    }
}

#[test]
fn explicit_independent_records_neither_snap_nor_follow_and_keep_native_caption() {
    let (mut s, id, source) = setup("top");
    let mut p = parameters(&id, source, "detached");
    p["projection_link"] = json!(false);
    let (section, out) = add(&mut s, &id, &p);
    near(insert(&s, section).insert.xy(), Vec2::new(160., 100.));
    assert_eq!(out["at"], json!([160., 100.]));
    assert!(relation(&s, section).projection.is_none());
    let native_before = native(&s, section);
    s.execute("move", &json!({"handles":[source.hex()],"delta":[5,7]})).unwrap();
    assert_eq!(native(&s, section), native_before);
    s.execute("move", &json!({"handles":[section.hex()],"delta":[2,3]})).unwrap();
    translated(&native_before, &native(&s, section), Vec2::new(2., 3.));
}

#[test]
fn ordinary_projection_ancestor_translation_reaches_source_then_section() {
    let (mut s, id, ancestor) = setup("front");
    let request = json!({"parent":ancestor.hex(),"direction":"top"});
    let canonical = projections::prepare(&s, &request).unwrap();
    let out = projections::apply(&mut s, &request, projected(&canonical)).unwrap();
    let source = handle(out["handle"].as_str().unwrap()).unwrap();
    let (section, _) = add(&mut s, &id, &parameters(&id, source, "detached"));
    let positions = [ancestor, source, section].map(|h| insert(&s, h).insert.xy());
    let geometry = native(&s, section);
    let before = s.doc().unwrap().clone();
    s.execute("move", &json!({"handles":[ancestor.hex()],"delta":[31,-7]})).unwrap();
    for (h, was) in [ancestor, source, section].into_iter().zip(positions) {
        near(insert(&s, h).insert.xy(), was + Vec2::new(31., -7.));
    }
    translated(&geometry, &native(&s, section), Vec2::new(31., -7.));
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap(), &before);
    s.execute("move", &json!({"handles":[ancestor.hex(),source.hex(),section.hex()],"delta":[3,4]})).unwrap();
    for (h, was) in [ancestor, source, section].into_iter().zip(positions) {
        near(insert(&s, h).insert.xy(), was + Vec2::new(3., 4.));
    }
}

#[test]
fn pair_copy_and_clipboard_remap_active_source_preserve_audit_and_survive_dxf_and_undo() {
    for clipboard in [false, true] {
        let (mut s, id, source) = setup("top");
        let (section, _) = add(&mut s, &id, &parameters(&id, source, "detached"));
        let before = s.doc().unwrap().clone();
        let constraint = relation(&s, section).projection;
        let geometry = native(&s, section);
        let out = if clipboard {
            s.execute("copybase", &json!({"handles":[section.hex(),source.hex()],"base":[0,0]})).unwrap();
            s.execute("pasteclip", &json!({"at":[100,0]})).unwrap()
        } else {
            s.execute("copy", &json!({"handles":[section.hex(),source.hex()],"delta":[100,0]})).unwrap()
        };
        let copied_section = handle(out["handles"][0].as_str().unwrap()).unwrap();
        let copied_source = handle(out["handles"][1].as_str().unwrap()).unwrap();
        let copied_relation = relation(&s, copied_section);
        assert_eq!(copied_relation.source_view, Some(copied_source.hex()));
        assert_eq!(copied_relation.original_source_view, source.hex());
        assert_eq!(copied_relation.projection, constraint);
        translated(&geometry, &native(&s, copied_section), Vec2::new(100., 0.));
        s.undo().unwrap();
        assert_eq!(s.doc().unwrap(), &before);
        s.redo().unwrap();
        let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
        s.open_drawing(reopened, "section projection DXF", None);
        let original_geometry = native(&s, section);
        let copied_geometry = native(&s, copied_section);
        s.execute("move", &json!({"handles":[copied_source.hex()],"delta":[7,9]})).unwrap();
        translated(&copied_geometry, &native(&s, copied_section), Vec2::new(7., 9.));
        assert_eq!(native(&s, section), original_geometry);
        assert_eq!(relation(&s, copied_section).projection, constraint);
    }
}

#[test]
fn section_only_transfer_freezes_constraint_and_never_captures_colliding_source() {
    for clipboard in [false, true] {
        let (mut s, id, source) = setup("top");
        let (section, _) = add(&mut s, &id, &parameters(&id, source, "detached"));
        let constraint = relation(&s, section).projection;
        let (mut destination, _, collision) = setup("top");
        assert_eq!(source, collision);
        let (target, copied) = if clipboard {
            s.execute("copyclip", &json!({"handles":[section.hex()]})).unwrap();
            destination.clipboard = s.clipboard.clone();
            destination.clipboard_base = s.clipboard_base;
            destination.clipboard_source = s.clipboard_source.clone();
            let out = destination.execute("pasteclip", &json!({"at":[100,0]})).unwrap();
            (destination, handle(out["handles"][0].as_str().unwrap()).unwrap())
        } else {
            let out = s.execute("copy", &json!({"handles":[section.hex()],"delta":[100,0]})).unwrap();
            (s, handle(out["handles"][0].as_str().unwrap()).unwrap())
        };
        let mut target = target;
        let copied_relation = relation(&target, copied);
        assert_eq!(copied_relation.status, "missing_source");
        assert!(copied_relation.source_view.is_none());
        assert_eq!(copied_relation.original_source_view, source.hex());
        assert_eq!(copied_relation.projection, constraint);
        let geometry = native(&target, copied);
        target.execute("move", &json!({"handles":[collision.hex()],"delta":[4,6]})).unwrap();
        assert_eq!(native(&target, copied), geometry);
        target.execute("move", &json!({"handles":[copied.hex()],"delta":[2,3]})).unwrap();
        translated(&geometry, &native(&target, copied), Vec2::new(2., 3.));
    }
}

#[test]
fn delete_and_explode_freeze_orphans_without_losing_native_geometry_and_undo_restores_link() {
    for command in ["erase", "explode"] {
        for remove_source in [false, true] {
            let (mut s, id, source) = setup("top");
            let (section, _) = add(&mut s, &id, &parameters(&id, source, "detached"));
            let before = s.doc().unwrap().clone();
            let geometry = native(&s, section);
            let constraint = relation(&s, section).projection;
            let h = if remove_source { source } else { section };
            let out = s.execute(command, &json!({"handles":[h.hex()]})).unwrap();
            if remove_source {
                let orphan = relation(&s, section);
                assert_eq!(orphan.status, "missing_source");
                assert!(orphan.source_view.is_none());
                assert_eq!(orphan.original_source_view, source.hex());
                assert_eq!(orphan.projection, constraint);
                assert_eq!(native(&s, section), geometry);
                s.execute("move", &json!({"handles":[section.hex()],"delta":[2,3]})).unwrap();
                translated(&geometry, &native(&s, section), Vec2::new(2., 3.));
                s.undo().unwrap();
            } else {
                let block = s.doc().unwrap().block(&insert(&s, source).block).unwrap();
                assert!(block.entities.iter().all(|e| section_notation::mark_owner(&e.common).unwrap().is_none()));
            }
            if command == "explode" {
                let created = out["handles"].as_array().unwrap();
                assert!(!created.is_empty());
                for value in created {
                    let e = s.doc().unwrap().entity(handle(value.as_str().unwrap()).unwrap()).unwrap();
                    assert!(section_notation::mark_owner(&e.common).unwrap().is_none());
                }
            }
            s.undo().unwrap();
            assert_eq!(s.doc().unwrap(), &before);
        }
    }
}

#[test]
fn large_paper_origins_do_not_turn_tolerance_into_millimetre_drift() {
    let (mut s, id, source) = setup("top");
    let (section, _) = add(&mut s, &id, &parameters(&id, source, "detached"));
    s.execute("move", &json!({"handles":[source.hex()],"delta":[900000000,900000000]})).unwrap();
    let before = s.doc().unwrap().clone();
    assert!(s.execute("move", &json!({"handles":[section.hex()],"delta":[0,0.001]})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    s.execute("move", &json!({"handles":[source.hex(),section.hex()],"delta":[0.125,0.25]})).unwrap();
    near(insert(&s, section).insert.xy(), Vec2::new(900000160.125, 900000098.25));
}

#[test]
fn nonzero_source_block_base_uses_native_origin_for_binding_and_translation() {
    for presentation in ["detached", "imposed"] {
        let (mut s, id, source) = setup("top");
        let name = insert(&s, source).block.clone();
        Arc::make_mut(s.doc_mut().unwrap().blocks.get_mut(&name).unwrap()).base = Vec3::new(7., -3., 0.);
        let (section, _) = add(&mut s, &id, &parameters(&id, source, presentation));
        let expected = if presentation == "imposed" { Vec2::new(96.5, 99.5) } else { Vec2::new(160., 99.5) };
        near(insert(&s, section).insert.xy(), expected);
        let target = native(&s, section);
        s.execute("move", &json!({"handles":[source.hex()],"delta":[5,7]})).unwrap();
        near(insert(&s, section).insert.xy(), expected + Vec2::new(5., 7.));
        translated(&target, &native(&s, section), Vec2::new(5., 7.));
    }
}

#[test]
fn legacy_v1_records_remain_independent_after_dxf_migration_and_native_move() {
    let (mut s, id, source) = setup("top");
    let mut p = parameters(&id, source, "detached");
    p["projection_link"] = json!(false);
    let (section, _) = add(&mut s, &id, &p);
    // Reconstruct the actual persisted v1 record rather than calling a migration
    // helper directly; consumers load it through the bounded Store decoder.
    let mut value = json!(load(s.doc().unwrap()).unwrap());
    let view = value["views"].as_array_mut().unwrap().iter_mut().find(|v| v["handle"] == section.hex()).unwrap();
    view["parameters"].as_object_mut().unwrap().remove("projection_link");
    view["parameters"].as_object_mut().unwrap().remove("symmetry_axis");
    view["provenance"]["section"].as_object_mut().unwrap().remove("symmetry");
    let notation = view["provenance"]["section"]["notation"].as_object_mut().unwrap();
    notation.insert("version".into(), json!(1));
    notation.remove("projection");
    notation.remove("kind");
    notation.remove("source_marks");
    let record = super::super::record(s.doc().unwrap()).unwrap().unwrap().handle;
    let object = Arc::make_mut(&mut s.doc_mut().unwrap().foreign).objects.iter_mut().find(|o| o.handle == record).unwrap();
    object.tags.retain(|t| t.code != 1);
    let text = serde_json::to_string(&value).unwrap();
    for chunk in text.as_bytes().chunks(240) {
        object.tags.push(super::super::tag(1, std::str::from_utf8(chunk).unwrap()));
    }
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(reopened, "legacy independent section DXF", None);
    let geometry = native(&s, section);
    let migrated = relation(&s, section);
    assert_eq!(migrated.version, 3);
    assert_eq!(migrated.kind, section_notation::Kind::CuttingPlane);
    assert!(migrated.projection.is_none());
    assert_eq!(saved(&s, section).parameters["projection_link"], false);
    s.execute("move", &json!({"handles":[source.hex()],"delta":[5,7]})).unwrap();
    assert_eq!(native(&s, section), geometry);
    s.execute("move", &json!({"handles":[section.hex()],"delta":[2,3]})).unwrap();
    translated(&geometry, &native(&s, section), Vec2::new(2., 3.));
}

#[test]
fn verified_axis_needs_no_unused_letter_or_cutting_line_layers() {
    let (mut s, id, source) = setup("top");
    for name in ["GOST-TEXT", "GOST-SECTION"] {
        s.execute("layer.set", &json!({"name":name,"locked":true})).unwrap();
    }
    let (section, _) = add(&mut s, &id, &parameters(&id, source, "imposed"));
    let link = relation(&s, section);
    assert_eq!(link.kind, section_notation::Kind::SymmetryAxis);
    assert_eq!(link.source_marks, 0);
    assert!(link.label.is_none());
    assert!(native(&s, section).iter().all(|e| !matches!(e, EntityKind::Text(_) | EntityKind::MText(_))));
    assert_eq!(crate::extensions::gost::standards::next_letter(s.doc().unwrap(), "test").unwrap(), "А");
}

#[test]
fn asymmetric_bore_keeps_lettered_notation_instead_of_using_outer_bbox_symmetry() {
    let (mut s, id, source) = setup("top");
    let p = parameters(&id, source, "detached");
    let canonical = sections::prepare_request(&s, &p).unwrap();
    let axis = sections::request(&canonical).unwrap().symmetry_axis.unwrap();
    let mut section = sections::tests::fixture();
    section.plane.origin = [0., 6., 0.];
    let cadcraft_doc::HatchEdge::Segment(cadcraft_geom::Segment::Arc { arc, .. }) = &mut section.caps[0].loops[1].edges[0] else {
        panic!("fixture circular bore required");
    };
    arc.center.y += 2.;
    section.symmetry = Some(cadcraft_doc::model_section::SectionSymmetry { axis, center: [10., 10.], symmetric: false, tolerance_mm: 1e-9 });
    let out = sections::apply(&mut s, &id, &"a".repeat(64), &p, section).unwrap();
    let target = handle(out["handle"].as_str().unwrap()).unwrap();
    let link = relation(&s, target);
    assert_eq!(link.kind, section_notation::Kind::CuttingPlane);
    assert_eq!(link.label.as_deref(), Some("А"));
    assert_eq!(link.source_marks, 8);
    assert!(native(&s, target).iter().any(|e| matches!(e, EntityKind::Text(_) | EntityKind::MText(_))));
}

#[test]
fn explicit_designation_or_different_scale_preserves_cutting_plane_style() {
    for explicit in [false, true] {
        let (mut s, id, source) = setup("top");
        let mut p = parameters(&id, source, "detached");
        if explicit {
            p["label"] = json!("Б");
        } else {
            p["scale"] = json!(4);
        }
        let (section, _) = add(&mut s, &id, &p);
        let link = relation(&s, section);
        assert_eq!(link.kind, section_notation::Kind::CuttingPlane);
        assert_eq!(link.label.as_deref(), Some(if explicit { "Б" } else { "А" }));
        assert!(saved(&s, section).parameters["symmetry_axis"].is_null());
        assert!(native(&s, section).iter().any(|e| matches!(e, EntityKind::Text(_) | EntityKind::MText(_))));
    }
}

#[test]
fn editing_verified_native_geometry_refuses_atomically_until_exploded() {
    let (mut s, id, source) = setup("top");
    let (section, _) = add(&mut s, &id, &parameters(&id, source, "imposed"));
    let block = insert(&s, section).block.clone();
    let before = s.doc().unwrap().clone();
    let result = s.execute_prepared("gost.model.section", &json!({}), |s| {
        let b = Arc::make_mut(s.doc_mut()?.blocks.get_mut(&block).unwrap());
        let h = b.entities.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap().handle;
        b.entities.modify(h, |e| {
            let EntityKind::Line(line) = &mut e.kind else { panic!("native contour required") };
            line.a.x += 1.;
        });
        Ok(json!(null))
    });
    assert!(result.is_err());
    assert_eq!(s.doc().unwrap(), &before);
    s.execute("explode", &json!({"handles":[section.hex()]})).unwrap();
    let e = super::super::entities(s.doc().unwrap()).find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap();
    let EntityKind::Line(line) = &e.kind else { panic!("exposed native line required") };
    let (handle, a) = (e.handle, line.a);
    s.execute("move", &json!({"handles":[handle.hex()],"delta":[1,0]})).unwrap();
    let EntityKind::Line(line) = &s.doc().unwrap().entity(handle).unwrap().kind else { panic!("native line remains editable") };
    near(line.a.xy(), a.xy() + Vec2::X);
}

#[test]
fn legacy_v2_link_keeps_projection_without_reclassifying_old_lettered_geometry() {
    let (mut s, id, source) = setup("top");
    let mut p = parameters(&id, source, "detached");
    p["label"] = json!("А");
    let (section, _) = add(&mut s, &id, &p);
    let before = native(&s, section);
    let mut value = json!(load(s.doc().unwrap()).unwrap());
    let view = value["views"].as_array_mut().unwrap().iter_mut().find(|v| v["handle"] == section.hex()).unwrap();
    view["parameters"].as_object_mut().unwrap().remove("symmetry_axis");
    view["provenance"]["section"].as_object_mut().unwrap().remove("symmetry");
    let relation = view["provenance"]["section"]["notation"].as_object_mut().unwrap();
    relation.insert("version".into(), json!(2));
    relation.remove("kind");
    relation.remove("source_marks");
    let record = super::super::record(s.doc().unwrap()).unwrap().unwrap().handle;
    let object = Arc::make_mut(&mut s.doc_mut().unwrap().foreign).objects.iter_mut().find(|o| o.handle == record).unwrap();
    object.tags.retain(|t| t.code != 1);
    let text = serde_json::to_string(&value).unwrap();
    for chunk in text.as_bytes().chunks(240) {
        object.tags.push(super::super::tag(1, std::str::from_utf8(chunk).unwrap()));
    }
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(reopened, "legacy projected section DXF", None);
    let migrated = super::section_notation::link(&saved(&s, section)).unwrap().unwrap();
    assert_eq!(migrated.version, 3);
    assert_eq!(migrated.kind, section_notation::Kind::CuttingPlane);
    assert_eq!(migrated.label.as_deref(), Some("А"));
    assert!(migrated.projection.is_some());
    assert_eq!(native(&s, section), before);
    s.execute("move", &json!({"handles":[source.hex()],"delta":[5,7]})).unwrap();
    translated(&before, &native(&s, section), Vec2::new(5., 7.));
}

#[test]
fn oblique_small_circular_section_keeps_thin_cross_without_duplicate_main_axis() {
    let (mut s, id, source) = setup("top");
    let mut p = parameters(&id, source, "imposed");
    p["plane"]["normal"] = json!([1, 1, 0]);
    let canonical = sections::prepare_request(&s, &p).unwrap();
    let request = sections::request(&canonical).unwrap();
    let mut section = sections::tests::fixture();
    section.plane = request.plane;
    let d = Vec3::new(request.direction[0], request.direction[1], request.direction[2]);
    let up = Vec3::new(request.up[0], request.up[1], request.up[2]);
    let x = d.cross(up).normalized();
    let y = x.cross(d);
    section.projection.basis = Basis { x: [x.x, x.y, x.z], y: [y.x, y.y, y.z], direction: request.direction };
    section.caps[0].loops[0].edges = vec![cadcraft_doc::HatchEdge::Segment(cadcraft_geom::Segment::Arc {
        arc: cadcraft_geom::Arc { center: Vec2::new(10., 10.), radius: 9., start: 0., end: 0. },
        ccw: true,
    })];
    section.symmetry = Some(cadcraft_doc::model_section::SectionSymmetry {
        axis: request.symmetry_axis.unwrap(),
        center: [10., 10.],
        symmetric: true,
        tolerance_mm: 1e-9,
    });
    let out = sections::apply(&mut s, &id, &"a".repeat(64), &p, section).unwrap();
    let target = handle(out["handle"].as_str().unwrap()).unwrap();
    let block = s.doc().unwrap().block(&insert(&s, target).block).unwrap();
    let mut length = 0.;
    for entity in block.entities.iter() {
        if let EntityKind::Line(line) = &entity.kind {
            assert_eq!(entity.common.layer, "GOST-THIN");
            length += (line.b - line.a).len();
        }
    }
    // Two 30-model-mm axes, regardless of whether their centre stroke is split.
    assert!((length - 60.).abs() < 1e-8, "duplicated or incorrectly scaled centre cross: {length}");
    assert_eq!(relation(&s, target).source_marks, 0);
}
