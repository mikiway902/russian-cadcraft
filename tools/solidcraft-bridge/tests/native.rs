use cadcraft_model_worker::{Error, MAX_FILE_BYTES, Worker};
use serde_json::{Value, json};
use solidcore_record::{Seg2, Vec3};
use solidcraft_doc::{Dir, Document, FeatureKind, Operation, UserMaterial, point_expr, rigid};
use solidcraft_format::container::{Limits, Writer};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("cadcraft-native-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
    fn worker(&self) -> Worker {
        Worker::new(vec![self.0.clone()]).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn kernel_test(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(solidcraft_kernel::KERNEL_STACK)
        .spawn(move || {
            solidcraft_kernel::start();
            f();
        })
        .unwrap()
        .join()
        .unwrap();
}
fn cylinder(doc: &mut Document, component: u64, name: &str) -> u64 {
    let id = doc
        .add_feature(
            FeatureKind::Cylinder {
                base: point_expr(Vec3::ZERO),
                axis: Dir(Vec3::Z),
                radius: "5".into(),
                height: "20".into(),
                operation: Operation::NewBody,
                references: Vec::new(),
            },
            Some(name),
        )
        .unwrap();
    let feature = doc.features.iter_mut().find(|f| f.id == id).unwrap();
    feature.component = component;
    feature.body_names = vec![name.into()];
    id
}
fn design() -> Document {
    let mut doc = Document::new("Synthetic native assembly");
    doc.metadata.0.insert("part_number".into(), json!("ASM-001"));
    doc.metadata.0.insert("org.example.evidence".into(), json!({"designation": "assembly data", "font": "opaque metadata, not an external font"}));
    let assembly = doc.add_component(Some("Subassembly"), 0).unwrap();
    let part = doc.add_component(Some("Pin definition"), assembly).unwrap();
    let parent = doc.occurrences.iter_mut().find(|o| o.component == assembly).unwrap();
    parent.transform = rigid(Vec3::new(100., 0., 0.), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
    let parent_id = parent.id;
    let first = doc.occurrences.iter_mut().find(|o| o.component == part).unwrap();
    first.transform = rigid(Vec3::new(10., 0., 0.), Vec3::ZERO, Vec3::Z, 0.);
    let first_id = first.id;
    let second = doc.add_occurrence(part, assembly, rigid(Vec3::new(-10., 0., 0.), Vec3::ZERO, Vec3::Z, 0.)).unwrap();
    assert_ne!(first_id, second);
    let definition = doc.components.iter_mut().find(|c| c.id == part).unwrap();
    definition.metadata.0.insert("part_number".into(), json!("PIN-007"));
    definition.metadata.0.insert("designation".into(), json!("АБВГ.01.007"));
    definition.metadata.0.insert("material".into(), json!("Stock grade, not a density"));
    definition.properties.insert("finish".into(), "original synthetic property".into());
    cylinder(&mut doc, part, "Pin");
    let suppressed = cylinder(&mut doc, part, "Suppressed pin");
    doc.features.iter_mut().find(|f| f.id == suppressed).unwrap().suppressed = true;
    doc.materials.insert("Pin".into(), "Synthetic alloy".into());
    doc.user_materials.push(UserMaterial { name: "Synthetic alloy".into(), density: 2.7, appearance: None, physical_card: None });
    doc.visibility.hidden_occurrences.push(vec![parent_id, first_id]);
    doc
}
const LIMITS: Limits = Limits {
    max_entries: 10_000,
    max_expanded: || 32 * 1024 * 1024,
    part_limit: |_| 32 * 1024 * 1024,
    level: |_, _| 1,
    check_size: |_| Ok(()),
    cancelled: None,
};
fn container(doc: &Document) -> Vec<u8> {
    let mut writer = Writer::new(LIMITS);
    writer.part("container.json", br#"{"format":"solidcraft-container/1"}"#, false).unwrap();
    writer.part("document.json", doc.to_json().as_bytes(), true).unwrap();
    // Cache content is deliberately bogus: native geometry must be re-evaluated, not adopted.
    writer.part("cache/preview.bin", b"not geometry", true).unwrap();
    writer.part("cache/model.bin", b"not trusted B-rep", true).unwrap();
    writer.finish().unwrap()
}
fn query(load: &Value) -> Value {
    json!({"source_sha256": load["source"]["sha256"], "direction": [1,1,1], "up": [0,0,1]})
}
fn assert_native_geometry(loaded: &Value, view: &Value) {
    assert_eq!(loaded["format"], "sc");
    assert_eq!(loaded["bodies"].as_array().unwrap().len(), 2);
    assert_eq!(loaded["metadata"]["part_number"], "ASM-001");
    assert_eq!(loaded["metadata"]["properties"]["org.example.evidence"]["designation"], "assembly data");
    assert_eq!(loaded["features"].as_array().unwrap().iter().filter(|f| f["suppressed"] == true).count(), 1);
    assert_eq!(loaded["bodies"].as_array().unwrap().iter().filter(|b| b["hidden"] == true).count(), 1);
    let mut centers = Vec::new();
    for body in loaded["bodies"].as_array().unwrap() {
        assert_eq!(body["part_number"], "PIN-007");
        assert_eq!(body["designation"], "АБВГ.01.007");
        assert_eq!(body["engineering_material"], "Synthetic alloy");
        assert_eq!(body["density_g_cm3"], 2.7);
        assert_eq!(body["component_static_material"], "Stock grade, not a density");
        assert_eq!(body["native_identity"]["definition_body"], "Pin");
        assert_eq!(body["native_identity"]["occurrence_path"].as_array().unwrap().len(), 2);
        assert!(body["source_item"].is_null() && body["face_origin"].is_null());
        assert_eq!(body["measure"]["status"], "ok", "{body}");
        assert_eq!(body["measure"]["volume_valid"], true);
        let measure = &body["measure"]["value"];
        let volume = measure["volume"].as_f64().unwrap();
        assert!((volume - 500. * std::f64::consts::PI).abs() < 1e-5 * volume);
        let centroid: Vec3 = serde_json::from_value(measure["centroid"].clone()).unwrap();
        assert!((centroid.x - 100.).abs() < 1e-6 && (centroid.z - 10.).abs() < 1e-6, "placement applied more/less than once: {centroid:?}");
        assert!((centroid.y.abs() - 10.).abs() < 1e-6);
        centers.push(centroid);
    }
    assert!((centers[0].y + centers[1].y).abs() < 1e-6);
    let basis = solidcraft_geom::hlr::View::new(Vec3::new(1., 1., 1.), Vec3::Z).unwrap();
    let mut conics = 0;
    for line in view["drawing"]["lines"].as_array().unwrap() {
        let center = centers[line["placement"].as_u64().unwrap() as usize];
        for piece in line["pieces"].as_array().unwrap() {
            for geometry in piece["geometry"].as_array().unwrap() {
                if let Seg2::Conic { a, apex, b, w } = serde_json::from_value(geometry.clone()).unwrap() {
                    conics += 1;
                    assert_eq!(piece["accuracy"]["kind"], "analytic");
                    let mid = (a + apex * (2. * w) + b) / (2. + 2. * w);
                    assert!(
                        [0., 20.].into_iter().any(|z| {
                            let cap = basis.screen(Vec3::new(center.x, center.y, z));
                            let (x, y) = (mid.x - cap[0], mid.y - cap[1]);
                            (x * x / 25. + y * y / (25. / 3.) - 1.).abs() < 1e-6
                        }),
                        "conic does not lie on a correctly transformed native cap: {geometry}"
                    );
                }
            }
        }
    }
    assert!(conics > 0, "native HLR lost exact oblique cap conics");
}

#[test]
fn native_json_and_real_container_roundtrip_preserve_exact_assembly_metadata_and_placement() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let doc = design();
        let mut worker = fixture.worker();
        let plain = fixture.write("assembly.sc", doc.to_json());
        let loaded = worker.call("v1.load", json!({"path": plain})).unwrap();
        let view = worker.call("v1.view", query(&loaded)).unwrap();
        assert_native_geometry(&loaded, &view);
        let packaged = fixture.write("packaged.sc", container(&doc));
        let roundtrip = worker.call("v1.load", json!({"path": packaged})).unwrap();
        let packaged_view = worker.call("v1.view", query(&roundtrip)).unwrap();
        assert_native_geometry(&roundtrip, &packaged_view);
        assert_eq!(loaded["tree"], roundtrip["tree"]);
        assert_eq!(view["drawing"]["lines"], packaged_view["drawing"]["lines"]);
        assert_eq!(view["quality"], packaged_view["quality"]);
        assert_ne!(loaded["source"]["sha256"], roundtrip["source"]["sha256"]);
        let changed = fixture.write("packaged.sc", b"changed source");
        assert_eq!(changed, packaged);
        let status = worker.call("v1.source_status", json!({})).unwrap();
        assert_eq!(status["changed"], true);
        assert_eq!(status["loaded"], roundtrip["source"]);
        assert!(matches!(
            worker.call(
                "v1.view",
                json!({"source_sha256": status["current"]["sha256"],
            "direction": [1,1,1], "up": [0,0,1]})
            ),
            Err(Error::StaleModel)
        ));
        assert_eq!(worker.call("v1.view", query(&roundtrip)).unwrap()["drawing"]["lines"], packaged_view["drawing"]["lines"]);
    });
}

#[test]
fn native_embedded_step_blob_uses_hash_checked_exact_geometry_and_reports_absent_material() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let body = solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 5., 20.).unwrap();
        let step = solidcraft_kernel::step_export(&[&body], "Original synthetic native test").unwrap();
        let mut doc = Document::new("Embedded exact STEP");
        doc.add_feature(
            FeatureKind::Import { file: "synthetic.step".into(), step: "".into(), components: Vec::new(), skip: Vec::new(), records: None },
            None,
        )
        .unwrap();
        let mut metadata = serde_json::to_value(&doc).unwrap();
        let mut writer = Writer::new(LIMITS);
        metadata["features"][0]["step_blob"] = writer.blob(step.as_bytes(), "step", true).unwrap();
        writer.part("container.json", br#"{"format":"solidcraft-container/1"}"#, false).unwrap();
        writer.part("document.json", &serde_json::to_vec(&metadata).unwrap(), true).unwrap();
        let path = fixture.write("embedded.sc", writer.finish().unwrap());
        let mut worker = fixture.worker();
        let loaded = worker.call("v1.load", json!({"path": path})).unwrap();
        let body = &loaded["bodies"][0];
        assert_eq!(body["measure"]["status"], "ok", "{loaded}");
        assert!((body["measure"]["value"]["volume"].as_f64().unwrap() - 500. * std::f64::consts::PI).abs() < 1e-3);
        assert!(body["engineering_material"].is_null() && body["density_g_cm3"].is_null() && body["part_number"].is_null());
    });
}

