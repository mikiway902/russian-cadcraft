//! Structured requirement editing and template selection using engine commands only.
use super::gost::Editor;
use super::{Action, no_drawing, set_temp, temp};
use crate::CadApp;
use egui::vec2;
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Clone, PartialEq)]
struct State {
    uid: u64,
    layout: String,
    handle: String,
    items: String,
    width: f64,
    height: f64,
    automatic: bool,
    at: [f64; 2],
    library: Value,
    template: String,
    values: BTreeMap<String, String>,
    custom_id: String,
    custom_name: String,
    custom_text: String,
    error: Option<String>,
    report: Option<Value>,
}
fn refresh(app: &mut CadApp, st: &mut State) {
    let p = if st.handle.is_empty() { json!({}) } else { json!({"handle":st.handle}) };
    match app.run("gost.requirements.templates", p) {
        Ok(v) => st.library = v,
        Err(e) => st.error = Some(e),
    }
}
fn load(app: &mut CadApp, st: &mut State) {
    match app.run("gost.requirements.inspect", json!({"handle":st.handle})) {
        Ok(v) => {
            let r = &v["requirements"][0];
            st.layout = r["layout"].as_str().unwrap_or("").into();
            let r = &r["record"];
            st.items = r["items"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")).unwrap_or_default();
            st.width = r["width"].as_f64().unwrap_or(175.0);
            st.height = r["height"].as_f64().unwrap_or(3.5);
            st.automatic = r["placement"]["mode"] == "auto";
            st.at = [r["anchor"][0].as_f64().unwrap_or(0.0), r["anchor"][1].as_f64().unwrap_or(0.0)];
            st.report = Some(v);
            st.error = None;
            refresh(app, st);
        }
        Err(e) => st.error = Some(e),
    }
}
fn editor_state(app: &mut CadApp, ctx: &egui::Context) -> Option<(State, Editor<State>)> {
    let doc = app.session.state().ok()?;
    let (uid, revision) = (doc.uid, doc.revision);
    let pass = ctx.cumulative_pass_nr();
    let selection = app.session.selection().first().map(|h| h.hex()).unwrap_or_default();
    let cached = temp::<Editor<State>>(ctx, "gost_requirements_editor");
    let mut st = temp::<State>(ctx, "gost_requirements_state")
        .filter(|s| s.uid == uid && cached.as_ref().is_some_and(|e| !e.reopened(pass)))
        .unwrap_or_else(|| {
            let selected = selection.clone();
            let mut s = State {
                uid,
                layout: app.session.space().layout().unwrap_or("GOST1").into(),
                handle: String::new(),
                items: String::new(),
                width: 175.0,
                height: 3.5,
                automatic: true,
                at: [25.0, 200.0],
                library: Value::Null,
                template: String::new(),
                values: BTreeMap::new(),
                custom_id: String::new(),
                custom_name: String::new(),
                custom_text: String::new(),
                error: None,
                report: None,
            };
            if !selected.is_empty() && app.run("gost.requirements.inspect", json!({"handle":selected})).is_ok() {
                s.handle = selected;
                load(app, &mut s);
            } else {
                refresh(app, &mut s);
            }
            s
        });
    let mut editor =
        cached.filter(|e| e.baseline().uid == uid && !e.reopened(pass)).unwrap_or_else(|| Editor::new(pass, revision, selection.clone(), &st));
    let candidate = if editor.selection != selection && !selection.is_empty() {
        app.run("gost.requirements.inspect", json!({"handle":selection})).ok().map(|_| selection.clone())
    } else {
        None
    };
    let owner_changed = candidate.as_ref().is_some_and(|h| h != &st.handle);
    editor.selection = selection;
    if editor.observe(pass, revision, owner_changed, &st) {
        if let Some(handle) = candidate {
            st.handle = handle;
            st.template.clear();
            st.values.clear();
            st.custom_id.clear();
            st.custom_name.clear();
            st.custom_text.clear();
        }
        if st.handle.is_empty() {
            st.layout = app.session.space().layout().unwrap_or("GOST1").into();
            refresh(app, &mut st);
        } else {
            load(app, &mut st);
        }
        editor.accept(revision, &st);
    }
    Some((st, editor))
}

pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let Some((mut st, mut editor)) = editor_state(app, ctx) else {
        no_drawing(ctx, crate::tl!("GOST / ESKD"), open);
        return None;
    };
    let mut action: Option<(&str, Value)> = None;
    let mut load_requested = false;
    let mut close = false;
    egui::Window::new(crate::tl!("GOST Technical Requirements"))
        .id(egui::Id::unique("gostrequirements"))
        .open(open)
        .default_size(vec2(660.0, 700.0))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.8).show(ui, |ui| {
                editor.warning(ui);
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("Existing object handle"));
                    ui.text_edit_singleline(&mut st.handle);
                    if ui.button(crate::tl!("Load")).clicked() {
                        load_requested = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("Layout name"));
                    ui.text_edit_singleline(&mut st.layout);
                });
                ui.label(crate::tl!("One requirement per line. Numbering and word wrapping are generated automatically."));
                ui.add(egui::TextEdit::multiline(&mut st.items).desired_rows(7).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("Column width (mm)"));
                    ui.add(egui::DragValue::new(&mut st.width).range(20.0..=1160.0));
                    ui.label(crate::tl!("Text height (mm)"));
                    craft_ui::ComboBox::from_id_salt("req_height").selected_text(st.height.to_string()).show_ui(ui, |ui| {
                        for h in [2.5, 3.5, 5.0] {
                            ui.selectable_value(&mut st.height, h, h.to_string());
                        }
                    });
                });
                ui.checkbox(&mut st.automatic, crate::tl!("Automatic placement above the title block"));
                if !st.automatic {
                    ui.horizontal(|ui| {
                        ui.label(crate::tl!("Top-left point"));
                        ui.add(egui::DragValue::new(&mut st.at[0]));
                        ui.add(egui::DragValue::new(&mut st.at[1]));
                    });
                }
                if ui.add_enabled(!editor.conflict, egui::Button::new(crate::tl!("Apply requirements"))).clicked() {
                    let items: Vec<_> = st.items.lines().filter(|l| !l.trim().is_empty()).collect();
                    let mut p = json!({"layout":st.layout,"items":items,"width":st.width,"height":st.height,"automatic":st.automatic});
                    if !st.handle.trim().is_empty() {
                        p["handle"] = json!(st.handle.trim());
                    }
                    if !st.automatic {
                        p["at"] = json!(st.at);
                    }
                    action = Some(("gost.requirements", p));
                }
                ui.separator();
                ui.collapsing(crate::tl!("Typical texts and custom templates"), |ui| {
                    let old = st.template.clone();
                    craft_ui::ComboBox::from_id_salt("req_template").selected_text(&st.template).show_ui(ui, |ui| {
                        if let Some(a) = st.library["templates"].as_array() {
                            for t in a {
                                if let Some(id) = t["id"].as_str() {
                                    ui.selectable_value(&mut st.template, id.into(), t["name"].as_str().unwrap_or(id));
                                }
                            }
                        }
                    });
                    if old != st.template {
                        st.values.clear();
                    }
                    if let Some(t) =
                        st.library["templates"].as_array().and_then(|a| a.iter().find(|t| t["id"].as_str() == Some(st.template.as_str())))
                    {
                        ui.label(t["text"].as_str().unwrap_or(""));
                        if let Some(parameters) = t["parameters"].as_array() {
                            for p in parameters {
                                if let Some(k) = p.as_str() {
                                    ui.horizontal(|ui| {
                                        ui.label(k);
                                        ui.text_edit_singleline(st.values.entry(k.into()).or_default());
                                    });
                                }
                            }
                        }
                    }
                    if st.handle.is_empty() {
                        ui.label(crate::tl!("Create requirements first to use or save templates in this document."));
                    }
                    if ui
                        .add_enabled(
                            !editor.conflict && !st.handle.is_empty() && !st.template.is_empty(),
                            egui::Button::new(crate::tl!("Append from template")),
                        )
                        .clicked()
                    {
                        action =
                            Some(("gost.requirements.templates", json!({"handle":st.handle,"action":"use","id":st.template,"values":st.values})));
                    }
                    egui::Grid::new("req_custom_fields").num_columns(2).show(ui, |ui| {
                        ui.label(crate::tl!("Template ID (ASCII)"));
                        ui.text_edit_singleline(&mut st.custom_id);
                        ui.end_row();
                        ui.label(crate::tl!("Name"));
                        ui.text_edit_singleline(&mut st.custom_name);
                        ui.end_row();
                        ui.label(crate::tl!("Template text with {parameter} slots"));
                        ui.text_edit_multiline(&mut st.custom_text);
                        ui.end_row();
                    });
                    if ui.add_enabled(!editor.conflict && !st.handle.is_empty(), egui::Button::new(crate::tl!("Save custom template"))).clicked() {
                        action = Some((
                            "gost.requirements.templates",
                            json!({"handle":st.handle,"action":"set","id":st.custom_id,"name":st.custom_name,"text":st.custom_text}),
                        ));
                    }
                    if ui
                        .add_enabled(
                            !editor.conflict && !st.handle.is_empty() && !st.template.is_empty(),
                            egui::Button::new(crate::tl!("Delete custom template")),
                        )
                        .clicked()
                    {
                        action = Some(("gost.requirements.templates", json!({"handle":st.handle,"action":"delete","id":st.template})));
                    }
                });
                if let Some(r) = &st.report
                    && let Some(a) = r["requirements"].as_array()
                {
                    for q in a {
                        ui.label(format!("{}: {} / {}: {}", crate::tl!("Columns"), q["pages"], crate::tl!("Lines"), q["lines"]));
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
    if let Some((cmd, p)) = action {
        match app.run(cmd, p) {
            Ok(v) => {
                if let Some(h) = v["handle"].as_str() {
                    st.handle = h.into();
                }
                load(app, &mut st);
                editor.accept(app.session.state().map_or(0, |s| s.revision), &st);
            }
            Err(e) => st.error = Some(e),
        }
    }
    if load_requested {
        load(app, &mut st);
        editor.accept(app.session.state().map_or(0, |s| s.revision), &st);
    }
    if close {
        *open = false;
    }
    if *open {
        set_temp(ctx, "gost_requirements_state", st);
        set_temp(ctx, "gost_requirements_editor", editor);
    } else {
        ctx.data_mut(|d| {
            d.remove::<State>(egui::Id::unique("gost_requirements_state"));
            d.remove::<Editor<State>>(egui::Id::unique("gost_requirements_editor"));
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requirements(app: &mut CadApp, layout: &str) -> String {
        app.run("gost.sheet", json!({"layout":layout})).unwrap();
        app.run("gost.requirements", json!({"layout":layout,"items":[layout]})).unwrap()["handle"].as_str().unwrap().into()
    }

    fn cache(ctx: &egui::Context, state: State, editor: Editor<State>) {
        set_temp(ctx, "gost_requirements_state", state);
        set_temp(ctx, "gost_requirements_editor", editor);
    }

    #[test]
    fn selecting_another_requirements_owner_reloads_its_fields() {
        let mut app = CadApp::new(cadcraft_engine::extensions::new_session(), crate::Services::default());
        let a = requirements(&mut app, "Лист А");
        let b = requirements(&mut app, "Лист Б");
        let ctx = egui::Context::default();
        app.session.set_selection(vec![cadcraft_doc::Handle::parse_hex(&a).unwrap()]);
        let (st, editor) = editor_state(&mut app, &ctx).unwrap();
        assert_eq!(st.items, "Лист А");
        cache(&ctx, st, editor);
        app.session.set_selection(vec![cadcraft_doc::Handle::parse_hex(&b).unwrap()]);
        let (st, editor) = editor_state(&mut app, &ctx).unwrap();
        assert_eq!(st.handle, b);
        assert_eq!(st.items, "Лист Б");
        assert!(!editor.conflict);
        cache(&ctx, st, editor);
        app.run("gost.requirements", json!({"handle":b,"items":["Внешнее изменение"],"width":150.0})).unwrap();
        let (st, editor) = editor_state(&mut app, &ctx).unwrap();
        assert_eq!(st.items, "Внешнее изменение");
        assert_eq!(st.width, 150.0);
        assert!(!editor.conflict);
    }

    #[test]
    fn changing_the_selected_owner_does_not_retarget_a_dirty_draft() {
        let mut app = CadApp::new(cadcraft_engine::extensions::new_session(), crate::Services::default());
        let a = requirements(&mut app, "Лист А");
        let b = requirements(&mut app, "Лист Б");
        let ctx = egui::Context::default();
        app.session.set_selection(vec![cadcraft_doc::Handle::parse_hex(&a).unwrap()]);
        let (mut st, editor) = editor_state(&mut app, &ctx).unwrap();
        st.items = "Несохранённый черновик".into();
        st.custom_name = "Новый шаблон".into();
        cache(&ctx, st, editor);
        app.session.set_selection(vec![cadcraft_doc::Handle::parse_hex(&b).unwrap()]);
        let (st, editor) = editor_state(&mut app, &ctx).unwrap();
        assert!(editor.conflict);
        assert_eq!(st.handle, a);
        assert_eq!(st.items, "Несохранённый черновик");
        assert_eq!(st.custom_name, "Новый шаблон");
        cache(&ctx, st, editor);
        // Replacing the dialog without its Close button is also a new editing session.
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
            output.textures_delta.clear();
        }
        let (st, editor) = editor_state(&mut app, &ctx).unwrap();
        assert_eq!(st.handle, b);
        assert_eq!(st.items, "Лист Б");
        assert!(st.custom_name.is_empty());
        assert!(!editor.conflict);
    }
}
