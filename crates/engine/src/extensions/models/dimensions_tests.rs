use super::super::tag;
use super::*;
use cadcraft_doc::{
    Line,
    model_projection::{Accuracy, Basis, LineProvenance, PieceProvenance, ProjectedEntity, Projection, Quality, SourceReference, Visibility},
};

fn metadata(sha: char) -> Value {
    json!({"protocol":"cadcraft-model-worker/1","path":"/models/dimension.step","source":{"sha256":sha.to_string().repeat(64),"bytes":512},
        "format":"step","units":{"length":"mm","area":"mm2","volume":"mm3","angle":"radian","coordinate_system":"right_handed_z_up"},
        "metadata":{},"bodies":[{"name":"Body","source_item":0}],"tree":{},"warnings":[],"report":{}})
}
fn projection() -> Projection {
    let piece = PieceProvenance { t: [0., 1.], visibility: Visibility::Visible, covered: false, accuracy: Accuracy::Analytic, segment_count: 1 };
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
        lines: vec![LineProvenance { placement: 0, source: SourceReference::Edge { edge: 0, tangent: false }, pieces: vec![piece] }],
        unsupported: vec![],
        prepare_reports: json!([]),
        drawing_stats: json!({}),
        source_mapping: "payload_scoped".into(),
        visibility_contract: "exact_hlr".into(),
    }
}
fn setup() -> (Session, String, Handle) {
    let mut s = crate::extensions::new_session();
    let id = super::super::apply_loaded(&mut s, metadata('a'), None).unwrap()["id"].as_str().unwrap().to_owned();
    let view =
        super::super::apply_view(&mut s, &id, &"a".repeat(64), &json!({"id":id,"layout":"GOST1","scale":2,"at":[20,50]}), projection()).unwrap();
    (s, id, Handle::parse_hex(view["handle"].as_str().unwrap()).unwrap())
}
fn audit(fraction: f64, point: [f64; 3]) -> Value {
    json!({"protocol":"cadcraft-references/1","source":{"sha256":"a".repeat(64),"bytes":512},"baseline_source_sha256":"a".repeat(64),
        "canonical_path":"/models/dimension.step","fraction":fraction,"fraction_convention":"trimmed_edge_arc_length_start_to_end",
        "status":"resolved","point":point,"placement":0,"edge":0,"verification":"payload_scoped","cross_rebuild":false,
        "identity_scope":"payload_sha256_and_canonical_path","native_identity":null,
        "evaluation":EVALUATION,"units":{"length":"mm","area":"mm2","volume":"mm3","angle":"radian",
        "coordinate_system":"right_handed_z_up","original_declaration":"not_exposed_by_selected_import_api"}})
}
fn params(view: Handle, kind: &str) -> Value {
    json!({"view":view.hex(),"a":{"reference":"opaque-exact-edge-a","fraction":0.},"b":{"reference":"opaque-exact-edge-b","fraction":1.},"kind":kind,"offset":-5.})
}
fn add(s: &mut Session, view: Handle) -> Handle {
    let out = apply_dimension(s, &params(view, "aligned"), [audit(0., [0., 0., 0.]), audit(1., [6., 8., 0.])]).unwrap();
    assert!((out["measurement"].as_f64().unwrap() - 10.).abs() < 1e-10);
    Handle::parse_hex(out["handle"].as_str().unwrap()).unwrap()
}
fn dm(s: &Session, h: Handle) -> &cadcraft_doc::Dimension {
    let EntityKind::Dimension(dm) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("expected native dimension") };
    dm
}
fn link(s: &Session, h: Handle) -> Link {
    data::load(&s.doc().unwrap().entity(h).unwrap().common, DIM).unwrap().unwrap()
}
fn report(s: &mut Session, h: Handle) -> Value {
    s.execute("gost.model.dimension.inspect", &json!({"handle":h.hex()})).unwrap()
}

