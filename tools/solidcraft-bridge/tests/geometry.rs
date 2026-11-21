use cadcraft_model_worker::{Error, Worker};
use serde_json::{Value, json};
use solidcore_record::{Seg2, Vec3};
use solidcraft_geom::{Vec3Geom, hlr};
use solidcraft_kernel::{ExportBody, StepHeader};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static SERIAL: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("cadcraft-model-worker-test-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
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

fn cylinder_step() -> String {
    let cylinder = solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 5.0, 20.0).unwrap();
    solidcraft_kernel::step_export_bodies(
        &[ExportBody { name: "analytic cylinder".into(), body: &cylinder, color: None }],
        &StepHeader {
            file_name: "cylinder.step".into(),
            description: "Original synthetic cylinder integration fixture".into(),
            ..Default::default()
        },
    )
    .unwrap()
}

fn measure(load: &Value) -> &Value {
    assert_eq!(load["bodies"][0]["measure"]["status"], "ok", "{load}");
    &load["bodies"][0]["measure"]["value"]
}

#[test]
fn step_cylinder_oblique_view_preserves_exact_ellipse_and_source_identity() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let path = fixture.write("cylinder.step", &cylinder_step());
        let mut worker = fixture.worker();
        let loaded = worker.call("v1.load", json!({"path": path})).unwrap();
        assert_eq!(loaded["units"]["length"], "mm");
        assert_eq!(loaded["checks_settled"], true);
        assert!(loaded["bodies"][0]["source_item"].as_u64().unwrap() > 0);
        let properties = measure(&loaded);
        let volume = properties["volume"].as_f64().unwrap();
        let area = properties["area"].as_f64().unwrap();
        assert!((volume - 500.0 * std::f64::consts::PI).abs() < 1e-5 * volume);
        assert!((area - 250.0 * std::f64::consts::PI).abs() < 1e-5 * area);
        assert_eq!(loaded["bodies"][0]["face_origin"].as_array().unwrap().len(), properties["faces"].as_u64().unwrap() as usize);
        let query = json!({"source_sha256": loaded["source"]["sha256"], "direction": [1,1,1], "up": [0,0,1]});
        let view = worker.call("v1.view", query.clone()).unwrap();
        let basis = hlr::View::new(Vec3::new(1., 1., 1.), Vec3::Z).unwrap();
        let mut conics = 0;
        for line in view["drawing"]["lines"].as_array().unwrap() {
            assert_eq!(line["placement"], 0);
            for piece in line["pieces"].as_array().unwrap() {
                assert!(piece.get("visibility").is_some() && piece.get("covered").is_some());
                for geometry in piece["geometry"].as_array().unwrap() {
                    if let Seg2::Conic { a, apex, b, w } = serde_json::from_value(geometry.clone()).unwrap() {
                        conics += 1;
                        assert_eq!(piece["accuracy"]["kind"], "analytic");
                        assert!(w > 0. && w <= 1.);
                        // A cap circle projects to an ellipse with semi-axes r and r/sqrt(3).
                        // Every rational midpoint must lie on either the lower or upper ellipse.
                        let midpoint = (a + apex * (2. * w) + b) / (2. + 2. * w);
                        let on_cap = |z| {
                            let center = basis.screen(Vec3::new(0., 0., z));
                            let x = midpoint.x - center[0];
                            let y = midpoint.y - center[1];
                            (x * x / 25. + y * y / (25. / 3.) - 1.).abs() < 1e-6
                        };
                        assert!(on_cap(0.) || on_cap(20.), "not a cap ellipse: {geometry}");
                    }
                }
            }
        }
        assert!(conics > 0, "raw HLR must retain noncircular conics: {view}");
        // Timing statistics are not deterministic; geometry, flags and provenance are.
        let repeated = worker.call("v1.view", query).unwrap();
        assert_eq!(view["drawing"]["lines"], repeated["drawing"]["lines"]);
        assert_eq!(view["drawing"]["unsupported"], repeated["drawing"]["unsupported"]);
        assert_eq!(view["quality"], repeated["quality"]);
        if view["quality"]["analytic_complete"] == false {
            assert!(matches!(
                worker.call(
                    "v1.view",
                    json!({"source_sha256": loaded["source"]["sha256"],
                "direction": [1,1,1], "up": [0,0,1], "require_analytic": true})
                ),
                Err(Error::Geometry(_))
            ));
        }
    });
}

