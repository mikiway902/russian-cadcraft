//! Specification properties and manual rows; extraction, pagination and numbering stay in the engine.
use super::{Action, no_drawing, set_temp, temp};
use crate::CadApp;
use serde_json::{Value, json};
use std::collections::BTreeMap;
const SECTIONS: [&str; 9] = [
    "Документация",
    "Комплексы",
    "Сборочные единицы",
    "Детали",
    "Программные изделия и базы данных",
    "Стандартные изделия",
    "Прочие изделия",
    "Материалы",
    "Комплекты",
];
#[derive(Clone)]
struct State {
    uid: u64,
    handle: String,
    layout: String,
    sources: String,
    replace_sources: bool,
    manual: Vec<Value>,
    fields: BTreeMap<String, String>,
    result: Option<Value>,
    error: Option<String>,
}
fn read(app: &mut CadApp, st: &mut State) {
    match app.run("gost.specification.inspect", json!({"handle":st.handle})) {
        Ok(v) => {
            let r = &v["record"];
            st.layout = r["layout"].as_str().unwrap_or("").into();
            st.manual = r["manual"].as_array().cloned().unwrap_or_default();
            st.fields = serde_json::from_value(r["fields"].clone()).unwrap_or_default();
            st.sources = r["linked"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|l| l["sources"].as_array().into_iter().flatten())
                .filter_map(Value::as_u64)
                .map(|n| format!("{n:X}"))
                .collect::<Vec<_>>()
                .join(" ");
            st.replace_sources = false;
            st.error = None;
            st.result = Some(v);
        }
        Err(e) => st.error = Some(e),
    }
}
fn field(ui: &mut egui::Ui, row: &mut Value, key: &str) {
    let mut value = row[key].as_str().unwrap_or("").to_string();
    if ui.add(egui::TextEdit::singleline(&mut value).desired_width(160.0).char_limit(255)).changed() {
        row[key] = json!(value);
    }
}
pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let Ok(doc) = app.session.state() else {
        no_drawing(ctx, crate::tl!("GOST / ESKD"), open);
        return None;
    };
    let uid = doc.uid;
    let mut st = temp::<State>(ctx, "gost_specification_state").filter(|s| s.uid == uid).unwrap_or_else(|| {
        let selected = app.session.selection().first().map(|h| h.hex()).unwrap_or_default();
        let fields = app.run("gost.document", json!({})).ok().and_then(|r| serde_json::from_value(r["fields"].clone()).ok()).unwrap_or_default();
        let mut st = State {
            uid,
            handle: selected,
            layout: "Спецификация".into(),
            sources: String::new(),
            replace_sources: false,
            manual: Vec::new(),
            fields,
            result: None,
            error: None,
        };
        if !st.handle.is_empty() {
            read(app, &mut st);
            if st.error.is_some() {
                st.handle.clear();
                st.error = None;
            }
        }
        st
    });
    let mut load = false;
    let mut apply = false;
    let mut close = false;
    egui::Window::new(crate::tl!("GOST Specification"))
        .id(egui::Id::unique("gostspecification"))
        .open(open)
        .default_size(egui::vec2(780.0, 650.0))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.8).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("Existing object handle"));
                    ui.text_edit_singleline(&mut st.handle);
                    if ui.button(crate::tl!("Load")).clicked() {
                        load = true;
                    }
                    if ui.button(crate::tl!("New")).clicked() {
                        st.handle.clear();
                        st.result = None;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("Layout name"));
                    ui.add_enabled(st.handle.is_empty(), egui::TextEdit::singleline(&mut st.layout));
                });
                ui.label(crate::tl!("Source block handles (space-separated). Attributes: DESIGNATION, NAME, SECTION, MATERIAL."));
                if ui.text_edit_singleline(&mut st.sources).changed() {
                    st.replace_sources = true;
                }
                if ui.button(crate::tl!("Use selected source blocks")).clicked() {
                    st.sources = app.session.selection().iter().map(|h| h.hex()).collect::<Vec<_>>().join(" ");
                    st.replace_sources = true;
                }
                ui.collapsing(crate::tl!("Title block fields"), |ui| {
                    egui::Grid::new("spec_fields").show(ui, |ui| {
                        for key in ["TITLE", "DESIGNATION", "ORG", "DEVELOPER", "CHECKER", "NORM_CONTROL", "APPROVER", "VERSION"] {
                            ui.label(key);
                            ui.text_edit_singleline(st.fields.entry(key.into()).or_default());
                            ui.end_row();
                        }
                    });
                });
                ui.heading(crate::tl!("Manual specification rows"));
                let mut remove = None;
                for (index, row) in st.manual.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(format!("{} {}", crate::tl!("Row"), index + 1));
                                craft_ui::ComboBox::from_id_salt("section").selected_text(row["section"].as_str().unwrap_or("Детали")).show_ui(
                                    ui,
                                    |ui| {
                                        for section in SECTIONS {
                                            if ui.selectable_label(row["section"].as_str() == Some(section), section).clicked() {
                                                row["section"] = json!(section);
                                            }
                                        }
                                    },
                                );
                                if ui.button(crate::tl!("Delete row")).clicked() {
                                    remove = Some(index);
                                }
                            });
                            egui::Grid::new("fields").num_columns(4).show(ui, |ui| {
                                for (key, label) in [
                                    ("designation", crate::tl!("Designation")),
                                    ("name", crate::tl!("Name")),
                                    ("material", crate::tl!("Material")),
                                    ("note", crate::tl!("Note")),
                                ] {
                                    ui.label(label);
                                    field(ui, row, key);
                                    if key == "name" || key == "note" {
                                        ui.end_row();
                                    }
                                }
                                ui.label(crate::tl!("Quantity"));
                                let mut n = row["quantity"].as_u64().unwrap_or(1);
                                if ui.add(egui::DragValue::new(&mut n).range(1..=1_000_000)).changed() {
                                    row["quantity"] = json!(n);
                                }
                                ui.end_row();
                            });
                        });
                    });
                }
                if let Some(i) = remove {
                    st.manual.remove(i);
                }
                if ui.add_enabled(st.manual.len() < 128, egui::Button::new(crate::tl!("Add row"))).clicked()
                    && let Some(id) = (1..10000).map(|n| format!("manual-{n}")).find(|id| !st.manual.iter().any(|r| r["id"].as_str() == Some(id)))
                {
                    st.manual.push(json!({"id":id,"section":"Детали","designation":"","name":"","material":"","note":"","quantity":1}));
                }
                if ui.button(crate::tl!("Apply specification")).clicked() {
                    apply = true;
                }
                if let Some(v) = &st.result {
                    if let Some(rows) = v["rows"].as_array() {
                        ui.label(format!(
                            "{}: {} / {}: {}",
                            crate::tl!("Rows"),
                            rows.len(),
                            crate::tl!("Sheets"),
                            v["pages"].as_array().map_or(0, Vec::len)
                        ));
                        egui::Grid::new("spec_preview").striped(true).show(ui, |ui| {
                            for row in rows {
                                ui.label(row["position"].as_u64().map(|n| n.to_string()).unwrap_or_default());
                                ui.label(row["fields"]["designation"].as_str().unwrap_or(""));
                                ui.label(row["fields"]["name"].as_str().unwrap_or(""));
                                ui.label(row["quantity"].to_string());
                                ui.end_row();
                            }
                        });
                    }
                    if let Some(a) = v["findings"].as_array() {
                        for f in a {
                            ui.colored_label(ui.visuals().warn_fg_color, f.as_str().unwrap_or(""));
                        }
                    }
                }
                if let Some(e) = &st.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                if ui.button(crate::tl!("Close")).clicked() {
                    close = true;
                }
            });
        });
    if load {
        read(app, &mut st);
    }
    if apply {
        let mut p = json!({"manual":st.manual,"fields":st.fields});
        let command = if st.handle.is_empty() {
            p["layout"] = json!(st.layout);
            "gost.specification"
        } else {
            p["handle"] = json!(st.handle);
            "gost.specification.edit"
        };
        if st.handle.is_empty() || st.replace_sources {
            p["sources"] = json!(st.sources.split_whitespace().collect::<Vec<_>>());
        }
        match app.run(command, p) {
            Ok(v) => {
                st.handle = v["handle"].as_str().unwrap_or("").into();
                st.error = None;
                st.result = Some(v);
                st.replace_sources = false;
            }
            Err(e) => st.error = Some(e),
        }
    }
    set_temp(ctx, "gost_specification_state", st);
    if close {
        *open = false;
    }
    None
}