#[test]
fn native_malformed_bounded_and_failed_geometry_loads_are_atomic() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let mut worker = fixture.worker();
        let path = fixture.write("good.sc", design().to_json());
        let good = worker.call("v1.load", json!({"path": path})).unwrap();
        let drawing = worker.call("v1.view", query(&good)).unwrap();
        let bad = fixture.write("bad.sc", b"{ not a document");
        assert!(matches!(worker.call("v1.load", json!({"path": bad})), Err(Error::Import(_))));
        let deep = fixture.write("deep.sc", format!("{}0{}", "[".repeat(33), "]".repeat(33)));
        assert!(matches!(worker.call("v1.load", json!({"path": deep})), Err(Error::Limit(_))));
        let huge = fixture.0.join("huge.sc");
        fs::File::create(&huge).unwrap().set_len(MAX_FILE_BYTES as u64 + 1).unwrap();
        assert!(matches!(worker.call("v1.load", json!({"path": huge})), Err(Error::Limit(_))));
        let mut bomb = Writer::new(LIMITS);
        bomb.part("container.json", br#"{"format":"solidcraft-container/1"}"#, false).unwrap();
        bomb.part("document.json", &vec![b' '; MAX_FILE_BYTES + 1], true).unwrap();
        let bomb = fixture.write("expanded.sc", bomb.finish().unwrap());
        assert!(matches!(worker.call("v1.load", json!({"path": bomb})), Err(Error::Import(_))));
        let mut many = Document::new("Too many exact bodies");
        for i in 0..33 {
            cylinder(&mut many, 0, &format!("Pin{i}"));
        }
        let many = fixture.write("many.sc", many.to_json());
        assert!(matches!(worker.call("v1.load", json!({"path": many})), Err(Error::Limit(_))));
        let mut nesting = Document::new("Too deep an assembly");
        let mut parent = 0;
        for i in 0..33 {
            parent = nesting.add_component(Some(&format!("Level{i}")), parent).unwrap();
        }
        cylinder(&mut nesting, parent, "Pin");
        let nesting = fixture.write("nested.sc", nesting.to_json());
        assert!(matches!(worker.call("v1.load", json!({"path": nesting})), Err(Error::Limit(_))));
        let mut failed = Document::new("Failed native evaluation");
        cylinder(&mut failed, 0, "Pin");
        if let FeatureKind::Cylinder { radius, .. } = &mut failed.features[0].kind {
            *radius = "-1".into();
        }
        let failed = fixture.write("failed.sc", failed.to_json());
        assert!(worker.call("v1.load", json!({"path": failed})).is_err());
        let outside = Fixture::new();
        let denied = outside.write("denied.sc", design().to_json());
        assert!(matches!(worker.call("v1.load", json!({"path": denied})), Err(Error::Path(_))));
        assert_eq!(worker.call("v1.source_status", json!({})).unwrap()["loaded"], good["source"]);
        assert_eq!(worker.call("v1.view", query(&good)).unwrap()["drawing"]["lines"], drawing["drawing"]["lines"]);
    });
}