#[test]
fn centimetre_step_is_normalized_and_source_change_invalidates_identity_scope() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let step = cylinder_step();
        assert!(step.contains("SI_UNIT(.MILLI.,.METRE.)"));
        let centimetres = step.replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT(.CENTI.,.METRE.)");
        let path = fixture.write("centimetres.stp", &centimetres);
        let mut worker = fixture.worker();
        let loaded = worker.call("v1.load", json!({"path": path})).unwrap();
        let properties = measure(&loaded);
        let volume = properties["volume"].as_f64().unwrap();
        assert!((volume - 500_000. * std::f64::consts::PI).abs() < 1e-5 * volume);
        assert_eq!(worker.call("v1.source_status", json!({})).unwrap()["changed"], false);
        fs::write(&path, format!("{centimetres}\n/* source edit */\n")).unwrap();
        let status = worker.call("v1.source_status", json!({})).unwrap();
        assert_eq!(status["changed"], true);
        assert_ne!(status["loaded"]["sha256"], status["current"]["sha256"]);
        assert!(matches!(
            worker.call(
                "v1.view",
                json!({"source_sha256": status["current"]["sha256"],
            "direction": [1,1,1], "up": [0,0,1]})
            ),
            Err(Error::StaleModel)
        ));
    });
}

#[test]
fn iges_uses_real_upstream_reader_and_exact_measures() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap();
        let text = solidcraft_kernel::iges_export_bodies(
            &[ExportBody { name: "box".into(), body: &body, color: None }],
            &StepHeader { description: "Original synthetic IGES box fixture".into(), ..Default::default() },
        )
        .unwrap();
        let path = fixture.write("box.igs", &text);
        let mut worker = fixture.worker();
        let loaded = worker.call("v1.load", json!({"path": path})).unwrap();
        assert_eq!(loaded["format"], "iges");
        assert!((measure(&loaded)["volume"].as_f64().unwrap() - 6000.).abs() < 1e-4);
        assert_eq!(loaded["metadata"]["density"], Value::Null);
        assert!(loaded["source_identity"].as_str().unwrap().contains("not_original_iges"));
    });
}

#[test]
fn paths_limits_and_failed_load_preserve_previous_model() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let other = Fixture::new();
        let outside = other.write("outside.step", &cylinder_step());
        let path = fixture.write("inside.step", &cylinder_step());
        let mut worker = fixture.worker();
        assert!(matches!(worker.call("v1.load", json!({"path": outside})), Err(Error::Path(_))));
        let loaded = worker.call("v1.load", json!({"path": path})).unwrap();
        let bad = fixture.write("bad.step", "ISO-10303-21; DATA; (((((((((((((((((((((((((((((((((;");
        assert!(matches!(worker.call("v1.load", json!({"path": bad})), Err(Error::Limit(_))));
        let status = worker.call("v1.source_status", json!({})).unwrap();
        assert_eq!(status["loaded"], loaded["source"]);
        assert!(matches!(
            worker.call(
                "v1.view",
                json!({"source_sha256": loaded["source"]["sha256"],
            "direction": [0,0,0], "up": [0,0,1]})
            ),
            Err(Error::Parameters(_))
        ));
        assert!(matches!(worker.call("v2.view", json!({})), Err(Error::Method(_))));
        let huge = fixture.0.join("huge.step");
        fs::File::create(&huge).unwrap().set_len(cadcraft_model_worker::MAX_FILE_BYTES as u64 + 1).unwrap();
        assert!(matches!(worker.call("v1.load", json!({"path": huge})), Err(Error::Limit(_))));
    });
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_escape_explicit_allowed_directory() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    let source = outside.write("source.step", "not parsed");
    let alias = fixture.0.join("alias.step");
    std::os::unix::fs::symlink(source, &alias).unwrap();
    assert!(matches!(fixture.worker().call("v1.load", json!({"path": alias})), Err(Error::Path(_))));
}

