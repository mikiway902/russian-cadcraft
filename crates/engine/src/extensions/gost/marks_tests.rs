use super::*;
use cadcraft_doc::{RawTag, Space};

fn session() -> Session {
    let mut s = Session::empty();
    s.new_drawing(false);
    super::super::profile::install(&mut s, &json!({})).unwrap();
    s
}
fn handle(r: &Value) -> Handle {
    Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap()
}
fn block(s: &Session, h: Handle) -> Block {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else {
        panic!("expected Insert");
    };
    s.doc().unwrap().block(&i.block).unwrap().as_ref().clone()
}
fn labels(b: &Block) -> Vec<String> {
    b.entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Text(t) => Some(t.value.clone()),
            _ => None,
        })
        .collect()
}
fn kinds(b: &Block) -> Vec<EntityKind> {
    b.entities.iter().map(|e| e.kind.clone()).collect()
}
fn input_cases() -> Vec<(&'static str, Value)> {
    vec![
        (
            "gost.roughness",
            json!({"at":[12,18],"parameter":"Rz","value":12.5,"removal":"required","lay":"crossed","direction":30,"allSurfaces":true}),
        ),
        (
            "gost.weld",
            json!({"at":[42,18],"type":"fillet","size":6,"length":40,"pitch":80,"continuous":false,"side":"other","field":true,"allAround":true,"leader":[-15,-12]}),
        ),
        ("gost.thread", json!({"at":[75,18],"nominal":20,"pitch":2.5,"length":40,"side":"internal","view":"both","rotation":30})),
    ]
}

#[test]
fn commands_publish_menus_factories_and_real_native_geometry() {
    let specs = specs();
    assert_eq!(specs.len(), 6);
    assert!(specs.iter().all(|s| !s.menu.is_empty() && s.interactive.is_some() && !s.params.is_empty()));
    assert!(!specs.iter().find(|s| s.id == INSPECT).unwrap().undoable);
    let mut s = session();
    for (id, p) in input_cases() {
        let r = s.execute(id, &p).unwrap();
        let b = block(&s, handle(&r));
        assert!(b.entities.len() >= 5, "{id}");
        assert!(!labels(&b).is_empty(), "{id}");
        assert!(b.entities.iter().all(|e| matches!(
            e.kind,
            EntityKind::Line(_) | EntityKind::Text(_) | EntityKind::Circle(_) | EntityKind::Arc(_) | EntityKind::Solid(_)
        )));
        assert!(b.entities.iter().all(|e| e.common.layer.starts_with("GOST-")));
        assert!(
            b.entities
                .iter()
                .filter_map(|e| match &e.kind {
                    EntityKind::Text(t) => Some(t),
                    _ => None,
                })
                .all(|t| t.style == "GOST")
        );
        assert_eq!(r["representation"], "native-2d-symbol");
    }
}

#[test]
fn roughness_edits_change_labels_and_optional_native_geometry() {
    let mut s = session();
    let r = roughness(&mut s, &json!({"at":[10,20],"value":1.6})).unwrap();
    let h = handle(&r);
    let before = block(&s, h);
    assert_eq!(labels(&before), vec!["Ra 1,6"]);
    let r = edit(
        &mut s,
        &json!({"handle":h.hex(),"patch":{"parameter":"Rz","value":25,"removal":"prohibited","lay":"radial","allSurfaces":true,"direction":45}}),
    )
    .unwrap();
    let after = block(&s, h);
    assert_eq!(labels(&after), vec!["Rz 25"]);
    assert_ne!(kinds(&before), kinds(&after));
    assert!(after.entities.iter().filter(|e| matches!(e.kind, EntityKind::Circle(_))).count() >= 2);
    assert!(after.entities.len() > before.entities.len());
    assert_eq!(handle(&r), h);
    edit(&mut s, &json!({"handle":h.hex(),"patch":{"lay":null,"direction":null,"allSurfaces":false,"removal":"permitted"}})).unwrap();
    assert_eq!(inspect(&mut s, &json!({"handle":h.hex()})).unwrap()["parameters"]["lay"], Value::Null);
}

