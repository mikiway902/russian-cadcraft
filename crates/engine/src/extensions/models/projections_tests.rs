use super::*;
use cadcraft_doc::{
    Block, Entity, Line,
    model_projection::{Accuracy, Basis, LineProvenance, PieceProvenance, ProjectedEntity, Quality, SourceReference, Visibility},
};

fn setup() -> (Session, String, Handle) {
    let mut s = crate::extensions::new_session();
    let metadata = json!({"protocol":"cadcraft-model-worker/1","path":"/models/projection.step",
        "source":{"sha256":"a".repeat(64),"bytes":512},"format":"step",
        "units":{"length":"mm","area":"mm2","volume":"mm3","angle":"radian","coordinate_system":"right_handed_z_up"},
        "metadata":{},"bodies":[{"name":"Part"}],"tree":{},"warnings":[],"report":{}});
    let id = super::super::apply_loaded(&mut s, metadata, None).unwrap()["id"].as_str().unwrap().to_owned();
    let p = json!({"id":id,"layout":"GOST1","direction":"front","at":[100,100],"scale":2});
    let projection = fixture(&p);
    let out = super::super::apply_view(&mut s, &id, &"a".repeat(64), &p, projection).unwrap();
    let h = handle(out["handle"].as_str().unwrap()).unwrap();
    (s, id, h)
}
// Deterministic native exact-curve payload over an off-origin world box. The host's actual
// HLR is exercised separately; these tests consume real engine commands and native geometry.
fn fixture(p: &Value) -> Projection {
    let (d, u) = super::super::view_vectors(p).unwrap();
    let d = Vec3::new(d[0], d[1], d[2]);
    let d = d * (1. / d.len());
    let u = Vec3::new(u[0], u[1], u[2]);
    let x = d.cross(u);
    let x = x * (1. / x.len());
    let y = x.cross(d);
    let mut bounds = Bounds2::EMPTY;
    for a in [10., 30.] {
        for b in [40., 70.] {
            for c in [5., 15.] {
                let world = Vec3::new(a, b, c);
                bounds.add(Vec2::new(world.dot(x), world.dot(y)));
            }
        }
    }
    let corners = [bounds.min, Vec2::new(bounds.max.x, bounds.min.y), bounds.max, Vec2::new(bounds.min.x, bounds.max.y)];
    let mut projection = Projection {
        source_sha256: "a".repeat(64),
        source_bytes: 512,
        basis: Basis { x: [x.x, x.y, x.z], y: [y.x, y.y, y.z], direction: [d.x, d.y, d.z] },
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
        entities: vec![],
        lines: vec![],
        unsupported: vec![],
        prepare_reports: json!([]),
        drawing_stats: json!({}),
        source_mapping: "fixture_exact_world_box".into(),
        visibility_contract: "exact_hlr".into(),
    };
    for n in 0..4 {
        projection.entities.push(ProjectedEntity {
            kind: EntityKind::Line(Line { a: corners[n].to3(0.), b: corners[(n + 1) % 4].to3(0.) }),
            line_index: n,
            piece_index: 0,
            segment_index: 0,
            t: [0., 1.],
            visibility: Visibility::Visible,
            covered: false,
            accuracy: Accuracy::Analytic,
            parameter_reversed: false,
            source_arc: None,
        });
        projection.lines.push(LineProvenance {
            placement: 0,
            source: SourceReference::Edge { edge: n, tangent: false },
            pieces: vec![PieceProvenance {
                t: [0., 1.],
                visibility: Visibility::Visible,
                covered: false,
                accuracy: Accuracy::Analytic,
                segment_count: 1,
            }],
        });
    }
    projection
}
fn insert(s: &Session, h: Handle) -> &Insert {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("native INSERT required") };
    i
}
fn origin(s: &Session, h: Handle) -> Vec2 {
    let i = insert(s, h);
    i.insert.xy() - s.doc().unwrap().block(&i.block).unwrap().base.xy() * i.scale.x
}
fn bounds(s: &Session, h: Handle) -> Bounds2 {
    cadcraft_doc::entity_bounds(s.doc().unwrap(), s.doc().unwrap().entity(h).unwrap(), 0)
}
fn add(s: &mut Session, parent: Handle, direction: &str) -> (Handle, Value) {
    let p = json!({"parent":parent.hex(),"direction":direction});
    let projection = fixture(&prepare(s, &p).unwrap());
    let out = apply(s, &p, projection).unwrap();
    (handle(out["handle"].as_str().unwrap()).unwrap(), out)
}
fn inspect(s: &mut Session, id: &str, h: Handle) -> Value {
    let out = s.execute("gost.model.inspect", &json!({"id":id})).unwrap();
    out["views"].as_array().unwrap().iter().find(|v| v["handle"] == h.hex()).unwrap().clone()
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}");
}