#[test]
fn native_corrupt_authoritative_blob_cannot_be_replaced_by_preview_geometry() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let mut worker = fixture.worker();
        let good = fixture.write("original.sc", design().to_json());
        let loaded = worker.call("v1.load", json!({"path": good})).unwrap();
        let before = worker.call("v1.view", query(&loaded)).unwrap();
        let cylinder = solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 5., 20.).unwrap();
        let step = solidcraft_kernel::step_export(&[&cylinder], "Original corrupt-blob fixture").unwrap();
        let mut doc = Document::new("Invalid authoritative hash");
        doc.add_feature(
            FeatureKind::Import { file: "embedded.step".into(), step: "".into(), components: Vec::new(), skip: Vec::new(), records: None },
            None,
        )
        .unwrap();
        let mut metadata = serde_json::to_value(&doc).unwrap();
        let forged = format!("blobs/{}.step", "0".repeat(64));
        metadata["features"][0]["step_blob"] = json!(forged);
        let mut writer = Writer::new(LIMITS);
        writer.part(&forged, step.as_bytes(), true).unwrap();
        writer.part("container.json", br#"{"format":"solidcraft-container/1"}"#, false).unwrap();
        writer.part("document.json", &serde_json::to_vec(&metadata).unwrap(), true).unwrap();
        writer.part("cache/preview.bin", b"untrusted geometry cannot repair an authoritative hash", true).unwrap();
        let path = fixture.write("bad-hash.sc", writer.finish().unwrap());
        assert!(matches!(worker.call("v1.load", json!({"path": path})), Err(Error::Import(_))));
        assert_eq!(worker.call("v1.source_status", json!({})).unwrap()["loaded"], loaded["source"]);
        assert_eq!(worker.call("v1.view", query(&loaded)).unwrap()["drawing"]["lines"], before["drawing"]["lines"]);
    });
}

