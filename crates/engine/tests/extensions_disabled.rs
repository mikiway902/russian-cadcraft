#![cfg(not(feature = "gost"))]

use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{Drawing, EntityKind, Handle, RawTag, Space};
use cadcraft_engine::{EngineError, Session, extensions};
use serde_json::{Value, json};

#[path = "support/temp_dir.rs"]
mod temp_dir;

// A future producer's metadata is deliberately not understood by the current GOST schema.
// Keep it on both model and paper entities: enabled lifecycle hooks inspect paper XDATA even
// when an unrelated native command only changes model space.
const FUTURE_DATA: &str = r#"{"version":99,"kind":"future-association","data":{"source":"future:42","opaque":[1,"keep",false]}}"#;

fn future_dxf() -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n\
9\n$ACADVER\n1\nAC1032\n9\n$MEASUREMENT\n70\n1\n9\n$INSUNITS\n70\n4\n\
9\n$TEXTSTYLE\n7\nGOST\n9\n$CENTERLAYER\n8\nGOST-CENTER\n9\n$CENTEREXE\n40\n2.5\n\
0\nENDSEC\n0\nSECTION\n2\nTABLES\n\
0\nTABLE\n2\nSTYLE\n70\n1\n0\nSTYLE\n2\nGOST\n70\n0\n40\n0\n41\n0.8\n50\n0\n71\n0\n42\n3.5\n3\nFutureGOST.ttf\n0\nENDTAB\n\
0\nTABLE\n2\nLTYPE\n70\n1\n0\nLTYPE\n2\nGOST-CENTER\n70\n0\n3\nFuture center pattern\n72\n65\n73\n2\n40\n10\n49\n7\n74\n0\n49\n-3\n74\n0\n0\nENDTAB\n\
0\nTABLE\n2\nLAYER\n70\n1\n0\nLAYER\n2\nGOST-CENTER\n70\n0\n62\n7\n6\nGOST-CENTER\n0\nENDTAB\n\
0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
0\nCIRCLE\n5\n2A\n8\n0\n10\n20\n20\n30\n30\n0\n40\n12\n\
1001\nCADCRAFTGOST\n1000\n{FUTURE_DATA}\n1001\nFUTURE_VENDOR\n1000\nkeep circle\n1070\n7\n\
0\nLINE\n5\n2B\n67\n1\n8\nGOST-CENTER\n6\nGOST-CENTER\n10\n5\n20\n5\n30\n0\n11\n25\n21\n5\n31\n0\n\
1001\nCADCRAFTGOST\n1000\n{FUTURE_DATA}\n1001\nFUTURE_VENDOR\n1000\nkeep paper\n1070\n9\n\
0\nENDSEC\n0\nEOF\n"
    )
}

fn handle(value: &Value) -> Handle {
    Handle::parse_hex(value.as_str().unwrap()).unwrap()
}

fn tags(xdata: &[RawTag]) -> Vec<(i32, String)> {
    xdata.iter().map(|t| (t.code, t.value.trim().to_string())).collect()
}

fn line_points(d: &Drawing, h: Handle) -> ([f64; 3], [f64; 3]) {
    let EntityKind::Line(line) = &d.entity(h).unwrap().kind else { panic!("expected a native LINE") };
    ([line.a.x, line.a.y, line.a.z], [line.b.x, line.b.y, line.b.z])
}

fn native_edits(s: &mut Session) {
    let before = s.doc().unwrap().clone();
    let result = s.execute("line", &json!({"points": [[1, 2], [11, 2]]})).unwrap();
    let original = handle(&result["handles"][0]);
    assert_eq!(line_points(s.doc().unwrap(), original), ([1.0, 2.0, 0.0], [11.0, 2.0, 0.0]));
    let result = s.execute("copy", &json!({"handles": [original.hex()], "delta": [3, 4]})).unwrap();
    let copied = handle(&result["handles"][0]);
    assert_ne!(original, copied);
    assert_eq!(line_points(s.doc().unwrap(), copied), ([4.0, 6.0, 0.0], [14.0, 6.0, 0.0]));
    s.execute("u", &json!({})).unwrap();
    assert!(s.doc().unwrap().entity(copied).is_none());
    assert!(s.doc().unwrap().entity(original).is_some());
    s.execute("u", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap(), &before, "native Undo restores the complete drawing");
}

