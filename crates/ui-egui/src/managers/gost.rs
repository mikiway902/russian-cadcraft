//! Thin UI for the same GOST commands exposed to scripts, CLI and MCP.

use egui::vec2;
use serde_json::{Value, json};

use super::{Action, no_drawing, set_temp, temp};
use crate::CadApp;

/// Like the text editor, a manager cache belongs to consecutive UI passes only.
/// The accepted snapshot distinguishes an intentional draft from a stale clean editor.
#[derive(Clone)]
pub(super) struct Editor<T> {
    pub pass: u64,
    pub selection: String,
    revision: u64,
    baseline: std::sync::Arc<T>,
    pub conflict: bool,
}

impl<T: Clone + PartialEq> Editor<T> {
    pub fn new(pass: u64, revision: u64, selection: String, state: &T) -> Self {
        Self { pass, revision, selection, baseline: std::sync::Arc::new(state.clone()), conflict: false }
    }

    pub fn baseline(&self) -> &T {
        &self.baseline
    }

    pub fn reopened(&self, pass: u64) -> bool {
        self.pass.saturating_add(1) < pass
    }

    pub fn observe(&mut self, pass: u64, revision: u64, owner_changed: bool, state: &T) -> bool {
        self.pass = pass;
        if revision == self.revision && !owner_changed {
            return false;
        }
        if self.conflict || state != self.baseline.as_ref() {
            self.conflict = true;
            return false;
        }
        true
    }

    pub fn accept(&mut self, revision: u64, state: &T) {
        self.revision = revision;
        self.baseline = std::sync::Arc::new(state.clone());
        self.conflict = false;
    }

    pub fn warning(&self, ui: &mut egui::Ui) {
        if self.conflict {
            ui.colored_label(ui.visuals().error_fg_color, crate::tl!("The drawing changed. Close the window and open it again."));
        }
    }
}