#[test]
fn every_roughness_removal_and_lay_has_a_distinct_visible_construction() {
    let mut s = session();
    let mut removal_geometry = Vec::new();
    for removal in ["permitted", "required", "prohibited"] {
        let r = roughness(&mut s, &json!({"at":[0,0],"value":3.2,"removal":removal})).unwrap();
        removal_geometry.push(kinds(&block(&s, handle(&r))));
    }
    assert!(removal_geometry.windows(2).all(|w| w[0] != w[1]));
    let mut lay_geometry = Vec::new();
    for lay in ["parallel", "perpendicular", "crossed", "multidirectional", "circular", "radial"] {
        let r = roughness(&mut s, &json!({"at":[0,0],"value":3.2,"lay":lay})).unwrap();
        let g = kinds(&block(&s, handle(&r)));
        assert!(!lay_geometry.contains(&g), "{lay}");
        lay_geometry.push(g);
    }
}

#[test]
fn weld_edits_rebuild_side_type_flags_leader_and_segment_label() {
    let mut s = session();
    let r = weld(&mut s, &json!({"at":[0,0],"type":"fillet","size":4})).unwrap();
    let h = handle(&r);
    let before = block(&s, h);
    let r = edit(&mut s, &json!({"handle":h.hex(),"patch":{"type":"butt","size":null,"length":30,"pitch":60,"continuous":false,"side":"other","field":true,"allAround":true,"leader":[-25,-14]}})).unwrap();
    let after = block(&s, h);
    assert!(labels(&before).join("").contains("ГОСТ 5264-80-Т1-"));
    let designation = labels(&after).join("");
    assert!(designation.contains("ГОСТ 5264-80-С1-") && designation.contains("30") && designation.ends_with("60"));
    assert_ne!(kinds(&before), kinds(&after));
    assert!(after.entities.iter().any(|e| matches!(e.kind, EntityKind::Circle(_))));
    assert_eq!(r["parameters"]["continuous"], false);
    edit(&mut s, &json!({"handle":h.hex(),"patch":{"continuous":true,"pitch":null}})).unwrap();
    let designation = labels(&block(&s, h)).join("");
    assert!(designation.contains("ГОСТ 5264-80-С1-") && designation.ends_with("30"));
}

#[test]
fn thread_edits_change_actual_dimensions_and_not_a_fake_3d_model() {
    let mut s = session();
    let r = thread(&mut s, &json!({"at":[0,0],"nominal":10,"pitch":1.5,"length":20})).unwrap();
    let h = handle(&r);
    let before = block(&s, h);
    assert_eq!(labels(&before), vec!["M10×1,5"]);
    let r = edit(&mut s, &json!({"handle":h.hex(),"patch":{"nominal":20,"pitch":2.5,"length":45,"side":"internal","view":"both"}})).unwrap();
    let after = block(&s, h);
    assert_eq!(labels(&after), vec!["M20×2,5"]);
    assert_ne!(kinds(&before), kinds(&after));
    assert!(after.entities.iter().any(|e| matches!(&e.kind, EntityKind::Line(l) if l.a.x == 45.0 || l.b.x == 45.0)));
    assert!(after.entities.iter().any(|e| matches!(e.kind, EntityKind::Arc(_))));
    assert!(r["limitations"].as_array().unwrap().iter().any(|v| v.as_str().unwrap().contains("never a modeled 3D helix")));
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else {
        panic!();
    };
    assert_eq!(i.scale, cadcraft_geom::Vec3::new(1.0, 1.0, 1.0));
}

#[test]
fn end_thread_uses_three_quarter_thin_circle_and_visible_length() {
    let mut s = session();
    for side in ["external", "internal"] {
        let r = thread(&mut s, &json!({"at":[0,0],"nominal":12,"pitch":1.75,"length":24,"side":side,"view":"end"})).unwrap();
        let b = block(&s, handle(&r));
        let a = b.entities.iter().find_map(|e| if let EntityKind::Arc(a) = &e.kind { Some(a) } else { None }).unwrap();
        assert!((a.end - a.start - 1.5 * std::f64::consts::PI).abs() < 1e-12);
        assert_eq!(labels(&b), vec!["M12×1,75", "l=24"]);
    }
}

