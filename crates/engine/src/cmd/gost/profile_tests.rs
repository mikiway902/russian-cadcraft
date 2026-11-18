use super::*;

fn imperial_session() -> Session {
    let mut s = Session::empty();
    s.new_drawing(false);
    s
}

#[test]
fn installation_is_idempotent_and_preserves_unrelated_records() {
    let mut s = imperial_session();
    let text = TextStyle { name: "Workshop".into(), font: "Local font".into(), width_factor: 0.8, ..TextStyle::default() };
    let dim = DimStyle { name: "Workshop".into(), text_height: 8.0, ..DimStyle::default() };
    let layer = Layer { name: "Workshop".into(), color: Color::Index(2), ..Layer::default() };
    let linetype = Linetype::simple("Workshop", "Unrelated", &[9.0, -1.0]);
    {
        let d = s.doc_mut().unwrap();
        d.text_styles.push(text.clone());
        d.dim_styles.push(dim.clone());
        d.layers.push(layer.clone());
        d.linetypes.push(linetype.clone());
    }
    let params = json!({"font": "Installed GOST font", "scale": 2});
    install(&mut s, &params).unwrap();
    let once = s.doc().unwrap().clone();
    install(&mut s, &params).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d, &once);
    assert_eq!(d.text_style("Workshop"), Some(&text));
    assert_eq!(d.dim_style("Workshop"), Some(&dim));
    assert_eq!(d.layer("Workshop"), Some(&layer));
    assert_eq!(d.linetype("Workshop"), Some(&linetype));
    assert_eq!(d.text_styles.iter().filter(|st| st.name.eq_ignore_ascii_case(NAME)).count(), 1);
    assert_eq!(d.dim_styles.iter().filter(|st| st.name.eq_ignore_ascii_case(NAME)).count(), 1);
    assert_eq!(d.text_style(NAME).unwrap().height, 0.0);
    assert_eq!(d.dim_style(NAME).unwrap().scale, 2.0);
}

#[test]
fn invalid_options_leave_every_drawing_field_unchanged() {
    let mut s = imperial_session();
    for params in [
        json!(null),
        json!([]),
        json!({"scale": 0}),
        json!({"scale": -1}),
        json!({"scale": 1e100}),
        json!({"scale": 1e-100}),
        json!({"scale": "2"}),
        json!({"scale": null}),
        json!({"scale": true}),
        json!({"font": 3}),
        json!({"font": null}),
        json!({"font": "  "}),
        json!({"font": "bad\nfont"}),
        json!({"font": "x".repeat(256)}),
    ] {
        let before = s.doc().unwrap().clone();
        assert!(install(&mut s, &params).is_err(), "{params}");
        assert_eq!(s.doc().unwrap(), &before, "{params}");
    }
}

#[test]
fn non_mm_and_unknown_units_cannot_be_relabelled_with_existing_geometry() {
    let mut s = imperial_session();
    s.execute("line", &json!({"points": [[0, 0], [1, 0]]})).unwrap();
    for units in [0, 1, 5, 6] {
        s.doc_mut().unwrap().header.set_i64("INSUNITS", units);
        let before = s.doc().unwrap().clone();
        let error = install(&mut s, &json!({})).unwrap_err().to_string();
        assert!(error.contains("millimetres"), "{error}");
        assert_eq!(s.doc().unwrap(), &before);
    }
    s.doc_mut().unwrap().header.vars.remove("INSUNITS");
    let before = s.doc().unwrap().clone();
    assert!(install(&mut s, &json!({})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
    s.doc_mut().unwrap().header.set_i64("INSUNITS", 4);
    let model = s.doc().unwrap().model.clone();
    install(&mut s, &json!({"scale": 5})).unwrap();
    assert_eq!(s.doc().unwrap().model, model, "annotation scale must not scale geometry");
}

#[test]
fn paper_geometry_also_guards_units() {
    let mut s = imperial_session();
    s.execute("line", &json!({"points": [[0, 0], [1, 0]]})).unwrap();
    let entity = s.doc().unwrap().model.iter().next().unwrap().as_ref().clone();
    // A paper-only drawing is still populated, even with an empty model.
    {
        let d = s.doc_mut().unwrap();
        d.model = cadcraft_doc::EntityStore::new();
        d.layouts.first_mut().unwrap().entities.push(entity);
    }
    let before = s.doc().unwrap().clone();
    assert!(install(&mut s, &json!({})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn empty_imperial_drawing_can_receive_metric_preset() {
    let mut s = imperial_session();
    assert_eq!(s.doc().unwrap().header.i64("INSUNITS", 0), 1);
    let result = install(&mut s, &json!({})).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.header.i64("INSUNITS", 0), 4);
    assert_eq!(d.header.i64("MEASUREMENT", 0), 1);
    assert_eq!(result["geometryRescaled"], false);
    assert!(result["warnings"][0].as_str().unwrap().contains("OpenGost"));
}

#[test]
fn rendered_dimension_uses_decimal_comma_and_paper_sizes() {
    let mut s = imperial_session();
    install(&mut s, &json!({"scale": 2})).unwrap();
    s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [120.5, 0], "at": [60, 20]})).unwrap();
    let d = s.doc().unwrap();
    let dim = d
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Dimension(dim) => Some(dim),
            _ => None,
        })
        .unwrap();
    let geometry = cadcraft_render::dimension_in(d, dim);
    assert_eq!(geometry.value, "120,5");
    assert_eq!(geometry.measurement, 120.5);
    assert_eq!(geometry.text_height, 7.0);
    assert_eq!(dim.style, NAME);
    assert!(dim.overrides.is_empty(), "setup must clear preexisting header DIM overrides");
}