#[test]
fn foreshortened_aligned_is_true_world_distance_and_native_dxf_roundtrips() {
    let (mut s, _, view) = setup();
    let h = add(&mut s, view);
    let d = dm(&s, h);
    assert_eq!(d.kind, DimKind::Aligned);
    assert_eq!(d.p13, Vec3::new(20., 50., 0.));
    assert_eq!(d.p14, Vec3::new(23., 50., 0.));
    assert_eq!(d.defpt, Vec3::new(21.5, 45., 0.));
    assert!((d.overrides["linearFactor"].as_f64().unwrap() - 10. / 3.).abs() < 1e-12);
    let style = s.doc().unwrap().dim_style(&d.style).unwrap().with_overrides(&d.overrides);
    let geometry = cadcraft_render::dimension_geometry(d, &style, 1.);
    assert!(geometry.mtext.contains("10"), "{}", geometry.mtext);
    let expected = link(&s, h);
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(reopened, "exact dimension", None);
    assert_eq!(link(&s, h), expected);
    assert!((report(&mut s, h)["measurement"].as_f64().unwrap() - 10.).abs() < 1e-10);
    let checked = s.execute("gost.check", &json!({})).unwrap();
    assert!(
        !checked["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| { finding["location"] == format!("dimension:{}", h.hex()) && finding["code"] == "dimension.preset.linear_factor" }),
        "norm control must not flag the exact world-to-paper measurement factor: {checked}"
    );
}

#[test]
fn projection_axes_use_actual_insert_scale_rotation_and_signed_offset() {
    let (mut s, _, view) = setup();
    let out = apply_dimension(&mut s, &params(view, "vertical"), [audit(0., [0., 0., 0.]), audit(1., [6., 8., 10.])]).unwrap();
    assert!((out["measurement"].as_f64().unwrap() - 10.).abs() < 1e-10);
    let h = Handle::parse_hex(out["handle"].as_str().unwrap()).unwrap();
    assert_eq!(dm(&s, h).defpt.x, 15.);
    s.execute("gost.model.dimension", &json!({"handle":h.hex(),"kind":"horizontal","offset":5})).unwrap();
    assert!((report(&mut s, h)["measurement"].as_f64().unwrap() - 6.).abs() < 1e-10);
    let p = json!({"handle":h.hex()});
    s.execute_prepared("gost.model.dimension", &p, |s| {
        s.doc_mut()?.modify_entity(view, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.rotation = std::f64::consts::FRAC_PI_2;
                i.scale = Vec3::new(0.25, 0.25, 0.25);
            }
        })?;
        Ok(json!({}))
    })
    .unwrap();
    assert!((report(&mut s, h)["measurement"].as_f64().unwrap() - 10.).abs() < 1e-10);
    assert_eq!(dm(&s, h).overrides["linearFactor"], 4.);
}

#[test]
fn move_preserves_annotation_tolerances_custom_text_and_undo() {
    let (mut s, _, view) = setup();
    let h = add(&mut s, view);
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            e.common.xdata.extend([tag(1001, "USER-ANNOTATION"), tag(1000, "Keep me")]);
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.text = "L=<> ± note".into();
                dm.user_text_pos = true;
                dm.text_mid = Vec3::new(81., 92., 0.);
                dm.text_rotation = 0.3;
                dm.overrides.insert("tolerance".into(), json!(true));
                dm.overrides.insert("tolPlus".into(), json!(0.2));
                dm.overrides.insert("tolMinus".into(), json!(0.1));
            }
        })
        .unwrap();
    let annotation = dm(&s, h).clone();
    s.execute("move", &json!({"handles":[view.hex()],"delta":[100,20]})).unwrap();
    assert_eq!(dm(&s, h).p13, Vec3::new(120., 70., 0.));
    let d = dm(&s, h);
    assert_eq!(d.text, annotation.text);
    assert_eq!(d.text_mid, annotation.text_mid);
    assert_eq!(d.text_rotation, annotation.text_rotation);
    assert_eq!(d.style, annotation.style);
    for key in ["tolerance", "tolPlus", "tolMinus"] {
        assert_eq!(d.overrides[key], annotation.overrides[key]);
    }
    assert!(s.doc().unwrap().entity(h).unwrap().common.xdata.iter().any(|t| t.value == "Keep me"));
    s.undo().unwrap();
    assert_eq!(dm(&s, h).p13, Vec3::new(20., 50., 0.));
    s.redo().unwrap();
    assert_eq!(dm(&s, h).p13, Vec3::new(120., 70., 0.));
}

