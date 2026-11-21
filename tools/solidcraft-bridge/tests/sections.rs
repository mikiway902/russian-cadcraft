use cadcraft_model_worker::{Error, Worker};
use serde_json::{Value, json};
use solidcore_record::{Region2, Seg2, Vec3};
use solidcraft_geom::{Loop2Geom, Seg2Geom, Vec3Geom, hlr};
use solidcraft_kernel::{Body, BoolOp, ExportBody, StepHeader, history::FaceOrigin};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static SERIAL: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("cadcraft-section-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn load(&self, bodies: &[(&str, &Body)]) -> (Worker, Value) {
        let exports: Vec<_> = bodies.iter().map(|(name, body)| ExportBody { name: (*name).into(), body, color: None }).collect();
        let step = solidcraft_kernel::step_export_bodies(&exports, &StepHeader { file_name: "sections.step".into(), ..Default::default() }).unwrap();
        let path = self.0.join("sections.step");
        fs::write(&path, step).unwrap();
        let mut worker = Worker::new(vec![self.0.clone()]).unwrap();
        let loaded = worker.call("v1.load", json!({"path":path})).unwrap();
        (worker, loaded)
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

fn query(loaded: &Value, z: f64, side: &str) -> Value {
    json!({"source_sha256":loaded["source"]["sha256"],"plane":{"origin":[0,0,z],"normal":[0,0,1]},
        "kept_side":side,"direction":if side == "positive" { [0,0,1] } else { [0,0,-1] },"up":[0,1,0]})
}

fn annulus() -> Body {
    let outer = solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 5.0, 20.0).unwrap();
    let bore = solidcraft_kernel::cylinder(Vec3::new(0., 0., -1.), Vec3::Z, 2.0, 22.0).unwrap();
    solidcraft_kernel::boolean(&outer, &bore, BoolOp::Cut).unwrap().unwrap()
}

fn geometry_snapshot(worker: &mut Worker, loaded: &Value) -> Value {
    let view = worker.call("v1.view", json!({"source_sha256":loaded["source"]["sha256"],"direction":[1,1,1],"up":[0,0,1]})).unwrap();
    json!({"lines":view["drawing"]["lines"],"unsupported":view["drawing"]["unsupported"],"quality":view["quality"],
        "references":worker.call("v1.references",json!({"source_sha256":loaded["source"]["sha256"]})).unwrap()})
}

#[test]
fn annular_cap_keeps_exact_hole_history_and_source_immutable() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let annulus = annulus();
        let (mut worker, loaded) = fixture.load(&[("annulus", &annulus)]);
        let before = geometry_snapshot(&mut worker, &loaded);
        let section = worker.call("v1.section", query(&loaded, 10., "positive")).unwrap();
        assert_eq!(section["protocol"], "cadcraft-section/1");
        assert_eq!(section["section"]["status"], "nonempty");
        assert_eq!(section["section"]["bodies"][0]["source_body"], 0);
        assert_eq!(section["section"]["bodies"][0]["result_body"], 0);
        assert_eq!(section["section"]["bodies"][0]["status"], "cut");
        let caps = section["section"]["caps"].as_array().unwrap();
        assert_eq!(caps.len(), 1, "{section}");
        let region: Region2 = serde_json::from_value(caps[0]["region"].clone()).unwrap();
        assert_eq!(region.holes.len(), 1);
        assert!((region.outer.signed_area() - 25. * std::f64::consts::PI).abs() < 1e-5);
        assert!((region.holes[0].signed_area() + 4. * std::f64::consts::PI).abs() < 1e-5);
        for ring in region.loops() {
            assert!(ring.is_closed(1e-5));
            assert!(ring.segs.iter().all(|seg| matches!(seg, Seg2::Arc { .. } | Seg2::Conic { .. })), "caps must not be polygonized: {region:?}");
        }
        let origins: Vec<FaceOrigin> = serde_json::from_value(caps[0]["provenance"]["face_origins"].clone()).unwrap();
        assert!(!origins.is_empty() && origins.iter().all(|origin| matches!(origin, FaceOrigin::Face { operand: 1, .. })));
        assert_eq!(caps[0]["provenance"]["topology_identity"], "section_result_local_only");
        // Strict mode must preserve the ordinary HLR acceptance policy, never hide uncertainty.
        let mut strict = query(&loaded, 10., "positive");
        strict["require_analytic"] = json!(true);
        if section["quality"]["analytic_complete"] == true {
            let accepted = worker.call("v1.section", strict).unwrap();
            assert_eq!(accepted["section"]["caps"], section["section"]["caps"]);
        } else {
            assert!(matches!(worker.call("v1.section", strict), Err(Error::Geometry(_))));
        }
        assert_eq!(geometry_snapshot(&mut worker, &loaded), before);
    });
}

