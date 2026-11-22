use super::super::{
    Accuracy, Block, Insert, Projection, Quality, RECORD, SavedView, Session, Space, Vec2, Vec3, Visibility, apply_loaded, apply_view, model, tag,
};
use super::*;
use crate::cmd::clipboard as native;
use cadcraft_doc::{
    Line, Text, TextStyle,
    model_projection::{Basis, LineProvenance, PieceProvenance, ProjectedEntity, SourceReference},
};

fn metadata(sha: char) -> Value {
    json!({"protocol":"cadcraft-model-worker/1","path":format!("/models/деталь-{sha}.step"),
        "source":{"sha256":sha.to_string().repeat(64),"bytes":512},"format":"step",
        "units":{"length":"mm","area":"mm2","volume":"mm3","angle":"radian","coordinate_system":"right_handed_z_up"},
        "metadata":{"part_number":"АБВГ.123","engineering_material":"Сталь","description":"Проверка буфера"},
        "bodies":[{"name":"Тело","source_item":17}],"tree":{"name":"Сборка","children":[]},"warnings":[],"report":{}})
}

fn projection(sha: char) -> Projection {
    let visible = PieceProvenance { t: [0., 1.], visibility: Visibility::Visible, covered: false, accuracy: Accuracy::Analytic, segment_count: 1 };
    let mut covered = visible;
    covered.visibility = Visibility::Uncertain;
    covered.covered = true;
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
            uncertain_pieces: 1,
            bounded_pieces: 0,
            sampled_pieces: 0,
            unsupported_trims: 0,
            import_issues: 0,
            open_bodies: 0,
            analytic_complete: false,
        },
        lines: vec![LineProvenance { placement: 0, source: SourceReference::Edge { edge: 7, tangent: false }, pieces: vec![visible, covered] }],
        unsupported: vec![],
        prepare_reports: json!([{"native":"raw report"}]),
        drawing_stats: json!({"curves":1}),
        source_mapping: "payload_scoped".into(),
        visibility_contract: "raw_uncertain_and_covered_preserved_no_mesh_fallback".into(),
    }
}

fn add_model(s: &mut Session, sha: char) -> (String, Handle) {
    let id = apply_loaded(s, metadata(sha), None).unwrap()["id"].as_str().unwrap().to_owned();
    let result = apply_view(s, &id, &sha.to_string().repeat(64), &json!({"id":id,"layout":"GOST1","at":[20,50],"scale":2}), projection(sha)).unwrap();
    (id, Handle::parse_hex(result["handle"].as_str().unwrap()).unwrap())
}

fn insert(d: &Drawing, h: Handle) -> &Insert {
    let EntityKind::Insert(i) = &entities(d).find(|e| e.handle == h).unwrap().kind else { panic!("expected native INSERT") };
    i
}

fn transfer(source: &Session, destination: &mut Session) {
    destination.clipboard = source.clipboard.clone();
    destination.clipboard_base = source.clipboard_base;
    destination.clipboard_source = source.clipboard_source.clone();
}

fn result_handle(result: &Value) -> Handle {
    Handle::parse_hex(result["handles"][0].as_str().unwrap()).unwrap()
}

fn note(s: &mut Session, view: Handle) -> Entity {
    let name = insert(s.doc().unwrap(), view).block.clone();
    let d = s.doc_mut().unwrap();
    d.text_styles.push(TextStyle { name: "USER-NOTES".into(), ..TextStyle::default() });
    d.ensure_layer("USER-NOTES");
    let note = Entity {
        handle: d.new_handle(),
        common: Common { layer: "USER-NOTES".into(), xdata: vec![tag(1001, "USER-ANNOTATION"), tag(1000, "Не менять")], ..Common::default() },
        kind: EntityKind::Text(Text {
            insert: Vec3::new(2., 4., 0.),
            align_pt: None,
            height: 3.5,
            value: "Примечание".into(),
            rotation: 0.2,
            width_factor: 0.8,
            oblique: 0.,
            style: "USER-NOTES".into(),
            halign: Default::default(),
            valign: Default::default(),
        }),
    };
    let block = Arc::make_mut(d.blocks.get_mut(&name).unwrap());
    block.entities.push(note.clone());
    let curve = block.entities.iter().find(|e| data::load::<Value>(&e.common, PIECE).unwrap().is_some()).unwrap().handle;
    block.entities.modify(curve, |e| e.common.xdata.extend([tag(1001, "USER-CURVE"), tag(1000, "Exact user metadata")]));
    note
}

fn assert_snapshot(original: &SavedView, copied: &SavedView) {
    let mut normalized = copied.clone();
    normalized.handle = original.handle.clone();
    normalized.model_id = original.model_id.clone();
    normalized.parameters["id"] = json!(original.model_id);
    assert_eq!(json!(normalized), json!(original), "raw quality, source metadata and frozen provenance must not change");
}