fn reference_extrusion() -> (Document, u64) {
    use solidcore_record::Vec2;
    use solidcraft_doc::{Direction, Extent, PlaneRef, ProfileSel, SketchId, sketch::Sketch};
    let mut doc = Document::new("Reference lineage fixture");
    let component = doc.add_component(Some("Extruded part"), 0).unwrap();
    let occurrence = doc.occurrences.iter_mut().find(|o| o.component == component).unwrap();
    occurrence.transform = rigid(Vec3::new(100., 200., 0.), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
    doc.add_occurrence(component, 0, rigid(Vec3::new(-100., 0., 0.), Vec3::ZERO, Vec3::Z, 0.)).unwrap();
    let mut sketch = Sketch::new();
    let points = [Vec2::new(0., 0.), Vec2::new(10., 0.), Vec2::new(10., 6.), Vec2::new(0., 6.)];
    let ids: Vec<_> = points.iter().map(|p| sketch.add_point(*p, None).unwrap()).collect();
    for i in 0..4 {
        sketch.add_line_pts(ids[i], ids[(i + 1) % 4], None).unwrap();
    }
    let sk = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch }, Some("Profile")).unwrap();
    doc.feature_mut(sk).unwrap().component = component;
    let extent: Extent = serde_json::from_value(json!({"distance": "20", "direction": "positive"})).unwrap();
    assert_eq!(extent.direction, Direction::Positive);
    let id = doc
        .add_feature(
            FeatureKind::Extrude {
                sketch: SketchId(sk),
                profiles: ProfileSel::All,
                extent,
                operation: Operation::NewBody,
                targets: vec![],
                thin: None,
            },
            Some("Extrusion"),
        )
        .unwrap();
    let f = doc.feature_mut(id).unwrap();
    f.component = component;
    f.body_names = vec!["Extruded body".into()];
    // Keep one live body when the target is suppressed/deleted, to exercise successful reloads.
    cylinder(&mut doc, 0, "Unrelated survivor");
    (doc, id)
}