#[test]
fn tangent_and_complete_removal_never_reuse_old_caps_or_body() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let body = solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 5., 20.).unwrap();
        let (mut worker, loaded) = fixture.load(&[("cylinder", &body)]);
        let before = geometry_snapshot(&mut worker, &loaded);
        let actual_cut = worker.call("v1.section", query(&loaded, 10., "positive")).unwrap();
        assert!(!actual_cut["section"]["caps"].as_array().unwrap().is_empty());
        for (z, side) in [(0., "positive"), (20., "negative")] {
            let touching = worker.call("v1.section", query(&loaded, z, side)).unwrap();
            assert_eq!(touching["section"]["status"], "nonempty");
            assert!(touching["section"]["caps"].as_array().unwrap().is_empty(), "original coplanar face is not a cut: {touching}");
            assert_eq!(touching["section"]["bodies"][0]["status"], "retained");
        }
        let removed = worker.call("v1.section", query(&loaded, 21., "positive")).unwrap();
        assert_eq!(removed["section"]["status"], "empty");
        assert_eq!(removed["section"]["bodies"][0]["status"], "removed");
        assert!(removed["section"]["bodies"][0]["result_body"].is_null());
        assert!(removed["section"]["caps"].as_array().unwrap().is_empty());
        assert!(removed["drawing"]["lines"].as_array().unwrap().is_empty());
        assert!(removed["prepare_reports"].as_array().unwrap().is_empty());
        assert_eq!(geometry_snapshot(&mut worker, &loaded), before);
    });
}

#[test]
fn oblique_plane_retains_exact_curves_or_explicitly_refuses_without_mutation() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let body = solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 5., 20.).unwrap();
        let (mut worker, loaded) = fixture.load(&[("cylinder", &body)]);
        let before = geometry_snapshot(&mut worker, &loaded);
        let request = json!({"source_sha256":loaded["source"]["sha256"],"plane":{"origin":[0,0,10],"normal":[1,0,1]},
            "kept_side":"positive","direction":[1,0,1],"up":[0,1,0]});
        match worker.call("v1.section", request) {
            Ok(section) => {
                let caps = section["section"]["caps"].as_array().unwrap();
                assert_eq!(caps.len(), 1, "a through-cut cannot silently lose its cap: {section}");
                let region: Region2 = serde_json::from_value(caps[0]["region"].clone()).unwrap();
                let view = hlr::View::new(Vec3::new(1., 0., 1.), Vec3::Y).unwrap();
                let normal = Vec3::new(1., 0., 1.).normalized().unwrap();
                assert!(
                    region.outer.segs.iter().any(|s| matches!(s, Seg2::Conic { .. } | Seg2::Arc { .. })),
                    "oblique exact conic required: {region:?}"
                );
                for segment in &region.outer.segs {
                    for t in [0., 0.25, 0.5, 0.75, 1.] {
                        let p = segment.point_at(t);
                        let world = view.x * p.x + view.y * p.y + normal * (10. / 2_f64.sqrt());
                        assert!((world.x * world.x + world.y * world.y - 25.).abs() < 1e-4, "{world:?}");
                        assert!((world.x + world.z - 10.).abs() < 1e-8);
                    }
                }
            }
            Err(Error::Geometry(reason)) => assert!(reason.contains("section"), "refusal must identify the actual section operation: {reason}"),
            Err(error) => panic!("oblique cut must produce exact curves or an explicit geometry refusal: {error}"),
        }
        assert_eq!(geometry_snapshot(&mut worker, &loaded), before);
    });
}