fn projection_child(s: &mut Session, id: &str, parent: Handle) -> Handle {
    let store = load(s.doc().unwrap()).unwrap();
    let saved = store.views.iter().find(|v| v.handle == parent.hex()).unwrap();
    let vector = |key: &str| std::array::from_fn(|i| saved.basis[key][i].as_f64().unwrap());
    let y: [f64; 3] = vector("y");
    let direction = y.map(|n| -n);
    let up = vector("direction");
    let mut projected = projection('a');
    projected.basis = Basis { x: vector("x"), y: up, direction };
    let at = insert(s.doc().unwrap(), parent).insert;
    let result = apply_view(
        s,
        id,
        &"a".repeat(64),
        &json!({"id":id,"layout":"GOST1","at":[at.x,at.y-30.],"scale":2,"direction":direction,"up":up}),
        projected,
    )
    .unwrap();
    let child = Handle::parse_hex(result["handle"].as_str().unwrap()).unwrap();
    let d = s.doc_mut().unwrap();
    let mut store = load(d).unwrap();
    // Documented v1 records, independent of the projected-view command implementation.
    store.views.iter_mut().find(|v| v.handle == child.hex()).unwrap().provenance["projection_link"] =
        json!({"version":1,"parent":parent.hex(),"original_parent":parent.hex(),"direction":"top","status":"linked"});
    save(d, &store).unwrap();
    let mut common = d.entity(child).unwrap().common.clone();
    common.xdata.extend([tag(1001, "CADCRAFTGOST_PROJECTION_LINK"), tag(1070, "1")]);
    d.modify_entity(child, |e| e.common = common).unwrap();
    child
}

fn projection_fixture() -> (Session, String, Handle, Handle) {
    let mut s = crate::extensions::new_session();
    let (id, parent) = add_model(&mut s, 'a');
    let child = projection_child(&mut s, &id, parent);
    s.state_mut().unwrap().space = Space::Paper("GOST1".into());
    (s, id, parent, child)
}

fn assert_projection_snapshot(original: &SavedView, copied: &SavedView, parent: Option<Handle>) {
    let link = &copied.provenance["projection_link"];
    assert_eq!(link["parent"], parent.map_or(Value::Null, |h| json!(h.hex())));
    assert_eq!(link["status"], if parent.is_some() { "linked" } else { "missing_parent" });
    assert_eq!(link["original_parent"], original.provenance["projection_link"]["original_parent"]);
    let mut normalized = copied.clone();
    normalized.provenance["projection_link"] = original.provenance["projection_link"].clone();
    assert_snapshot(original, &normalized);
}

#[test]
fn projection_pair_native_copy_remaps_only_the_new_pair_and_preserves_source_records() {
    let (mut s, _, parent, child) = projection_fixture();
    let before = load(s.doc().unwrap()).unwrap();
    let marker = [tag(1001, "CADCRAFTGOST_PROJECTION_LINK"), tag(1070, "1")];
    let result = s.execute("copy", &json!({"handles":[child.hex(),parent.hex()],"delta":[80,0]})).unwrap();
    let copied_child = Handle::parse_hex(result["handles"][0].as_str().unwrap()).unwrap();
    let copied_parent = Handle::parse_hex(result["handles"][1].as_str().unwrap()).unwrap();
    let after = load(s.doc().unwrap()).unwrap();
    assert_eq!((after.models.len(), after.views.len()), (1, 4));
    for original in &before.views {
        assert_eq!(json!(after.views.iter().find(|v| v.handle == original.handle).unwrap()), json!(original));
    }
    assert_snapshot(&before.views[0], after.views.iter().find(|v| v.handle == copied_parent.hex()).unwrap());
    assert_projection_snapshot(&before.views[1], after.views.iter().find(|v| v.handle == copied_child.hex()).unwrap(), Some(copied_parent));
    assert!(s.doc().unwrap().entity(copied_child).unwrap().common.xdata.windows(2).any(|tags| tags == marker));
}

#[test]
fn child_only_copy_and_clipboard_paste_freeze_missing_parent_without_capturing_it() {
    for clipboard in [false, true] {
        let (mut s, _, parent, child) = projection_fixture();
        let before = load(s.doc().unwrap()).unwrap();
        let parent_entity = s.doc().unwrap().entity(parent).unwrap().clone();
        let copied = if clipboard {
            s.execute("copyclip", &json!({"handles":[child.hex()]})).unwrap();
            let captured = load(&s.clipboard_source.as_ref().unwrap().defs).unwrap();
            assert_eq!(captured.views.len(), 1);
            assert_eq!(captured.views[0].handle, child.hex());
            result_handle(&s.execute("pasteclip", &json!({"at":[100,100]})).unwrap())
        } else {
            result_handle(&s.execute("copy", &json!({"handles":[child.hex()],"delta":[100,0]})).unwrap())
        };
        let after = load(s.doc().unwrap()).unwrap();
        assert_eq!(after.views.len(), 3);
        assert_projection_snapshot(&before.views[1], after.views.iter().find(|v| v.handle == copied.hex()).unwrap(), None);
        assert_eq!(s.doc().unwrap().entity(parent).unwrap(), &parent_entity);
        for original in &before.views {
            assert_eq!(json!(after.views.iter().find(|v| v.handle == original.handle).unwrap()), json!(original));
        }
        let listed = s.execute("gost.model.inspect", &json!({"id":after.views.last().unwrap().model_id})).unwrap();
        let visible = listed["views"].as_array().unwrap().iter().find(|v| v["handle"] == copied.hex()).unwrap();
        assert_eq!(visible["provenance"]["projection_link"]["status"], "missing_parent");
    }
}

