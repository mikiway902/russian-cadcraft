#![cfg(feature = "gost")]

use cadcraft_engine::{
    doc::{Drawing, EntityKind, Space},
    fonts,
};
use serde_json::json;

#[test]
fn fresh_session_has_offline_gost_font_sheet_and_clean_undo_baseline() {
    let mut s = cadcraft_engine::extensions::new_session();
    assert_eq!(s.doc().unwrap().header.i64("INSUNITS", 0), 4);
    assert_eq!(s.doc().unwrap().header.str("DIMSTYLE", ""), "GOST");
    assert_eq!(s.doc().unwrap().text_style("GOST").unwrap().font, fonts::GOST_FONT);
    let face = fonts::ttf::find(fonts::GOST_FONT).unwrap();
    for c in "АБВГабвг0123456789⌀±°".chars() {
        assert!(face.has_glyph(c), "missing {c}");
    }
    let l = s.doc().unwrap().layout("GOST1").unwrap();
    assert_eq!(s.doc().unwrap().layouts.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["GOST1"]);
    assert_eq!((l.page.width_mm, l.page.height_mm), (210.0, 297.0));
    assert!(!l.page.scale_to_fit);
    assert!(!s.state().unwrap().is_dirty());
    assert!(s.state().unwrap().undo.is_empty());
    let r = s.execute("dimlinear", &json!({"p1":[0,0],"p2":[12.5,0],"at":[5,10]})).unwrap();
    let h = cadcraft_engine::doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let EntityKind::Dimension(dim) = &s.doc().unwrap().model.get(h).unwrap().kind else { panic!("dimension missing") };
    let geom = cadcraft_render::dimension_in(s.doc().unwrap(), dim);
    assert_eq!(geom.value, "12,5");
    s.undo().unwrap();
    assert!(s.doc().unwrap().layout("GOST1").is_some());
    assert!(s.doc().unwrap().model.is_empty());
}

#[test]
fn new_uses_paper_gost_but_open_and_explicit_imperial_are_not_converted() {
    let mut s = cadcraft_engine::extensions::new_session();
    s.execute("new", &json!({})).unwrap();
    assert_eq!(s.space(), Space::Paper("GOST1".into()));
    assert!(!s.state().unwrap().is_dirty());
    s.execute("new", &json!({"metric":false})).unwrap();
    assert_ne!(s.doc().unwrap().header.i64("INSUNITS", 0), 4);
    let foreign = Drawing::new_imperial();
    s.open_drawing(foreign.clone(), "Foreign", None);
    assert_eq!(s.doc().unwrap(), &foreign);
}

#[test]
fn ordinary_hatch_draws_three_mm_spacing_without_a_gost_command() {
    let mut s = cadcraft_engine::extensions::new_session();
    let boundary = s.execute("rectang", &json!({"p1":[0,0],"p2":[40,70]})).unwrap()["handle"].clone();
    let result = s.execute("hatch", &json!({"handles":[boundary]})).unwrap();
    let h = cadcraft_engine::doc::Handle::parse_hex(result["handles"][0].as_str().unwrap()).unwrap();
    let d = s.doc().unwrap();
    let entity = d.model.get(h).unwrap();
    assert_eq!(entity.common.layer, "GOST-HATCH");
    let display = cadcraft_render::build(d, &Space::Model, &cadcraft_render::Options::default());
    let mut offsets: Vec<_> = display
        .prims
        .iter()
        .filter(|prim| prim.handle == h)
        .map(|prim| {
            let p = display.points(prim)[0];
            (p.y - p.x) / std::f64::consts::SQRT_2
        })
        .collect();
    offsets.sort_by(f64::total_cmp);
    assert!(offsets.get(1).is_some(), "hatch needs multiple visible parallel lines");
    for pair in offsets.windows(2) {
        assert!((pair[1] - pair[0] - 3.0).abs() < 1e-6, "actual paper spacing: {}", pair[1] - pair[0]);
    }
}
