use super::{Action, no_drawing, set_temp, temp};
use crate::CadApp;
use serde_json::json;
#[derive(Clone)]
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
pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, name: &str, open: &mut bool) -> Action {
    let Ok(doc) = app.session.state() else {
        no_drawing(ctx, crate::tl!("GOST / ESKD"), open);
        return None;
    };
    let uid = doc.uid;
    let key = format!("{name}_state");
    let mut st = temp::<State>(ctx, &key).filter(|s| s.uid == uid).unwrap_or(State {
        uid,
        handle: if name == "gostpositionedit" { app.session.selection().first().map(|h| h.hex()).unwrap_or_default() } else { String::new() },
        specification: String::new(),
        target: String::new(),
        at: [app.session.cursor.x, app.session.cursor.y],
        height: 5.0,
        shelf: 12.0,
        error: None,
    });
    let mut load = false;
    let mut apply = false;
    let mut close = false;
    egui::Window::new(crate::tl!("GOST Position")).id(egui::Id::unique(name)).open(open).default_size(egui::vec2(580.0, 400.0)).show(ctx, |ui| {
        text(ui, crate::tl!("Existing object handle"), &mut st.handle);
        if ui.button(crate::tl!("Load selected position")).clicked() {
            st.handle = app.session.selection().first().map(|h| h.hex()).unwrap_or_default();
            load = true;
        }
        if ui.button(crate::tl!("Load")).clicked() {
            load = true;
        }
        text(ui, crate::tl!("Specification handle"), &mut st.specification);
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
        if ui.add_enabled(name != "gostpositionedit" || !st.handle.is_empty(), egui::Button::new(crate::tl!("Apply position"))).clicked() {
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