#[test]
fn cross_document_projection_paste_never_binds_colliding_destination_parents() {
    for include_parent in [false, true] {
        let (mut source, id, parent, child) = projection_fixture();
        let (mut destination, other_id, other_parent, other_child) = projection_fixture();
        assert_eq!((id, parent, child), (other_id, other_parent, other_child));
        let original_source = source.doc().unwrap().clone();
        let untouched = load(destination.doc().unwrap()).unwrap();
        let handles = if include_parent { vec![child.hex(), parent.hex()] } else { vec![child.hex()] };
        source.execute("copybase", &json!({"handles":handles,"base":[0,0]})).unwrap();
        transfer(&source, &mut destination);
        let result = destination.execute("pasteclip", &json!({"at":[100,200]})).unwrap();
        let copied_child = result_handle(&result);
        let copied_parent = include_parent.then(|| Handle::parse_hex(result["handles"][1].as_str().unwrap()).unwrap());
        let after = load(destination.doc().unwrap()).unwrap();
        let copied = after.views.iter().find(|v| v.handle == copied_child.hex()).unwrap();
        assert_ne!(copied.model_id, untouched.models[0].id);
        assert_projection_snapshot(&untouched.views[1], copied, copied_parent);
        for original in &untouched.views {
            assert_eq!(json!(after.views.iter().find(|v| v.handle == original.handle).unwrap()), json!(original));
        }
        assert_eq!(source.doc().unwrap(), &original_source);
    }
}

#[test]
fn projection_ancestor_chain_survives_repeated_native_copy_with_immutable_audit_handles() {
    let (mut s, id, root, child) = projection_fixture();
    let leaf = projection_child(&mut s, &id, child);
    let before = load(s.doc().unwrap()).unwrap();
    let mut handles = vec![leaf.hex(), child.hex(), root.hex()];
    for _ in 0..2 {
        let result = s.execute("copy", &json!({"handles":handles,"delta":[80,0]})).unwrap();
        handles = result["handles"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect();
        let leaf = Handle::parse_hex(&handles[0]).unwrap();
        let child = Handle::parse_hex(&handles[1]).unwrap();
        let root = Handle::parse_hex(&handles[2]).unwrap();
        let after = load(s.doc().unwrap()).unwrap();
        assert_projection_snapshot(&before.views[2], after.views.iter().find(|v| v.handle == leaf.hex()).unwrap(), Some(child));
        assert_projection_snapshot(&before.views[1], after.views.iter().find(|v| v.handle == child.hex()).unwrap(), Some(root));
        assert_snapshot(&before.views[0], after.views.iter().find(|v| v.handle == root.hex()).unwrap());
        for original in &before.views {
            assert_eq!(json!(after.views.iter().find(|v| v.handle == original.handle).unwrap()), json!(original));
        }
    }
}

#[test]
fn projection_missing_parent_never_revives_when_the_audited_parent_is_copied_too() {
    let (mut s, _, parent, child) = projection_fixture();
    let d = s.doc_mut().unwrap();
    let mut store = load(d).unwrap();
    store.views[1].provenance["projection_link"]["parent"] = Value::Null;
    store.views[1].provenance["projection_link"]["status"] = json!("missing_parent");
    save(d, &store).unwrap();
    let result = s.execute("copy", &json!({"handles":[child.hex(),parent.hex()],"delta":[80,0]})).unwrap();
    let copied = result_handle(&result);
    let after = load(s.doc().unwrap()).unwrap();
    assert_projection_snapshot(&store.views[1], after.views.iter().find(|v| v.handle == copied.hex()).unwrap(), None);
}

#[test]
fn projection_snapshot_placeholder_is_not_a_transferred_parent_insert() {
    let (mut source, id, parent, child) = projection_fixture();
    let curve = source.doc().unwrap().block(&insert(source.doc().unwrap(), parent).block).unwrap().entities.iter().next().unwrap().clone();
    let detached = source.doc_mut().unwrap().add(&Space::Model, curve.common.clone(), curve.kind.clone()).unwrap();
    apply_loaded(&mut source, metadata('b'), Some((&id, &"a".repeat(64)))).unwrap();
    let before = load(source.doc().unwrap()).unwrap();
    source.execute("copyclip", &json!({"handles":[child.hex(),detached.hex()]})).unwrap();
    let captured = load(&source.clipboard_source.as_ref().unwrap().defs).unwrap();
    assert_eq!(captured.views.len(), 2, "old curve requires both matching snapshot records, not their INSERTs");
    let mut destination = crate::extensions::new_session();
    transfer(&source, &mut destination);
    let result = destination.execute("pasteclip", &json!({"at":[100,200]})).unwrap();
    let copied = result_handle(&result);
    let after = load(destination.doc().unwrap()).unwrap();
    assert_projection_snapshot(&before.views[1], after.views.iter().find(|v| v.handle == copied.hex()).unwrap(), None);
    let snapshot = after.views.iter().find(|v| v.handle != copied.hex()).unwrap();
    assert!(destination.doc().unwrap().entity(Handle::parse_hex(&snapshot.handle).unwrap()).is_none());
    assert_snapshot(&before.views[0], snapshot);
    let detached = Handle::parse_hex(result["handles"][1].as_str().unwrap()).unwrap();
    let piece: Value = data::load(&destination.doc().unwrap().entity(detached).unwrap().common, PIECE).unwrap().unwrap();
    assert_eq!(piece["source_sha256"], "a".repeat(64));
    assert_eq!(piece["model_id"], snapshot.model_id);
}

fn corrupt_projection(store: &mut Store, malformed: usize) {
    let parent = store.views[0].handle.clone();
    let child = store.views[1].handle.clone();
    let link = &mut store.views[1].provenance["projection_link"];
    match malformed {
        0 => link["version"] = json!(2),
        1 => link["direction"] = json!("front"),
        2 => link["parent"] = json!(42),
        3 => link["parent"] = Value::Null,
        4 => link["status"] = json!("missing_parent"),
        5 => link["original_parent"] = json!("not-a-handle"),
        6 => link["status"] = json!("unknown"),
        7 => link["extra"] = json!(true),
        8 => link["parent"] = json!(child),
        9 => {
            store.views[0].provenance["projection_link"] =
                json!({"version":1,"parent":child,"original_parent":child,"direction":"bottom","status":"linked"});
        }
        _ => panic!("unexpected malformed projection fixture"),
    }
    assert_eq!(store.views[0].handle, parent);
}

fn save_corrupt_projection(d: &mut cadcraft_doc::Drawing, store: &Store) {
    // Model a hostile serialized input, not an invalid write through the validating production API.
    let text = serde_json::to_string(store).unwrap();
    let foreign = Arc::make_mut(&mut d.foreign);
    let handle = foreign.dictionary.iter().find(|(name, _)| name == RECORD).unwrap().1;
    let object = foreign.objects.iter_mut().find(|o| o.handle == handle).unwrap();
    object.tags.retain(|t| t.code != 1);
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + 255).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        object.tags.push(tag(1, &text[start..end]));
        start = end;
    }
}

