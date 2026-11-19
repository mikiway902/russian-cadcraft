use super::*;
use cadcraft_doc::{Insert, RawTag, Space};

fn frame<'a>(d: &'a Drawing, name: &str) -> (&'a cadcraft_doc::Entity, &'a Insert) {
    d.layout(name)
        .unwrap()
        .entities
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Insert(i) if i.block.starts_with("GOST-SHEET-") => Some((e.as_ref(), i)),
            _ => None,
        })
        .unwrap()
}
fn value(d: &Drawing, name: &str, tag: &str) -> String {
    frame(d, name).1.attribs.iter().find(|a| a.tag == tag).unwrap().text.value.clone()
}
fn form(d: &Drawing, name: &str) -> String {
    d.block(&frame(d, name).1.block).unwrap().description.clone()
}

#[test]
fn common_fields_numbering_and_promotion_survive_dxf() {
    let mut s = crate::extensions::new_session();
    s.execute("gost.document", &json!({"fields":{"DESIGNATION":"АБВГ.001","TITLE":"Кронштейн","MATERIAL":"Сталь","DEVELOPER":"Иванов"}})).unwrap();
    s.execute("layout.new", &json!({"name":"Продолжение"})).unwrap();
    assert_eq!(value(s.doc().unwrap(), "GOST1", "SHEETS"), "2");
    assert_eq!(value(s.doc().unwrap(), "Продолжение", "SHEET"), "2");
    assert_eq!(value(s.doc().unwrap(), "Продолжение", "DESIGNATION_TOP"), "АБВГ.001");
    assert!(form(s.doc().unwrap(), "Продолжение").contains("form 2a,"));
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(reopened, "Reopened", None);
    s.execute("layout.delete", &json!({"name":"GOST1"})).unwrap();
    assert!(form(s.doc().unwrap(), "Продолжение").contains("form 1,"));
    for (tag, text) in [("TITLE", "Кронштейн"), ("MATERIAL", "Сталь"), ("DEVELOPER", "Иванов"), ("SHEET", "1"), ("SHEETS", "1")] {
        assert_eq!(value(s.doc().unwrap(), "Продолжение", tag), text);
    }
    s.undo().unwrap();
    assert_eq!(value(s.doc().unwrap(), "GOST1", "SHEETS"), "2");
    s.redo().unwrap();
    assert_eq!(value(s.doc().unwrap(), "Продолжение", "SHEETS"), "1");
}

