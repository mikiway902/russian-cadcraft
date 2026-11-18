//! Book properties edit the same authoritative engine records used by CLI/MCP.
use super::{Action, no_drawing, set_temp, temp};
use crate::CadApp;
use egui::vec2;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone)]
struct State {
    uid: u64,
    report: Value,
    selected: String,
    format: String,
    landscape: bool,
    automatic: bool,
    common: BTreeMap<String, String>,
    local: BTreeMap<String, String>,
    error: Option<String>,
}

fn read_fields(v: &Value) -> BTreeMap<String, String> {
    v.as_object().map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()).unwrap_or_default()
}

fn selected(st: &mut State) {
    let sheet = st.report["sheets"].as_array().and_then(|a| a.iter().find(|s| s["layout"].as_str() == Some(st.selected.as_str())));
    if let Some(sheet) = sheet {
        st.format = sheet["format"].as_str().unwrap_or("A4").into();
        st.landscape = sheet["landscape"].as_bool().unwrap_or(false);
        st.automatic = sheet["automatic"].as_bool().unwrap_or(true);
        st.local = read_fields(&sheet["fields"]);
    }
}
fn refresh(app: &mut CadApp, st: &mut State) {
    match app.run("gost.document", json!({})) {
        Ok(report) => {
            st.report = report;
            st.common = read_fields(&st.report["fields"]);
            for tag in ["DESIGNATION", "TITLE", "MATERIAL", "MASS", "ORG", "VERSION", "DEVELOPER", "CHECKER", "NORM_CONTROL", "APPROVER"] {
                st.common.entry(tag.into()).or_default();
            }
            if !st.report["sheets"].as_array().is_some_and(|a| a.iter().any(|s| s["layout"].as_str() == Some(st.selected.as_str()))) {
                st.selected = st.report["sheets"][0]["layout"].as_str().unwrap_or_default().into();
            }
            selected(st);
            st.error = None;
        }
        Err(e) => st.error = Some(e),
    }
}
fn label(tag: &str) -> String {
    if let Some(role) = tag.strip_suffix("_SIGNATURE") {
        return format!("{} · {}", label(role), crate::tl!("Signature"));
    }
    if let Some(role) = tag.strip_suffix("_DATE") {
        return format!("{} · {}", label(role), crate::tl!("Date"));
    }
    if let Some(rest) = tag.strip_prefix("REV")
        && let Some((number, field)) = rest.split_once('_')
    {
        return format!("{} {} · {}", crate::tl!("Revision"), number, label(field));
    }
    match tag {
        "DESIGNATION" => crate::tl!("Designation"),
        "TITLE" => crate::tl!("Title"),
        "MATERIAL" => crate::tl!("Material"),
        "MASS" => crate::tl!("Mass"),
        "ORG" => crate::tl!("Organization"),
        "VERSION" => crate::tl!("Version"),
        "DEVELOPER" => crate::tl!("Developer"),
        "CHECKER" => crate::tl!("Checker"),
        "NORM_CONTROL" => crate::tl!("Norm control"),
        "APPROVER" => crate::tl!("Approver"),
        "SCALE" => crate::tl!("Scale"),
        "TECH_CONTROL" => crate::tl!("Technical control"),
        "EXTRA_ROLE" => crate::tl!("Additional approver"),
        "INVENTORY" => crate::tl!("Original inventory number"),
        "DUPLICATE_INVENTORY" => crate::tl!("Duplicate inventory number"),
        "REPLACES_INVENTORY" => crate::tl!("Replaces inventory number"),
        "INVENTORY_SIGNATURE" => crate::tl!("Original inventory approval"),
        "DUPLICATE_SIGNATURE" => crate::tl!("Duplicate inventory approval"),
        "COPIED_BY" => crate::tl!("Copied by"),
        "LITERA1" => crate::tl!("Document letter 1"),
        "LITERA2" => crate::tl!("Document letter 2"),
        "LITERA3" => crate::tl!("Document letter 3"),
        "CHANGE" => crate::tl!("Change"),
        "SHEET" => crate::tl!("Sheet"),
        "DOCUMENT" => crate::tl!("Document number"),
        "SIGNATURE" => crate::tl!("Signature"),
        "DATE" => crate::tl!("Date"),
        _ => tag,
    }
    .to_string()
}

pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, name: &str, open: &mut bool) -> Action {
    let Ok(doc) = app.session.state() else {
        no_drawing(ctx, crate::tl!("GOST / ESKD"), open);
        return None;
    };
    let uid = doc.uid;
    let key = format!("{name}_state");
    let mut st = match temp::<State>(ctx, &key).filter(|s| s.uid == uid) {
        Some(s) => s,
        None => {
            let selected = app.session.space().layout().unwrap_or("").to_string();
            let mut s = State {
                uid,
                report: Value::Null,
                selected,
                format: "A4".into(),
                landscape: false,
                automatic: true,
                common: BTreeMap::new(),
                local: BTreeMap::new(),
                error: None,
            };
            refresh(app, &mut s);
            s
        }
    };
    let is_document = name == "gostdocument";
    let title = if is_document { crate::tl!("GOST Document Properties") } else { crate::tl!("GOST Sheet Properties") };
    let mut action: Option<(&str, Value)> = None;
    let mut close = false;
    egui::Window::new(title).id(egui::Id::unique(name)).open(open).default_size(vec2(620.0, 620.0)).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.75).show(ui, |ui| {
            ui.label(crate::tl!("Managed sheets share document properties; numbering and first/continuation forms update automatically."));
            if is_document {
                egui::Grid::new("gost_common_fields").num_columns(2).show(ui, |ui| {
                    for (tag, value) in &mut st.common {
                        let l = ui.label(label(tag));
                        ui.text_edit_singleline(value).labelled_by(l.id);
                        ui.end_row();
                    }
                });
                if ui.button(crate::tl!("Apply common properties")).clicked() {
                    action = Some(("gost.document", json!({"fields":st.common})));
                }
            } else {
                let old = st.selected.clone();
                craft_ui::ComboBox::from_id_salt("gost_existing_sheet").selected_text(&st.selected).show_ui(ui, |ui| {
                    if let Some(sheets) = st.report["sheets"].as_array() {
                        for sheet in sheets {
                            if let Some(name) = sheet["layout"].as_str() {
                                ui.selectable_value(&mut st.selected, name.to_string(), name);
                            }
                        }
                    }
                });
                if st.selected != old {
                    selected(&mut st);
                }
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("Paper format"));
                    craft_ui::ComboBox::from_id_salt("gost_existing_format").selected_text(&st.format).show_ui(ui, |ui| {
                        for f in ["A4", "A3", "A2", "A1", "A0"] {
                            ui.selectable_value(&mut st.format, f.into(), f);
                        }
                    });
                    if st.format == "A4" {
                        st.landscape = false;
                    }
                    ui.add_enabled_ui(st.format != "A4", |ui| {
                        ui.checkbox(&mut st.landscape, crate::tl!("Landscape"));
                    });
                });
                ui.checkbox(&mut st.automatic, crate::tl!("Automatic drawing-book sheet"));
                ui.label(crate::tl!("Turning automatic mode off makes this an independent sheet, excluded from book numbering."));
                egui::Grid::new("gost_local_fields").num_columns(2).show(ui, |ui| {
                    for (tag, value) in &mut st.local {
                        let local = tag == "SCALE"
                            || tag.starts_with("REV")
                            || tag.contains("INVENTORY")
                            || tag == "COPIED_BY"
                            || (!st.automatic && matches!(tag.as_str(), "SHEET" | "SHEETS"));
                        if !local {
                            continue;
                        }
                        let l = ui.label(label(tag));
                        ui.text_edit_singleline(value).labelled_by(l.id);
                        ui.end_row();
                    }
                });
                if ui.button(crate::tl!("Apply sheet properties")).clicked() {
                    let fields: BTreeMap<_, _> = st
                        .local
                        .iter()
                        .filter(|(k, _)| {
                            k.as_str() == "SCALE"
                                || k.starts_with("REV")
                                || k.contains("INVENTORY")
                                || k.as_str() == "COPIED_BY"
                                || (!st.automatic && matches!(k.as_str(), "SHEET" | "SHEETS"))
                        })
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    action = Some((
                        "gost.sheet.set",
                        json!({"layout":st.selected,"format":st.format,"landscape":st.landscape,"automatic":st.automatic,"fields":fields}),
                    ));
                }
                ui.horizontal(|ui| {
                    if ui.button(crate::tl!("Show sheet")).clicked() {
                        action = Some(("layout.set", json!({"name":st.selected})));
                    }
                    let mut names: Vec<_> =
                        app.session.doc().map(|d| d.layouts.iter().map(|l| (l.tab_order, l.name.clone())).collect()).unwrap_or_default();
                    names.sort_by_key(|(i, _)| *i);
                    if let Some(index) = names.iter().position(|(_, n)| n == &st.selected) {
                        if ui.add_enabled(index > 0, egui::Button::new(crate::tl!("Move up"))).clicked() {
                            action = Some(("gost.sheet.order", json!({"layout":st.selected,"index":index})));
                        }
                        if ui.add_enabled(index + 1 < names.len(), egui::Button::new(crate::tl!("Move down"))).clicked() {
                            action = Some(("gost.sheet.order", json!({"layout":st.selected,"index":index+2})));
                        }
                    }
                });
            }
            ui.separator();
            if ui.button(crate::tl!("Reload properties")).clicked() {
                refresh(app, &mut st);
            }
            if let Some(e) = &st.error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            if ui.button(crate::tl!("Close")).clicked() {
                close = true;
            }
        });
    });
    if let Some((cmd, p)) = action {
        match app.run(cmd, p) {
            Ok(_) => refresh(app, &mut st),
            Err(e) => st.error = Some(e),
        }
    }
    set_temp(ctx, &key, st);
    if close {
        *open = false;
    }
    None
}