#[test]
fn malformed_projection_copy_and_cut_fail_without_touching_source_or_clipboard() {
    for malformed in 0..10 {
        for command in ["copy", "copyclip", "cutclip"] {
            let (mut s, _, parent, child) = projection_fixture();
            s.execute("copyclip", &json!({"handles":[parent.hex()]})).unwrap();
            let previous_clipboard = s.clipboard.clone();
            let previous_serial = s.clipboard_source.as_ref().unwrap().serial;
            let d = s.doc_mut().unwrap();
            let mut store = load(d).unwrap();
            corrupt_projection(&mut store, malformed);
            save_corrupt_projection(d, &store);
            s.set_selection(vec![parent]);
            let before = s.doc().unwrap().clone();
            let undo = s.state().unwrap().undo.len();
            let revision = s.state().unwrap().revision;
            assert!(s.execute(command, &json!({"handles":[child.hex(),parent.hex()],"delta":[80,0]})).is_err(), "{command}: {malformed}");
            assert_eq!(s.doc().unwrap(), &before);
            assert_eq!(s.clipboard, previous_clipboard);
            assert_eq!(s.clipboard_source.as_ref().unwrap().serial, previous_serial);
            assert_eq!(s.state().unwrap().selection, vec![parent]);
            assert_eq!(s.state().unwrap().undo.len(), undo);
            assert_eq!(s.state().unwrap().revision, revision);
        }
    }
}

#[test]
fn malformed_projection_paste_rolls_back_actual_adopted_entities_and_metadata() {
    for malformed in 0..10 {
        for command in ["pasteclip", "pasteblock"] {
            let (mut source, _, parent, child) = projection_fixture();
            source.execute("copyclip", &json!({"handles":[parent.hex(),child.hex()]})).unwrap();
            let defs = Arc::make_mut(&mut source.clipboard_source.as_mut().unwrap().defs);
            let mut store = load(defs).unwrap();
            corrupt_projection(&mut store, malformed);
            save_corrupt_projection(defs, &store);
            let (mut destination, _, existing, _) = projection_fixture();
            transfer(&source, &mut destination);
            destination.set_selection(vec![existing]);
            let before = destination.doc().unwrap().clone();
            let undo = destination.state().unwrap().undo.len();
            let revision = destination.state().unwrap().revision;
            assert!(destination.execute(command, &json!({"at":[100,200]})).is_err(), "{command}: {malformed}");
            assert_eq!(destination.doc().unwrap(), &before);
            assert_eq!(destination.state().unwrap().selection, vec![existing]);
            assert_eq!(destination.state().unwrap().undo.len(), undo);
            assert_eq!(destination.state().unwrap().revision, revision);
        }
    }
}

