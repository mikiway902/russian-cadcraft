use super::*;
fn circle_and_axes(s: &mut Session) -> (Handle, Handle) {
    let c = s.execute("circle", &json!({"center":[20,30],"radius":10})).unwrap()["handle"].as_str().and_then(Handle::parse_hex).unwrap();
    let h = s.execute("gost.center", &json!({"handles":[c.hex()]})).unwrap()["handles"][0].as_str().and_then(Handle::parse_hex).unwrap();
    (c, h)
}
fn at(s: &Session, h: Handle) -> Vec2 {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
    i.insert.xy()
}
#[test]
fn source_move_resize_delete_and_undo_keep_axes_consistent() {
    let mut s = crate::extensions::new_session();
    let (c, h) = circle_and_axes(&mut s);
    s.execute("move", &json!({"handles":[c.hex()],"delta":[50,10]})).unwrap();
    assert_eq!(at(&s, h), Vec2::new(70.0, 40.0));
    s.execute("scale", &json!({"handles":[c.hex()],"base":[70,40],"factor":2})).unwrap();
    let r: Record = data::load(&s.doc().unwrap().entity(h).unwrap().common, KIND).unwrap().unwrap();
    assert_eq!(r.radius, 20.0);
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
    let b = s.doc().unwrap().block(&i.block).unwrap();
    assert!(b.entities.iter().any(|e| matches!(&e.kind,EntityKind::Line(l) if l.b.x==23.0)));
    s.execute("erase", &json!({"handles":[c.hex()]})).unwrap();
    assert!(s.doc().unwrap().entity(h).is_none());
    s.undo().unwrap();
    assert_eq!(at(&s, h), Vec2::new(70.0, 40.0));
}
#[test]
fn copied_pair_owns_new_source_and_links_persist_in_dxf() {
    let mut s = crate::extensions::new_session();
    let (c, h) = circle_and_axes(&mut s);
    s.execute("copy", &json!({"handles":[c.hex(),h.hex()],"delta":[100,0]})).unwrap();
    let copied = s
        .doc()
        .unwrap()
        .model
        .iter()
        .find(|e| e.handle != h && matches!(&e.kind,EntityKind::Insert(i) if i.block.starts_with("GOST-AXES-")&&i.insert.x>100.0))
        .unwrap()
        .handle;
    let r: Record = data::load(&s.doc().unwrap().entity(copied).unwrap().common, KIND).unwrap().unwrap();
    assert_ne!(r.source, c);
    let d = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(d, "again", None);
    s.execute("move", &json!({"handles":[r.source.hex()],"delta":[0,50]})).unwrap();
    assert_eq!(at(&s, copied), Vec2::new(120.0, 80.0));
    assert_eq!(at(&s, h), Vec2::new(20.0, 30.0));
}
#[test]
fn invalid_axis_only_copy_is_atomic() {
    let mut s = crate::extensions::new_session();
    let (_, h) = circle_and_axes(&mut s);
    let before = s.doc().unwrap().clone();
    assert!(s.execute("copy", &json!({"handles":[h.hex()],"delta":[100,0]})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}
