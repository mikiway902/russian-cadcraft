use super::*;
use cadcraft_doc::{Drawing, Handle};
use cadcraft_geom::Bounds2;

fn session() -> Session {
    let mut s = Session::empty();
    s.new_drawing(false);
    s.execute("line", &json!({"points": [[0,0], [10,10]]})).unwrap();
    s
}

fn insert<'a>(d: &'a Drawing, layout: &str) -> &'a Insert {
    let e = d.layout(layout).unwrap().entities.iter().next().unwrap();
    match &e.kind {
        EntityKind::Insert(i) => i,
        _ => panic!("sheet must be an editable block reference"),
    }
}

fn attr<'a>(ins: &'a Insert, tag: &str) -> &'a Text {
    &ins.attribs.iter().find(|a| a.tag == tag).unwrap().text
}

fn has_line(block: &Block, a: (f64, f64), b: (f64, f64), layer: &str) -> bool {
    block.entities.iter().any(|e| {
        e.common.layer == layer && matches!(&e.kind, EntityKind::Line(l) if l.a.xy() == Vec2::new(a.0,a.1) && l.b.xy() == Vec2::new(b.0,b.1))
    })
}

#[test]
fn a4_exact_page_frame_and_2023_form_one_geometry() {
    let mut s = session();
    let r = s.execute(CMD, &json!({"layout": "Drawing", "scale": 20})).unwrap();
    let d = s.doc().unwrap();
    let l = d.layout("Drawing").unwrap();
    assert_eq!(l.page.width_mm, 210.0);
    assert_eq!(l.page.height_mm, 297.0);
    assert_eq!(l.page.margins_mm, [0.0; 4]);
    assert!(!l.page.landscape && !l.page.scale_to_fit && !l.page.center && l.page.paper_mm);
    assert_eq!(l.page.scale, 1.0);
    assert_eq!(l.page.plot_area, "layout");
    assert_eq!(l.entities.len(), 1, "no arbitrary model viewport");
    assert_eq!(s.space(), Space::Paper("Drawing".into()));
    assert!(l.view.is_some());
    let sheet = cadcraft_render::sheet(d, "Drawing").unwrap();
    assert_eq!(sheet.size, Vec2::new(210.0, 297.0));
    assert_eq!(sheet.unit_mm, 1.0, "paper mm even in an imperial model");
    assert_eq!(sheet.printable, Bounds2::new(Vec2::ZERO, sheet.size));
    let b = d.block(r["block"].as_str().unwrap()).unwrap();
    assert!(has_line(b, (20.0, 5.0), (205.0, 5.0), "GOST-FRAME"));
    assert!(has_line(b, (205.0, 5.0), (205.0, 292.0), "GOST-FRAME"));
    assert!(has_line(b, (205.0, 292.0), (20.0, 292.0), "GOST-FRAME"));
    assert!(has_line(b, (20.0, 292.0), (20.0, 5.0), "GOST-FRAME"));
    assert!(has_line(b, (205.0, 60.0), (20.0, 60.0), "GOST-FRAME"), "185×55 title block");
    for x in [27.0, 37.0, 60.0, 75.0, 85.0] {
        assert!(has_line(b, (x, 35.0), (x, 60.0), "GOST-FRAME"), "7/10/23/15/10 revision columns");
    }
    assert!(has_line(b, (195.0, 45.0), (195.0, 60.0), "GOST-FRAME"), "2023 version box");
    assert!(has_line(b, (195.0, 55.0), (205.0, 55.0), "GOST-THIN"));
    let ins = insert(d, "Drawing");
    assert_eq!(attr(ins, "TITLE").height, 5.0, "model annotation scale cannot enlarge paper text");
    assert_eq!(attr(ins, "DESIGNATION_TOP").height, 7.0);
    assert_eq!(attr(ins, "DESIGNATION_TOP").rotation, std::f64::consts::PI);
    assert_eq!(attr(ins, "FORMAT").value, "A4");
    assert!(r["fields"].as_array().unwrap().iter().any(|t| t == "INVENTORY_SIGNATURE_DATE"));
    let rendered = cadcraft_render::build(d, &Space::Paper("Drawing".into()), &cadcraft_render::Options::default());
    assert!(!rendered.prims.is_empty());
    assert!(rendered.sheet.is_some());
    assert!(rendered.prims.iter().all(|p| p.handle != cadcraft_render::VIEWPORT_CONTENT));
    let pdf = cadcraft_io::pdf(d, &Space::Paper("Drawing".into()), &cadcraft_io::PdfOptions::default()).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
}