#[test]
fn cross_document_commands_never_bind_colliding_model_or_view_ids() {
    let mut source = crate::extensions::new_session();
    let (id, old) = add_model(&mut source, 'a');
    let mut destination = crate::extensions::new_session();
    let (other_id, other) = add_model(&mut destination, 'c');
    assert_eq!(id, other_id);
    assert_eq!(old, other, "exercise an actual model/view identity collision");
    let annotation = note(&mut source, old);
    apply_loaded(&mut source, metadata('b'), Some((&id, &"a".repeat(64)))).unwrap();
    apply_loaded(&mut source, metadata('d'), None).unwrap(); // Unselected source metadata must not leak.
    let source_store = load(source.doc().unwrap()).unwrap();
    let expected = source_store.views[0].clone();
    let untouched = destination.doc().unwrap().clone();
    let source_before = source.doc().unwrap().clone();
    source.execute("copybase", &json!({"handles":[old.hex()],"base":[0,0]})).unwrap();
    assert_eq!(source.doc().unwrap(), &source_before);
    let captured = load(&source.clipboard_source.as_ref().unwrap().defs).unwrap();
    assert_eq!((captured.models.len(), captured.views.len()), (1, 1));
    assert_eq!(captured.models[0].source.sha256, "b".repeat(64));
    transfer(&source, &mut destination);
    let pasted = result_handle(&destination.execute("pasteclip", &json!({"at":[100,200]})).unwrap());
    let d = destination.doc().unwrap();
    let link: ViewLink = data::load(&d.entity(pasted).unwrap().common, VIEW).unwrap().unwrap();
    assert_ne!(link.model_id, other_id);
    assert_eq!(link.view, pasted.hex());
    assert_eq!(d.entity(other), untouched.entity(other));
    let store = load(d).unwrap();
    assert_eq!(store.models.len(), 2);
    assert_eq!(model(&store, &other_id).unwrap().source.sha256, "c".repeat(64));
    assert_eq!(model(&store, &link.model_id).unwrap().source.sha256, "b".repeat(64));
    let saved = store.views.iter().find(|v| v.handle == pasted.hex()).unwrap();
    assert_snapshot(&expected, saved);
    let inspected = destination.execute("gost.model.inspect", &json!({"id":link.model_id})).unwrap();
    assert_eq!(inspected["views"][0]["stale"], true);
    assert_eq!(inspected["views"][0]["source"]["sha256"], "a".repeat(64));
    let d = destination.doc().unwrap();
    let block = d.block(&insert(d, pasted).block).unwrap();
    assert_ne!(block.name, insert(d, other).block);
    let copied_note = block.entities.iter().find(|e| matches!(e.kind, EntityKind::Text(_))).unwrap();
    assert_eq!(copied_note.common, annotation.common);
    assert_eq!(copied_note.kind, annotation.kind);
    assert!(d.text_styles.iter().any(|style| style.name == "USER-NOTES"));
    let curve = block.entities.iter().find(|e| data::load::<Value>(&e.common, PIECE).unwrap().is_some()).unwrap();
    let piece: Value = data::load(&curve.common, PIECE).unwrap().unwrap();
    assert_eq!(piece["model_id"], link.model_id);
    assert_eq!(piece["source_sha256"], "a".repeat(64));
    assert!(curve.common.xdata.iter().any(|t| t.value == "Exact user metadata"));
    let original_insert = insert(source.doc().unwrap(), old);
    let pasted_insert = insert(d, pasted);
    assert_eq!(pasted_insert.scale, original_insert.scale);
    assert_eq!(pasted_insert.rotation, original_insert.rotation);
    assert_eq!(pasted_insert.insert, original_insert.insert + Vec3::new(100., 200., 0.));
    destination.undo().unwrap();
    assert_eq!(destination.doc().unwrap(), &untouched);
    destination.redo().unwrap();
    assert_eq!(load(destination.doc().unwrap()).unwrap().models.len(), 2);
}

fn wrap(s: &mut Session, view: Handle) -> Handle {
    let d = s.doc_mut().unwrap();
    let view_entity = d.remove_entity(view).unwrap();
    let mut block = Block::new("user-assembly");
    block.entities.push((*view_entity).clone());
    d.blocks.insert(block.name.clone(), Arc::new(block));
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::Insert(Insert {
            block: "USER-ASSEMBLY".into(),
            insert: Vec3::new(5., 6., 7.),
            scale: Vec3::new(2., 3., 4.),
            rotation: 0.3,
            ..Insert::default()
        }),
    )
    .unwrap()
}