#[test]
fn principal_bases_first_angle_native_bounds_and_world_origin_alignment() {
    for (direction, d, u) in [
        ("top", [0., 0., -1.], [0., 1., 0.]),
        ("bottom", [0., 0., 1.], [0., -1., 0.]),
        ("right", [-1., 0., 0.], [0., 0., 1.]),
        ("left", [1., 0., 0.], [0., 0., 1.]),
    ] {
        let (mut s, id, parent) = setup();
        // The true projected world origin differs from the INSERT when a native block has a base.
        let name = insert(&s, parent).block.clone();
        Arc::make_mut(s.doc_mut().unwrap().blocks.get_mut(&name).unwrap()).base = Vec3::new(7., -3., 0.);
        let p = json!({"parent":parent.hex(),"direction":direction});
        let prepared = prepare(&s, &p).unwrap();
        assert_eq!(prepared["direction"], json!(d));
        assert_eq!(prepared["up"], json!(u));
        assert_eq!(prepared["scale"], 2.);
        assert_eq!(prepared["layout"], "GOST1");
        let (child, out) = add(&mut s, parent, direction);
        let (pb, cb) = (bounds(&s, parent), bounds(&s, child));
        let (po, co) = (origin(&s, parent), origin(&s, child));
        match direction {
            "top" => {
                near(po.x, co.x);
                near(pb.min.y - cb.max.y, 20.);
            }
            "bottom" => {
                near(po.x, co.x);
                near(cb.min.y - pb.max.y, 20.);
            }
            "right" => {
                near(po.y, co.y);
                near(pb.min.x - cb.max.x, 20.);
            }
            "left" => {
                near(po.y, co.y);
                near(cb.min.x - pb.max.x, 20.);
            }
            _ => panic!("unexpected projection direction"),
        }
        assert_eq!(
            out["projection_link"],
            json!({"version":1,"parent":parent.hex(),"original_parent":parent.hex(),"direction":direction,"status":"linked"})
        );
        let saved = inspect(&mut s, &id, child);
        assert_eq!(saved["source"]["sha256"], "a".repeat(64));
        assert_eq!(saved["revision"], 1);
        let block = s.doc().unwrap().block(&insert(&s, child).block).unwrap();
        assert_eq!(block.entities.len(), 4);
        assert!(block.entities.iter().all(|e| matches!(e.kind, EntityKind::Line(_))));
        let marker = &s.doc().unwrap().entity(child).unwrap().common.xdata;
        let n = marker.iter().position(|t| t.value == MARKER).unwrap();
        assert_eq!(marker[n + 1], tag(1070, "1"));
        assert!(marker[n + 1..].iter().all(|t| t.code != 1005));
    }
}