fn reference_point(worker: &mut Worker, load: &Value, reference: &Value, fraction: f64) -> Value {
    worker.call("v1.resolve_point", json!({"source_sha256": load["source"]["sha256"], "reference": reference, "fraction": fraction})).unwrap()
}

fn vertical_reference(worker: &mut Worker, load: &Value, refs: &Value) -> Value {
    for placement in refs["placements"].as_array().unwrap() {
        if placement["native_identity"]["definition_body"] != "Extruded body" {
            continue;
        }
        for edge in placement["edges"].as_array().unwrap() {
            if edge["verification"] != "verified_upstream_history" || edge["cross_rebuild"] != true {
                continue;
            }
            let a = reference_point(worker, load, &edge["reference"], 0.);
            let b = reference_point(worker, load, &edge["reference"], 1.);
            if a["status"] != "resolved" || b["status"] != "resolved" {
                continue;
            }
            let a: Vec3 = serde_json::from_value(a["point"].clone()).unwrap();
            let b: Vec3 = serde_json::from_value(b["point"].clone()).unwrap();
            if (a.x - 100.).abs() < 1e-7
                && (a.y - 200.).abs() < 1e-7
                && (b.x - 100.).abs() < 1e-7
                && (b.y - 200.).abs() < 1e-7
                && (a.z - b.z).abs() > 19.9
            {
                return edge["reference"].clone();
            }
        }
    }
    panic!("real named extrusion must expose its verified vertical construction edge: {refs}");
}

