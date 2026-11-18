//! The anonymous `*D` block written for a dimension (what other programs draw) holds its text at
//! the height it is drawn at: the dimension's overrides and DIMSCALE (0 = the drawing's) count
//! (issue #367). Angular text also keeps its decimal separator in the stored block (#1646).

use cadcraft_doc::{Common, DimKind, DimStyle, Dimension, Drawing, EntityKind, Space};
use cadcraft_geom::Vec3;

/// The group 40 (height) of the first MTEXT in the BLOCKS section.
fn block_mtext_height(dxf: &str) -> f64 {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let blocks = lines.windows(2).position(|w| w == ["2", "BLOCKS"]).unwrap();
    let mtext = blocks + lines[blocks..].windows(2).position(|w| w == ["0", "MTEXT"]).unwrap();
    let h = mtext + lines[mtext..].chunks(2).position(|w| w[0] == "40").unwrap() * 2;
    lines[h + 1].parse().unwrap()
}

fn linear_drawing(style_scale: f64, overrides: serde_json::Value) -> Drawing {
    let mut d = Drawing::new_imperial();
    d.dim_styles.iter_mut().for_each(|s| s.scale = style_scale);
    let dm = Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(15.0, 5.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(30.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: overrides.as_object().cloned().unwrap_or_default(),
        assoc: Vec::new(),
        marks: Default::default(),
        insertion_point: Vec3::ZERO,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm)).unwrap();
    d
}

fn height_written(style_scale: f64, overrides: serde_json::Value) -> f64 {
    block_mtext_height(&cadcraft_io::write_dxf(&linear_drawing(style_scale, overrides)))
}

#[test]
fn dimension_block_text_has_the_drawn_height() {
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    let plain = height_written(1.0, serde_json::json!({}));
    assert!(near(plain, 0.18), "{plain}");
    let scaled = height_written(1.0, serde_json::json!({"scale": 10.0}));
    assert!(near(scaled, 1.8), "a DIMSCALE override scales the block text: {scaled}");
    let txt = height_written(1.0, serde_json::json!({"DIMTXT": 0.5}));
    assert!(near(txt, 0.5), "a DIMTXT override sets the block text height: {txt}");
    let zero = height_written(0.0, serde_json::json!({}));
    assert!(near(zero, 0.18), "DIMSCALE 0 in model space is scale 1, not ~0: {zero}");
}

fn angular_drawing(separator_override: Option<&str>) -> Drawing {
    let mut d = Drawing::new_metric();
    d.dim_styles.push(DimStyle { name: "Comma".into(), decimal_separator: ",".into(), angular_decimals: 1, ..DimStyle::default() });
    let angle = 39.4_f64.to_radians();
    let mut dm = Dimension {
        kind: DimKind::Angular3P,
        defpt: Vec3::new(50.0 * (angle / 2.0).cos(), 50.0 * (angle / 2.0).sin(), 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::new(100.0, 0.0, 0.0),
        p14: Vec3::new(100.0 * angle.cos(), 100.0 * angle.sin(), 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Comma".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
        insertion_point: Vec3::ZERO,
    };
    if let Some(separator) = separator_override {
        dm.overrides.insert("DIMDSEP".into(), serde_json::json!(separator));
    }
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm)).unwrap();
    d
}

#[test]
fn dxf_angular_block_text_keeps_the_decimal_separator() {
    for (separator_override, expected) in [(None, "39,4°"), (Some("."), "39.4°")] {
        let dxf = cadcraft_io::write_dxf(&angular_drawing(separator_override));
        let stored = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
        // Inspect the saved block's MTEXT, without regenerating the dimension's text.
        let texts: Vec<&str> = stored
            .blocks
            .values()
            .filter(|b| b.name.starts_with("*D"))
            .flat_map(|b| b.entities.iter())
            .filter_map(|e| match &e.kind {
                EntityKind::MText(t) => Some(t.contents.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, [expected], "DIMDSEP override: {separator_override:?}");
    }
}

#[cfg(not(any(target_arch = "wasm32", all(target_os = "freebsd", target_arch = "aarch64"))))]
#[test]
fn dwg_angular_block_text_keeps_the_decimal_separator() {
    use std::io::Cursor;

    use acadrust::entities::EntityType;
    use cadcraft_io::{SaveOptions, Version};

    for (separator_override, expected) in [(None, "39,4°"), (Some("."), "39.4°")] {
        let dwg =
            cadcraft_io::write_with(&angular_drawing(separator_override), "angle.dwg", &SaveOptions { version: Some(Version::R2018), binary: false })
                .unwrap();
        // DWG consumers use the stored *D geometry: read it directly, without CADCraft rendering it.
        let stored = acadrust::DwgReader::from_stream(Cursor::new(dwg)).read().unwrap();
        let texts: Vec<&str> = stored
            .entities()
            .filter_map(|e| match e {
                EntityType::MText(t) if stored.block_records.iter().any(|b| b.handle == t.common.owner_handle && b.name.starts_with("*D")) => {
                    Some(t.value.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(texts, [expected], "DIMDSEP override: {separator_override:?}");
    }
}

#[test]
fn dimension_stroke_weights_survive_dxf_block_rendering() {
    let mut d = linear_drawing(1.0, serde_json::json!({}));
    d.layer_mut("0").unwrap().lineweight = cadcraft_doc::Lineweight::Mm100(50);
    let style = d.dim_styles.iter_mut().find(|s| s.name == "Standard").unwrap();
    style.dim_lineweight = 25;
    style.ext_lineweight = 35;
    let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    let opts = cadcraft_render::Options { lineweights: true, ..Default::default() };
    for doc in [&d, &reopened] {
        let rendered = cadcraft_render::build_plot(doc, &Space::Model, &opts);
        for expected in [0.25_f32, 0.35] {
            assert!(
                rendered.prims.iter().any(|p| p.kind == cadcraft_render::Kind::Polyline && (p.lw - expected).abs() < 1e-6),
                "dimension stroke {expected} mm disappeared; layer weight must not replace DIMLWD/DIMLWE"
            );
        }
    }
}