#[test]
fn relative_additional_basis_is_not_replaced_by_named_global_views() {
    let (mut s, id, _) = setup();
    let p = json!({"id":id,"layout":"GOST1","direction":[1,2,-3],"up":[0,0,1],"at":[200,200]});
    let out = super::super::apply_view(&mut s, &id, &"a".repeat(64), &p, fixture(&p)).unwrap();
    let parent = handle(out["handle"].as_str().unwrap()).unwrap();
    let pv = inspect(&mut s, &id, parent);
    let vec = |key: &str| {
        let a = vector(&pv["basis"][key]).unwrap();
        Vec3::new(a[0], a[1], a[2])
    };
    for (direction, d, u) in
        [("top", -vec("y"), vec("direction")), ("bottom", vec("y"), -vec("direction")), ("right", -vec("x"), vec("y")), ("left", vec("x"), vec("y"))]
    {
        let prepared = prepare(&s, &json!({"parent":parent.hex(),"direction":direction})).unwrap();
        let a = vector(&prepared["direction"]).unwrap();
        let b = vector(&prepared["up"]).unwrap();
        assert!((Vec3::new(a[0], a[1], a[2]) - d).len() < 1e-10);
        assert!((Vec3::new(b[0], b[1], b[2]) - u).len() < 1e-10);
        add(&mut s, parent, direction);
    }
}

#[test]
fn translation_moves_all_descendants_and_undo_redo_restores_whole_graph() {
    let (mut s, _, parent) = setup();
    let (child, _) = add(&mut s, parent, "top");
    let (grandchild, _) = add(&mut s, child, "left");
    let original = s.doc().unwrap().clone();
    let positions = [parent, child, grandchild].map(|h| origin(&s, h));
    s.execute("move", &json!({"handles":[parent.hex()],"delta":[31,-7]})).unwrap();
    for (h, was) in [parent, child, grandchild].into_iter().zip(positions) {
        assert!((origin(&s, h) - was - Vec2::new(31., -7.)).len() < 1e-8);
    }
    let translated = s.doc().unwrap().clone();
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap(), &original);
    s.redo().unwrap();
    assert_eq!(s.doc().unwrap(), &translated);
    s.execute("move", &json!({"handles":[parent.hex(),child.hex(),grandchild.hex()],"delta":[3,4]})).unwrap();
    for (h, was) in [parent, child, grandchild].into_iter().zip(positions) {
        assert!((origin(&s, h) - was - Vec2::new(34., -3.)).len() < 1e-8);
    }
}

#[test]
fn child_free_axis_move_propagates_and_misalignment_rolls_back_every_entity() {
    for (direction, free, bad) in [("top", [0, 9], [2, 0]), ("bottom", [0, -9], [2, 0]), ("left", [9, 0], [0, 2]), ("right", [-9, 0], [0, 2])] {
        let (mut s, _, parent) = setup();
        let (child, _) = add(&mut s, parent, direction);
        let (grandchild, _) = add(&mut s, child, "top");
        let old = origin(&s, grandchild);
        s.execute("move", &json!({"handles":[child.hex()],"delta":free})).unwrap();
        assert!((origin(&s, grandchild) - old - Vec2::new(f64::from(free[0]), f64::from(free[1]))).len() < 1e-8);
        let before = s.doc().unwrap().clone();
        assert!(
            s.execute("move", &json!({"handles":[child.hex(),grandchild.hex()],"delta":bad})).unwrap_err().to_string().contains("free paper axis")
        );
        assert_eq!(s.doc().unwrap(), &before);
    }
}

