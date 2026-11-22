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

#[test]
fn ordinary_circle_axes_have_gaps_but_cross_and_end_in_equal_dashes() {
    let mut s = crate::extensions::new_session();
    let (_, h) = circle_and_axes(&mut s);
    let EntityKind::Insert(insert) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("axes insert required") };
    let block = s.doc().unwrap().block(&insert.block).unwrap();
    let positive: Vec<_> = block
        .entities
        .iter()
        .filter_map(|entity| {
            if let EntityKind::Line(line) = &entity.kind
                && line.a.y == 0.
                && line.b.y == 0.
                && line.a.x >= 0.
                && line.b.x > 0.
            {
                Some((line.a.x, line.b.x))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(positive.first().unwrap().0, 0.);
    assert_eq!(positive.last().unwrap().1, 13.);
    assert!(positive.windows(2).any(|pair| pair[1].0 > pair[0].1));
    let first = positive.first().unwrap();
    let last = positive.last().unwrap();
    assert!((2. * (first.1 - first.0) - (last.1 - last.0)).abs() < 1e-12);
    let dash = last.1 - last.0;
    let interval = last.0 - first.1;
    assert!((5.0..=30.0).contains(&dash));
    assert!((3.0..=5.0).contains(&interval));
    assert!(positive.iter().any(|(a, b)| *a <= 10. && *b >= 10.));
}

#[test]
fn twelve_mm_circle_resize_after_dxf_reopen_adapts_extension_and_refuses_impossible_fixed_one() {
    let mut s = crate::extensions::new_session();
    let (circle, axes) = circle_and_axes(&mut s);
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(reopened, "auto extension DXF", None);
    s.execute("scale", &json!({"handles":[circle.hex()],"base":[20,30],"factor":0.6})).unwrap();
    let EntityKind::Insert(insert) = &s.doc().unwrap().entity(axes).unwrap().kind else { panic!("axes insert required") };
    let block = s.doc().unwrap().block(&insert.block).unwrap();
    let reach = block
        .entities
        .iter()
        .filter_map(|entity| if let EntityKind::Line(line) = &entity.kind { Some(line.a.xy().len().max(line.b.xy().len())) } else { None })
        .fold(0., f64::max);
    assert!((reach - 10.5).abs() < 1e-10);
    assert!(block.entities.iter().all(|entity| entity.common.layer == "GOST-CENTER"));
    let before = s.doc().unwrap().clone();
    assert!(s.execute("gost.center", &json!({"handles":[circle.hex()],"extension":3})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}