#[test]
fn saved_native_reference_follows_real_extrude_rebuild_in_new_session_with_one_world_placement() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let (mut doc, feature) = reference_extrusion();
        let path = fixture.write("references.sc", container(&doc));
        let mut worker = fixture.worker();
        let loaded = worker.call("v1.load", json!({"path": path})).unwrap();
        let refs = worker.call("v1.references", json!({"source_sha256": loaded["source"]["sha256"]})).unwrap();
        assert_eq!(refs["protocol"], "cadcraft-references/1");
        assert_eq!(refs["placements"].as_array().unwrap().len(), 3);
        let reference = vertical_reference(&mut worker, &loaded, &refs);
        let before = reference_point(&mut worker, &loaded, &reference, 0.25);
        let before_point: Vec3 = serde_json::from_value(before["point"].clone()).unwrap();
        assert!((before_point.z - 5.).abs() < 1e-7 || (before_point.z - 15.).abs() < 1e-7);
        let FeatureKind::Extrude { extent, .. } = &mut doc.feature_mut(feature).unwrap().kind else { panic!("extrude") };
        extent.distance = "40".into();
        fs::write(&path, container(&doc)).unwrap();
        drop(worker);
        // Opaque tokens persist across worker restarts; no in-memory index registry or echo.
        let mut worker = fixture.worker();
        let rebuilt = worker.call("v1.load", json!({"path": path})).unwrap();
        assert_ne!(rebuilt["source"]["sha256"], loaded["source"]["sha256"]);
        let after = reference_point(&mut worker, &rebuilt, &reference, 0.25);
        assert_eq!(after["status"], "resolved", "{after}");
        assert_eq!(after["verification"], "verified_upstream_history");
        assert_eq!(after["cross_rebuild"], true);
        assert_eq!(after["baseline_source_sha256"], loaded["source"]["sha256"]);
        let p: Vec3 = serde_json::from_value(after["point"].clone()).unwrap();
        assert!((p.x - 100.).abs() < 1e-7 && (p.y - 200.).abs() < 1e-7, "{p:?}");
        assert!((p.z - 2. * before_point.z).abs() < 1e-7, "{p:?}");
        assert!(matches!(worker.call("v1.references", json!({"source_sha256": loaded["source"]["sha256"]})), Err(Error::StaleModel)));
    });
}

#[test]
fn native_references_never_jump_on_rename_delete_suppression_or_another_project_path() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let (doc, feature) = reference_extrusion();
        let path = fixture.write("lineage.sc", container(&doc));
        let mut worker = fixture.worker();
        let loaded = worker.call("v1.load", json!({"path": path})).unwrap();
        let refs = worker.call("v1.references", json!({"source_sha256": loaded["source"]["sha256"]})).unwrap();
        let reference = vertical_reference(&mut worker, &loaded, &refs);
        let other = fixture.write("different-project.sc", container(&doc));
        let other_load = worker.call("v1.load", json!({"path": other})).unwrap();
        let lost = reference_point(&mut worker, &other_load, &reference, 0.5);
        assert_eq!(lost["status"], "lost");
        assert!(lost.get("point").is_none());
        for (change, expected) in [("rename", "lost"), ("delete", "lost"), ("suppress", "suppressed"), ("occurrence", "lost"), ("sketch", "lost")] {
            let mut changed = doc.clone();
            match change {
                "rename" => changed.feature_mut(feature).unwrap().name = "Renamed feature".into(),
                "delete" => changed.features.retain(|f| f.id != feature),
                "suppress" => changed.feature_mut(feature).unwrap().suppressed = true,
                "occurrence" => changed.occurrences.clear(),
                "sketch" => {
                    let FeatureKind::Sketch { sketch, .. } =
                        &mut changed.features.iter_mut().find(|f| matches!(f.kind, FeatureKind::Sketch { .. })).unwrap().kind
                    else {
                        panic!("sketch")
                    };
                    sketch.add_point(solidcore_record::Vec2::new(99., 99.), None).unwrap();
                }
                _ => unreachable!(),
            }
            fs::write(&path, container(&changed)).unwrap();
            let rebuilt = worker.call("v1.load", json!({"path": path})).unwrap();
            let result = reference_point(&mut worker, &rebuilt, &reference, 0.5);
            assert_eq!(result["status"], expected, "{change}: {result}");
            assert!(result.get("point").is_none(), "{change}: {result}");
        }
    });
}