#[test]
fn uniform_scale_and_other_unsafe_transforms_refuse_the_entire_command() {
    let (mut s, _, parent) = setup();
    let (child, _) = add(&mut s, parent, "top");
    for h in [parent, child] {
        for (cmd, p) in [
            ("scale", json!({"handles":[h.hex()],"base":[0,0],"factor":2})),
            ("rotate", json!({"handles":[h.hex()],"base":[0,0],"angle":30})),
            ("mirror", json!({"handles":[h.hex()],"p1":[0,0],"p2":[1,0],"erase":true})),
        ] {
            let before = s.doc().unwrap().clone();
            let error = s.execute(cmd, &p).unwrap_err().to_string();
            assert!(error.contains("linked projection family"), "{error}");
            assert_eq!(s.doc().unwrap(), &before);
        }
    }
    let before = s.doc().unwrap().clone();
    assert!(
        s.execute_prepared("move", &json!({}), |s| {
            s.doc_mut()?.modify_entity(parent, |e| {
                if let EntityKind::Insert(i) = &mut e.kind {
                    i.scale.y *= 2.;
                }
            })?;
            s.doc_mut()?.modify_entity(child, |e| {
                if let EntityKind::Insert(i) = &mut e.kind {
                    i.insert.x += 123.;
                }
            })?;
            Ok(json!({}))
        })
        .is_err()
    );
    assert_eq!(s.doc().unwrap(), &before);
    assert!(
        s.execute_prepared("move", &json!({}), |s| {
            let d = s.doc_mut()?;
            let entity = d.remove_entity(parent).unwrap();
            d.space_mut(&Space::Model).unwrap().push(entity.as_ref().clone());
            Ok(json!({}))
        })
        .is_err()
    );
    assert_eq!(s.doc().unwrap(), &before);
    assert!(
        s.execute_prepared("move", &json!({}), |s| {
            let d = s.doc_mut()?;
            let entity = d.remove_entity(parent).unwrap();
            let mut block = Block::new("PROJECTION-WRAPPER");
            block.entities.push(entity.as_ref().clone());
            d.blocks.insert(block.name.clone(), Arc::new(block));
            d.add(
                &Space::Paper("GOST1".into()),
                Common::default(),
                EntityKind::Insert(Insert { block: "PROJECTION-WRAPPER".into(), ..Insert::default() }),
            )?;
            Ok(json!({}))
        })
        .is_err()
    );
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn deleted_parent_preserves_frozen_native_geometry_and_never_rebinds_a_collision() {
    let (mut s, id, parent) = setup();
    let (child, _) = add(&mut s, parent, "top");
    let old = insert(&s, child).clone();
    let geometry = s.doc().unwrap().block(&old.block).unwrap().clone();
    let before = s.doc().unwrap().clone();
    s.execute_prepared("move", &json!({}), |s| {
        s.doc_mut()?.remove_entity(parent);
        Ok(json!({}))
    })
    .unwrap();
    assert_eq!(insert(&s, child), &old);
    assert_eq!(s.doc().unwrap().block(&old.block).unwrap(), &geometry);
    let orphan = inspect(&mut s, &id, child)["provenance"]["projection_link"].clone();
    assert_eq!(orphan["status"], "missing_parent");
    assert!(orphan["parent"].is_null());
    assert_eq!(orphan["original_parent"], parent.hex());
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap(), &before);
    s.redo().unwrap();
    s.execute_prepared("move", &json!({}), |s| {
        s.doc_mut()?.space_mut(&Space::Paper("GOST1".into())).unwrap().push(Entity {
            handle: parent,
            common: Common::default(),
            kind: EntityKind::Insert(old.clone()),
        });
        Ok(json!({}))
    })
    .unwrap();
    assert_eq!(inspect(&mut s, &id, child)["provenance"]["projection_link"], orphan);
    let was = origin(&s, child);
    s.execute("move", &json!({"handles":[parent.hex()],"delta":[10,0]})).unwrap();
    assert_eq!(origin(&s, child), was);
}