#[test]
fn system_dxf_roundtrip_and_pasteblock_remap_nested_views_and_preserve_transforms() {
    let mut source = crate::extensions::new_session();
    let (_, view) = add_model(&mut source, 'a');
    let annotation = note(&mut source, view);
    let expected_insert = insert(source.doc().unwrap(), view).clone();
    let outer = wrap(&mut source, view);
    let expected_outer = insert(source.doc().unwrap(), outer).clone();
    source.execute("copybase", &json!({"handles":[outer.hex()],"base":[3,4]})).unwrap();
    // These are the actual drawing builder/loader used by system_text/load_system_text. The real
    // DXF codec is supplied directly, avoiding process-global test IO hooks that may be fake.
    let exported = native::system_drawing(&source).unwrap();
    let dxf = cadcraft_io::write_dxf(&exported);
    let decoded = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let mut destination = crate::extensions::new_session();
    let (other_id, other_view) = add_model(&mut destination, 'c');
    let _other_outer = wrap(&mut destination, other_view);
    let other_before = destination.doc().unwrap().clone();
    assert!(native::load_system_drawing(&mut destination, decoded));
    assert_eq!(destination.clipboard_base, Vec2::new(3., 4.));
    let result = destination.execute("pasteblock", &json!({"at":[100,200]})).unwrap();
    let h = Handle::parse_hex(result["handle"].as_str().unwrap()).unwrap();
    let d = destination.doc().unwrap();
    let enclosing = d.block(&insert(d, h).block).unwrap();
    assert_eq!(enclosing.base, Vec3::new(3., 4., 0.));
    let outer = enclosing.entities.iter().next().unwrap();
    let EntityKind::Insert(outer_insert) = &outer.kind else { panic!() };
    assert_eq!(outer_insert.insert, expected_outer.insert);
    assert_eq!(outer_insert.scale, expected_outer.scale);
    assert_eq!(outer_insert.rotation, expected_outer.rotation);
    assert_ne!(outer_insert.block.to_ascii_uppercase(), "USER-ASSEMBLY");
    let wrapper = d.block(&outer_insert.block).unwrap();
    let nested = wrapper.entities.iter().next().unwrap();
    let link: ViewLink = data::load(&nested.common, VIEW).unwrap().unwrap();
    assert_ne!(link.model_id, other_id);
    assert_eq!(link.view, nested.handle.hex());
    let EntityKind::Insert(nested_insert) = &nested.kind else { panic!() };
    assert_eq!(nested_insert.insert, expected_insert.insert);
    assert_eq!(nested_insert.scale, expected_insert.scale);
    assert_eq!(nested_insert.rotation, expected_insert.rotation);
    let block = d.block(&nested_insert.block).unwrap();
    let copied_note = block.entities.iter().find(|e| matches!(e.kind, EntityKind::Text(_))).unwrap();
    let EntityKind::Text(copied_text) = &copied_note.kind else { panic!() };
    let EntityKind::Text(expected_text) = &annotation.kind else { panic!() };
    // DXF stores degrees, so the radians round-trip has ordinary floating-point error.
    assert!((copied_text.rotation - expected_text.rotation).abs() < 1e-12);
    let mut expected_text = expected_text.clone();
    expected_text.rotation = copied_text.rotation;
    assert_eq!(copied_text, &expected_text);
    assert_eq!(copied_note.common.xdata, annotation.common.xdata);
    let old_block = insert(&other_before, other_view).block.clone();
    assert_eq!(d.block(&old_block), other_before.block(&old_block));
    let store = load(d).unwrap();
    let saved = store.views.iter().find(|v| v.handle == nested.handle.hex()).unwrap();
    assert_snapshot(&load(source.doc().unwrap()).unwrap().views[0], saved);
    // A normal saved drawing can also reopen without relying on the clipboard's in-memory maps.
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(d).as_bytes()).unwrap();
    destination.open_drawing(reopened, "pasted model", None);
    let inspected = destination.execute("gost.model.inspect", &json!({"id":link.model_id})).unwrap();
    assert_eq!(inspected["views"][0]["quality"]["uncertain_pieces"], 1);
    assert_eq!(inspected["views"][0]["provenance"]["lines"][0]["source"]["edge"], 7);
}

#[test]
fn native_copy_isolates_managed_geometry_without_duplicating_the_same_document_model() {
    let mut s = crate::extensions::new_session();
    let (id, view) = add_model(&mut s, 'a');
    note(&mut s, view);
    let original_entity = s.doc().unwrap().entity(view).unwrap().clone();
    let original_block = s.doc().unwrap().block(&insert(s.doc().unwrap(), view).block).unwrap().clone();
    let original_snapshot = load(s.doc().unwrap()).unwrap().views[0].clone();
    let copied = result_handle(&s.execute("copy", &json!({"handles":[view.hex()],"delta":[70,20]})).unwrap());
    let new_name = insert(s.doc().unwrap(), copied).block.clone();
    assert_ne!(new_name, original_block.name);
    let link: ViewLink = data::load(&s.doc().unwrap().entity(copied).unwrap().common, VIEW).unwrap().unwrap();
    assert_eq!(link.model_id, id);
    assert_eq!(link.view, copied.hex());
    let store = load(s.doc().unwrap()).unwrap();
    assert_eq!((store.models.len(), store.views.len()), (1, 2));
    assert_snapshot(&original_snapshot, store.views.iter().find(|v| v.handle == copied.hex()).unwrap());
    let block = Arc::make_mut(s.doc_mut().unwrap().blocks.get_mut(&new_name).unwrap());
    let curve = block.entities.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap().handle;
    assert!(!original_block.entities.contains(curve));
    block.entities.modify(curve, |e| {
        if let EntityKind::Line(line) = &mut e.kind {
            line.b.x = 999.;
        }
    });
    assert_eq!(s.doc().unwrap().entity(view).unwrap(), &original_entity);
    assert_eq!(s.doc().unwrap().block(&original_block.name).unwrap(), &original_block);
    s.undo().unwrap();
    assert_eq!(load(s.doc().unwrap()).unwrap().views.len(), 1);
    assert!(s.doc().unwrap().entity(copied).is_none());
    s.redo().unwrap();
    assert_eq!(load(s.doc().unwrap()).unwrap().views.len(), 2);
    assert_eq!(insert(s.doc().unwrap(), copied).block, new_name);
}

