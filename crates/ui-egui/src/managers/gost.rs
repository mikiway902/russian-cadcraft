//! Thin UI for the same GOST commands exposed to scripts, CLI and MCP.

use egui::vec2;
use serde_json::{Value, json};

use super::{Action, no_drawing, set_temp, temp};
use crate::CadApp;

#[derive(Clone)]
struct State {
    uid: u64,
    layout: String,
    format: String,
    landscape: bool,
    continuation: bool,
    font: String,
    scale: f64,
    fields: Vec<(&'static str, String)>,
    result: Option<Value>,
    error: Option<String>,
}

fn initial(app: &CadApp) -> State {
    let d = app.session.doc().ok();
    let layout = (1..=10_000).map(|i| format!("Лист {i}")).find(|n| d.is_none_or(|d| d.layout(n).is_none())).unwrap_or_default();
    State {
        uid: app.session.state().map_or(0, |s| s.uid),
        layout,
        format: "A4".into(),
        landscape: false,
        continuation: false,
        font: d.and_then(|d| d.text_style("GOST")).map_or_else(|| cadcraft_fonts::GOST_FONT.into(), |s| s.font.clone()),
        scale: d.and_then(|d| d.dim_style("GOST")).map_or(1.0, |s| s.scale),
        fields: [
            "DESIGNATION",
            "TITLE",
            "MATERIAL",
            "MASS",
            "SCALE",
            "SHEET",
            "SHEETS",
            "ORG",
            "DEVELOPER",
            "CHECKER",
            "NORM_CONTROL",
            "APPROVER",
            "VERSION",
        ]
        .into_iter()
        .map(|tag| (tag, String::new()))
        .collect(),
        result: None,
        error: None,
    }
}

fn field_label(tag: &str) -> &str {
    match tag {
        "DESIGNATION" => crate::tl!("Designation"),
        "TITLE" => crate::tl!("Drawing title"),
        "MATERIAL" => crate::tl!("Material"),
        "MASS" => crate::tl!("Mass"),
        "SCALE" => crate::tl!("Drawing scale"),
        "SHEET" => crate::tl!("Sheet number"),
        "SHEETS" => crate::tl!("Number of sheets"),
        "ORG" => crate::tl!("Organization"),
        "DEVELOPER" => crate::tl!("Developed by"),
        "CHECKER" => crate::tl!("Checked by"),
        "NORM_CONTROL" => crate::tl!("Standards control"),
        "APPROVER" => crate::tl!("Approved by"),
        "VERSION" => crate::tl!("Document version"),
        _ => tag,
    }
}

pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, name: &str, open: &mut bool) -> Action {
    let Ok(doc) = app.session.state() else {
        no_drawing(ctx, crate::tl!("GOST / ESKD"), open);
        return None;
    };
    let key = format!("{name}_state");
    let mut st = temp::<State>(ctx, &key).filter(|s| s.uid == doc.uid).unwrap_or_else(|| initial(app));
    let mut action = None;
    let mut close = false;
    if name == "gostcheck" && st.result.is_none() && st.error.is_none() {
        action = Some(("gost.check", json!({})));
    }
    let title = match name {
        "gostsheet" => crate::tl!("GOST Drawing Sheet"),
        "gostcheck" => crate::tl!("GOST Drawing Check"),
        _ => crate::tl!("GOST Drawing Setup"),
    };
    egui::Window::new(title).id(egui::Id::unique(name)).open(open).default_size(vec2(540.0, 560.0)).resizable(true).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.75).show(ui, |ui| {
            ui.label(crate::tl!("ESKD profile for millimetre mechanical drawings. This is not a conformity certificate."));
            if name != "gostcheck" {
                ui.separator();
                let l = ui.label(crate::tl!("Drawing font family or file"));
                ui.text_edit_singleline(&mut st.font).labelled_by(l.id);
                ui.label(crate::tl!("OpenGost Type B is included for offline GOST lettering. Verify final printed output."));
            }
            if name == "gostsetup" {
                ui.horizontal(|ui| {
                    let l = ui.label(crate::tl!("Model units per paper millimetre"));
                    ui.add(egui::DragValue::new(&mut st.scale).range(0.0001..=10_000.0).speed(0.1)).labelled_by(l.id);
                });
                ui.label(crate::tl!("Use 2 for a 1:2 model view; use 1 in paper space. Existing geometry is never rescaled."));
                if ui.button(crate::tl!("Apply GOST Profile")).clicked() {
                    action = Some(("gost.setup", json!({"font": st.font.trim(), "scale": st.scale})));
                }
            } else if name == "gostsheet" {
                egui::Grid::new("gost_sheet_options").num_columns(2).show(ui, |ui| {
                    let l = ui.label(crate::tl!("Layout name"));
                    ui.text_edit_singleline(&mut st.layout).labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label(crate::tl!("Paper format"));
                    craft_ui::ComboBox::from_id_salt("gost_paper")
                        .selected_text(&st.format)
                        .show_ui(ui, |ui| {
                            for f in ["A4", "A3", "A2", "A1", "A0"] {
                                ui.selectable_value(&mut st.format, f.into(), f);
                            }
                        })
                        .response
                        .labelled_by(l.id);
                    ui.end_row();
                });
                if st.format == "A4" {
                    st.landscape = false;
                }
                ui.add_enabled_ui(st.format != "A4", |ui| {
                    ui.checkbox(&mut st.landscape, crate::tl!("Landscape"));
                });
                ui.checkbox(&mut st.continuation, crate::tl!("Continuation sheet (form 2a)"));
                ui.label(crate::tl!("First drawing sheet: form 1, 185 × 55 mm. Continuation: form 2a, 185 × 15 mm."));
                ui.separator();
                egui::Grid::new("gost_title_fields").num_columns(2).show(ui, |ui| {
                    for (tag, value) in &mut st.fields {
                        if st.continuation && !matches!(*tag, "DESIGNATION" | "VERSION" | "SHEET") {
                            continue;
                        }
                        let l = ui.label(field_label(tag));
                        ui.text_edit_singleline(value).labelled_by(l.id);
                        ui.end_row();
                    }
                });
                ui.label(crate::tl!("Empty approval and signature fields remain empty. Edit block attributes for additional fields."));
                if ui.button(crate::tl!("Create GOST Sheet")).clicked() {
                    let fields: serde_json::Map<String, Value> = st
                        .fields
                        .iter()
                        .filter(|(k, v)| !v.trim().is_empty() && (!st.continuation || matches!(*k, "DESIGNATION" | "VERSION" | "SHEET")))
                        .map(|(k, v)| ((*k).into(), json!(v.trim())))
                        .collect();
                    action = Some((
                        "gost.sheet",
                        json!({"layout": st.layout.trim(), "format": st.format,
                        "landscape": st.landscape, "form": if st.continuation { "2a" } else { "1" },
                        "font": st.font.trim(), "fields": fields}),
                    ));
                }
            } else if ui.button(crate::tl!("Run GOST Check")).clicked() {
                action = Some(("gost.check", json!({})));
            }
            if let Some(e) = &st.error {
                ui.colored_label(egui::Color32::from_rgb(220, 80, 70), e);
            }
            if let Some(result) = &st.result {
                ui.separator();
                ui.label(crate::tl!("Command report (includes unverified and manual requirements)"));
                if let Ok(t) = serde_json::to_string_pretty(result) {
                    ui.add(egui::Label::new(egui::RichText::new(t).monospace()).wrap());
                }
            }
            ui.separator();
            if ui.button(crate::tl!("Close")).clicked() {
                close = true;
            }
        });
    });
    if let Some((cmd, params)) = action {
        match app.run(cmd, params) {
            Ok(v) => {
                st.result = Some(v);
                st.error = None;
                // Keep the sheet report visible: it includes limitations and font warnings.
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