#[test]
fn completion_recomputes_live_placement_and_rejects_wrong_source_basis_or_quality_atomically() {
    let (mut s, _, parent) = setup();
    let p = json!({"parent":parent.hex(),"direction":"top","gap":0});
    let projection = fixture(&prepare(&s, &p).unwrap());
    s.execute("move", &json!({"handles":[parent.hex()],"delta":[13,7]})).unwrap();
    let out = apply(&mut s, &p, projection.clone()).unwrap();
    let child = handle(out["handle"].as_str().unwrap()).unwrap();
    near(origin(&s, parent).x, origin(&s, child).x);
    near(bounds(&s, parent).min.y, bounds(&s, child).max.y);
    let before = s.doc().unwrap().clone();
    let mut bad = projection.clone();
    bad.source_sha256 = "b".repeat(64);
    assert!(apply(&mut s, &p, bad).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    let mut bad = projection.clone();
    bad.basis.x[0] *= -1.;
    assert!(apply(&mut s, &p, bad).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    let mut bad = projection.clone();
    bad.quality.open_bodies = 1;
    assert!(apply(&mut s, &p, bad.clone()).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    let mut accept = p.clone();
    accept["accept_incomplete"] = json!(true);
    let out = apply(&mut s, &accept, bad).unwrap();
    assert!(!out["warnings"].as_array().unwrap().is_empty());
}

// Emulate an untrusted persisted XRECORD, not a production mutator bypassing Store validation.
fn corrupt(s: &mut Session, edit: impl FnOnce(&mut Value)) {
    let store = load(s.doc().unwrap()).unwrap();
    let mut value = json!(store);
    edit(&mut value);
    let record = super::super::record(s.doc().unwrap()).unwrap().unwrap().handle;
    let object = Arc::make_mut(&mut s.doc_mut().unwrap().foreign).objects.iter_mut().find(|o| o.handle == record).unwrap();
    object.tags.retain(|t| t.code != 1);
    let text = serde_json::to_string(&value).unwrap();
    for chunk in text.as_bytes().chunks(240) {
        object.tags.push(tag(1, std::str::from_utf8(chunk).unwrap()));
    }
}
#[test]
fn cyclic_malformed_duplicate_and_foreign_source_graphs_refuse_without_partial_moves() {
    for kind in ["cycle", "foreign_source", "revision", "duplicate", "malformed"] {
        let (mut s, _, parent) = setup();
        let (child, _) = add(&mut s, parent, "top");
        corrupt(&mut s, |store| {
            let views = store["views"].as_array_mut().unwrap();
            match kind {
                "cycle" => {
                    views[0]["provenance"]["projection_link"] =
                        json!({"version":1,"parent":child.hex(),"original_parent":child.hex(),"direction":"bottom","status":"linked"})
                }
                "foreign_source" => {
                    views[1]["source"]["sha256"] = json!("b".repeat(64));
                    views[1]["metadata"]["source"]["sha256"] = json!("b".repeat(64));
                }
                "revision" => views[1]["revision"] = json!(2),
                "duplicate" => views.push(views[1].clone()),
                "malformed" => views[1]["provenance"]["projection_link"]["status"] = json!("linked_later"),
                _ => panic!("unexpected malformed graph fixture"),
            }
        });
        let before = s.doc().unwrap().clone();
        assert!(s.execute("move", &json!({"handles":[parent.hex()],"delta":[12,17]})).is_err(), "{kind}");
        assert_eq!(s.doc().unwrap(), &before);
    }
}

#[test]
fn invalid_requests_stale_parent_and_unsupported_placements_refuse_without_changes() {
    let (mut s, _, parent) = setup();
    let good = json!({"parent":parent.hex(),"direction":"top"});
    for (key, value) in [
        ("direction", json!("rear")),
        ("gap", json!(-1)),
        ("gap", json!(10001)),
        ("hidden", json!(1)),
        ("scale", json!(2)),
        ("parent", json!("not-a-handle")),
    ] {
        let mut p = good.clone();
        p[key] = value;
        assert!(prepare(&s, &p).is_err());
    }
    let original = s.doc().unwrap().clone();
    for policy in ["rotation", "array", "clipless_mirror", "space"] {
        s.state_mut().unwrap().doc = Arc::new(original.clone());
        if policy == "space" {
            let d = s.doc_mut().unwrap();
            let e = d.remove_entity(parent).unwrap();
            d.model.push(e.as_ref().clone());
        } else {
            s.doc_mut()
                .unwrap()
                .modify_entity(parent, |e| {
                    if let EntityKind::Insert(i) = &mut e.kind {
                        match policy {
                            "rotation" => i.rotation = 0.1,
                            "array" => i.cols = 2,
                            _ => i.scale.x = -1.,
                        }
                    }
                })
                .unwrap();
        }
        let before = s.doc().unwrap().clone();
        assert!(apply(&mut s, &good, fixture(&json!({"direction":"top"}))).is_err());
        assert_eq!(s.doc().unwrap(), &before);
    }
    s.state_mut().unwrap().doc = Arc::new(original);
    let metadata = load(s.doc().unwrap()).unwrap().models[0].metadata.clone();
    super::super::apply_loaded(&mut s, metadata, Some(("1", &"a".repeat(64)))).unwrap();
    assert!(prepare(&s, &good).unwrap_err().to_string().contains("stale_source"));
}

#[test]
fn rational_conic_extrema_accept_zero_and_negative_middle_weights_without_refitting() {
    for weight in [0., -0.5, -0.8, 0.5] {
        let (mut s, _, parent) = setup();
        let p = json!({"parent":parent.hex(),"direction":"bottom"});
        let mut projection = fixture(&prepare(&s, &p).unwrap());
        let mut spline = Spline::from_control(vec![Vec2::new(-5., 0.), Vec2::new(0., 8.), Vec2::new(5., 0.)], 2);
        spline.weights = vec![1., weight, 1.];
        projection.entities[0].kind = EntityKind::Spline(spline.clone());
        projection.entities.truncate(1);
        let pb = bounds(&s, parent);
        // The negative conic minimum is analytic at t=1/2.
        let minimum = if weight < 0. { 8. * weight / (1. + weight) } else { 0. };
        let out = apply(&mut s, &p, projection).unwrap();
        let child = handle(out["handle"].as_str().unwrap()).unwrap();
        near(insert(&s, child).insert.y, pb.max.y + 20. - minimum * 0.5);
        assert_eq!(out["projection_placement"]["conservative_spline_margin"], false);
        let block = s.doc().unwrap().block(&insert(&s, child).block).unwrap();
        assert!(block.entities.iter().any(|e| e.kind == EntityKind::Spline(spline.clone())));
    }
    for weight in [-1., -2.] {
        let (mut s, _, parent) = setup();
        let p = json!({"parent":parent.hex(),"direction":"bottom"});
        let mut projection = fixture(&prepare(&s, &p).unwrap());
        let mut spline = Spline::from_control(vec![Vec2::new(-5., 0.), Vec2::new(0., 8.), Vec2::new(5., 0.)], 2);
        spline.weights = vec![1., weight, 1.];
        projection.entities[0].kind = EntityKind::Spline(spline);
        let before = s.doc().unwrap().clone();
        assert!(apply(&mut s, &p, projection).is_err());
        assert_eq!(s.doc().unwrap(), &before);
    }
}

#[test]
fn general_spline_hull_gap_is_explicit_and_unproven_denominators_refuse_atomically() {
    let (mut s, _, parent) = setup();
    let p = json!({"parent":parent.hex(),"direction":"top"});
    let mut projection = fixture(&prepare(&s, &p).unwrap());
    let mut spline = Spline::from_control(vec![Vec2::new(0., 0.), Vec2::new(1., 100.), Vec2::new(2., 100.), Vec2::new(3., 0.)], 3);
    projection.entities[0].kind = EntityKind::Spline(spline.clone());
    let pb = bounds(&s, parent);
    let out = apply(&mut s, &p, projection.clone()).unwrap();
    let child = handle(out["handle"].as_str().unwrap()).unwrap();
    near(insert(&s, child).insert.y, pb.min.y - 20. - 100. * 0.5);
    assert_eq!(out["projection_placement"]["conservative_spline_margin"], true);
    assert_eq!(out["projection_placement"]["gap_contract"], "minimum_native_curve_gap");
    let block = s.doc().unwrap().block(&insert(&s, child).block).unwrap();
    assert!(block.entities.iter().any(|e| e.kind == EntityKind::Spline(spline.clone())));
    spline.weights = vec![1., -0.2, 1., 1.];
    projection.entities[0].kind = EntityKind::Spline(spline);
    let before = s.doc().unwrap().clone();
    assert!(apply(&mut s, &p, projection).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn graph_depth_32_is_accepted_and_next_projection_is_atomic() {
    let (mut s, _, mut parent) = setup();
    for _ in 0..32 {
        parent = add(&mut s, parent, "top").0;
    }
    let p = json!({"parent":parent.hex(),"direction":"top"});
    let projection = fixture(&prepare(&s, &p).unwrap());
    let before = s.doc().unwrap().clone();
    assert!(apply(&mut s, &p, projection).unwrap_err().to_string().contains("depth"));
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn refresh_marks_family_stale_without_reprojecting_or_moving_native_curves() {
    let (mut s, id, parent) = setup();
    let (child, _) = add(&mut s, parent, "top");
    let parent_insert = insert(&s, parent).clone();
    let child_insert = insert(&s, child).clone();
    let parent_geometry = s.doc().unwrap().block(&parent_insert.block).unwrap().clone();
    let child_geometry = s.doc().unwrap().block(&child_insert.block).unwrap().clone();
    let mut metadata = load(s.doc().unwrap()).unwrap().models[0].metadata.clone();
    metadata["source"]["sha256"] = json!("b".repeat(64));
    super::super::apply_loaded(&mut s, metadata, Some((&id, &"a".repeat(64)))).unwrap();
    assert_eq!(insert(&s, parent), &parent_insert);
    assert_eq!(insert(&s, child), &child_insert);
    assert_eq!(s.doc().unwrap().block(&parent_insert.block).unwrap(), &parent_geometry);
    assert_eq!(s.doc().unwrap().block(&child_insert.block).unwrap(), &child_geometry);
    let view = inspect(&mut s, &id, child);
    assert_eq!(view["stale"], true);
    assert_eq!(view["provenance"]["projection_link"]["status"], "linked");
    assert!(prepare(&s, &json!({"parent":child.hex(),"direction":"left"})).unwrap_err().to_string().contains("stale_source"));
}

#[test]
fn unrelated_line_and_unmanaged_insert_edits_do_not_decode_the_model_store() {
    let (mut s, _, parent) = setup();
    add(&mut s, parent, "top");
    let unrelated = s
        .execute_prepared("move", &json!({}), |s| {
            let d = s.doc_mut()?;
            let mut block = Block::new("UNRELATED-PROJECTION-TEST");
            block.entities.push(Entity {
                handle: d.new_handle(),
                common: Common::default(),
                kind: EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1., 0., 0.) }),
            });
            d.blocks.insert(block.name.clone(), Arc::new(block));
            let h = d.add(
                &Space::Paper("GOST1".into()),
                Common::default(),
                EntityKind::Insert(Insert { block: "UNRELATED-PROJECTION-TEST".into(), ..Insert::default() }),
            )?;
            Ok(json!({"handle":h.hex()}))
        })
        .unwrap()["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    corrupt(&mut s, |store| store["views"][1]["provenance"]["projection_link"]["status"] = json!("bad"));
    s.execute("move", &json!({"handles":[unrelated],"delta":[1,2]})).unwrap();
    s.execute_prepared("move", &json!({}), |s| {
        s.doc_mut()?.add(&Space::Paper("GOST1".into()), Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1., 0., 0.) }))?;
        Ok(json!({}))
    })
    .unwrap();
    // A relevant operation must read and reject exactly that malformed record.
    let before = s.doc().unwrap().clone();
    assert!(s.execute("move", &json!({"handles":[parent.hex()],"delta":[1,2]})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn section_parent_and_detached_relationship_marker_are_explicitly_refused() {
    let (mut s, _, parent) = setup();
    corrupt(&mut s, |store| store["views"][0]["provenance"]["section"] = json!({"kept_side":"positive"}));
    assert!(prepare(&s, &json!({"parent":parent.hex(),"direction":"top"})).unwrap_err().to_string().contains("section"));
    let (mut s, _, parent) = setup();
    let (child, _) = add(&mut s, parent, "top");
    let before = s.doc().unwrap().clone();
    assert!(
        s.execute_prepared("move", &json!({}), |s| {
            s.doc_mut()?.modify_entity(child, |e| {
                let start = e.common.xdata.iter().position(|t| t.code == 1001 && t.value == MARKER).unwrap();
                e.common.xdata.drain(start..start + 2);
            })?;
            Ok(json!({}))
        })
        .is_err()
    );
    assert_eq!(s.doc().unwrap(), &before);
}
