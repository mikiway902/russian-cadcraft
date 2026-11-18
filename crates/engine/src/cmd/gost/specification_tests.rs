use super::*;
use cadcraft_doc::Attrib;
fn part(s: &mut Session, name: &str, designation: &str, x: f64) -> Handle {
    let d = s.doc_mut().unwrap();
    let mut block = Block::new(name);
    block.entities.push(Entity {
        handle: d.new_handle(),
        common: Common::default(),
        kind: EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) }),
    });
    d.blocks.insert(name.into(), Arc::new(block));
    let attribs = [("DESIGNATION", designation), ("NAME", name), ("SECTION", "Детали")]
        .into_iter()
        .map(|(tag, value)| Attrib {
            tag: tag.into(),
            text: text(value.into(), Vec2::ZERO, 3.5, 0.0),
            prompt: String::new(),
            constant: false,
            invisible: false,
            props: Default::default(),
            multiline: None,
            modes: Default::default(),
        })
        .collect();
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::Insert(Insert { block: name.into(), insert: Vec3::new(x, 0.0, 0.0), attribs, ..Default::default() }),
    )
    .unwrap()
}
fn handle_result(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap()
}
fn inspect_rows(s: &mut Session, h: Handle) -> Value {
    s.execute("gost.specification.inspect", &json!({"handle":h.hex()})).unwrap()
}
fn labels(d: &Drawing, h: Handle) -> Vec<String> {
    let EntityKind::Insert(i) = &d.entity(h).unwrap().kind else { panic!() };
    d.block(&i.block).unwrap().entities.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect()
}
#[test]
fn specification_counts_copy_and_keeps_positions_synced_after_attribute_changes() {
    let mut s = Session::new();
    let a = part(&mut s, "Корпус", "Б.002", 0.0);
    let b = part(&mut s, "Втулка", "А.001", 30.0);
    let spec = handle_result(&s.execute(CMD, &json!({"layout":"Спецификация","sources":[a.hex(),b.hex()]})).unwrap());
    let leader = handle_result(&s.execute("gost.position", &json!({"specification":spec.hex(),"target":a.hex(),"at":[10,40]})).unwrap());
    assert!(labels(s.doc().unwrap(), leader).contains(&"2".to_string()));
    let copied = s.execute("copy", &json!({"handles":[a.hex()],"delta":[60,0]})).unwrap();
    assert!(!copied.is_null());
    let rows = inspect_rows(&mut s, spec);
    assert_eq!(rows["rows"][1]["quantity"], 2);
    s.execute("attedit", &json!({"handle":b.hex(),"values":{"DESIGNATION":"Я.999"}})).unwrap();
    assert!(labels(s.doc().unwrap(), leader).contains(&"1".to_string()));
    s.undo().unwrap();
    assert!(labels(s.doc().unwrap(), leader).contains(&"2".to_string()));
    let d = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(d, "again", None);
    assert_eq!(inspect_rows(&mut s, spec)["rows"][1]["quantity"], 2);
}
#[test]
fn unresolved_sources_remain_reported_and_positions_are_not_stale() {
    let mut s = Session::new();
    let a = part(&mut s, "Опора", "А.001", 0.0);
    let spec = handle_result(&s.execute(CMD, &json!({"layout":"Состав","sources":[a.hex()]})).unwrap());
    let leader = handle_result(&s.execute("gost.position", &json!({"specification":spec.hex(),"target":a.hex(),"at":[10,40]})).unwrap());
    s.execute("erase", &json!({"handles":[a.hex()]})).unwrap();
    let r = inspect_rows(&mut s, spec);
    assert_eq!(r["rows"][0]["quantity"], 0);
    assert!(!r["findings"].as_array().unwrap().is_empty());
    assert!(labels(s.doc().unwrap(), leader).contains(&"?".to_string()));
    s.undo().unwrap();
    assert!(labels(s.doc().unwrap(), leader).contains(&"1".to_string()));
}
#[test]
fn manual_rows_sections_and_page_bounds_are_persistent() {
    let mut s = Session::new();
    let manual: Vec<_> =
        (0..45).map(|i| json!({"id":format!("p{i}"),"section":"Детали","designation":format!("А.{i:03}"),"name":"Деталь","quantity":1})).collect();
    let r = s.execute(CMD, &json!({"layout":"Ведомость","manual":manual})).unwrap();
    let spec = handle_result(&r);
    assert!(r["pages"].as_array().unwrap().len() > 1);
    for raw in r["pages"].as_array().unwrap() {
        let h = Handle::parse_hex(raw.as_str().unwrap()).unwrap();
        let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
        let block = s.doc().unwrap().block(&i.block).unwrap();
        for e in block.entities.iter() {
            let b = cadcraft_doc::entity_bounds(s.doc().unwrap(), e, 0);
            assert!(b.min.x >= 0.0 && b.max.x <= 210.0 && b.min.y >= 0.0 && b.max.y <= 297.0, "{b:?}");
            if matches!(e.kind, EntityKind::Table(_)) {
                assert!((b.min.x - 20.0).abs() < 1e-8 && (b.max.x - 205.0).abs() < 1e-8 && (b.max.y - 292.0).abs() < 1e-8);
                assert!((b.min.y - if h == spec { 45.0 } else { 20.0 }).abs() < 1e-8);
            }
        }
    }
    let before = s.doc().unwrap().clone();
    assert!(
        s.execute(
            "gost.specification.edit",
            &json!({"handle":spec.hex(),"manual":[{"id":"bad","section":"invalid","designation":"x","name":"x","quantity":1}]})
        )
        .is_err()
    );
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn copied_part_and_position_remain_a_pair() {
    let mut s = Session::new();
    let a = part(&mut s, "Опора", "А.001", 0.0);
    let spec = handle_result(&s.execute(CMD, &json!({"layout":"Состав","sources":[a.hex()]})).unwrap());
    let leader = handle_result(&s.execute("gost.position", &json!({"specification":spec.hex(),"target":a.hex(),"at":[20,40]})).unwrap());
    s.execute("copy", &json!({"handles":[a.hex(),leader.hex()],"delta":[100,0]})).unwrap();
    let copied = s
        .doc()
        .unwrap()
        .model
        .iter()
        .find(|e| e.handle != leader && matches!(&e.kind,EntityKind::Insert(i) if i.block.starts_with("GOST-POS-")))
        .unwrap()
        .handle;
    let position = s.execute("gost.position.inspect", &json!({"handle":copied.hex()})).unwrap();
    let target = position["target"].as_str().and_then(Handle::parse_hex).unwrap();
    assert_ne!(target, a);
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(target).unwrap().kind else { panic!() };
    assert_eq!(i.insert.x, 100.0);
    assert_eq!(inspect_rows(&mut s, spec)["rows"][0]["quantity"], 2);
    assert!(labels(s.doc().unwrap(), copied).contains(&"1".into()));
}

#[test]
fn shrinking_specification_removes_empty_pages_but_preserves_user_content() {
    let mut s = Session::new();
    let manual: Vec<_> =
        (0..45).map(|i| json!({"id":format!("p{i}"),"section":"Детали","designation":format!("А.{i:03}"),"name":"Деталь","quantity":1})).collect();
    let r = s.execute(CMD, &json!({"layout":"Состав","manual":manual})).unwrap();
    let spec = handle_result(&r);
    s.execute("layout.set", &json!({"name":"Состав-2"})).unwrap();
    s.execute("gost.specification.edit", &json!({"handle":spec.hex(),"manual":[manual[0].clone()]})).unwrap();
    assert!(s.doc().unwrap().layout("Состав-2").is_none());
    assert!(matches!(s.layout_space(), Space::Model));
    s.undo().unwrap();
    s.execute("layout.set", &json!({"name":"Состав-2"})).unwrap();
    let line = s.execute("line", &json!({"points":[[30,40],[50,60]]})).unwrap()["handles"][0].as_str().and_then(Handle::parse_hex).unwrap();
    s.execute("gost.specification.edit", &json!({"handle":spec.hex(),"manual":[manual[0].clone()]})).unwrap();
    assert!(s.doc().unwrap().entity(line).is_some());
    assert!(s.doc().unwrap().layout("Состав-2").is_some());
    assert_eq!(inspect_rows(&mut s, spec)["pages"].as_array().unwrap().len(), 1);
    s.execute("gost.specification.edit", &json!({"handle":spec.hex(),"manual":manual})).unwrap();
    assert_eq!(inspect_rows(&mut s, spec)["pages"].as_array().unwrap().len(), 2);
    assert!(s.doc().unwrap().entity(line).is_some());
    assert!(s.doc().unwrap().layout("Состав-2-1").is_some());
}

#[test]
fn section_underlines_and_position_quantity_are_centered_after_dxf_reopen() {
    let mut s = Session::new();
    let manual = json!([{"id":"p1","section":"Детали","designation":"А.001","name":"Корпус","quantity":2}]);
    let spec = handle_result(&s.execute(CMD, &json!({"layout":"Состав","manual":manual})).unwrap());
    let d = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(d, "again", None);
    // Rebuild after import must retain the native table style, not discard it as a conflict.
    s.execute("gost.specification.edit", &json!({"handle":spec.hex(),"manual":manual})).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Insert(i) = &d.entity(spec).unwrap().kind else { panic!() };
    let block = d.block(&i.block).unwrap();
    let table = block.entities.iter().find_map(|e| if let EntityKind::Table(t) = &e.kind { Some(t) } else { None }).unwrap();
    let geometry = cadcraft_render::table_geometry(d, table);
    let heading = geometry.texts.iter().find(|t| t.text == "Детали").unwrap();
    assert_eq!(heading.attach, 5);
    assert_eq!(heading.at.x, 141.5);
    let underline = block
        .entities
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Line(l) if (l.a.y - 271.0).abs() < 1e-8 && (l.b.y - 271.0).abs() < 1e-8 => Some(l),
            _ => None,
        })
        .unwrap();
    assert!(((underline.a.x + underline.b.x) * 0.5 - heading.at.x).abs() < 1e-8);
    let position = geometry.texts.iter().find(|t| t.col == 2 && t.text == "1").unwrap();
    let quantity = geometry.texts.iter().find(|t| t.col == 5 && t.text == "2").unwrap();
    assert_eq!((position.attach, position.at.x), (5, 36.0));
    assert_eq!((quantity.attach, quantity.at.x), (5, 178.0));
}