#[test]
fn format_changes_keep_geometry_fields_handles_and_foreign_data() {
    let mut s = crate::extensions::new_session();
    s.execute("layout.set", &json!({"name":"GOST1"})).unwrap();
    let h = s.execute("line", &json!({"points":[[30,100],[80,100]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    let reference = frame(s.doc().unwrap(), "GOST1").0.handle;
    s.doc_mut()
        .unwrap()
        .modify_entity(reference, |e| {
            e.common.xdata.extend([RawTag { code: 1001, value: "OTHER_APP".into() }, RawTag { code: 1000, value: "keep me".into() }])
        })
        .unwrap();
    s.execute("gost.sheet.set", &json!({"layout":"GOST1","format":"A3","landscape":true,"fields":{"TITLE":"Корпус","REV1_DOCUMENT":"Изм.1"}}))
        .unwrap();
    let d = s.doc().unwrap();
    assert_eq!(frame(d, "GOST1").0.handle, reference);
    assert!(d.entity(Handle::parse_hex(&h).unwrap()).is_some());
    assert_eq!(value(d, "GOST1", "TITLE"), "Корпус");
    assert_eq!(value(d, "GOST1", "REV1_DOCUMENT"), "Изм.1");
    assert_eq!(value(d, "GOST1", "FORMAT"), "A3");
    assert!(frame(d, "GOST1").0.common.xdata.iter().any(|t| t.value == "keep me"));
    assert_eq!(cadcraft_render::sheet(d, "GOST1").unwrap().size, cadcraft_geom::Vec2::new(420.0, 297.0));
    s.execute("gost.sheet.set", &json!({"layout":"GOST1","format":"A1","landscape":false})).unwrap();
    assert_eq!(cadcraft_render::sheet(s.doc().unwrap(), "GOST1").unwrap().size, cadcraft_geom::Vec2::new(594.0, 841.0));
    let before = s.doc().unwrap().clone();
    assert!(s.execute("gost.sheet.set", &json!({"layout":"GOST1","format":"A4","landscape":true})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    assert_eq!(s.space(), Space::Paper("GOST1".into()));
}

#[test]
fn attribute_edits_and_layout_copy_rename_keep_common_values_consistent() {
    let mut s = crate::extensions::new_session();
    s.execute("gost.document", &json!({"fields":{"TITLE":"Опора"}})).unwrap();
    s.execute("layout.copy", &json!({"from":"GOST1","to":"Лист2"})).unwrap();
    s.execute("layout.rename", &json!({"from":"Лист2","to":"Сборка"})).unwrap();
    let h = frame(s.doc().unwrap(), "Сборка").0.handle.hex();
    s.execute("attedit", &json!({"handle":h,"values":{"DESIGNATION":"АБВГ.002"}})).unwrap();
    assert_eq!(value(s.doc().unwrap(), "GOST1", "DESIGNATION"), "АБВГ.002");
    assert_eq!(value(s.doc().unwrap(), "Сборка", "DESIGNATION_TOP"), "АБВГ.002");
    assert_eq!(value(s.doc().unwrap(), "GOST1", "TITLE"), "Опора");
    assert_ne!(frame(s.doc().unwrap(), "GOST1").1.block, frame(s.doc().unwrap(), "Сборка").1.block);
    assert_eq!(value(s.doc().unwrap(), "Сборка", "SHEET"), "2");
}

#[test]
fn manual_sheet_is_independent_and_invalid_common_updates_are_atomic() {
    let mut s = crate::extensions::new_session();
    s.execute("gost.document", &json!({"fields":{"DESIGNATION":"ОБЩ.001"}})).unwrap();
    s.execute("gost.sheet", &json!({"layout":"Независимый","automatic":false,"form":"2a","fields":{"DESIGNATION":"ДРУГ.001","SHEET":"7"}})).unwrap();
    assert_eq!(value(s.doc().unwrap(), "GOST1", "DESIGNATION"), "ОБЩ.001");
    assert_eq!(value(s.doc().unwrap(), "GOST1", "SHEETS"), "1");
    assert_eq!(value(s.doc().unwrap(), "Независимый", "SHEET"), "7");
    let before = s.doc().unwrap().clone();
    for p in [json!({"fields":{"BOGUS":"x"}}), json!({"fields":{"TITLE":"Я".repeat(255)}}), json!({"fields":{"DESIGNATION":false}})] {
        assert!(s.execute("gost.document", &p).is_err());
        assert_eq!(s.doc().unwrap(), &before);
    }
}

#[test]
fn erasing_the_first_frame_promotes_the_next_managed_sheet_in_one_undo() {
    let mut s = crate::extensions::new_session();
    s.execute("gost.document", &json!({"fields":{"TITLE":"Корпус"}})).unwrap();
    s.execute("layout.new", &json!({"name":"Second"})).unwrap();
    let h = frame(s.doc().unwrap(), "GOST1").0.handle;
    s.execute("erase", &json!({"handles":[h.hex()]})).unwrap();
    assert_eq!(value(s.doc().unwrap(), "Second", "SHEET"), "1");
    assert_eq!(value(s.doc().unwrap(), "Second", "SHEETS"), "1");
    assert_eq!(value(s.doc().unwrap(), "Second", "TITLE"), "Корпус");
    assert!(form(s.doc().unwrap(), "Second").contains("form 1,"));
    s.undo().unwrap();
    assert!(s.doc().unwrap().entity(h).is_some());
    assert_eq!(value(s.doc().unwrap(), "Second", "SHEET"), "2");
    s.redo().unwrap();
    assert_eq!(value(s.doc().unwrap(), "Second", "SHEETS"), "1");
}
