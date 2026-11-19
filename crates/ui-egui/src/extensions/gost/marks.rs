//! Typed property controls for native engineering symbols; no separate rendering path.
use crate::CadApp;
use crate::managers::{Action, no_drawing, set_temp, temp};
use egui::vec2;
use serde_json::{Value, json};

#[derive(Clone)]
struct State {
    uid: u64,
    kind: String,
    handle: Option<String>,
    parameters: Value,
    at: [f64; 2],
    rotation: f64,
    error: Option<String>,
    result: Option<Value>,
}
fn defaults(kind: &str) -> Value {
    match kind {
        "roughness" => json!({"parameter":"Ra","value":3.2,"removal":"permitted","lay":null,"allSurfaces":false,"scale":1.0,"textHeight":3.5}),
        "weld" => {
            json!({"type":"fillet","size":4.0,"standard":"ГОСТ 5264-80","joint":null,"method":null,"staggered":false,"length":null,"pitch":null,"side":"arrow","continuous":true,"field":false,"allAround":false,"leader":[-12,-10],"scale":1.0,"textHeight":3.5})
        }
        "thread" => json!({"nominal":10.0,"pitch":1.5,"length":20.0,"side":"external","view":"longitudinal","scale":1.0,"textHeight":3.5}),
        _ => json!({"symbol":"position","tol1":0.1,"datums":[],"rotation":0.0}),
    }
}
fn choice(ui: &mut egui::Ui, p: &mut Value, key: &str, label: &str, options: &[(&str, &str)], nullable: bool) {
    ui.label(label);
    craft_ui::ComboBox::from_id_salt(key).selected_text(options.iter().find(|(v, _)| p[key].as_str() == Some(*v)).map_or("—", |(_, l)| *l)).show_ui(
        ui,
        |ui| {
            if nullable && ui.selectable_label(p[key].is_null(), "—").clicked() {
                p[key] = Value::Null;
            }
            for (value, label) in options {
                if ui.selectable_label(p[key].as_str() == Some(*value), *label).clicked() {
                    p[key] = json!(value);
                }
            }
        },
    );
    ui.end_row();
}
fn number(ui: &mut egui::Ui, p: &mut Value, key: &str, label: &str, range: std::ops::RangeInclusive<f64>, nullable: bool) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut enabled = !p[key].is_null();
        if nullable && ui.checkbox(&mut enabled, crate::tl!("Specify")).changed() {
            p[key] = if enabled { json!(1.0) } else { Value::Null };
        }
        if !nullable || enabled {
            let mut value = p[key].as_f64().unwrap_or(1.0);
            if ui.add(egui::DragValue::new(&mut value).range(range).speed(0.1)).changed() {
                p[key] = json!(value);
            }
        }
    });
    ui.end_row();
}
fn flag(ui: &mut egui::Ui, p: &mut Value, key: &str, label: &str) {
    let mut value = p[key].as_bool().unwrap_or(false);
    if ui.checkbox(&mut value, label).changed() {
        p[key] = json!(value);
    }
}

pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, name: &str, open: &mut bool) -> Action {
    let Ok(doc) = app.session.state() else {
        no_drawing(ctx, crate::tl!("GOST / ESKD"), open);
        return None;
    };
    let selected = if name == "gostsymboledit" { app.session.selection().first().map(|h| h.hex()).unwrap_or_default() } else { String::new() };
    let key = format!("{name}_{selected}_state");
    let mut st = temp::<State>(ctx, &key).filter(|s| s.uid == doc.uid).unwrap_or_else(|| {
        let kind = match name {
            "gostroughness" => "roughness",
            "gostweld" => "weld",
            "gostthread" => "thread",
            _ => "tolerance",
        };
        State {
            uid: doc.uid,
            kind: kind.into(),
            handle: None,
            parameters: defaults(kind),
            at: [app.session.cursor.x, app.session.cursor.y],
            rotation: 0.0,
            error: None,
            result: None,
        }
    });
    if name == "gostsymboledit" && st.handle.is_none() && st.error.is_none() {
        match app.run("gost.symbol.inspect", json!({})) {
            Ok(v) => {
                st.handle = v["handle"].as_str().map(str::to_string);
                st.kind = v["kind"].as_str().unwrap_or("roughness").into();
                st.parameters = v["parameters"].clone();
            }
            Err(e) => st.error = Some(e),
        }
    }
    let mut apply = false;
    let mut close = false;
    egui::Window::new(crate::tl!("GOST Engineering Symbol")).id(egui::Id::unique(name)).open(open).default_size(vec2(520.0, 600.0)).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.75).show(ui, |ui| {
            ui.label(crate::tl!("Editable native geometry. Verify manufacturing requirements and final printed output."));
            if let Some(h) = &st.handle {
                ui.label(format!("{}: {h}", crate::tl!("Object")));
            }
            let p = &mut st.parameters;
            egui::Grid::new("gost_engineering_fields").num_columns(2).show(ui, |ui| {
                if st.handle.is_none() {
                    ui.label(crate::tl!("Insertion point"));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut st.at[0]).speed(1.0));
                        ui.add(egui::DragValue::new(&mut st.at[1]).speed(1.0));
                    });
                    ui.end_row();
                }
                match st.kind.as_str() {
                    "roughness" => {
                        choice(ui, p, "parameter", crate::tl!("Roughness parameter"), &[("Ra", "Ra"), ("Rz", "Rz")], false);
                        number(ui, p, "value", crate::tl!("Value (µm)"), 0.001..=10000.0, false);
                        choice(
                            ui,
                            p,
                            "removal",
                            crate::tl!("Material removal"),
                            &[("permitted", crate::tl!("Permitted")), ("required", crate::tl!("Required")), ("prohibited", crate::tl!("Prohibited"))],
                            false,
                        );
                        choice(
                            ui,
                            p,
                            "lay",
                            crate::tl!("Surface lay"),
                            &[
                                ("parallel", crate::tl!("Parallel")),
                                ("perpendicular", crate::tl!("Perpendicular")),
                                ("crossed", crate::tl!("Crossed")),
                                ("multidirectional", crate::tl!("Multidirectional")),
                                ("circular", crate::tl!("Circular")),
                                ("radial", crate::tl!("Radial")),
                            ],
                            true,
                        );
                    }
                    "weld" => {
                        choice(
                            ui,
                            p,
                            "type",
                            crate::tl!("Weld type"),
                            &[("fillet", crate::tl!("Fillet weld")), ("butt", crate::tl!("Butt weld"))],
                            false,
                        );
                        for (key, label, nullable) in [
                            ("standard", crate::tl!("Weld joint standard"), false),
                            ("joint", crate::tl!("Joint code (blank: Т1/С1)"), true),
                            ("method", crate::tl!("Welding process (optional)"), true),
                        ] {
                            ui.label(label);
                            let mut value = p[key].as_str().unwrap_or("").to_string();
                            if ui.text_edit_singleline(&mut value).changed() {
                                p[key] = if nullable && value.is_empty() { Value::Null } else { json!(value) };
                            }
                            ui.end_row();
                        }
                        if p["type"] == "fillet" {
                            if p["size"].is_null() {
                                p["size"] = json!(4.0);
                            }
                            number(ui, p, "size", crate::tl!("Fillet leg (mm)"), 0.1..=10000.0, false);
                        } else {
                            p["size"] = Value::Null;
                        }
                        number(ui, p, "length", crate::tl!("Weld length (mm)"), 0.1..=100000.0, true);
                        number(ui, p, "pitch", crate::tl!("Weld pitch (mm)"), 0.1..=100000.0, true);
                        choice(ui, p, "side", crate::tl!("Side"), &[("arrow", crate::tl!("Arrow side")), ("other", crate::tl!("Other side"))], false);
                    }
                    "thread" => {
                        number(ui, p, "nominal", crate::tl!("Nominal diameter (mm)"), 0.1..=10000.0, false);
                        number(ui, p, "pitch", crate::tl!("Pitch (mm)"), 0.01..=1000.0, false);
                        number(ui, p, "length", crate::tl!("Thread length (mm)"), 0.1..=100000.0, false);
                        choice(
                            ui,
                            p,
                            "side",
                            crate::tl!("Thread type"),
                            &[("external", crate::tl!("External")), ("internal", crate::tl!("Internal"))],
                            false,
                        );
                        choice(
                            ui,
                            p,
                            "view",
                            crate::tl!("Representation"),
                            &[("longitudinal", crate::tl!("Longitudinal")), ("end", crate::tl!("End view")), ("both", crate::tl!("Both views"))],
                            false,
                        );
                    }
                    _ => {
                        choice(
                            ui,
                            p,
                            "symbol",
                            crate::tl!("Geometric characteristic"),
                            &[
                                ("position", crate::tl!("Position")),
                                ("flatness", crate::tl!("Flatness")),
                                ("straightness", crate::tl!("Straightness")),
                                ("circularity", crate::tl!("Circularity")),
                                ("cylindricity", crate::tl!("Cylindricity")),
                                ("parallelism", crate::tl!("Parallelism")),
                                ("perpendicularity", crate::tl!("Perpendicularity")),
                                ("circularrunout", crate::tl!("Circular runout")),
                                ("totalrunout", crate::tl!("Total runout")),
                            ],
                            false,
                        );
                        number(ui, p, "tol1", crate::tl!("Tolerance (mm)"), 0.001..=1000.0, false);
                        for index in 0..3 {
                            ui.label(format!("{} {}", crate::tl!("Datum"), index + 1));
                            let mut value = p["datums"].as_array().and_then(|a| a.get(index)).and_then(Value::as_str).unwrap_or("").to_string();
                            if ui.add(egui::TextEdit::singleline(&mut value).char_limit(3)).changed() {
                                let mut bases = p["datums"].as_array().cloned().unwrap_or_default();
                                if bases.len() <= index {
                                    bases.resize(index + 1, Value::Null);
                                }
                                if let Some(base) = bases.get_mut(index) {
                                    *base = json!(value.trim().to_uppercase());
                                }
                                bases.retain(|v| v.as_str().is_some_and(|s| !s.is_empty()));
                                p["datums"] = json!(bases);
                            }
                            ui.end_row();
                        }
                    }
                }
                if st.kind != "tolerance" {
                    number(ui, p, "scale", crate::tl!("Model units per paper millimetre"), 0.0001..=10000.0, false);
                    number(ui, p, "textHeight", crate::tl!("Text height (mm)"), 2.5..=14.0, false);
                }
                if st.handle.is_none() {
                    ui.label(crate::tl!("Rotation (degrees)"));
                    ui.add(egui::DragValue::new(&mut st.rotation).range(-360.0..=360.0));
                    ui.end_row();
                }
            });
            match st.kind.as_str() {
                "roughness" => flag(ui, p, "allSurfaces", crate::tl!("All surfaces")),
                "weld" => {
                    flag(ui, p, "continuous", crate::tl!("Continuous weld"));
                    flag(ui, p, "field", crate::tl!("Field weld"));
                    flag(ui, p, "allAround", crate::tl!("All around"));
                    flag(ui, p, "staggered", crate::tl!("Staggered intermittent weld"));
                }
                "thread" => {
                    ui.label(crate::tl!("Symbolic 2D thread, not a modeled 3D thread."));
                }
                _ => {}
            }
            if ui.add_enabled(name != "gostsymboledit" || st.handle.is_some(), egui::Button::new(crate::tl!("Apply"))).clicked() {
                apply = true;
            }
            if let Some(e) = &st.error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            if let Some(v) = &st.result
                && let Some(labels) = v["labels"].as_array()
            {
                for label in labels {
                    if let Some(t) = label.as_str() {
                        ui.label(t);
                    }
                }
            }
            if ui.button(crate::tl!("Close")).clicked() {
                close = true;
            }
        });
    });
    if apply {
        let (command, params) = if let Some(handle) = &st.handle {
            ("gost.symbol.edit".to_string(), json!({"handle":handle,"patch":st.parameters}))
        } else {
            let mut params = st.parameters.clone();
            params["at"] = json!(st.at);
            params["rotation"] = json!(st.rotation);
            (format!("gost.{}", st.kind), params)
        };
        match app.run(&command, params) {
            Ok(v) => {
                st.result = Some(v.clone());
                st.error = None;
                if st.kind != "tolerance" {
                    st.handle = v["handle"].as_str().map(str::to_string);
                }
            }
            Err(e) => st.error = Some(e),
        }
    }
    set_temp(ctx, &key, st);
    if close {
        *open = false;
    }
    None
}