#[test]
fn annotation_scaling_does_not_scale_physical_thread_diameter_or_length() {
    let mut s = session();
    super::super::profile::install(&mut s, &json!({"scale":5})).unwrap();
    let before = s.doc().unwrap().clone();
    let error = thread(&mut s, &json!({"at":[0,0],"nominal":10,"pitch":1.5,"length":20})).unwrap_err().to_string();
    assert!(error.contains("Enlarge the view"));
    assert_eq!(s.doc().unwrap(), &before);
    super::super::profile::install(&mut s, &json!({"scale":0.5})).unwrap();
    let r = thread(&mut s, &json!({"at":[0,0],"nominal":10,"pitch":1.5,"length":20})).unwrap();
    let b = block(&s, handle(&r));
    assert!(b.entities.iter().any(|e| matches!(&e.kind, EntityKind::Line(l) if l.a.y == 5.0 && l.b.y == 5.0 && l.b.x == 20.0)));
    assert!(b.entities.iter().any(|e| matches!(&e.kind, EntityKind::Text(t) if t.height == 1.75)));
    let model = roughness(&mut s, &json!({"at":[0,0],"value":1.6})).unwrap();
    assert_eq!(model["parameters"]["scale"], 0.5);
    let paper_name = s.doc().unwrap().layouts.first().unwrap().name.clone();
    s.state_mut().unwrap().space = Space::Paper(paper_name);
    let paper = roughness(&mut s, &json!({"at":[0,0],"value":1.6})).unwrap();
    assert_eq!(paper["parameters"]["scale"], 1.0);
    assert!(block(&s, handle(&paper)).entities.iter().any(|e| matches!(&e.kind, EntityKind::Text(t) if t.height == 3.5)));
}

#[test]
fn invalid_creations_are_atomic_even_without_session_transaction() {
    let mut s = session();
    for p in [
        json!({"at":[0,0],"value":0}),
        json!({"at":[0,0],"value":-1}),
        json!({"at":[0,0],"value":1,"parameter":"Ry"}),
        json!({"at":[0,0],"value":1,"removal":"unknown"}),
        json!({"at":[0,0],"value":1,"allSurfaces":"true"}),
        json!({"at":[0,0],"value":1,"scale":0}),
        json!({"at":[0,0],"value":1,"textHeight":4}),
        json!({"at":[1e100,0],"value":1}),
        json!({"at":[0,0],"value":1,"rotation":"bad"}),
        json!({"at":[0,0],"value":1,"unknown":1}),
    ] {
        let before = s.doc().unwrap().clone();
        assert!(roughness(&mut s, &p).is_err(), "{p}");
        assert_eq!(s.doc().unwrap(), &before, "{p}");
    }
    for p in [
        json!({"at":[0,0],"type":"fillet","size":0}),
        json!({"at":[0,0],"type":"fillet","size":6,"continuous":false}),
        json!({"at":[0,0],"type":"fillet","size":6,"pitch":30}),
        json!({"at":[0,0],"type":"fillet","size":6,"continuous":false,"length":40,"pitch":40}),
        json!({"at":[0,0],"type":"butt","size":6,"leader":[0,0]}),
    ] {
        let before = s.doc().unwrap().clone();
        assert!(weld(&mut s, &p).is_err(), "{p}");
        assert_eq!(s.doc().unwrap(), &before, "{p}");
    }
    for p in [
        json!({"at":[0,0],"nominal":1,"pitch":1,"length":10}),
        json!({"at":[0,0],"nominal":10,"pitch":0,"length":10}),
        json!({"at":[0,0],"nominal":10,"pitch":1.5,"length":1}),
        json!({"at":[0,0],"nominal":10,"pitch":1.5,"length":10,"side":"unknown"}),
    ] {
        let before = s.doc().unwrap().clone();
        assert!(thread(&mut s, &p).is_err(), "{p}");
        assert_eq!(s.doc().unwrap(), &before, "{p}");
    }
}