#[test]
fn check_is_read_only_and_never_certifies_a_bare_drawing() {
    let mut s = imperial_session();
    let before = s.doc().unwrap().clone();
    let report = run_check(&mut s, &json!({})).unwrap();
    assert_eq!(s.doc().unwrap(), &before);
    assert_eq!(report["profilePresent"], false);
    assert_eq!(report["automaticChecksPassed"], false);
    assert_eq!(report["status"], "manual_review_required");
    assert!(report["findings"].as_array().unwrap().iter().any(|f| f["code"] == "profile.missing"));
    install(&mut s, &json!({})).unwrap();
    let before = s.doc().unwrap().clone();
    let report = run_check(&mut s, &json!({})).unwrap();
    assert_eq!(s.doc().unwrap(), &before);
    assert_eq!(report["automaticChecksPassed"], true);
    assert_eq!(report["status"], "manual_review_required");
    assert_eq!(report["manualChecks"].as_array().unwrap().len(), 5);
}

#[test]
fn check_inspects_effective_dimension_overrides_and_literal_measurement_text() {
    let mut s = imperial_session();
    install(&mut s, &json!({})).unwrap();
    s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [100, 0], "at": [50, 15]})).unwrap();
    let handle = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Dimension(_))).unwrap().handle;
    s.doc_mut()
        .unwrap()
        .modify_entity(handle, |entity| {
            if let EntityKind::Dimension(dim) = &mut entity.kind {
                dim.text = "999".into();
                dim.overrides.insert("decimalSeparator".into(), json!("."));
                dim.assoc.clear();
            }
        })
        .unwrap();
    let report = run_check(&mut s, &json!({})).unwrap();
    let findings = report["findings"].as_array().unwrap();
    for code in ["dimension.measurementReplaced", "dimension.disassociated", "dimension.preset.decimalSeparator"] {
        assert!(findings.iter().any(|f| f["code"] == code), "{code}: {report}");
    }
    assert_eq!(report["automaticChecksPassed"], false);
}

#[test]
fn block_only_geometry_also_guards_units() {
    let mut s = imperial_session();
    s.execute("line", &json!({"points": [[0, 0], [1, 0]]})).unwrap();
    let entity = s.doc().unwrap().model.iter().next().unwrap().as_ref().clone();
    let mut block = cadcraft_doc::Block::new("Part");
    block.entities.push(entity);
    {
        let d = s.doc_mut().unwrap();
        d.model = cadcraft_doc::EntityStore::new();
        d.blocks.insert("Part".into(), std::sync::Arc::new(block));
    }
    let before = s.doc().unwrap().clone();
    assert!(install(&mut s, &json!({})).is_err());
    assert_eq!(s.doc().unwrap(), &before);
}