#[test]
fn stale_and_missing_views_retain_provenance_and_mark_annotation_without_io() {
    let (mut s, id, view) = setup();
    let h = add(&mut s, view);
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.text = "Length <>".into();
            }
        })
        .unwrap();
    let original = link(&s, h);
    super::super::apply_loaded(&mut s, metadata('b'), Some((&id, &"a".repeat(64)))).unwrap();
    let out = report(&mut s, h);
    assert_eq!(out["resolved"], false);
    assert!(out["measurement"].is_null());
    assert_eq!(dm(&s, h).text, "? Length <>");
    assert_eq!(link(&s, h).a, original.a);
    assert_eq!(link(&s, h).b, original.b);
    let before = s.doc().unwrap().clone();
    assert!(apply_dimension(&mut s, &params(view, "aligned"), [audit(0., [0., 0., 0.]), audit(1., [6., 8., 0.])]).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    s.undo().unwrap();
    assert_eq!(dm(&s, h).text, "Length <>");
    assert_eq!(report(&mut s, h)["resolved"], true);
    s.execute("erase", &json!({"handles":[view.hex()]})).unwrap();
    assert_eq!(dm(&s, h).text, "? Length <>");
    assert_eq!(report(&mut s, h)["resolved"], false);
    s.undo().unwrap();
    assert_eq!(dm(&s, h).text, "Length <>");
}