#[test]
fn display_preview_preserves_cylinder_shape_and_cannot_change_exact_projection() {
    kernel_test(|| {
        let fixture = Fixture::new();
        let path = fixture.write("preview-cylinder.step", &cylinder_step());
        let mut worker = fixture.worker();
        let loaded = worker.call("v1.load", json!({"path":path})).unwrap();
        let query = json!({"source_sha256":loaded["source"]["sha256"],"direction":[1,1,1],"up":[0,0,1],"require_analytic":true});
        let before = worker.call("v1.view", query.clone()).unwrap();
        let preview = worker.call("v1.preview", json!({"source_sha256":loaded["source"]["sha256"],"detail":4})).unwrap();
        assert_eq!(preview["purpose"], "display_only_not_measurement_or_projection");
        assert_eq!(preview["quality"]["bounds_certified"], false);
        assert_eq!(preview["quality"]["missing_faces"], 0);
        let body = &preview["bodies"][0];
        let positions: Vec<Vec3> = serde_json::from_value(body["positions"].clone()).unwrap();
        let triangles: Vec<[usize; 3]> = serde_json::from_value(body["triangles"].clone()).unwrap();
        let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for p in &positions {
            let radial = p.x.hypot(p.y);
            assert!(radial <= 5.0 + 1e-8 && p.z >= -1e-8 && p.z <= 20.0 + 1e-8);
            assert!(p.z.abs() < 1e-8 || (p.z - 20.0).abs() < 1e-8 || (radial - 5.0).abs() < 1e-8, "vertex not on a cylinder face: {p:?}");
            for (i, coordinate) in [p.x, p.y, p.z].into_iter().enumerate() {
                bounds[0][i] = bounds[0][i].min(coordinate);
                bounds[1][i] = bounds[1][i].max(coordinate);
            }
        }
        for (actual, expected) in bounds.into_iter().flatten().zip([-5.0, -5.0, 0.0, 5.0, 5.0, 20.0]) {
            assert!((actual - expected).abs() < 0.1, "preview lost cylinder extent: {actual} != {expected}");
        }
        for [a, b, c] in triangles {
            let (a, b, c) = (positions[a], positions[b], positions[c]);
            assert!((b - a).cross(c - a).len() > 1e-12, "degenerate triangle");
            let center = (a + b + c) / 3.0;
            let on_cap = center.z.abs() < 1e-8 || (center.z - 20.0).abs() < 1e-8;
            assert!(on_cap || (center.x.hypot(center.y) - 5.0).abs() < 0.1, "triangle not near a cylinder face");
        }
        for detail in [0, 9] {
            assert!(matches!(
                worker.call("v1.preview", json!({"source_sha256":loaded["source"]["sha256"],"detail":detail})),
                Err(Error::Parameters(_))
            ));
        }
        assert!(matches!(worker.call("v1.preview", json!({"source_sha256":"stale","detail":4})), Err(Error::StaleModel)));
        let after = worker.call("v1.view", query).unwrap();
        assert_eq!(after["drawing"]["lines"], before["drawing"]["lines"]);
        assert_eq!(after["quality"], before["quality"]);
    });
}

#[test]
fn neutral_reference_fraction_uses_real_exact_trim_and_changed_step_iges_payload_is_lost() {
    kernel_test(|| {
        let fixture = Fixture::new();
        for extension in ["step", "igs"] {
            let mut worker = fixture.worker();
            let make = |width| {
                let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(width, 20., 30.)).unwrap();
                let bodies = [ExportBody { name: "Reference box".into(), body: &body, color: None }];
                if extension == "step" {
                    solidcraft_kernel::step_export_bodies(&bodies, &StepHeader::default()).unwrap()
                } else {
                    solidcraft_kernel::iges_export_bodies(&bodies, &StepHeader::default()).unwrap()
                }
            };
            let path = fixture.write(&format!("reference.{extension}"), &make(10.));
            let loaded = worker.call("v1.load", json!({"path": path})).unwrap();
            let refs = worker.call("v1.references", json!({"source_sha256": loaded["source"]["sha256"]})).unwrap();
            let edge = &refs["placements"][0]["edges"][0];
            assert_eq!(edge["verification"], "payload_scoped");
            assert_eq!(edge["cross_rebuild"], false);
            let resolve = |worker: &mut Worker, loaded: &Value, t| {
                worker
                    .call("v1.resolve_point", json!({"source_sha256": loaded["source"]["sha256"], "reference": edge["reference"], "fraction": t}))
                    .unwrap()
            };
            let a: Vec3 = serde_json::from_value(resolve(&mut worker, &loaded, 0.)["point"].clone()).unwrap();
            let b: Vec3 = serde_json::from_value(resolve(&mut worker, &loaded, 1.)["point"].clone()).unwrap();
            let middle = resolve(&mut worker, &loaded, 0.25);
            assert_eq!(middle["status"], "resolved");
            let p: Vec3 = serde_json::from_value(middle["point"].clone()).unwrap();
            assert!((p - (a * 0.75 + b * 0.25)).len() < 1e-7);
            fs::write(&path, make(12.)).unwrap();
            let rebuilt = worker.call("v1.load", json!({"path": path})).unwrap();
            let lost = resolve(&mut worker, &rebuilt, 0.25);
            assert_eq!(lost["status"], "lost");
            assert!(lost.get("point").is_none());
        }
    });
}