#[test]
fn same_document_clipboard_paste_and_cut_paste_keep_independent_editable_views() {
    for cut in [false, true] {
        let mut s = crate::extensions::new_session();
        let (_, view) = add_model(&mut s, 'a');
        let original_name = insert(s.doc().unwrap(), view).block.clone();
        let original = s.doc().unwrap().block(&original_name).unwrap().clone();
        s.execute(if cut { "cutclip" } else { "copyclip" }, &json!({"handles":[view.hex()]})).unwrap();
        assert_eq!(s.doc().unwrap().entity(view).is_none(), cut);
        let pasted = result_handle(&s.execute("pasteclip", &json!({"at":[100,200]})).unwrap());
        assert_ne!(insert(s.doc().unwrap(), pasted).block, original_name);
        assert_eq!(s.doc().unwrap().block(&original_name).unwrap(), &original);
        let result = s.execute("gost.model.list", &json!({})).unwrap();
        assert!(result.to_string().contains(&pasted.hex()));
    }
}

#[test]
fn stale_detached_curve_carries_its_exact_old_source_snapshot() {
    let mut source = crate::extensions::new_session();
    let (id, view) = add_model(&mut source, 'a');
    let curve = source.doc().unwrap().block(&insert(source.doc().unwrap(), view).block).unwrap().entities.iter().next().unwrap().clone();
    let curve_handle = source.doc_mut().unwrap().add(&Space::Model, curve.common.clone(), curve.kind.clone()).unwrap();
    apply_loaded(&mut source, metadata('b'), Some((&id, &"a".repeat(64)))).unwrap();
    source.execute("copyclip", &json!({"handles":[curve_handle.hex()]})).unwrap();
    let mut destination = crate::extensions::new_session();
    let (_, other) = add_model(&mut destination, 'c');
    transfer(&source, &mut destination);
    let pasted = result_handle(&destination.execute("pasteclip", &json!({})).unwrap());
    let piece: Value = data::load(&destination.doc().unwrap().entity(pasted).unwrap().common, PIECE).unwrap().unwrap();
    assert_ne!(piece["model_id"], "1");
    assert_eq!(piece["source_sha256"], "a".repeat(64));
    let store = load(destination.doc().unwrap()).unwrap();
    let snapshot = store.views.iter().find(|v| v.model_id == piece["model_id"].as_str().unwrap()).unwrap();
    assert_eq!(snapshot.source.sha256, "a".repeat(64));
    assert_ne!(snapshot.handle, other.hex());
    assert_ne!(snapshot.handle, pasted.hex());
    assert_eq!(snapshot.metadata, metadata('a'));
}

#[test]
fn failed_paste_rolls_back_definitions_handles_metadata_selection_and_undo() {
    for command in ["pasteclip", "pasteblock"] {
        for malformed in [false, true] {
            let mut source = crate::extensions::new_session();
            let (_, view) = add_model(&mut source, 'a');
            source.execute("copyclip", &json!({"handles":[view.hex()]})).unwrap();
            let mut destination = crate::extensions::new_session();
            let (_, other) = add_model(&mut destination, 'c');
            transfer(&source, &mut destination);
            if malformed {
                let mut link: ViewLink = data::load(&destination.clipboard[0].common, VIEW).unwrap().unwrap();
                link.model_id = "999".into();
                data::save(&mut destination.clipboard[0].common, VIEW, &link).unwrap();
            } else {
                let d = destination.doc_mut().unwrap();
                let mut store = load(d).unwrap();
                let original = store.models[0].clone();
                for i in 2..=MAX_MODELS {
                    let mut model = original.clone();
                    model.id = i.to_string();
                    store.models.push(model);
                }
                store.next_id = MAX_MODELS as u64 + 1;
                save(d, &store).unwrap();
            }
            destination.set_selection(vec![other]);
            let before = destination.doc().unwrap().clone();
            let state = destination.state().unwrap();
            let undo = state.undo.len();
            let revision = state.revision;
            let selection = state.selection.clone();
            assert!(destination.execute(command, &json!({"at":[100,200]})).is_err());
            assert_eq!(destination.doc().unwrap(), &before, "{command} left a partial model transfer");
            assert_eq!(destination.state().unwrap().selection, selection);
            assert_eq!(destination.state().unwrap().undo.len(), undo);
            assert_eq!(destination.state().unwrap().revision, revision);
        }
    }
}

#[test]
fn bounded_capture_refuses_cut_atomically_and_invalid_system_import_keeps_the_clipboard() {
    let mut source = crate::extensions::new_session();
    let (_, first) = add_model(&mut source, 'a');
    source.execute("copyclip", &json!({"handles":[first.hex()]})).unwrap();
    let previous = source.clipboard.clone();
    let serial = source.clipboard_source.as_ref().unwrap().serial;
    let mut handles = vec![first.hex()];
    for _ in 0..MAX_CLIPBOARD_MODELS {
        handles.push(add_model(&mut source, 'a').1.hex());
    }
    let before = source.doc().unwrap().clone();
    for command in ["copyclip", "cutclip"] {
        assert!(source.execute(command, &json!({"handles":handles})).is_err());
        assert_eq!(source.doc().unwrap(), &before);
        assert_eq!(source.clipboard, previous);
        assert_eq!(source.clipboard_source.as_ref().unwrap().serial, serial);
    }
    let mut exported = native::system_drawing(&source).unwrap();
    let foreign = Arc::make_mut(&mut exported.foreign);
    let handle = foreign.dictionary.iter().find(|(name, _)| name == RECORD).unwrap().1;
    let object = foreign.objects.iter_mut().find(|o| o.handle == handle).unwrap();
    object.tags.retain(|t| t.code != 1);
    object.tags.push(tag(1, "x".repeat(256)));
    let decoded = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&exported).as_bytes()).unwrap();
    assert!(!native::load_system_drawing(&mut source, decoded));
    assert_eq!(source.clipboard, previous);
    assert_eq!(source.clipboard_source.as_ref().unwrap().serial, serial);
}