#[test]
fn refused_transforms_and_failed_metadata_mutation_are_atomic() {
    for scale in [Vec3::new(1., 2., 1.), Vec3::new(-1., -1., -1.), Vec3::ZERO] {
        let (mut s, _, view) = setup();
        s.doc_mut()
            .unwrap()
            .modify_entity(view, |e| {
                if let EntityKind::Insert(i) = &mut e.kind {
                    i.scale = scale;
                }
            })
            .unwrap();
        let before = s.doc().unwrap().clone();
        assert!(apply_dimension(&mut s, &params(view, "aligned"), [audit(0., [0., 0., 0.]), audit(1., [6., 8., 0.])]).is_err());
        assert_eq!(s.doc().unwrap(), &before);
    }
    let (mut s, _, view) = setup();
    let before = s.doc().unwrap().clone();
    // Both anchors are valid but cannot fit the bounded native association envelope. This
    // fails after dimlinear has inserted an entity and must roll the whole completion back.
    let mut p = params(view, "aligned");
    p["a"]["reference"] = json!("a".repeat(8192));
    p["b"]["reference"] = json!("b".repeat(8192));
    assert!(apply_dimension(&mut s, &p, [audit(0., [0., 0., 0.]), audit(1., [6., 8., 0.])]).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    let mut wrong = audit(1., [6., 8., 0.]);
    wrong["source"]["sha256"] = json!("b".repeat(64));
    assert!(apply_dimension(&mut s, &params(view, "aligned"), [audit(0., [0., 0., 0.]), wrong]).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn copy_and_cross_document_paste_remap_only_transferred_views() {
    let (mut source, _, view) = setup();
    let dim = add(&mut source, view);
    let copied = source.execute("copy", &json!({"handles":[view.hex(),dim.hex()],"delta":[100,0]})).unwrap();
    let hs: Vec<_> = copied["handles"].as_array().unwrap().iter().map(|v| Handle::parse_hex(v.as_str().unwrap()).unwrap()).collect();
    let copied_dim = *hs.iter().find(|h| matches!(source.doc().unwrap().entity(**h).unwrap().kind, EntityKind::Dimension(_))).unwrap();
    let copied_view = *hs.iter().find(|h| matches!(source.doc().unwrap().entity(**h).unwrap().kind, EntityKind::Insert(_))).unwrap();
    assert_eq!(link(&source, copied_dim).view, copied_view.hex());
    assert_eq!(dm(&source, copied_dim).p13.x, 120.);
    let (mut destination, _, collision) = setup();
    assert_eq!(collision, view);
    for together in [true, false] {
        let handles = if together { vec![view.hex(), dim.hex()] } else { vec![dim.hex()] };
        source.execute("copybase", &json!({"handles":handles,"base":[0,0]})).unwrap();
        destination.clipboard = source.clipboard.clone();
        destination.clipboard_base = source.clipboard_base;
        destination.clipboard_source = source.clipboard_source.clone();
        let pasted = destination.execute("pasteclip", &json!({"at":[200,0]})).unwrap();
        let hs: Vec<_> = pasted["handles"].as_array().unwrap().iter().map(|v| Handle::parse_hex(v.as_str().unwrap()).unwrap()).collect();
        let h = *hs.iter().find(|h| matches!(destination.doc().unwrap().entity(**h).unwrap().kind, EntityKind::Dimension(_))).unwrap();
        let r = link(&destination, h);
        assert_ne!(r.view, collision.hex());
        if together {
            assert_eq!(report(&mut destination, h)["resolved"], true);
            assert_eq!(dm(&destination, h).p13.x, 220.);
            assert!(hs.contains(&Handle::parse_hex(&r.view).unwrap()));
        } else {
            assert_eq!(report(&mut destination, h)["resolved"], false);
            assert_eq!(dm(&destination, h).text, "?");
            assert!(destination.doc().unwrap().entity(Handle::parse_hex(&r.view).unwrap()).is_none());
        }
    }
}

#[test]
fn unsupported_existing_placement_becomes_unresolved_and_restores_on_undo() {
    let (mut s, _, view) = setup();
    let h = add(&mut s, view);
    let before = dm(&s, h).clone();
    s.execute_prepared("gost.model.dimension", &json!({"handle":h.hex()}), |s| {
        s.doc_mut()?.modify_entity(view, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.scale.y *= 2.;
            }
        })?;
        Ok(json!({}))
    })
    .unwrap();
    assert_eq!(report(&mut s, h)["resolved"], false);
    assert_eq!(dm(&s, h).text, "?");
    assert_eq!(dm(&s, h).p13, before.p13);
    assert_eq!(dm(&s, h).p14, before.p14);
    assert!(dimension_factor(s.doc().unwrap(), s.doc().unwrap().entity(h).unwrap()).is_none());
    s.undo().unwrap();
    assert_eq!(report(&mut s, h)["resolved"], true);
    assert_eq!(dm(&s, h).text, "");
}

#[test]
fn prepared_completion_rejects_unresolved_and_forged_audits_without_mutation() {
    let (mut s, _, view) = setup();
    let before = s.doc().unwrap().clone();
    for (pointer, value) in [
        ("/units/length", json!("inch")),
        ("/canonical_path", json!("/unrelated.step")),
        ("/source/bytes", json!(513)),
        ("/fraction", json!(0.75)),
        ("/point", json!([1, 2])),
        ("/cross_rebuild", json!(true)),
        ("/baseline_source_sha256", json!("b".repeat(64))),
        ("/evaluation", json!("preview_triangle")),
        ("/status", json!("ambiguous")),
    ] {
        let mut b = audit(1., [6., 8., 0.]);
        *b.pointer_mut(pointer).unwrap() = value;
        assert!(apply_dimension(&mut s, &params(view, "aligned"), [audit(0., [0., 0., 0.]), b]).is_err(), "{pointer}");
        assert_eq!(s.doc().unwrap(), &before);
    }
}

#[test]
fn pasteblock_keeps_exact_provenance_but_never_claims_a_nested_paper_measurement() {
    let (mut source, _, view) = setup();
    let h = add(&mut source, view);
    let original = link(&source, h);
    source.execute("copybase", &json!({"handles":[view.hex(),h.hex()],"base":[0,0]})).unwrap();
    let mut destination = crate::extensions::new_session();
    destination.clipboard = source.clipboard.clone();
    destination.clipboard_base = source.clipboard_base;
    destination.clipboard_source = source.clipboard_source.clone();
    let out = destination.execute("pasteblock", &json!({"at":[200,0]})).unwrap();
    let wrapper = Handle::parse_hex(out["handle"].as_str().unwrap()).unwrap();
    let EntityKind::Insert(i) = &destination.doc().unwrap().entity(wrapper).unwrap().kind else { panic!() };
    let block = destination.doc().unwrap().block(&i.block).unwrap();
    let e = block.entities.iter().find(|e| matches!(e.kind, EntityKind::Dimension(_))).unwrap();
    let EntityKind::Dimension(dm) = &e.kind else { panic!() };
    let r: Link = data::load(&e.common, DIM).unwrap().unwrap();
    assert_eq!(dm.text, "?");
    assert!(r.status.contains("nested"));
    assert_eq!(r.a, original.a);
    assert_eq!(r.b, original.b);
    assert!(block.entities.iter().any(|e| e.handle.hex() == r.view));
    assert!(destination.doc().unwrap().entity(Handle::parse_hex(&r.view).unwrap()).is_none());
}
