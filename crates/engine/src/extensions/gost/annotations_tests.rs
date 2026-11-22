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

#[test]
fn designation_alphabet_rejects_every_excluded_letter_before_any_mutation() {
    let mut s = session();
    for label in ["Ё", "З", "Й", "О", "Х", "Ч", "Ъ", "Ы", "Ь", "A", "АБ", "АA", "А-1", "А1Б", "А1.2", "А4294967296"] {
        for (command, params) in [("gost.section", json!({"p1":[0,0],"p2":[100,0],"label":label})), ("gost.view", json!({"at":[0,0],"label":label}))]
        {
            let before = s.doc().unwrap().clone();
            let undo = s.state().unwrap().undo.len();
            let result = if command == "gost.section" { super::section(&mut s, &params) } else { super::view(&mut s, &params) };
            assert!(result.is_err(), "{command}: {label}");
            assert_eq!(s.doc().unwrap(), &before, "direct consumer must validate before mutation, without relying on command rollback");
            assert_eq!(s.state().unwrap().undo.len(), undo);
        }
    }
    s.execute("gost.profile", &json!({"action":"set","id":"rf-2026-v1","alphabet":"latin"})).unwrap();
    for label in ["I", "O", "V", "X", "А", "WА"] {
        let before = s.doc().unwrap().clone();
        assert!(super::view(&mut s, &json!({"at":[0,0],"label":label})).is_err(), "{label}");
        assert_eq!(s.doc().unwrap(), &before);
    }
    s.execute("gost.view", &json!({"at":[0,0],"label":"W"})).unwrap();
    let section = s.execute("gost.section", &json!({"p1":[0,0],"p2":[100,0]})).unwrap();
    assert_eq!(section["designation"], "A—A", "default letter must follow explicitly selected alphabet");
}

