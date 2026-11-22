use super::*;
fn created(s: &mut Session, c: &str, p: Value) -> Handle {
    s.execute(c, &p).unwrap()["handle"].as_str().and_then(Handle::parse_hex).unwrap()
}
fn model(s: &mut Session) -> Handle {
    created(s, "gost.solid", json!({"box":{"min":[0,0,0],"max":[40,20,30]}}))
}
fn dm(s: &Session, h: Handle) -> &cadcraft_doc::Dimension {
    let EntityKind::Dimension(dm) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
    dm
}
fn shown(s: &Session, h: Handle) -> String {
    let d = s.doc().unwrap();
    let dm = dm(s, h);
    let st = d.dim_style(&dm.style).unwrap().with_overrides(&dm.overrides);
    cadcraft_render::dimension_geometry(dm, &st, 1.0).mtext
}
#[test]
fn model_edit_updates_paper_geometry_model_measurement_and_dxf_references() {
    let mut s = crate::extensions::new_session();
    let source = model(&mut s);
    let view = created(&mut s, "gost.projection", json!({"sources":[source.hex()],"layout":"GOST1","at":[60,100],"direction":"top","scale":2}));
    let dim = created(
        &mut s,
        "gost.projection.dimension",
        json!({"view":view.hex(),"a":{"source":source.hex(),"vertex":1},"b":{"source":source.hex(),"vertex":2}}),
    );
    assert!((dm(&s, dim).p14.x - dm(&s, dim).p13.x - 20.0).abs() < 1e-8);
    assert_eq!(shown(&s, dim), "40");
    s.execute("gost.solid", &json!({"handle":source.hex(),"box":{"min":[0,0,0],"max":[60,20,30]}})).unwrap();
    assert_eq!(shown(&s, dim), "60");
    s.undo().unwrap();
    assert_eq!(shown(&s, dim), "40");
    let d = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(d, "again", None);
    s.execute("gost.solid", &json!({"handle":source.hex(),"box":{"min":[0,0,0],"max":[80,20,30]}})).unwrap();
    assert_eq!(shown(&s, dim), "80");
    s.execute("erase", &json!({"handles":[source.hex()]})).unwrap();
    assert_eq!(dm(&s, dim).text, "?");
    assert!(!load_view(s.doc().unwrap(), view).unwrap().issues.is_empty());
    s.undo().unwrap();
    assert!(dm(&s, dim).text.is_empty());
}
#[test]
fn real_section_has_closed_hatch_and_intersection_dimension_follows_plane() {
    let mut s = crate::extensions::new_session();
    let source = model(&mut s);
    let view = created(
        &mut s,
        "gost.section.view",
        json!({"sources":[source.hex()],"plane":{"normal":[0,0,1],"offset":15},"layout":"GOST1","at":[60,150],"scale":2}),
    );
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(view).unwrap().kind else { panic!() };
    let b = s.doc().unwrap().block(&i.block).unwrap();
    let hatch = b.entities.iter().find_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h) } else { None }).unwrap();
    let polygon: Vec<_> = hatch.loops[0].vertices.iter().map(|v| v.p).collect();
    assert!((cadcraft_geom::shoelace(&polygon).abs() - 200.0).abs() < 1e-8);
    assert_eq!(hatch.pattern_lines[0].delta, (0.0, 3.0));
    let dim = created(
        &mut s,
        "gost.projection.dimension",
        json!({"view":view.hex(),"a":{"source":source.hex(),"edge":[1,5]},"b":{"source":source.hex(),"edge":[2,6]}}),
    );
    assert_eq!(shown(&s, dim), "40");
    s.execute("gost.section.view", &json!({"handle":view.hex(),"label":null})).unwrap();
    s.execute("gost.section.view", &json!({"handle":view.hex(),"plane":{"normal":[0,0,1],"offset":20}})).unwrap();
    assert_eq!(shown(&s, dim), "40");
    let EntityKind::Insert(insert) = &s.doc().unwrap().entity(view).unwrap().kind else { panic!() };
    assert!(
        !s.doc()
            .unwrap()
            .block(&insert.block)
            .unwrap()
            .entities
            .iter()
            .any(|entity| matches!(entity.kind, EntityKind::Text(_) | EntityKind::MText(_))),
        "editing the plane must preserve an intentionally removed designation"
    );
    let before = s.doc().unwrap().clone();
    assert!(s.execute("gost.section.view", &json!({"handle":view.hex(),"plane":{"normal":[0,0,1],"offset":50}})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}
#[test]
fn topology_changes_require_stable_identities_and_unrelated_edits_keep_blocks() {
    let mut s = crate::extensions::new_session();
    let source = model(&mut s);
    let view = created(&mut s, "gost.projection", json!({"sources":[source.hex()],"direction":"isometric"}));
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(view).unwrap().kind else { panic!() };
    let old = s.doc().unwrap().block(&i.block).unwrap().clone();
    s.execute("line", &json!({"points":[[0,0],[1,2]]})).unwrap();
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(view).unwrap().kind else { panic!() };
    assert_eq!(&old, s.doc().unwrap().block(&i.block).unwrap());
    let before = s.doc().unwrap().clone();
    assert!(s.execute("gost.solid", &json!({"handle":source.hex(),"profile":[[0,0],[10,0],[0,10]],"z0":0,"z1":10})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    assert!(s.execute("gost.projection", &json!({"sources":[source.hex()],"direction":[0,0,0]})).is_err());
}

#[test]
fn invalid_cut_diagonal_and_view_transform_cannot_silently_break_anchors() {
    let mut s = crate::extensions::new_session();
    let source = model(&mut s);
    let view = created(&mut s, "gost.section.view", json!({"sources":[source.hex()],"plane":{"normal":[0,0,1],"offset":15}}));
    let before = s.doc().unwrap().clone();
    assert!(
        s.execute(
            "gost.projection.dimension",
            &json!({"view":view.hex(),"a":{"source":source.hex(),"edge":[1,7]},"b":{"source":source.hex(),"edge":[2,6]}})
        )
        .is_err()
    );
    assert_eq!(s.doc().unwrap(), &before);
    assert!(s.execute("scale", &json!({"handles":[view.hex()],"base":[0,0],"factor":1e20})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn isometric_aligned_dimensions_measure_source_edges_and_keep_editable_offsets() {
    let mut s = crate::extensions::new_session();
    let source = model(&mut s);
    let view = created(&mut s, "gost.projection", json!({"sources":[source.hex()],"direction":"isometric","scale":2,"at":[80,130]}));
    let dim = created(
        &mut s,
        "gost.projection.dimension",
        json!({"view":view.hex(),"kind":"aligned","a":{"source":source.hex(),"vertex":1},"b":{"source":source.hex(),"vertex":2}}),
    );
    assert_eq!(shown(&s, dim), "40");
    assert!((dm(&s, dim).p13.xy().dist(dm(&s, dim).p14.xy()) - 20.0).abs() > 1.0);
    let first = dm(&s, dim).defpt;
    s.execute("gost.projection.dimension", &json!({"handle":dim.hex(),"offset":20})).unwrap();
    assert!((dm(&s, dim).defpt - first).len() > 9.9);
    assert_eq!(shown(&s, dim), "40");
    s.execute("gost.solid", &json!({"handle":source.hex(),"box":{"min":[0,0,0],"max":[60,20,30]}})).unwrap();
    assert_eq!(shown(&s, dim), "60");
    let report = s.execute("gost.check", &json!({})).unwrap();
    assert!(report["findings"].as_array().unwrap().iter().all(|f| f["code"] != "dimension.disassociated"));
}

#[test]
fn dimension_text_survives_model_updates_and_unresolved_dxf_relinking() {
    let mut s = crate::extensions::new_session();
    let source = model(&mut s);
    let view = created(&mut s, "gost.projection", json!({"sources":[source.hex()],"direction":"top","scale":2}));
    let dimension = created(
        &mut s,
        "gost.projection.dimension",
        json!({"view":view.hex(),"a":{"source":source.hex(),"vertex":1},"b":{"source":source.hex(),"vertex":2}}),
    );
    s.doc_mut()
        .unwrap()
        .modify_entity(dimension, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.text = "<>*".into();
            }
        })
        .unwrap();
    s.execute("gost.solid", &json!({"handle":source.hex(),"box":{"min":[0,0,0],"max":[60,20,30]}})).unwrap();
    assert_eq!(shown(&s, dimension), "60*");
    s.execute("erase", &json!({"handles":[source.hex()]})).unwrap();
    assert!(dm(&s, dimension).text.contains("<>*"));
    let d = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(d, "again", None);
    let replacement = created(&mut s, "gost.solid", json!({"box":{"min":[0,0,0],"max":[80,20,30]}}));
    s.execute("gost.projection", &json!({"handle":view.hex(),"sources":[replacement.hex()]})).unwrap();
    s.execute(
        "gost.projection.dimension",
        &json!({"handle":dimension.hex(),"a":{"source":replacement.hex(),"vertex":1},"b":{"source":replacement.hex(),"vertex":2}}),
    )
    .unwrap();
    assert_eq!(dm(&s, dimension).text, "<>*");
    assert_eq!(shown(&s, dimension), "80*");
}

#[test]
fn copied_view_and_dimension_keep_their_pair_after_model_update_and_dxf() {
    let mut s = crate::extensions::new_session();
    let source = model(&mut s);
    let view = created(&mut s, "gost.projection", json!({"sources":[source.hex()],"direction":"top","at":[50,120]}));
    let dimension = created(
        &mut s,
        "gost.projection.dimension",
        json!({"view":view.hex(),"a":{"source":source.hex(),"vertex":1},"b":{"source":source.hex(),"vertex":2}}),
    );
    s.execute("layout.set", &json!({"name":"GOST1"})).unwrap();
    let copied = s.execute("copy", &json!({"handles":[view.hex(),dimension.hex()],"delta":[100,0]})).unwrap();
    let handles: Vec<_> = copied["handles"].as_array().unwrap().iter().map(|v| Handle::parse_hex(v.as_str().unwrap()).unwrap()).collect();
    let new_view = handles.iter().copied().find(|h| matches!(s.doc().unwrap().entity(*h).unwrap().kind, EntityKind::Insert(_))).unwrap();
    let new_dim = handles.iter().copied().find(|h| matches!(s.doc().unwrap().entity(*h).unwrap().kind, EntityKind::Dimension(_))).unwrap();
    assert_eq!(data::load::<Dimension>(&s.doc().unwrap().entity(new_dim).unwrap().common, DIM).unwrap().unwrap().view, new_view);
    assert!((dm(&s, new_dim).p13.x - dm(&s, dimension).p13.x - 100.0).abs() < 1e-8);
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(reopened, "copy", None);
    s.execute("gost.solid", &json!({"handle":source.hex(),"box":{"min":[0,0,0],"max":[60,20,30]}})).unwrap();
    assert_eq!(shown(&s, new_dim), "60");
    assert_eq!(shown(&s, dimension), "60");
    assert!((dm(&s, new_dim).p13.x - dm(&s, dimension).p13.x - 100.0).abs() < 1e-8);
}

#[test]
fn extrusion_caps_hide_only_artificial_diagonals_in_native_dxf_geometry() {
    let mut s = crate::extensions::new_session();
    let h = created(&mut s, "gost.solid", json!({"profile":[[0,0],[10,0],[12,5],[5,10],[0,5]],"z0":0,"z1":10}));
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    let EntityKind::PolyfaceMesh(mesh) = &reopened.entity(h).unwrap().kind else { panic!() };
    let faces = mesh.faces3d();
    let expected = std::collections::BTreeSet::from([(0, 1), (1, 2), (2, 3), (3, 4), (0, 4)]);
    for z in [0.0, 10.0] {
        let edges: std::collections::BTreeSet<_> = faces
            .iter()
            .flat_map(|f| f.visible_edges())
            .filter(|(a, b)| a.z == z && b.z == z)
            .map(|(a, b)| {
                let a = mesh.vertices.iter().position(|v| *v == a).unwrap() % 5;
                let b = mesh.vertices.iter().position(|v| *v == b).unwrap() % 5;
                (a.min(b), a.max(b))
            })
            .collect();
        assert_eq!(edges, expected);
    }
}

#[test]
fn existing_view_refuses_creation_only_layout_change_without_touching_the_document() {
    let mut s = crate::extensions::new_session();
    let source = model(&mut s);
    let view = created(&mut s, "gost.projection", json!({"sources":[source.hex()],"layout":"GOST1","direction":"top"}));
    s.execute("layout.new", &json!({"name":"Other"})).unwrap();
    let before = s.doc().unwrap().clone();
    let undo = s.state().unwrap().undo.len();
    assert!(s.execute("gost.projection", &json!({"handle":view.hex(),"layout":"Other"})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    assert_eq!(s.state().unwrap().undo.len(), undo);
}

#[test]
fn projection_rejects_ruled_out_and_mixed_alphabet_labels_atomically() {
    let mut s = crate::extensions::new_session();
    let source = model(&mut s);
    for label in ["Ё", "З", "Й", "О", "Х", "Ч", "Ъ", "Ы", "Ь", "A", "АA"] {
        let before = s.doc().unwrap().clone();
        let undo = s.state().unwrap().undo.len();
        assert!(view(&mut s, &json!({"sources":[source.hex()],"label":label})).is_err(), "{label}");
        assert_eq!(s.doc().unwrap(), &before);
        assert_eq!(s.state().unwrap().undo.len(), undo);
    }
    s.execute("gost.profile", &json!({"action":"set","id":"rf-2026-v1","alphabet":"latin"})).unwrap();
    for label in ["I", "O", "V", "X", "А"] {
        let before = s.doc().unwrap().clone();
        assert!(view(&mut s, &json!({"sources":[source.hex()],"label":label})).is_err(), "{label}");
        assert_eq!(s.doc().unwrap(), &before);
    }
    let h = created(&mut s, "gost.projection", json!({"sources":[source.hex()],"label":"W"}));
    let before = s.doc().unwrap().clone();
    assert!(view(&mut s, &json!({"handle":h.hex(),"label":"V"})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn paper_projection_caption_and_dimension_keep_paper_heights_across_model_and_view_scales() {
    let mut s = crate::extensions::new_session();
    s.execute("gost.setup", &json!({"scale":8})).unwrap();
    let source = model(&mut s);
    s.execute("gost.sheet.set", &json!({"layout":"GOST1","fields":{"SCALE":"1:5"}})).unwrap();
    for (scale, scale_text) in [(0.25, "4:1"), (0.4, "2,5:1"), (1.0, "1:1"), (2.0, "1:2"), (10.0, "1:10")] {
        let h = created(&mut s, "gost.projection", json!({"sources":[source.hex()],"direction":"top","label":"Б","scale":scale}));
        let dimension = created(
            &mut s,
            "gost.projection.dimension",
            json!({"view":h.hex(),"a":{"source":source.hex(),"vertex":1},"b":{"source":source.hex(),"vertex":2}}),
        );
        let d = s.doc().unwrap();
        let EntityKind::Insert(insert) = &d.entity(h).unwrap().kind else { panic!() };
        let block = d.block(&insert.block).unwrap();
        let caption = block.entities.iter().find_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t) } else { None }).unwrap();
        let top = block
            .entities
            .iter()
            .filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some(l.a.y.max(l.b.y)) } else { None })
            .fold(f64::NEG_INFINITY, f64::max);
        let numbers = cadcraft_render::dimension_in(d, dm(&s, dimension));
        assert!((numbers.measurement - 40.0 / scale).abs() < 1e-9, "definition points measure the actual paper projection");
        assert_eq!(numbers.value, "40", "displayed measurement must retain the true model length through the projection factor");
        assert_eq!(numbers.text_height, 3.5, "paper dimensions must not inherit model DIMSCALE=8");
        assert_eq!(caption.height, numbers.text_height * 2.0);
        assert_eq!(caption.value, format!("Б ({scale_text})"), "caption must state the actual image scale, not model annotation DIMSCALE");
        assert!(caption.insert.y >= top + 8.0 - 1e-9, "caption clearance must account for both ends of every projected edge");
    }
}