#[derive(Clone)]
struct State {
    uid: u64,
    revision: u64,
    automatic_continuation: Option<bool>,
    layout: String,
    format: String,
    landscape: bool,
    continuation: bool,
    automatic: bool,
    font: String,
    scale: f64,
    fields: Vec<(&'static str, String)>,
    result: Option<Value>,
    error: Option<String>,
}

fn initial(app: &mut CadApp) -> State {
    let book = app.run("gost.document", json!({}));
    let d = app.session.doc().ok();
    let layout = (1..=10_000).map(|i| format!("Лист {i}")).find(|n| d.is_none_or(|d| d.layout(n).is_none())).unwrap_or_default();
    State {
        uid: app.session.state().map_or(0, |s| s.uid),
        revision: app.session.state().map_or(0, |s| s.revision),
        automatic_continuation: book.as_ref().ok().map(managed_continuation),
        layout,
        format: "A4".into(),
        landscape: false,
        continuation: false,
        automatic: true,
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
        error: book.err(),
    }
}

fn managed_continuation(report: &Value) -> bool {
    report["sheets"].as_array().is_some_and(|sheets| sheets.iter().any(|s| s["automatic"].as_bool() == Some(true)))
}

fn sheet_params(st: &State, continuation: bool) -> Value {
    let fields: serde_json::Map<String, Value> = st
        .fields
        .iter()
        .filter(|(k, v)| {
            !v.trim().is_empty()
                && (!continuation || matches!(*k, "DESIGNATION" | "VERSION" | "SHEET"))
                && (!st.automatic || !matches!(*k, "SHEET" | "SHEETS"))
        })
        .map(|(k, v)| ((*k).into(), json!(v.trim())))
        .collect();
    let mut params = json!({"layout":st.layout.trim(),"format":st.format,"landscape":st.landscape,
        "automatic":st.automatic,"font":st.font.trim(),"fields":fields});
    if !st.automatic {
        params["form"] = json!(if continuation { "2a" } else { "1" });
    }
    params
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
    let uid = doc.uid;
    let key = format!("{name}_state");
    let mut st = temp::<State>(ctx, &key).filter(|s| s.uid == uid).unwrap_or_else(|| initial(app));
    let revision = app.session.state().map_or(0, |s| s.revision);
    if name == "gostsheet" && st.revision != revision {
        match app.run("gost.document", json!({})) {
            Ok(report) => {
                st.automatic_continuation = Some(managed_continuation(&report));
                st.error = None;
            }
            Err(e) => {
                st.automatic_continuation = None;
                st.error = Some(e);
            }
        }
        st.revision = revision;
    }
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
                ui.checkbox(&mut st.automatic, crate::tl!("Automatic drawing-book sheet"));
                let mut continuation = if st.automatic { st.automatic_continuation.unwrap_or(false) } else { st.continuation };
                ui.add_enabled_ui(!st.automatic, |ui| {
                    ui.checkbox(&mut continuation, crate::tl!("Continuation sheet (form 2a)"));
                });
                if !st.automatic {
                    st.continuation = continuation;
                }
                ui.label(crate::tl!("First drawing sheet: form 1, 185 × 55 mm. Continuation: form 2a, 185 × 15 mm."));
                ui.separator();
                egui::Grid::new("gost_title_fields").num_columns(2).show(ui, |ui| {
                    for (tag, value) in &mut st.fields {
                        if st.automatic && matches!(*tag, "SHEET" | "SHEETS") {
                            continue;
                        }
                        if continuation && !matches!(*tag, "DESIGNATION" | "VERSION" | "SHEET") {
                            continue;
                        }
                        let l = ui.label(field_label(tag));
                        ui.text_edit_singleline(value).labelled_by(l.id);
                        ui.end_row();
                    }
                });
                ui.label(crate::tl!("Empty approval and signature fields remain empty. Edit block attributes for additional fields."));
                if ui.add_enabled(!st.automatic || st.automatic_continuation.is_some(), egui::Button::new(crate::tl!("Create GOST Sheet"))).clicked()
                {
                    action = Some(("gost.sheet", sheet_params(&st, continuation)));
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
                if name == "gostcheck"
                    && let Some(findings) = result.get("findings").and_then(Value::as_array)
                {
                    for (index, finding) in findings.iter().enumerate() {
                        ui.push_id(index, |ui| {
                            ui.group(|ui| {
                                ui.label(format!("{} · {}", finding["severity"].as_str().unwrap_or(""), finding["code"].as_str().unwrap_or("")));
                                ui.label(finding["message"].as_str().unwrap_or(""));
                                if finding.get("handles").and_then(Value::as_array).is_some_and(|a| !a.is_empty())
                                    && ui.button(crate::tl!("Show finding")).clicked()
                                {
                                    action = Some(("gost.check.show", json!({"space":finding["space"],"handles":finding["handles"]})));
                                }
                            });
                        });
                    }
                }
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
                if cmd != "gost.check.show" {
                    st.result = Some(v);
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_frames_do_not_hide_first_automatic_sheet_fields() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.run("gost.sheet", json!({"layout":"Независимый","automatic":false,"form":"2a"})).unwrap();
        let mut st = initial(&mut app);
        assert_eq!(st.automatic_continuation, Some(false));
        st.layout = "Первый".into();
        for (tag, value) in &mut st.fields {
            if matches!(*tag, "TITLE" | "MATERIAL" | "ORG") {
                *value = format!("Значение {tag}");
            }
        }
        let request = sheet_params(&st, st.automatic_continuation.unwrap());
        app.run("gost.sheet", request).unwrap();
        let report = app.run("gost.document", json!({})).unwrap();
        let first = report["sheets"].as_array().unwrap().iter().find(|s| s["layout"] == "Первый").unwrap();
        assert_eq!(first["form"], "1");
        for tag in ["TITLE", "MATERIAL", "ORG"] {
            assert_eq!(first["fields"][tag], format!("Значение {tag}"));
        }
        assert_eq!(initial(&mut app).automatic_continuation, Some(true));
    }
}