#[test]
fn designation_height_tracks_dimension_numbers_once_in_model_and_not_view_scale_in_paper() {
    use cadcraft_doc::{Handle, Space};
    let mut s = session();
    for scale in [0.5, 2.0, 4.0] {
        s.execute("gost.setup", &json!({"scale":scale})).unwrap();
        let dimension = s.execute("dimlinear", &json!({"p1":[0,0],"p2":[120.5,0],"at":[60,30]})).unwrap();
        let dim = Handle::parse_hex(dimension["handle"].as_str().unwrap()).unwrap();
        let label = s.execute("gost.view", &json!({"at":[0,50],"label":"Б","viewScale":"1:2"})).unwrap();
        let label = Handle::parse_hex(label["handle"].as_str().unwrap()).unwrap();
        let d = s.doc().unwrap();
        let EntityKind::Dimension(dim) = &d.entity(dim).unwrap().kind else { panic!() };
        let numbers = cadcraft_render::dimension_in(d, dim);
        let EntityKind::Text(text) = &d.entity(label).unwrap().kind else { panic!() };
        assert_eq!(numbers.measurement, 120.5);
        assert_eq!(numbers.text_height, 3.5 * scale);
        assert_eq!(text.height, numbers.text_height * 2.0);
    }
    s.state_mut().unwrap().space = Space::Paper("GOST1".into());
    let label = s.execute("gost.view", &json!({"at":[50,100],"label":"Б","viewScale":"1:10"})).unwrap();
    let h = Handle::parse_hex(label["handle"].as_str().unwrap()).unwrap();
    let EntityKind::Text(text) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
    assert_eq!(text.height, 7.0, "paper lettering must not inherit the model annotation scale or represented view scale");
    let before = s.doc().unwrap().clone();
    assert!(super::view(&mut s, &json!({"at":[0,0],"label":"Б","scale":10})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn designation_uses_actual_dimension_style_height_and_keeps_clear_of_source_arrow() {
    let mut s = session();
    let d = s.doc_mut().unwrap();
    d.dim_styles.iter_mut().find(|style| style.name == "GOST").unwrap().text_height = 5.0;
    d.sync_dim_vars();
    let result = s.execute("gost.section", &json!({"p1":[0,0],"p2":[100,0],"label":"Б"})).unwrap();
    let labels: Vec<_> = result["handles"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|value| value.as_str().and_then(cadcraft_doc::Handle::parse_hex))
        .filter_map(|h| match &s.doc().unwrap().entity(h).unwrap().kind {
            EntityKind::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(labels.len(), 2);
    for label in labels {
        assert_eq!(label.height, 10.0, "designation must follow the actual 5 mm dimension numbers, not a hardcoded 5/7 mm caption");
        assert!(label.insert.y + label.height <= -13.0, "lettering must stay at least 3 mm below the arrow shaft starting at y=-10");
    }
}

fn created_view_letter<'a>(s: &'a Session, out: &serde_json::Value) -> std::borrow::Cow<'a, str> {
    let handle = cadcraft_doc::Handle::parse_hex(out["handle"].as_str().unwrap()).unwrap();
    match &s.doc().unwrap().entity(handle).unwrap().kind {
        EntityKind::Text(text) => text.value.as_str().into(),
        EntityKind::MText(text) => cadcraft_fonts::plain_mtext(&text.contents).into(),
        _ => panic!("designation text required"),
    }
}

#[test]
fn automatic_letters_share_native_designations_and_formatted_mtext_across_dxf_and_undo() {
    let mut s = session();
    let section = s.execute("gost.section", &json!({"p1":[0,0],"p2":[100,0],"caption":[50,-25]})).unwrap();
    assert_eq!(section["designation"], "А—А");
    let view = s.execute("gost.view", &json!({"at":[50,50]})).unwrap();
    assert_eq!(created_view_letter(&s, &view), "Б");
    s.execute("mtext", &json!({"at":[0,70],"text":"{\\C1;В — В (1:2)}\\PГ"})).unwrap();
    s.state_mut().unwrap().space = cadcraft_doc::Space::Paper("GOST1".into());
    s.execute("gost.view", &json!({"at":[50,50],"label":"Е"})).unwrap();
    s.state_mut().unwrap().space = cadcraft_doc::Space::Model;
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(reopened, "designation DXF", None);
    let next = s.execute("gost.view", &json!({"at":[80,50]})).unwrap();
    assert_eq!(created_view_letter(&s, &next), "Д");
    s.execute("undo", &json!({})).unwrap();
    let reused_free = s.execute("gost.view", &json!({"at":[90,50]})).unwrap();
    assert_eq!(created_view_letter(&s, &reused_free), "Д");
    let across_sheets = s.execute("gost.view", &json!({"at":[100,50]})).unwrap();
    assert_eq!(created_view_letter(&s, &across_sheets), "Ж", "existing designations on other sheets reserve the same document alphabet");
    let explicit = s.execute("gost.view", &json!({"at":[100,50],"label":"А"})).unwrap();
    assert_eq!(created_view_letter(&s, &explicit), "А");
}

#[test]
fn automatic_letters_skip_profile_exclusions_and_continue_with_lowered_indices_without_reuse() {
    for (alphabet, expected) in [("cyrillic", "АБВГДЕЖИКЛМНПРСТУФЦШЩЭЮЯ"), ("latin", "ABCDEFGHJKLMNPQRSTUWYZ")] {
        let mut s = session();
        s.execute("gost.profile", &json!({"action":"set","id":"rf-2026-v1","alphabet":alphabet})).unwrap();
        for (index, letter) in expected.chars().enumerate() {
            let out = s.execute("gost.view", &json!({"at":[index as f64 *10.,50]})).unwrap();
            assert_eq!(created_view_letter(&s, &out), letter.to_string());
        }
        let first = expected.chars().next().unwrap();
        let section = s.execute("gost.section", &json!({"p1":[0,0],"p2":[100,0],"caption":[50,-25]})).unwrap();
        assert_eq!(section["designation"], format!("{first}1—{first}1"));
        let caption = cadcraft_doc::Handle::parse_hex(section["handles"].as_array().unwrap().last().unwrap().as_str().unwrap()).unwrap();
        let EntityKind::MText(text) = &s.doc().unwrap().entity(caption).unwrap().kind else { panic!("lowered numeric indices require native MTEXT") };
        assert_eq!(cadcraft_fonts::plain_mtext(&text.contents), format!("{first}1—{first}1"));
        for (index, letter) in expected.chars().skip(1).enumerate() {
            let out = s.execute("gost.view", &json!({"at":[index as f64 *10.,80]})).unwrap();
            assert_eq!(created_view_letter(&s, &out), format!("{letter}1"));
        }
        let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
        s.open_drawing(reopened, "indexed designation DXF", None);
        let next = s.execute("gost.view", &json!({"at":[0,100]})).unwrap();
        assert_eq!(created_view_letter(&s, &next), format!("{first}2"));
        s.execute("undo", &json!({})).unwrap();
        let next = s.execute("gost.view", &json!({"at":[0,100]})).unwrap();
        assert_eq!(created_view_letter(&s, &next), format!("{first}2"));
        let reuse = expected.chars().next().unwrap().to_string();
        let out = s.execute("gost.view", &json!({"at":[0,80],"label":reuse})).unwrap();
        assert_eq!(created_view_letter(&s, &out), reuse);
    }
}

#[test]
fn designation_scan_budget_refuses_without_marks_and_explicit_letter_remains_available() {
    let mut s = session();
    s.doc_mut()
        .unwrap()
        .add(
            &cadcraft_doc::Space::Model,
            cadcraft_doc::Common::default(),
            EntityKind::MText(cadcraft_doc::MText {
                insert: cadcraft_geom::Vec3::ZERO,
                height: 3.5,
                width: 0.,
                attach: 1,
                rotation: 0.,
                style: "GOST".into(),
                contents: "x".repeat(8 * 1024 * 1024 + 1),
                line_spacing: 1.,
                line_spacing_exact: false,
                columns: None,
            }),
        )
        .unwrap();
    let before = s.doc().unwrap().clone();
    assert!(s.execute("gost.section", &json!({"p1":[0,0],"p2":[100,0]})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    let explicit = s.execute("gost.view", &json!({"at":[50,50],"label":"А"})).unwrap();
    assert_eq!(created_view_letter(&s, &explicit), "А");
}