#[test]
fn failed_native_copy_restores_the_entity_created_before_the_hook_rejected_it() {
    let mut s = crate::extensions::new_session();
    let (_, view) = add_model(&mut s, 'a');
    let mut common = s.doc().unwrap().entity(view).unwrap().common.clone();
    let mut link: ViewLink = data::load(&common, VIEW).unwrap().unwrap();
    link.model_id = "999".into();
    data::save(&mut common, VIEW, &link).unwrap();
    s.doc_mut().unwrap().modify_entity(view, |e| e.common = common).unwrap();
    s.set_selection(vec![view]);
    let before = s.doc().unwrap().clone();
    let undo = s.state().unwrap().undo.len();
    let revision = s.state().unwrap().revision;
    assert!(s.execute("copy", &json!({"handles":[view.hex()],"delta":[100,0]})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    assert_eq!(s.state().unwrap().selection, vec![view]);
    assert_eq!(s.state().unwrap().undo.len(), undo);
    assert_eq!(s.state().unwrap().revision, revision);
}

#[test]
fn model_clipboard_keeps_legacy_engineering_mark_normalization_and_native_annotations() {
    let mut source = crate::extensions::new_session();
    let (_, view) = add_model(&mut source, 'a');
    let mark = source.execute("gost.roughness", &json!({"at":[0,0],"value":1.6})).unwrap();
    let mark = Handle::parse_hex(mark["handle"].as_str().unwrap()).unwrap();
    let source_name = insert(source.doc().unwrap(), mark).block.clone();
    Arc::make_mut(source.doc_mut().unwrap().blocks.get_mut(&source_name).unwrap()).anonymous = false;
    let expected: Vec<_> = source.doc().unwrap().block(&source_name).unwrap().entities.iter().map(|e| e.kind.clone()).collect();
    source.execute("copyclip", &json!({"handles":[view.hex(),mark.hex()]})).unwrap();
    assert!(!source.doc().unwrap().block(&source_name).unwrap().anonymous);
    assert!(source.clipboard_source.as_ref().unwrap().defs.block(&source_name).unwrap().anonymous);
    let mut destination = crate::extensions::new_session();
    add_model(&mut destination, 'c');
    let other = destination.execute("gost.roughness", &json!({"at":[0,0],"value":6.3})).unwrap();
    let other = Handle::parse_hex(other["handle"].as_str().unwrap()).unwrap();
    let other_name = insert(destination.doc().unwrap(), other).block.clone();
    assert_eq!(source_name, other_name);
    let original = destination.doc().unwrap().block(&other_name).unwrap().clone();
    transfer(&source, &mut destination);
    let pasted = destination.execute("pasteclip", &json!({"at":[100,200]})).unwrap();
    let mark = Handle::parse_hex(pasted["handles"][1].as_str().unwrap()).unwrap();
    let d = destination.doc().unwrap();
    let copied = d.block(&insert(d, mark).block).unwrap();
    assert_ne!(copied.name, other_name);
    assert_eq!(copied.entities.iter().map(|e| e.kind.clone()).collect::<Vec<_>>(), expected);
    assert_eq!(d.block(&other_name).unwrap(), &original);
}

#[test]
fn native_copy_of_a_wrapper_remaps_the_nested_saved_view_and_curve_handles() {
    let mut s = crate::extensions::new_session();
    let (_, view) = add_model(&mut s, 'a');
    let outer = wrap(&mut s, view);
    let old_wrapper = s.doc().unwrap().block(&insert(s.doc().unwrap(), outer).block).unwrap().clone();
    let old_view_block = s.doc().unwrap().block(&insert(s.doc().unwrap(), view).block).unwrap().clone();
    let copied = result_handle(&s.execute("copy", &json!({"handles":[outer.hex()],"delta":[100,0]})).unwrap());
    let d = s.doc().unwrap();
    let new_wrapper = d.block(&insert(d, copied).block).unwrap();
    assert_ne!(new_wrapper.name, old_wrapper.name);
    let nested = new_wrapper.entities.iter().next().unwrap();
    assert_ne!(nested.handle, view);
    let link: ViewLink = data::load(&nested.common, VIEW).unwrap().unwrap();
    assert_eq!(link.view, nested.handle.hex());
    let new_view_block = d.block(&insert(d, nested.handle).block).unwrap();
    assert_ne!(new_view_block.name, old_view_block.name);
    for curve in new_view_block.entities.iter() {
        assert!(!old_view_block.entities.contains(curve.handle));
        assert_eq!(data::load::<Value>(&curve.common, PIECE).unwrap().unwrap()["model_id"], link.model_id);
    }
    assert_eq!(d.block(&old_wrapper.name).unwrap(), &old_wrapper);
    assert_eq!(d.block(&old_view_block.name).unwrap(), &old_view_block);
    let store = load(d).unwrap();
    assert_eq!((store.models.len(), store.views.len()), (1, 2));
    assert!(store.views.iter().any(|v| v.handle == nested.handle.hex()));
}