#[test]
fn result_mapping_preserves_original_indices_for_removed_retained_and_excluded_bodies() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let removed = solidcraft_kernel::box_solid(Vec3::new(0., 0., -10.), Vec3::new(4., 4., -5.)).unwrap();
        let crossing = solidcraft_kernel::box_solid(Vec3::new(10., 0., -5.), Vec3::new(14., 4., 5.)).unwrap();
        let retained = solidcraft_kernel::box_solid(Vec3::new(20., 0., 5.), Vec3::new(24., 4., 10.)).unwrap();
        let excluded = solidcraft_kernel::box_solid(Vec3::new(30., 0., -5.), Vec3::new(34., 4., 5.)).unwrap();
        let (mut worker, loaded) = fixture.load(&[("removed", &removed), ("crossing", &crossing), ("retained", &retained), ("excluded", &excluded)]);
        let input = loaded["bodies"].as_array().unwrap();
        let index = |name: &str| input.iter().position(|body| body["name"] == name).unwrap();
        let mut request = query(&loaded, 0., "positive");
        request["exclude_bodies"] = json!([index("excluded")]);
        let section = worker.call("v1.section", request).unwrap();
        let dispositions = section["section"]["bodies"].as_array().unwrap();
        assert_eq!(dispositions.len(), input.len());
        for (i, disposition) in dispositions.iter().enumerate() {
            assert_eq!(disposition["source_body"], i);
            assert_eq!(disposition["source_placement"], i);
        }
        assert_eq!(dispositions[index("removed")]["status"], "removed");
        assert_eq!(dispositions[index("excluded")]["status"], "excluded");
        assert!(dispositions[index("removed")]["result_body"].is_null());
        assert!(dispositions[index("excluded")]["result_body"].is_null());
        assert_eq!(dispositions[index("crossing")]["status"], "cut");
        assert_eq!(dispositions[index("retained")]["status"], "retained");
        let results: Vec<_> = dispositions.iter().filter_map(|item| item["result_body"].as_u64()).collect();
        assert_eq!(results, vec![0, 1]);
        assert_eq!(section["section"]["caps"][0]["source_body"], index("crossing"));
        for line in section["drawing"]["lines"].as_array().unwrap() {
            let placement = line["placement"].as_u64().unwrap();
            assert!(results.contains(&placement));
        }
        // Omitting all bodies is an explicit empty scene, not a request for the cached full view.
        let mut all_excluded = query(&loaded, 0., "positive");
        all_excluded["exclude_bodies"] = json!((0..input.len()).collect::<Vec<_>>());
        let empty = worker.call("v1.section", all_excluded).unwrap();
        assert_eq!(empty["section"]["status"], "empty");
        assert!(empty["drawing"]["lines"].as_array().unwrap().is_empty());
        assert!(empty["section"]["bodies"].as_array().unwrap().iter().all(|body| body["status"] == "excluded"));
    });
}