#[test]
fn invalid_edits_preserve_payload_block_and_handle_allocator() {
    let mut s = session();
    let r = weld(&mut s, &json!({"at":[3,4],"type":"fillet","size":6,"continuous":false,"length":40,"pitch":80})).unwrap();
    let h = handle(&r);
    for patch in [
        json!({"pitch":30}),
        json!({"size":-2}),
        json!({"continuous":true}),
        json!({"kind":"thread"}),
        json!({"at":[1,2]}),
        json!({"rotation":30}),
        json!({"unknown":1}),
        json!({"size":null}),
        json!({}),
    ] {
        let before = s.doc().unwrap().clone();
        assert!(edit(&mut s, &json!({"handle":h.hex(),"patch":patch})).is_err(), "{patch}");
        assert_eq!(s.doc().unwrap(), &before, "{patch}");
    }
}

#[test]
fn stable_insert_transform_arrays_and_foreign_xdata_survive_edit() {
    let mut s = session();
    let r = roughness(&mut s, &json!({"at":[3,4],"value":1.6,"rotation":27})).unwrap();
    let h = handle(&r);
    let foreign = vec![RawTag { code: 1001, value: "SHOP".into() }, RawTag { code: 1000, value: "Original foreign data".into() }];
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            e.common.xdata.extend(foreign.clone());
            let EntityKind::Insert(i) = &mut e.kind else {
                panic!();
            };
            i.insert.z = 12.0;
            i.scale = cadcraft_geom::Vec3::new(-2.0, 3.0, 1.0);
            i.rows = 2;
            i.cols = 3;
            i.row_spacing = 15.0;
            i.col_spacing = 20.0;
        })
        .unwrap();
    let old = s.doc().unwrap().entity(h).unwrap().clone();
    let EntityKind::Insert(old_insert) = &old.kind else {
        panic!();
    };
    edit(&mut s, &json!({"handle":h.hex(),"patch":{"value":6.3}})).unwrap();
    let new = s.doc().unwrap().entity(h).unwrap();
    let EntityKind::Insert(new_insert) = &new.kind else {
        panic!();
    };
    assert_eq!(new_insert, old_insert);
    assert_eq!(new.handle, old.handle);
    for tag in foreign {
        assert!(new.common.xdata.contains(&tag));
    }
    assert_eq!(new.common.xdata.iter().filter(|t| t.code == 1001 && t.value == "CADCRAFTGOST").count(), 1);
}

#[test]
fn copied_symbol_edits_use_copy_on_write_not_shared_geometry_mutation() {
    let mut s = session();
    let r = roughness(&mut s, &json!({"at":[3,4],"value":1.6})).unwrap();
    let h = handle(&r);
    let e = s.doc().unwrap().entity(h).unwrap().clone();
    let copy = s.doc_mut().unwrap().add(&Space::Model, e.common.clone(), e.kind.clone()).unwrap();
    let before = block(&s, copy);
    let r = edit(&mut s, &json!({"handle":h.hex(),"patch":{"value":12.5}})).unwrap();
    assert_ne!(r["block"], before.name);
    assert_eq!(block(&s, copy), before);
    assert_eq!(labels(&block(&s, h)), vec!["Ra 12,5"]);
    let copy_parameters = inspect(&mut s, &json!({"handle":copy.hex()})).unwrap();
    assert_eq!(copy_parameters["parameters"]["value"], 1.6);
}

#[test]
fn typed_parameters_and_visible_native_geometry_survive_dxf_then_edit() {
    let mut s = session();
    let mut created = Vec::new();
    for (id, p) in input_cases() {
        created.push(s.execute(id, &p).unwrap());
    }
    let bytes = cadcraft_io::write_dxf(s.doc().unwrap());
    let reopened = cadcraft_io::read_dxf(bytes.as_bytes()).unwrap();
    let mut s2 = Session::empty();
    s2.open_drawing(reopened, "engineering marks", None);
    for r in &created {
        let h = handle(r);
        let before = s2.doc().unwrap().clone();
        let info = inspect(&mut s2, &json!({"handle":h.hex()})).unwrap();
        assert_eq!(s2.doc().unwrap(), &before, "inspection must be read-only");
        assert_eq!(info["parameters"], r["parameters"]);
        assert_eq!(info["labels"], r["labels"]);
        assert_eq!(info["kind"], r["kind"]);
        let e = s2.doc().unwrap().entity(h).unwrap();
        assert!(super::super::data::load::<Symbol>(&e.common, META).unwrap().is_some());
        assert!(block(&s2, h).entities.len() >= 5);
    }
    let h = handle(&created[0]);
    edit(&mut s2, &json!({"handle":h.hex(),"patch":{"value":50}})).unwrap();
    assert_eq!(labels(&block(&s2, h)), vec!["Rz 50"]);
}

