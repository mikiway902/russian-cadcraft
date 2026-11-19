use cadcraft_doc::EntityKind;
use cadcraft_geom::Vec2;
use serde_json::json;

use crate::Session;

fn session() -> Session {
    let mut s = crate::extensions::new_session();
    s.execute("gost.setup", &json!({})).unwrap();
    s
}

#[test]
fn small_circle_axes_are_continuous_and_ignore_current_entity_overrides() {
    let mut s = session();
    let c = s.execute("circle", &json!({"center":[20,20], "radius":5})).unwrap()["handle"].as_str().unwrap().to_owned();
    s.execute("setvar", &json!({"name":"CECOLOR", "value":1})).unwrap();
    s.execute("setvar", &json!({"name":"THICKNESS", "value":9})).unwrap();
    let result = s.execute("gost.center", &json!({"handles":[c]})).unwrap();
    let h = cadcraft_doc::Handle::parse_hex(result["handles"][0].as_str().unwrap()).unwrap();
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("expected centre object") };
    assert_eq!(i.insert.xy(), Vec2::new(20.0, 20.0));
    for e in s.doc().unwrap().block(&i.block).unwrap().entities.iter() {
        assert_eq!(e.common.layer, "GOST-THIN");
        assert_eq!(e.common.color, cadcraft_doc::color::Color::ByLayer);
        assert_eq!(e.common.thickness, 0.0);
        let EntityKind::Line(l) = &e.kind else { panic!("expected axis") };
        assert_eq!(l.a.xy(), Vec2::ZERO);
        assert!((l.a.xy().dist(l.b.xy()) - 8.0).abs() < 1e-9);
    }
}

#[test]
fn invalid_section_does_not_leave_marks_or_undo_steps() {
    let mut s = session();
    let doc = s.doc().unwrap().clone();
    let undo = s.state().unwrap().undo.len();
    for params in [json!({"p1":[0,0],"p2":[10,0]}), json!({"p1":[0,0],"p2":[100,0],"label":"О"}), json!({"p1":[0,0],"p2":[100,0],"reverse":"yes"})] {
        assert!(s.execute("gost.section", &params).is_err());
        assert_eq!(s.doc().unwrap(), &doc);
        assert_eq!(s.state().unwrap().undo.len(), undo);
    }
}

#[test]
fn section_is_open_and_arrow_tips_are_offset_from_external_ends() {
    let mut s = session();
    s.execute("gost.section", &json!({"p1":[0,0],"p2":[100,0],"label":"Б","caption":[50,-25]})).unwrap();
    let marks: Vec<_> = s.doc().unwrap().model.iter().filter(|e| e.common.layer == "GOST-SECTION").collect();
    assert_eq!(marks.len(), 2);
    for e in marks {
        let EntityKind::Line(l) = &e.kind else { panic!("expected stroke") };
        assert!((l.a.xy().dist(l.b.xy()) - 10.0).abs() < 1e-9);
    }
    let tips: Vec<_> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Solid(v) = &e.kind { Some(v.corners[0].xy()) } else { None }).collect();
    assert_eq!(tips, [Vec2::new(2.5, 0.0), Vec2::new(97.5, 0.0)]);
    assert!(s.doc().unwrap().model.iter().any(|e| matches!(&e.kind, EntityKind::Text(t) if t.value == "Б—Б")));
    s.undo().unwrap();
    assert!(s.doc().unwrap().model.is_empty());
}

#[test]
fn metal_hatch_has_actual_millimetre_spacing_and_tracks_its_boundary() {
    let mut s = session();
    let r = s.execute("rectang", &json!({"p1":[0,0], "p2":[40,20]})).unwrap();
    let boundary = r["handle"].as_str().unwrap();
    let out = s.execute("gost.hatch", &json!({"handles":[boundary], "spacing":4, "scale":2, "angle":-45})).unwrap();
    let h = cadcraft_doc::Handle::parse_hex(out["handles"][0].as_str().unwrap()).unwrap();
    let EntityKind::Hatch(hatch) = &s.doc().unwrap().model.get(h).unwrap().kind else { panic!("expected hatch") };
    assert!(hatch.associative);
    assert!(!hatch.loops[0].sources.is_empty());
    assert_eq!(hatch.pattern_lines[0].delta, (0.0, 4.0));
    assert_eq!(hatch.scale, 2.0);
    assert_eq!(hatch.angle, -45_f64.to_radians());
}

#[test]
fn dashed_axes_intersect_and_end_on_strokes_despite_global_linetype_scale() {
    let mut s = session();
    let c = s.execute("circle", &json!({"center":[0,0], "radius":20})).unwrap()["handle"].as_str().unwrap().to_owned();
    s.execute("setvar", &json!({"name":"LTSCALE", "value":7})).unwrap();
    let out = s.execute("gost.center", &json!({"handles":[c]})).unwrap();
    let h = cadcraft_doc::Handle::parse_hex(out["handles"][0].as_str().unwrap()).unwrap();
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("expected centre object") };
    let axes: Vec<_> = s.doc().unwrap().block(&i.block).unwrap().entities.iter().collect();
    for tip in [Vec2::new(23.0, 0.0), Vec2::new(-23.0, 0.0), Vec2::new(0.0, 23.0), Vec2::new(0.0, -23.0)] {
        assert!(axes.iter().any(|e| matches!(&e.kind, EntityKind::Line(l) if l.b.xy() == tip)));
    }
    assert_eq!(axes.iter().filter(|e| matches!(&e.kind, EntityKind::Line(l) if l.a.xy() == Vec2::ZERO)).count(), 4);
    assert!(axes.iter().all(|e| e.common.linetype == "Continuous"));
}