#[test]
fn hostile_section_parameters_and_failed_calls_are_atomic() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let body = annulus();
        let (mut worker, loaded) = fixture.load(&[("annulus", &body)]);
        let before = geometry_snapshot(&mut worker, &loaded);
        let valid = query(&loaded, 10., "positive");
        let mut stale = valid.clone();
        stale["source_sha256"] = json!("0".repeat(64));
        assert!(matches!(worker.call("v1.section", stale), Err(Error::StaleModel)));
        let mut cases = Vec::new();
        for normal in [json!([0, 0, 0]), json!([1e7, 0, 1]), json!([null, 0, 1])] {
            let mut case = valid.clone();
            case["plane"]["normal"] = normal;
            cases.push(case);
        }
        for direction in [json!([0, 0, -1]), json!([1, 0, 1]), json!([0, 0, 0])] {
            let mut case = valid.clone();
            case["direction"] = direction;
            cases.push(case);
        }
        for indices in [json!([0, 0]), json!([1]), json!([-1])] {
            let mut case = valid.clone();
            case["exclude_bodies"] = indices;
            cases.push(case);
        }
        let mut case = valid.clone();
        case["plane"]["origin"] = json!([0, 0, 1e7]);
        cases.push(case);
        let mut case = valid.clone();
        case["kept_side"] = json!("nearest");
        cases.push(case);
        let mut case = valid.clone();
        case["plane"]["extra"] = json!(true);
        cases.push(case);
        for case in cases {
            assert!(matches!(worker.call("v1.section", case), Err(Error::Parameters(_))));
        }
        assert_eq!(geometry_snapshot(&mut worker, &loaded), before);
        let cap = worker.call("v1.section", valid).unwrap();
        assert_eq!(cap["section"]["caps"][0]["region"]["holes"].as_array().unwrap().len(), 1);
    });
}

#[test]
fn native_suppression_and_world_placements_do_not_renumber_source_bodies() {
    kernel_test(|| {
        use solidcraft_doc::{Dir, Document, FeatureKind, Operation, point_expr, rigid};
        let fixture = Fixture::new();
        let mut doc = Document::new("Native section placement mapping");
        let component = doc.add_component(Some("Part"), 0).unwrap();
        let mut features = Vec::new();
        for name in ["Suppressed producer", "Live cylinder"] {
            let feature = doc
                .add_feature(
                    FeatureKind::Cylinder {
                        base: point_expr(Vec3::ZERO),
                        axis: Dir(Vec3::Z),
                        radius: "5".into(),
                        height: "20".into(),
                        operation: Operation::NewBody,
                        references: vec![],
                    },
                    Some(name),
                )
                .unwrap();
            let item = doc.features.iter_mut().find(|item| item.id == feature).unwrap();
            item.component = component;
            item.body_names = vec![name.into()];
            features.push(feature);
        }
        doc.features.iter_mut().find(|item| item.id == features[0]).unwrap().suppressed = true;
        let first = doc.occurrences.iter_mut().find(|item| item.component == component).unwrap();
        first.transform = rigid(Vec3::new(0., 0., -30.), Vec3::ZERO, Vec3::Z, 0.);
        doc.add_occurrence(component, 0, rigid(Vec3::new(0., 0., 0.), Vec3::ZERO, Vec3::Z, 0.)).unwrap();
        let path = fixture.0.join("placed.sc");
        fs::write(&path, doc.to_json()).unwrap();
        let mut worker = Worker::new(vec![fixture.0.clone()]).unwrap();
        let loaded = worker.call("v1.load", json!({"path":path})).unwrap();
        let bodies = loaded["bodies"].as_array().unwrap();
        assert_eq!(bodies.len(), 2);
        assert!(loaded["features"].as_array().unwrap().iter().any(|item| item["suppressed"] == true));
        assert!(bodies.iter().all(|body| body["native_identity"]["feature"] == features[1]));
        let before = geometry_snapshot(&mut worker, &loaded);
        let section = worker.call("v1.section", query(&loaded, 10., "positive")).unwrap();
        let dispositions = section["section"]["bodies"].as_array().unwrap();
        let removed = dispositions.iter().find(|body| body["status"] == "removed").unwrap();
        let cut = dispositions.iter().find(|body| body["status"] == "cut").unwrap();
        let removed_source = removed["source_body"].as_u64().unwrap() as usize;
        let cut_source = cut["source_body"].as_u64().unwrap() as usize;
        assert_ne!(removed_source, cut_source);
        assert_ne!(bodies[removed_source]["native_identity"]["occurrence_path"], bodies[cut_source]["native_identity"]["occurrence_path"]);
        assert_eq!(cut["result_body"], 0);
        assert!(removed["result_body"].is_null());
        assert_eq!(section["section"]["caps"][0]["source_body"], cut_source);
        assert_eq!(section["section"]["caps"][0]["result_body"], 0);
        assert!(section["drawing"]["lines"].as_array().unwrap().iter().all(|line| line["placement"] == 0));
        assert_eq!(geometry_snapshot(&mut worker, &loaded), before);
    });
}