#[test]
fn subsequent_form_has_exact_fifteen_mm_grid_and_two_revision_rows() {
    let mut s = session();
    let r = s
        .execute(
            CMD,
            &json!({"layout": "Second", "format": "A3", "landscape": true, "form": "2a", "fields": {"designation": "АБВГ.123456.001", "sheet": "2"}}),
        )
        .unwrap();
    let d = s.doc().unwrap();
    assert_eq!((r["width"].as_f64().unwrap(), r["height"].as_f64().unwrap()), (420.0, 297.0));
    let b = d.block(r["block"].as_str().unwrap()).unwrap();
    assert!(has_line(b, (415.0, 20.0), (230.0, 20.0), "GOST-FRAME"));
    assert!(has_line(b, (395.0, 5.0), (395.0, 20.0), "GOST-FRAME"));
    assert!(has_line(b, (405.0, 5.0), (405.0, 20.0), "GOST-FRAME"));
    let ins = insert(d, "Second");
    assert_eq!(attr(ins, "SHEET").value, "2");
    assert_eq!(attr(ins, "DESIGNATION_TOP").value, attr(ins, "DESIGNATION").value);
    assert!(ins.attribs.iter().any(|a| a.tag == "REV2_DATE"));
    assert!(!ins.attribs.iter().any(|a| a.tag == "REV3_DATE" || a.tag == "TITLE"));
    assert_eq!(attr(ins, "DESIGNATION_TOP").rotation, std::f64::consts::PI);
}

#[test]
fn every_basic_format_has_exact_orientation_and_portrait_designation_placement() {
    let mut s = session();
    for (format, w, h) in [("A4", 210.0, 297.0), ("A3", 297.0, 420.0), ("A2", 420.0, 594.0), ("A1", 594.0, 841.0), ("A0", 841.0, 1189.0)] {
        for landscape in [false, true] {
            if format == "A4" && landscape {
                continue;
            }
            let r = s.execute(CMD, &json!({"format": format, "landscape": landscape})).unwrap();
            let name = r["layout"].as_str().unwrap();
            let d = s.doc().unwrap();
            let sheet = cadcraft_render::sheet(d, name).unwrap();
            assert_eq!(sheet.size, if landscape { Vec2::new(h, w) } else { Vec2::new(w, h) });
            let top = attr(insert(d, name), "DESIGNATION_TOP");
            if format != "A4" && !landscape {
                assert_eq!(top.rotation, std::f64::consts::FRAC_PI_2);
                assert_eq!(top.insert.xy(), Vec2::new(w - 12.0, h - 40.0));
            }
        }
    }
}

#[test]
fn sheet_attributes_are_editable_and_survive_dxf() {
    let mut s = session();
    let r = s
        .execute(
            CMD,
            &json!({"fields": {"title": "Кронштейн", "designation": "АБВГ.001", "org": "Бюро", "developer": "Иванов", "material": "Сталь"}}),
        )
        .unwrap();
    s.execute("attedit", &json!({"handle": r["insert"], "values": {"TITLE": "Корпус", "DEVELOPER_DATE": "18.11.2026"}})).unwrap();
    let bytes = cadcraft_io::write_dxf(s.doc().unwrap());
    let d = cadcraft_io::read_dxf(bytes.as_bytes()).unwrap();
    let name = r["layout"].as_str().unwrap();
    let ins = insert(&d, name);
    assert_eq!(attr(ins, "TITLE").value, "Корпус");
    assert_eq!(attr(ins, "DEVELOPER_DATE").value, "18.11.2026");
    assert_eq!(attr(ins, "MATERIAL").value, "Сталь");
    assert_eq!(attr(ins, "DESIGNATION_TOP").rotation, std::f64::consts::PI);
    let block = d.block(&ins.block).unwrap();
    assert_eq!(block.entities.iter().filter(|e| matches!(e.kind, EntityKind::AttDef(_))).count(), ins.attribs.len());
    assert!(ins.attribs.iter().all(|a| !a.constant && !a.invisible));
    let l = d.layout(name).unwrap();
    assert!(l.page.paper_mm);
    assert_eq!(l.page.margins_mm, [0.0; 4]);
    assert_eq!(l.page.scale, 1.0);
    assert!(!l.page.scale_to_fit);
}

