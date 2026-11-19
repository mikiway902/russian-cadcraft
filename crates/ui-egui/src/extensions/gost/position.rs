use super::Editor;
use crate::CadApp;
use crate::managers::{Action, no_drawing, set_temp, temp};
use serde_json::json;
#[derive(Clone, PartialEq)]
struct State {
    uid: u64,
    handle: String,
    specification: String,
    target: String,
    at: [f64; 2],
    height: f64,
    shelf: f64,
    error: Option<String>,
}
fn text(ui: &mut egui::Ui, label: &str, s: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(s);
    });
}
fn load_position(app: &mut CadApp, st: &mut State) {
    match app.run("gost.position.inspect", json!({"handle":st.handle})) {
        Ok(v) => {
            st.specification = v["specification"].as_str().unwrap_or("").into();
            st.target = v["target"].as_str().unwrap_or("").into();
            st.height = v["height"].as_f64().unwrap_or(5.0);
            st.shelf = v["shelf"].as_f64().unwrap_or(12.0);
            st.at = [v["at"][0].as_f64().unwrap_or(0.0), v["at"][1].as_f64().unwrap_or(0.0)];
            st.error = None;
        }
        Err(e) => st.error = Some(e),
    }
}
pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, name: &str, open: &mut bool) -> Action {
    let Ok(doc) = app.session.state() else {
        no_drawing(ctx, crate::tl!("GOST / ESKD"), open);
        return None;
    };
    let (uid, revision) = (doc.uid, doc.revision);
    let pass = ctx.cumulative_pass_nr();
    let selection = app.session.selection().first().map(|h| h.hex()).unwrap_or_default();
    let key = format!("{name}_state");
    let editor_key = format!("{name}_editor");
    let cached = temp::<Editor<State>>(ctx, &editor_key).filter(|e| e.baseline().uid == uid && !e.reopened(pass));
    let fresh = cached.is_none();
    let mut st = temp::<State>(ctx, &key).filter(|s| s.uid == uid && !fresh).unwrap_or(State {
        uid,
        handle: if name == "gostpositionedit" { app.session.selection().first().map(|h| h.hex()).unwrap_or_default() } else { String::new() },
        specification: String::new(),
        target: String::new(),
        at: [app.session.cursor.x, app.session.cursor.y],
        height: 5.0,
        shelf: 12.0,
        error: None,
    });
    if fresh && !st.handle.is_empty() {
        load_position(app, &mut st);
    }
    let mut editor = cached.unwrap_or_else(|| Editor::new(pass, revision, selection.clone(), &st));
    let candidate = if name == "gostpositionedit" && editor.selection != selection && !selection.is_empty() {
        app.run("gost.position.inspect", json!({"handle":selection})).ok().map(|_| selection.clone())
    } else {
        None
    };
    let owner_changed = candidate.as_ref().is_some_and(|h| h != &st.handle);
    editor.selection = selection;
    if editor.observe(pass, revision, owner_changed, &st) {
        if let Some(handle) = candidate {
            st.handle = handle;
        }
        if !st.handle.is_empty() {
            load_position(app, &mut st);
        }
        editor.accept(revision, &st);
    }
    let mut load = false;
    let mut apply = false;
    let mut close = false;
    egui::Window::new(crate::tl!("GOST Position")).id(egui::Id::unique(name)).open(open).default_size(egui::vec2(580.0, 400.0)).show(ctx, |ui| {
        editor.warning(ui);
        text(ui, crate::tl!("Existing object handle"), &mut st.handle);
        if ui.button(crate::tl!("Load selected position")).clicked() {
            st.handle = app.session.selection().first().map(|h| h.hex()).unwrap_or_default();
            load = true;
        }
        if ui.button(crate::tl!("Load")).clicked() {
            load = true;
        }
        ui.add_enabled_ui(st.handle.trim().is_empty(), |ui| {
            text(ui, crate::tl!("Specification handle"), &mut st.specification);
        });
        text(ui, crate::tl!("Target part handle"), &mut st.target);
        if ui.button(crate::tl!("Use selected part")).clicked() {
            st.target = app.session.selection().first().map(|h| h.hex()).unwrap_or_default();
        }
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Shelf centre"));
            for n in &mut st.at {
                ui.add(egui::DragValue::new(n));
            }
        });
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Text height (mm)"));
            craft_ui::ComboBox::from_id_salt("position_height").selected_text(st.height.to_string()).show_ui(ui, |ui| {
                for h in [5.0, 7.0, 10.0, 14.0] {
                    ui.selectable_value(&mut st.height, h, h.to_string());
                }
            });
            ui.label(crate::tl!("Shelf width (mm)"));
            ui.add(egui::DragValue::new(&mut st.shelf).range(8.0..=60.0));
        });
        if ui
            .add_enabled(!editor.conflict && (name != "gostpositionedit" || !st.handle.is_empty()), egui::Button::new(crate::tl!("Apply position")))
            .clicked()
        {
            apply = true;
        }
        if let Some(e) = &st.error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        if ui.button(crate::tl!("Close")).clicked() {
            close = true;
        }
    });
    if load {
        load_position(app, &mut st);
        editor.accept(app.session.state().map_or(0, |s| s.revision), &st);
    }
    if apply {
        let mut p = json!({"target":st.target,"at":st.at,"height":st.height,"shelf":st.shelf});
        let cmd = if st.handle.trim().is_empty() {
            p["specification"] = json!(st.specification.trim());
            "gost.position"
        } else {
            p["handle"] = json!(st.handle.trim());
            "gost.position.edit"
        };
        match app.run(cmd, p) {
            Ok(v) => {
                st.handle = v["handle"].as_str().unwrap_or("").into();
                st.error = None;
                editor.accept(app.session.state().map_or(0, |s| s.revision), &st);
            }
            Err(e) => st.error = Some(e),
        }
    }
    if close {
        *open = false;
    }
    if *open {
        set_temp(ctx, &key, st);
        set_temp(ctx, &editor_key, editor);
    } else {
        ctx.data_mut(|d| {
            d.remove::<State>(egui::Id::unique(&key));
            d.remove::<Editor<State>>(egui::Id::unique(&editor_key));
        });
    }
    None
}