#[test]
fn original_coplanar_step_face_is_not_a_cut_cap() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let crossing = solidcraft_kernel::box_solid(Vec3::new(-10., 0., 0.), Vec3::new(10., 10., 5.)).unwrap();
        let touching = solidcraft_kernel::box_solid(Vec3::new(0., 0., 0.), Vec3::new(10., 10., 10.)).unwrap();
        let step = solidcraft_kernel::boolean(&crossing, &touching, BoolOp::Union).unwrap().unwrap();
        let (mut worker, loaded) = fixture.load(&[("stepped block", &step)]);
        let section = worker
            .call(
                "v1.section",
                json!({"source_sha256":loaded["source"]["sha256"],
            "plane":{"origin":[0,0,0],"normal":[1,0,0]},"kept_side":"positive","direction":[1,0,0],"up":[0,0,1]}),
            )
            .unwrap();
        let regions: Vec<Region2> =
            section["section"]["caps"].as_array().unwrap().iter().map(|cap| serde_json::from_value(cap["region"].clone()).unwrap()).collect();
        let area: f64 = regions.iter().map(|region| region.outer.signed_area() + region.holes.iter().map(Loop2Geom::signed_area).sum::<f64>()).sum();
        assert!((area - 50.).abs() < 1e-6, "only the crossed lower step is cut, not the original upper face: {section}");
    });
}

#[test]
fn selected_open_body_rejects_whole_section_and_exclusion_is_explicit() {
    kernel_test(|| {
        use solidcore_record::{Plane, Vec2};
        let fixture = Fixture::new();
        let solid = solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 5., 20.).unwrap();
        let sheet =
            solidcraft_kernel::extrude_sheet(&Plane::XY, &[vec![Seg2::Line { a: Vec2::new(20., 0.), b: Vec2::new(30., 0.) }]], 0., 20.).unwrap();
        let (mut worker, loaded) = fixture.load(&[("closed cylinder", &solid), ("open sheet", &sheet)]);
        let bodies = loaded["bodies"].as_array().unwrap();
        let open = bodies.iter().position(|body| body["closed"] == false).unwrap();
        let before = geometry_snapshot(&mut worker, &loaded);
        let error = worker.call("v1.section", query(&loaded, 10., "positive")).unwrap_err();
        assert!(matches!(&error,Error::Geometry(reason) if reason.contains("open, refused or mesh-only")));
        assert_eq!(geometry_snapshot(&mut worker, &loaded), before, "a late geometry refusal must not publish an earlier body's partial cut");
        let mut excluded = query(&loaded, 10., "positive");
        excluded["exclude_bodies"] = json!([open]);
        let result = worker.call("v1.section", excluded).unwrap();
        assert_eq!(result["section"]["bodies"][open]["status"], "excluded");
        assert!(result["section"]["bodies"][open]["result_body"].is_null());
        let caps = result["section"]["caps"].as_array().unwrap();
        assert_eq!(caps.len(), 1);
        assert_ne!(caps[0]["source_body"], open);
        assert_eq!(geometry_snapshot(&mut worker, &loaded), before);
    });
}