#[test]
fn disabled_product_factories_keep_native_drafting_and_undo() {
    let mut start = extensions::session();
    assert!(matches!(start.doc(), Err(EngineError::NoDocument)));
    for metric in [false, true] {
        extensions::new_drawing(&mut start, metric);
        let expected = if metric { Drawing::new_metric() } else { Drawing::new_imperial() };
        assert_eq!(start.doc().unwrap(), &expected);
        assert!(!start.state().unwrap().is_dirty());
        assert!(start.state().unwrap().undo.is_empty());
        native_edits(&mut start);
    }

    let mut s = extensions::new_session();
    let expected = if s.doc().unwrap().header.i64("MEASUREMENT", 0) == 1 { Drawing::new_metric() } else { Drawing::new_imperial() };
    assert_eq!(s.doc().unwrap(), &expected);
    assert_eq!(s.space(), Space::Model);
    native_edits(&mut s);
    for metric in [true, false] {
        s.execute("new", &json!({"metric": metric})).unwrap();
        let expected = if metric { Drawing::new_metric() } else { Drawing::new_imperial() };
        assert_eq!(s.doc().unwrap(), &expected);
        assert!(s.state().unwrap().undo.is_empty());
    }
    let before = s.doc().unwrap().clone();
    assert!(matches!(s.execute("gost.sheet", &json!({})), Err(EngineError::UnknownCommand(id)) if id == "gost.sheet"));
    assert_eq!(s.doc().unwrap(), &before);
    assert!(s.state().unwrap().undo.is_empty());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn future_gost_dxf_stays_editable_and_opaque_without_the_extension() {
    set_io(IoHooks {
        read: |bytes, name| cadcraft_io::read(bytes, name).map_err(|e| e.to_string()),
        write: |drawing, name| cadcraft_io::write(drawing, name).map_err(|e| e.to_string()),
        plot: None,
    });
    let dir = temp_dir::TempDir::new("extensions-disabled");
    let path = dir.file("future.dxf", future_dxf());
    let saved_path = dir.path_str("edited.dxf");
    let source = cadcraft_io::read(&std::fs::read(&path).unwrap(), &path).unwrap();
    let circle = Handle::parse_hex("2A").unwrap();
    let paper_line = Handle::parse_hex("2B").unwrap();
    let circle_tags = tags(&source.entity(circle).unwrap().common.xdata);
    let paper_tags = tags(&source.entity(paper_line).unwrap().common.xdata);
    assert!(circle_tags.contains(&(1000, FUTURE_DATA.to_string())));
    assert!(paper_tags.contains(&(1000, FUTURE_DATA.to_string())));
    assert!(matches!(source.space_of(paper_line), Some(Space::Paper(_))));
    assert_eq!(source.header.str("TEXTSTYLE", ""), "GOST");
    assert_eq!(source.header.str("CENTERLAYER", ""), "GOST-CENTER");
    assert_eq!(source.header.f64("CENTEREXE", 0.0), 2.5);

    let mut s = extensions::new_session();
    s.execute("open", &json!({"path": path})).unwrap();
    assert_eq!(s.doc().unwrap(), &source, "opening must not migrate foreign extension data");
    assert!(!s.state().unwrap().is_dirty());
    assert!(s.state().unwrap().undo.is_empty());
    native_edits(&mut s);

    let before_copy = s.doc().unwrap().clone();
    let result = s.execute("copy", &json!({"handles": [circle.hex(), paper_line.hex()], "delta": [50, 0]})).unwrap();
    let circle_copy = handle(&result["handles"][0]);
    let paper_copy = handle(&result["handles"][1]);
    assert_eq!(tags(&s.doc().unwrap().entity(circle_copy).unwrap().common.xdata), circle_tags);
    assert_eq!(tags(&s.doc().unwrap().entity(paper_copy).unwrap().common.xdata), paper_tags);
    assert_eq!(s.doc().unwrap().space_of(paper_copy), source.space_of(paper_line));
    s.execute("u", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap(), &before_copy);
    // Keep native copies in the saved drawing, not just the original untouched metadata.
    let result = s.execute("copy", &json!({"handles": [circle.hex(), paper_line.hex()], "delta": [50, 0]})).unwrap();
    let circle_copy = handle(&result["handles"][0]);
    let paper_copy = handle(&result["handles"][1]);

    // Do not pass `viewport`: that used to route TEXTSTYLE=GOST to an unavailable generator.
    let result = s.execute("layout.new", &json!({"name": "Native sheet"})).unwrap();
    let viewport = handle(&result["viewport"]);
    let layout = s.doc().unwrap().layout("Native sheet").unwrap();
    assert_eq!(layout.entities.len(), 1);
    assert!(matches!(layout.entities.get(viewport).unwrap().kind, EntityKind::Viewport(_)));
    s.execute("u", &json!({})).unwrap();
    assert!(s.doc().unwrap().layout("Native sheet").is_none());
    s.execute("layout.new", &json!({"name": "Native sheet"})).unwrap();

    // CENTERLAYER=GOST-CENTER is ordinary foreign drawing data, not an enabled GOST hook.
    let result = s.execute("centermark", &json!({"handle": circle.hex()})).unwrap();
    let marks: Vec<_> = result["handles"].as_array().unwrap().iter().map(handle).collect();
    assert_eq!(marks.len(), 2);
    assert_eq!(line_points(s.doc().unwrap(), marks[0]), ([5.5, 30.0, 0.0], [34.5, 30.0, 0.0]));
    assert_eq!(line_points(s.doc().unwrap(), marks[1]), ([20.0, 15.5, 0.0], [20.0, 44.5, 0.0]));
    for h in &marks {
        assert_eq!(s.doc().unwrap().entity(*h).unwrap().common.linetype, "CENTER");
    }
    s.execute("u", &json!({})).unwrap();
    for h in marks {
        assert!(s.doc().unwrap().entity(h).is_none());
    }
    s.execute("centermark", &json!({"handle": circle.hex()})).unwrap();

    s.execute("saveas", &json!({"path": saved_path})).unwrap();
    let mut reopened = extensions::session();
    reopened.execute("open", &json!({"path": saved_path})).unwrap();
    let back = reopened.doc().unwrap();
    for h in [circle, circle_copy] {
        assert_eq!(tags(&back.entity(h).unwrap().common.xdata), circle_tags);
    }
    for h in [paper_line, paper_copy] {
        let e = back.entity(h).unwrap();
        assert_eq!(tags(&e.common.xdata), paper_tags);
        assert_eq!(e.common.layer, "GOST-CENTER");
        assert_eq!(e.common.linetype, "GOST-CENTER");
        assert_eq!(back.space_of(h), source.space_of(paper_line));
    }
    assert_eq!(back.text_style("GOST"), source.text_style("GOST"));
    assert_eq!(back.linetype("GOST-CENTER"), source.linetype("GOST-CENTER"));
    assert!(back.layout("Native sheet").unwrap().entities.iter().any(|e| matches!(e.kind, EntityKind::Viewport(_))));
    native_edits(&mut reopened);
}