#[test]
fn multiple_sheets_preserve_user_objects_and_have_one_undo_each() {
    let mut s = session();
    let original = cadcraft_io::write_dxf(s.doc().unwrap());
    let first = s.execute(CMD, &json!({})).unwrap();
    let after_first = cadcraft_io::write_dxf(s.doc().unwrap());
    let second = s.execute(CMD, &json!({"form": "2a"})).unwrap();
    assert_ne!(first["layout"], second["layout"]);
    assert_ne!(first["block"], second["block"]);
    assert_ne!(first["insert"], second["insert"]);
    assert_eq!(s.doc().unwrap().space(&Space::Model).unwrap().len(), 1);
    s.undo().unwrap();
    assert!(s.doc().unwrap().layout(second["layout"].as_str().unwrap()).is_none());
    assert_eq!(cadcraft_io::write_dxf(s.doc().unwrap()), after_first);
    s.undo().unwrap();
    assert_eq!(cadcraft_io::write_dxf(s.doc().unwrap()), original);
}

#[test]
fn all_invalid_parameters_are_atomic_before_profile_installation() {
    for p in [
        json!(null),
        json!({"format": "A5"}),
        json!({"format": 4}),
        json!({"landscape": true}),
        json!({"landscape": "yes"}),
        json!({"form": "2"}),
        json!({"form": 1}),
        json!({"layout": "Model"}),
        json!({"layout": "bad/name"}),
        json!({"layout": "Layout1"}),
        json!({"fields": []}),
        json!({"fields": {"TITLE": 10}}),
        json!({"fields": {"TITLE": "a\nb"}}),
        json!({"fields": {"TITLE": "x".repeat(256)}}),
        json!({"fields": {"title": "a", "TITLE": "b"}}),
        json!({"fields": {"FORMAT": "A3"}}),
        json!({"fields": {"UNKNOWN": "x"}}),
        json!({"form": "2a", "fields": {"TITLE": "not in this form"}}),
        json!({"font": 123}),
        json!({"scale": 0}),
        json!({"scale": "1"}),
        json!({"typo": true}),
    ] {
        let mut s = session();
        let before = s.state().unwrap().doc.clone();
        assert!(s.execute(CMD, &p).is_err(), "must reject {p}");
        assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc), "must not edit for {p}");
    }
}

#[test]
fn existing_block_name_and_layout_are_never_redefined() {
    let mut s = session();
    let block = Block::new("GOST-SHEET-Reserved");
    s.doc_mut().unwrap().blocks.insert(block.name.clone(), Arc::new(block));
    let before = s.state().unwrap().doc.clone();
    assert!(s.execute(CMD, &json!({"layout": "Reserved"})).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    let first = s.execute(CMD, &json!({"layout": "Unique"})).unwrap();
    let h = Handle::parse_hex(first["insert"].as_str().unwrap()).unwrap();
    assert!(s.execute(CMD, &json!({"layout": "unique"})).is_err());
    assert_eq!(s.doc().unwrap().layout("Unique").unwrap().entities.iter().next().unwrap().handle, h);
}

#[test]
fn designation_fits_rotated_top_box_without_squeezing_or_changing_its_value() {
    let mut s = session();
    let designation = "АБВГ.123456.001.123456.001";
    let r = s.execute(CMD, &json!({"fields":{"DESIGNATION":designation}})).unwrap();
    let d = s.doc().unwrap();
    let ins = insert(d, r["layout"].as_str().unwrap());
    let top = attr(ins, "DESIGNATION_TOP");
    let font = cadcraft_render::style_font(d.text_style("GOST").unwrap());
    let width = cadcraft_fonts::text_width(&font, &top.value, top.height, top.width_factor);
    assert!(width <= 68.0, "upper designation overflow: {width} mm");
    assert!(top.height < 7.0, "the original 7 mm text overflowed the box");
    assert!([5.0, 3.5, 2.5].contains(&top.height));
    assert_eq!(top.width_factor, 1.0);
    assert_eq!(top.value, designation);
    assert_eq!(attr(ins, "DESIGNATION").value, designation);
}

#[test]
fn text_that_cannot_fit_is_rejected_before_any_profile_or_sheet_mutation() {
    let mut s = session();
    let before = s.state().unwrap().doc.clone();
    let undo = s.state().unwrap().undo.len();
    let err = s.execute(CMD, &json!({"fields":{"TITLE":"Ш".repeat(200)}})).unwrap_err();
    assert!(err.to_string().contains("does not fit"));
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    assert_eq!(s.state().unwrap().undo.len(), undo);
}
