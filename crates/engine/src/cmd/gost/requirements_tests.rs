use super::*;

fn labels(d: &Drawing, h: Handle) -> Vec<String> {
    let EntityKind::Insert(i) = &d.entity(h).unwrap().kind else { panic!("not insert") };
    d.block(&i.block).unwrap().entities.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect()
}
fn h(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap()
}
#[test]
fn numbered_wrapping_templates_and_custom_library_survive_dxf() {
    let mut s = Session::new();
    let r=s.execute(CMD,&json!({"items":["Контролировать размеры детали перед сборкой по технологической карте.",{"template":"edges","values":{"size":"0,5"}}],"width":60})).unwrap();
    let id = h(&r);
    let texts = labels(s.doc().unwrap(), id);
    assert!(texts.iter().any(|t| t == "1."));
    assert!(texts.iter().any(|t| t == "2."));
    assert!(r["lines"].as_u64().unwrap() > 2);
    s.execute(
        "gost.requirements.templates",
        &json!({"handle":id.hex(),"action":"set","id":"custom","name":"Испытание","text":"Испытать при давлении {pressure} МПа."}),
    )
    .unwrap();
    s.execute("gost.requirements.templates", &json!({"handle":id.hex(),"action":"use","id":"custom","values":{"pressure":"2,5"}})).unwrap();
    let d = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(d, "again", None);
    let r = s.execute("gost.requirements.inspect", &json!({"handle":id.hex()})).unwrap();
    assert_eq!(r["requirements"][0]["record"]["items"][2], "Испытать при давлении 2,5 МПа.");
    let library = s.execute("gost.requirements.templates", &json!({"handle":id.hex()})).unwrap();
    assert!(library["templates"].as_array().unwrap().iter().any(|t| t["id"] == "custom"));
}
#[test]
fn requirements_reflow_with_sheet_and_undo_as_one_transaction() {
    let mut s = Session::new();
    let id = h(&s.execute(CMD, &json!({"items":["Удалить загрязнения перед сборкой."]})).unwrap());
    let before = s.doc().unwrap().clone();
    let x = match &s.doc().unwrap().entity(id).unwrap().kind {
        EntityKind::Insert(i) => i.insert.x,
        _ => panic!(),
    };
    s.execute("gost.sheet.set", &json!({"layout":"GOST1","format":"A3","landscape":true})).unwrap();
    let new = match &s.doc().unwrap().entity(id).unwrap().kind {
        EntityKind::Insert(i) => i.insert.x,
        _ => panic!(),
    };
    assert!((new - x - 210.0).abs() < 1e-6);
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap(), &before);
    s.redo().unwrap();
    assert_ne!(s.doc().unwrap(), &before);
}
#[test]
fn columns_fit_and_overflow_rejects_without_losing_existing_requirements() {
    let mut s = Session::new();
    let item = "Контроль размеров перед сборкой по карте технологического процесса ".repeat(3);
    let id = h(&s.execute(CMD, &json!({"items":vec![item.clone();8],"width":45})).unwrap());
    let r = s.execute("gost.requirements.inspect", &json!({"handle":id.hex()})).unwrap();
    assert!(r["requirements"][0]["pages"].as_u64().unwrap() > 1);
    let before = s.doc().unwrap().clone();
    assert!(s.execute(CMD, &json!({"handle":id.hex(),"items":vec![item;24],"width":45,"height":5})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    assert!(s.execute(CMD, &json!({"handle":id.hex(),"items":["x".repeat(1000)],"width":20})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}