#[test]
fn native_reference_bounds_face_refusal_and_unverified_names_remain_honest() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let mut doc = Document::new("Unverified sphere");
        doc.add_feature(FeatureKind::Sphere { center: point_expr(Vec3::ZERO), radius: "5".into(), operation: Operation::NewBody }, Some("Sphere"))
            .unwrap();
        let path = fixture.write("sphere.sc", doc.to_json());
        let mut worker = fixture.worker();
        let loaded = worker.call("v1.load", json!({"path": path})).unwrap();
        let refs = worker.call("v1.references", json!({"source_sha256": loaded["source"]["sha256"]})).unwrap();
        for face in refs["placements"][0]["faces"].as_array().unwrap() {
            assert_eq!(face["verification"], "unverified_upstream_name");
            assert_eq!(face["cross_rebuild"], false);
        }
        let face = &refs["placements"][0]["faces"][0]["reference"];
        let refused = reference_point(&mut worker, &loaded, face, 0.5);
        assert_eq!(refused["status"], "unmeasurable");
        assert!(refused.get("point").is_none());
        let edge = refs["placements"][0]["edges"][0]["reference"].clone();
        assert!(edge.is_string(), "the real sphere must expose its unverified seam edge: {refs}");
        let FeatureKind::Sphere { radius, .. } = &mut doc.features[0].kind else { panic!("sphere") };
        *radius = "10".into();
        fs::write(&path, doc.to_json()).unwrap();
        let rebuilt = worker.call("v1.load", json!({"path": path})).unwrap();
        let lost = reference_point(&mut worker, &rebuilt, &edge, 0.5);
        assert_eq!(lost["status"], "lost");
        assert!(lost.get("point").is_none());
        for fraction in [-0.1, 1.1] {
            assert!(matches!(
                worker.call("v1.resolve_point", json!({"source_sha256": loaded["source"]["sha256"], "reference": face, "fraction": fraction})),
                Err(Error::Parameters(_))
            ));
        }
        assert!(matches!(
            worker.call("v1.resolve_point", json!({"source_sha256": rebuilt["source"]["sha256"], "reference": "x".repeat(8193), "fraction": 0.5})),
            Err(Error::Limit(_))
        ));
        assert!(matches!(
            worker.call("v1.references", json!({"source_sha256": loaded["source"]["sha256"], "unexpected": true})),
            Err(Error::Parameters(_))
        ));
    });
}

#[test]
fn native_circular_edge_fraction_is_on_trimmed_curve_not_its_rendered_chord() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let mut doc = Document::new("Exact circular reference");
        cylinder(&mut doc, 0, "Cylinder");
        let path = fixture.write("circular.sc", container(&doc));
        let mut worker = fixture.worker();
        let loaded = worker.call("v1.load", json!({"path": path})).unwrap();
        let refs = worker.call("v1.references", json!({"source_sha256": loaded["source"]["sha256"]})).unwrap();
        let mut curved = 0;
        for edge in refs["placements"][0]["edges"].as_array().unwrap() {
            let a = reference_point(&mut worker, &loaded, &edge["reference"], 0.);
            let b = reference_point(&mut worker, &loaded, &edge["reference"], 1.);
            let middle = reference_point(&mut worker, &loaded, &edge["reference"], 0.5);
            assert_eq!(middle["status"], "resolved", "{middle}");
            let a: Vec3 = serde_json::from_value(a["point"].clone()).unwrap();
            let b: Vec3 = serde_json::from_value(b["point"].clone()).unwrap();
            let p: Vec3 = serde_json::from_value(middle["point"].clone()).unwrap();
            if (a.z - b.z).abs() > 1e-8 {
                continue;
            }
            curved += 1;
            assert!((p.z - a.z).abs() < 1e-8);
            assert!((p.x * p.x + p.y * p.y - 25.).abs() < 1e-7);
            let chord = (a + b) * 0.5;
            assert!((p.x - chord.x).abs() + (p.y - chord.y).abs() > 1e-4, "a rim midpoint is not its chord midpoint");
        }
        assert!(curved > 0, "real native cylinder must contain a circular rim edge");
    });
}