#[test]
fn setup_clears_header_overrides_but_preserves_global_dash_scaling() {
    let mut s = imperial_session();
    {
        let d = s.doc_mut().unwrap();
        d.header.set_f64("DIMTXT", 99.0);
        d.header.set_str("DIMDSEP", ".");
        d.header.set_f64("LTSCALE", 2.0);
    }
    let result = install(&mut s, &json!({})).unwrap();
    let d = s.doc().unwrap();
    assert!(d.dim_overrides().is_empty());
    assert_eq!(d.header.f64("LTSCALE", 0.0), 2.0);
    assert!(result["warnings"].as_array().unwrap().iter().any(|v| v.as_str().unwrap().contains("LTSCALE")));
}

#[test]
fn paper_install_preserves_imperial_model_headers_geometry_and_selected_font_scale() {
    let mut s = imperial_session();
    install(&mut s, &json!({"font": "Selected GOST font", "scale": 2})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [1, 0]]})).unwrap();
    s.doc_mut().unwrap().header.set_i64("INSUNITS", 1);
    let header = s.doc().unwrap().header.clone();
    let model = s.doc().unwrap().model.clone();
    let result = install_paper(&mut s, &json!({"format": "A4"})).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.header, header);
    assert_eq!(d.model, model);
    assert_eq!(d.text_style(NAME).unwrap().font, "Selected GOST font");
    assert_eq!(d.dim_style(NAME).unwrap().scale, 2.0);
    assert_eq!(result["units"], "paper_mm");
    assert_eq!(result["modelHeaderChanged"], false);
    for params in [json!({"font": 123}), json!({"scale": "1"}), json!({"scale": 0})] {
        let before = s.doc().unwrap().clone();
        assert!(install_paper(&mut s, &params).is_err());
        assert_eq!(s.doc().unwrap(), &before);
    }
}

#[test]
fn paper_install_does_not_reset_any_existing_gost_table_record() {
    let mut s = imperial_session();
    install(&mut s, &json!({"font": "Selected GOST font", "scale": 2})).unwrap();
    {
        let d = s.doc_mut().unwrap();
        let dim = d.dim_styles.iter_mut().find(|style| style.name == NAME).unwrap();
        dim.decimals = 5;
        dim.tolerance = true;
        dim.tol_plus = 0.015;
        dim.tol_minus = 0.01;
        let text = d.text_styles.iter_mut().find(|style| style.name == NAME).unwrap();
        text.width_factor = 0.9;
        let layer = d.layer_mut("GOST-HIDDEN").unwrap();
        layer.on = false;
        layer.locked = true;
        d.linetypes.iter_mut().find(|record| record.name == "GOST-HIDDEN").unwrap().description = "Reviewed custom definition".into();
    }
    let before = s.doc().unwrap().clone();
    install_paper(&mut s, &json!({})).unwrap();
    assert_eq!(s.doc().unwrap(), &before);
    for params in [json!({"font": "Different font"}), json!({"scale": 3})] {
        let error = install_paper(&mut s, &params).unwrap_err().to_string();
        assert!(error.contains("use gost.setup explicitly"), "{error}");
        assert_eq!(s.doc().unwrap(), &before);
    }
}

#[test]
fn native_sheet_check_reports_page_frame_and_unfilled_attributes() {
    let mut s = imperial_session();
    let sheet = s.execute("gost.sheet", &json!({})).unwrap();
    let layout_name = sheet["layout"].as_str().unwrap();
    let report = run_check(&mut s, &json!({})).unwrap();
    assert_eq!(report["gostSheetsInspected"], 1);
    assert_eq!(report["automaticChecksPassed"], false);
    assert!(report["findings"].as_array().unwrap().iter().any(|f| f["code"] == "sheet.attributeEmpty"));
    {
        let d = s.doc_mut().unwrap();
        let layout = d.layouts.iter_mut().find(|layout| layout.name == layout_name).unwrap();
        layout.page.scale_to_fit = true;
        layout.page.scale = 0.9;
        layout.page.height_mm += 1.0;
    }
    let before = s.doc().unwrap().clone();
    let report = run_check(&mut s, &json!({})).unwrap();
    assert_eq!(s.doc().unwrap(), &before);
    for code in ["sheet.plot", "sheet.frame"] {
        assert!(report["findings"].as_array().unwrap().iter().any(|f| f["code"] == code), "{report}");
    }
}