#[test]
fn symbol_commands_support_undo_redo_as_one_native_transaction() {
    let mut s = session();
    let before = s.doc().unwrap().clone();
    let r = s.execute("gost.thread", &json!({"at":[0,0],"nominal":10,"pitch":1.5,"length":20})).unwrap();
    let h = handle(&r);
    s.execute(EDIT, &json!({"handle":h.hex(),"patch":{"length":40}})).unwrap();
    let after = cadcraft_io::write_dxf(s.doc().unwrap());
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(inspect(&mut s, &json!({"handle":h.hex()})).unwrap()["parameters"]["length"], 20.0);
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(cadcraft_io::write_dxf(s.doc().unwrap()), after);
    s.execute("undo", &json!({})).unwrap();
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn gost_tolerance_is_the_existing_native_engine_and_same_frame_geometry() {
    let mut s = session();
    let p = json!({"at":[10,20],"symbol":"position","tol1":{"value":0.1,"diameter":true,"material":"M"},"datums":["A","B(M)"],"rotation":25});
    let r = tolerance(&mut s, &p).unwrap();
    assert_eq!(r["delegatedTo"], "tolerance");
    let h = handle(&r);
    let e = s.doc().unwrap().entity(h).unwrap();
    let EntityKind::Tolerance(t) = &e.kind else {
        panic!("wrapper must not duplicate the tolerance primitive engine");
    };
    assert_eq!(t.style, "GOST");
    assert_eq!(e.common.layer, "GOST-DIM");
    let t = t.clone();
    let mut native = p.clone();
    native["style"] = json!("GOST");
    native["tol1"]["value"] = json!("0,1");
    let raw = (find_command("tolerance").unwrap().run)(&mut s, &native).unwrap();
    let EntityKind::Tolerance(raw_t) = &s.doc().unwrap().entity(handle(&raw)).unwrap().kind else {
        panic!();
    };
    assert_eq!(raw_t, &t);
    assert!(!cadcraft_render::tolerance_in(s.doc().unwrap(), &t).frame.is_empty());
}

#[test]
fn tolerance_rejects_invalid_or_bypassing_inputs_before_any_mutation() {
    let mut s = session();
    for p in [
        json!({"at":[0,0],"symbol":"flatness","tol1":0}),
        json!({"at":[0,0],"symbol":"flatness","tol1":-0.1}),
        json!({"at":[0,0],"symbol":"flatness","tol1":"not a number"}),
        json!({"at":[0,0],"symbol":"roundness","tol1":0.1,"datums":["A"]}),
        json!({"at":[0,0],"symbol":"flatness","tol1":{"value":0.1,"diameter":true}}),
        json!({"at":[0,0],"symbol":"position","tol1":"0.1(L)"}),
        json!({"at":[0,0],"symbol":"position","tol1":0.1,"datums":["A","A"]}),
        json!({"at":[0,0],"symbol":"position","tol1":0.1,"datums":["A","B","C","D"]}),
        json!({"at":[0,0],"symbol":"position","tol1":0.1,"projected":true}),
        json!({"at":[0,0],"symbol":"position","tol1":0.1,"height":5}),
        json!({"at":[0,0],"symbol":"position","tol1":0.1,"text":"raw frame"}),
        json!({"at":[0,0],"symbol":"position","tol1":0.1,"style":"Standard"}),
        json!({"at":[0,0],"symbol":"position","tol1":{"value":{"value":0.1}}}),
    ] {
        let before = s.doc().unwrap().clone();
        assert!(tolerance(&mut s, &p).is_err(), "{p}");
        assert_eq!(s.doc().unwrap(), &before, "{p}");
    }
}

#[test]
fn tolerance_paper_defaults_do_not_multiply_text_by_model_scale() {
    let mut s = session();
    super::super::profile::install(&mut s, &json!({"scale":5})).unwrap();
    let name = s.doc().unwrap().layouts.first().unwrap().name.clone();
    s.state_mut().unwrap().space = Space::Paper(name);
    let p = json!({"at":[10,20],"symbol":"position","tol1":0.1,"projected":true,"height":10,"datumId":"A","line2":{"symbol":"parallelism","tol1":0.05,"datums":["B"]}});
    let r = tolerance(&mut s, &p).unwrap();
    let EntityKind::Tolerance(t) = &s.doc().unwrap().entity(handle(&r)).unwrap().kind else {
        panic!();
    };
    assert_eq!(t.text_size(s.doc().unwrap()).0, 3.5);
    assert_eq!(s.doc().unwrap().dim_style("GOST").unwrap().scale, 5.0);
    let before = s.doc().unwrap().dim_styles.len();
    tolerance(&mut s, &p).unwrap();
    assert_eq!(s.doc().unwrap().dim_styles.len(), before);
}

#[test]
fn oversized_hostile_structure_is_rejected_before_drawing_changes() {
    let mut s = session();
    let p = json!({"at":[0,0],"value":1.6,"lay":"x".repeat(10000)});
    let before = s.doc().unwrap().clone();
    assert!(roughness(&mut s, &p).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    let nested = json!({"at":[0,0],"symbol":"position","tol1":{"value":{"value":{"value":{"value":1}}}}});
    assert!(tolerance(&mut s, &nested).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn cross_document_paste_keeps_symbol_geometry_even_for_legacy_named_blocks() {
    for legacy in [false, true] {
        let mut s = session();
        let first = handle(&s.execute("gost.roughness", &json!({"at":[0,0],"value":1.6})).unwrap());
        let expected = kinds(&block(&s, first));
        if legacy {
            let name = block(&s, first).name;
            std::sync::Arc::make_mut(s.doc_mut().unwrap().blocks.get_mut(&name).unwrap()).anonymous = false;
        }
        let e = (**s.doc().unwrap().entity(first).unwrap()).clone();
        crate::cmd::clipboard::copy(&mut s, vec![e], Vec2::ZERO).unwrap();
        s.new_drawing(false);
        super::super::profile::install(&mut s, &json!({})).unwrap();
        let other = handle(&s.execute("gost.roughness", &json!({"at":[0,0],"value":6.3})).unwrap());
        let other_geometry = kinds(&block(&s, other));
        let copied = crate::cmd::clipboard::paste(&mut s, &cadcraft_geom::Mat3::translate(Vec2::new(50.0, 0.0))).unwrap()[0];
        assert_eq!(kinds(&block(&s, copied)), expected);
        assert_eq!(kinds(&block(&s, other)), other_geometry);
        assert_ne!(block(&s, copied).name, block(&s, other).name);
        s.execute(EDIT, &json!({"handle":copied.hex(),"patch":{"value":3.2}})).unwrap();
        assert_eq!(kinds(&block(&s, other)), other_geometry);
    }
}

#[test]
fn thread_spacing_rejects_infeasible_creation_and_edit_without_clamping_physical_values() {
    let mut s = session();
    for side in ["external", "internal"] {
        for view in ["longitudinal", "end"] {
            let before = s.doc().unwrap().clone();
            let undo = s.state().unwrap().undo.len();
            let p = json!({"at":[0,0],"nominal":10,"pitch":1,"length":20,"side":side,"view":view});
            let error = thread(&mut s, &p).unwrap_err().to_string();
            assert!(error.contains("paper mm") && error.contains("scale <="));
            assert_eq!(s.doc().unwrap(), &before, "preflight must not allocate handles or install a block");
            assert_eq!(s.state().unwrap().undo.len(), undo);
            let enlarged = thread(&mut s, &json!({"at":[0,0],"nominal":10,"pitch":1,"length":20,"side":side,"view":view,"scale":0.5})).unwrap();
            let h = handle(&enlarged);
            let b = block(&s, h);
            let (thick, thin) = if view == "end" {
                let thick = b.entities.iter().find_map(|e| if let EntityKind::Circle(c) = &e.kind { Some(c.radius) } else { None }).unwrap();
                let thin = b.entities.iter().find_map(|e| if let EntityKind::Arc(a) = &e.kind { Some(a.radius) } else { None }).unwrap();
                (thick, thin)
            } else {
                let radius = |layer: &str| {
                    b.entities
                        .iter()
                        .find_map(|e| match &e.kind {
                            EntityKind::Line(l) if e.common.layer == layer && l.a.x == 0.0 && l.b.x == 20.0 => Some(l.a.y.abs()),
                            _ => None,
                        })
                        .unwrap()
                };
                (radius("GOST-OUTLINE"), radius("GOST-THIN"))
            };
            let physical_minor = (10.0 - if side == "external" { 1.226869 } else { 1.082532 }) * 0.5;
            let (major, minor) = if side == "external" { (thick, thin) } else { (thin, thick) };
            assert!((major - 5.0).abs() < 1e-12);
            assert!((minor - physical_minor).abs() < 1e-12, "root geometry must not be clamped to satisfy paper spacing");
            let paper_gap = (major - minor) / 0.5;
            assert!((0.8..=2.0).contains(&paper_gap));
            let before = s.doc().unwrap().clone();
            assert!(edit(&mut s, &json!({"handle":h.hex(),"patch":{"scale":2}})).is_err());
            assert_eq!(s.doc().unwrap(), &before, "infeasible edit must preserve payload, cached block and allocator");
            assert_eq!(inspect(&mut s, &json!({"handle":h.hex()})).unwrap()["parameters"]["nominal"], 10.0);
        }
    }
}

#[test]
fn enlarged_thread_retains_physical_metadata_and_spacing_after_dxf_and_undo_redo() {
    let mut s = session();
    let r = s.execute("gost.thread", &json!({"at":[0,0],"nominal":10,"pitch":1,"length":20,"scale":0.5,"view":"both"})).unwrap();
    let h = handle(&r);
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(reopened, "enlarged thread", None);
    assert_eq!(inspect(&mut s, &json!({"handle":h.hex()})).unwrap()["parameters"], r["parameters"]);
    let original = kinds(&block(&s, h));
    s.execute(EDIT, &json!({"handle":h.hex(),"patch":{"length":30}})).unwrap();
    let changed = kinds(&block(&s, h));
    assert_ne!(original, changed);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(kinds(&block(&s, h)), original);
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(kinds(&block(&s, h)), changed);
    assert_eq!(inspect(&mut s, &json!({"handle":h.hex()})).unwrap()["parameters"]["pitch"], 1.0);
}

#[test]
fn paper_thread_cannot_fake_spacing_by_changing_only_annotation_scale() {
    let mut s = session();
    let layout = s.doc().unwrap().layouts.first().unwrap().name.clone();
    s.state_mut().unwrap().space = Space::Paper(layout);
    for scale in [0.5, 1.0] {
        let before = s.doc().unwrap().clone();
        assert!(thread(&mut s, &json!({"at":[0,0],"nominal":10,"pitch":1,"length":20,"scale":scale})).is_err());
        assert_eq!(s.doc().unwrap(), &before);
    }
    thread(&mut s, &json!({"at":[0,0],"nominal":10,"pitch":1.5,"length":20})).unwrap();
}

#[test]
fn legacy_unreadable_thread_inspection_warns_without_silent_repair() {
    let mut s = session();
    let r = thread(&mut s, &json!({"at":[0,0],"nominal":10,"pitch":1.5,"length":20})).unwrap();
    let h = handle(&r);
    let mut common = s.doc().unwrap().entity(h).unwrap().common.clone();
    let mut symbol: Symbol = super::super::data::load(&common, META).unwrap().unwrap();
    let Symbol::Thread(q) = &mut symbol else { panic!() };
    q.scale = 5.0;
    super::super::data::save(&mut common, META, &symbol).unwrap();
    s.doc_mut().unwrap().modify_entity(h, |e| e.common = common).unwrap();
    let before = s.doc().unwrap().clone();
    let report = inspect(&mut s, &json!({"handle":h.hex()})).unwrap();
    assert!(report["reviewWarnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("Enlarge the view")));
    assert_eq!(s.doc().unwrap(), &before);
}
