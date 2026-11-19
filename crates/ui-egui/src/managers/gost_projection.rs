//! Thin typed controls for the shared model/projection/dimension commands.
use super::gost::Editor;
use super::{Action, no_drawing, set_temp, temp};
use crate::CadApp;
use serde_json::{Value, json};
fn text(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(value);
    });
}
fn xyz(ui: &mut egui::Ui, label: &str, value: &mut [f64; 3]) {
    ui.horizontal(|ui| {
        ui.label(label);
        for n in value {
            ui.add(egui::DragValue::new(n).speed(1.0));
        }
    });
}
fn xy(ui: &mut egui::Ui, label: &str, value: &mut [f64; 2]) {
    ui.horizontal(|ui| {
        ui.label(label);
        for n in value {
            ui.add(egui::DragValue::new(n).speed(1.0));
        }
    });
}
fn error(ui: &mut egui::Ui, e: &Option<String>) {
    if let Some(e) = e {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
}
fn selected(app: &CadApp) -> String {
    app.session.selection().first().map(|h| h.hex()).unwrap_or_default()
}
fn selected_owner(app: &mut CadApp, changed: bool, selection: &str, field: &str) -> Option<String> {
    if !changed || selection.is_empty() {
        return None;
    }
    app.run("gost.projection.inspect", json!({"handle":selection})).ok().filter(|v| !v[field].is_null()).map(|_| selection.to_string())
}

fn finish_editor<T: Clone + Send + Sync + 'static>(
    ctx: &egui::Context,
    key: &str,
    editor_key: &str,
    state: T,
    editor: Editor<T>,
    open: &mut bool,
    close: bool,
) {
    if close {
        *open = false;
    }
    if *open {
        set_temp(ctx, key, state);
        set_temp(ctx, editor_key, editor);
    } else {
        ctx.data_mut(|d| {
            d.remove::<T>(egui::Id::unique(key));
            d.remove::<Editor<T>>(egui::Id::unique(editor_key));
        });
    }
}
fn coordinates(value: &str, count: usize) -> std::result::Result<Vec<Vec<f64>>, String> {
    let mut out = Vec::new();
    for line in value.lines().filter(|l| !l.trim().is_empty()) {
        if out.len() >= 512 {
            return Err("At most 512 vertices".into());
        }
        let row = line
            .split_whitespace()
            .map(|s| s.parse::<f64>().map_err(|_| "Invalid numeric coordinate".to_string()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if row.len() != count {
            return Err(format!("Each line needs {count} coordinates"));
        }
        out.push(row);
    }
    Ok(out)
}
fn array3(v: &Value) -> Option<[f64; 3]> {
    if v.is_object() {
        Some([v.get("x")?.as_f64()?, v.get("y")?.as_f64()?, v.get("z")?.as_f64()?])
    } else {
        Some([v.get(0)?.as_f64()?, v.get(1)?.as_f64()?, v.get(2)?.as_f64()?])
    }
}
fn direction_label(direction: &str) -> &'static str {
    match direction {
        "front" => crate::tl!("Front"),
        "top" => crate::tl!("Top"),
        "left" => crate::tl!("Left"),
        "right" => crate::tl!("Right"),
        "rear" => crate::tl!("Rear"),
        "bottom" => crate::tl!("Bottom"),
        "isometric" => crate::tl!("Isometric"),
        _ => crate::tl!("Custom"),
    }
}
#[derive(Clone, PartialEq)]
struct Solid {
    uid: u64,
    handle: String,
    mode: String,
    min: [f64; 3],
    max: [f64; 3],
    profile: String,
    z0: f64,
    z1: f64,
    vertices: String,
    faces: String,
    ids: String,
    error: Option<String>,
    result: Option<Value>,
}
fn load_solid(app: &mut CadApp, st: &mut Solid) {
    match app.run("gost.projection.inspect", json!({"handle":st.handle})) {
        Ok(v) => {
            if let Some(vertices) = v["vertices"].as_array() {
                st.vertices = vertices.iter().filter_map(array3).map(|p| format!("{} {} {}", p[0], p[1], p[2])).collect::<Vec<_>>().join("\n");
                st.faces = v["faces"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|f| f.as_array().into_iter().flatten().filter_map(Value::as_u64).map(|n| (n + 1).to_string()).collect::<Vec<_>>().join(" "))
                    .collect::<Vec<_>>()
                    .join("\n");
                st.ids = v["ids"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|n| n.to_string()).collect::<Vec<_>>().join(" ");
                st.mode = "mesh".into();
                st.result = Some(v);
                st.error = None;
            } else {
                st.error = Some("Select a native 3D body".into());
            }
        }
        Err(e) => st.error = Some(e),
    }
}
fn apply_solid(app: &mut CadApp, st: &mut Solid) -> std::result::Result<(), String> {
    let mut p = match st.mode.as_str() {
        "box" => json!({"box":{"min":st.min,"max":st.max}}),
        "extrusion" => json!({"profile":coordinates(&st.profile,2)?,"z0":st.z0,"z1":st.z1}),
        _ => {
            let faces = st
                .faces
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| {
                    l.split_whitespace()
                        .map(|n| n.parse::<u32>().map_err(|_| "Invalid face index".to_string()))
                        .collect::<std::result::Result<Vec<_>, _>>()
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            json!({"vertices":coordinates(&st.vertices,3)?,"faces":faces})
        }
    };
    if !st.handle.trim().is_empty() {
        p["handle"] = json!(st.handle.trim());
    }
    let generated_ids = st.ids.trim().is_empty();
    if !generated_ids {
        let ids = st
            .ids
            .split_whitespace()
            .map(|s| s.parse::<u32>().map_err(|_| "Invalid vertex ID".to_string()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        p["ids"] = json!(ids);
    }
    let v = app.run("gost.solid", p)?;
    st.handle = v["handle"].as_str().unwrap_or("").into();
    st.result = Some(v);
    if generated_ids {
        // Inspect identity only: loading the entire mesh would discard the profile/box editor.
        let inspected = app.run("gost.projection.inspect", json!({"handle":st.handle}))?;
        st.ids = inspected["ids"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|n| n.to_string()).collect::<Vec<_>>().join(" ");
    }
    Ok(())
}
fn solid(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let Ok(doc) = app.session.state() else {
        no_drawing(ctx, crate::tl!("GOST / ESKD"), open);
        return None;
    };
    let (uid, revision) = (doc.uid, doc.revision);
    let pass = ctx.cumulative_pass_nr();
    let selection = selected(app);
    let cached = temp::<Editor<Solid>>(ctx, "gost_solid_editor").filter(|e| e.baseline().uid == uid && !e.reopened(pass));
    let fresh = cached.is_none();
    let mut st = temp::<Solid>(ctx, "gost_solid_state").filter(|s| s.uid == uid && !fresh).unwrap_or(Solid {
        uid,
        handle: String::new(),
        mode: "box".into(),
        min: [0.0; 3],
        max: [40.0, 20.0, 30.0],
        profile: "0 0\n40 0\n40 20\n0 20".into(),
        z0: 0.0,
        z1: 30.0,
        vertices: String::new(),
        faces: String::new(),
        ids: String::new(),
        error: None,
        result: None,
    });
    let mut editor = cached.unwrap_or_else(|| Editor::new(pass, revision, selection.clone(), &st));
    let candidate = selected_owner(app, fresh || editor.selection != selection, &selection, "vertices");
    let owner_changed = candidate.as_ref().is_some_and(|h| h != &st.handle);
    editor.selection = selection;
    if editor.observe(pass, revision, owner_changed, &st) {
        if let Some(handle) = candidate {
            st.handle = handle;
        }
        if !st.handle.is_empty() {
            load_solid(app, &mut st);
        }
        editor.accept(revision, &st);
    }
    let mut load = false;
    let mut apply = false;
    let mut close = false;
    egui::Window::new(crate::tl!("GOST Polyhedral Solid")).id(egui::Id::unique("gostsolid")).open(open).default_size(egui::vec2(650.0, 620.0)).show(
        ctx,
        |ui| {
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.8).show(ui, |ui| {
                editor.warning(ui);
                ui.label(crate::tl!("Closed polyhedral geometry only. Curved surfaces are not approximated automatically."));
                text(ui, crate::tl!("Existing object handle"), &mut st.handle);
                ui.horizontal(|ui| {
                    if ui.button(crate::tl!("Load selected body")).clicked() {
                        st.handle = selected(app);
                        load = true;
                    }
                    if ui.button(crate::tl!("Load")).clicked() {
                        load = true;
                    }
                    if ui.button(crate::tl!("New")).clicked() {
                        st.handle.clear();
                        st.ids.clear();
                    }
                });
                craft_ui::ComboBox::from_id_salt("solid_mode")
                    .selected_text(match st.mode.as_str() {
                        "box" => crate::tl!("Box"),
                        "extrusion" => crate::tl!("Extrusion"),
                        _ => crate::tl!("Explicit mesh"),
                    })
                    .show_ui(ui, |ui| {
                        for (v, label) in [("box", crate::tl!("Box")), ("extrusion", crate::tl!("Extrusion")), ("mesh", crate::tl!("Explicit mesh"))]
                        {
                            ui.selectable_value(&mut st.mode, v.into(), label);
                        }
                    });
                match st.mode.as_str() {
                    "box" => {
                        xyz(ui, crate::tl!("Minimum point"), &mut st.min);
                        xyz(ui, crate::tl!("Maximum point"), &mut st.max);
                    }
                    "extrusion" => {
                        ui.label(crate::tl!("Convex profile: X Y per line, counterclockwise"));
                        ui.add(egui::TextEdit::multiline(&mut st.profile).desired_rows(5));
                        ui.horizontal(|ui| {
                            ui.label("Z");
                            ui.add(egui::DragValue::new(&mut st.z0));
                            ui.add(egui::DragValue::new(&mut st.z1));
                        });
                    }
                    _ => {
                        ui.label(crate::tl!("Vertices: X Y Z per line"));
                        ui.add(egui::TextEdit::multiline(&mut st.vertices).desired_rows(6));
                        ui.label(crate::tl!("Faces: 3 or 4 one-based vertex indices per line"));
                        ui.add(egui::TextEdit::multiline(&mut st.faces).desired_rows(6));
                    }
                }
                text(ui, crate::tl!("Stable vertex IDs (optional for new bodies)"), &mut st.ids);
                if ui.add_enabled(!editor.conflict, egui::Button::new(crate::tl!("Apply body"))).clicked() {
                    apply = true;
                }
                error(ui, &st.error);
                if let Some(v) = &st.result {
                    ui.label(format!("{}: {}", crate::tl!("Object"), v["handle"].as_str().unwrap_or("")));
                }
                if ui.button(crate::tl!("Close")).clicked() {
                    close = true;
                }
            });
        },
    );
    if load {
        load_solid(app, &mut st);
        editor.accept(app.session.state().map_or(0, |s| s.revision), &st);
    }
    if apply {
        st.error = apply_solid(app, &mut st).err();
        if st.error.is_none() {
            editor.accept(app.session.state().map_or(0, |s| s.revision), &st);
        }
    }
    finish_editor(ctx, "gost_solid_state", "gost_solid_editor", st, editor, open, close);
    None
}
#[derive(Clone, PartialEq)]
struct View {
    uid: u64,
    handle: String,
    sources: String,
    layout: String,
    at: [f64; 2],
    direction: String,
    vector: [f64; 3],
    up: [f64; 3],
    scale: f64,
    hidden: bool,
    section: bool,
    offset: f64,
    label: String,
    error: Option<String>,
    result: Option<Value>,
}
fn load_view(app: &mut CadApp, st: &mut View) {
    match app.run("gost.projection.inspect", json!({"handle":st.handle})) {
        Ok(v) => {
            let r = &v["parameters"];
            if r.is_object() {
                st.sources =
                    r["sources"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|n| format!("{n:X}")).collect::<Vec<_>>().join(" ");
                st.direction = "custom".into();
                st.vector = array3(&r["direction"]).unwrap_or([0.0, -1.0, 0.0]);
                st.up = array3(&r["up"]).unwrap_or([0.0, 0.0, 1.0]);
                st.scale = r["scale"].as_f64().unwrap_or(1.0);
                st.hidden = r["hidden"].as_bool().unwrap_or(true);
                st.section = r["cut"].is_object();
                if st.section {
                    st.vector = array3(&r["cut"]["normal"]).unwrap_or(st.vector);
                    st.offset = r["cut"]["offset"].as_f64().unwrap_or(0.0);
                }
                st.label = r["label"].as_str().unwrap_or("").into();
                st.layout = v["layout"].as_str().unwrap_or("GOST1").to_string();
                st.at = [v["transform"]["at"][0].as_f64().unwrap_or(0.0), v["transform"]["at"][1].as_f64().unwrap_or(0.0)];
                st.error = None;
                st.result = Some(v);
            } else {
                st.error = Some("Select an associative drawing view".into());
            }
        }
        Err(e) => st.error = Some(e),
    }
}
fn view(app: &mut CadApp, ctx: &egui::Context, name: &str, open: &mut bool) -> Action {
    let Ok(doc) = app.session.state() else {
        no_drawing(ctx, crate::tl!("GOST / ESKD"), open);
        return None;
    };
    let (uid, revision) = (doc.uid, doc.revision);
    let pass = ctx.cumulative_pass_nr();
    let selection = selected(app);
    let key = format!("{name}_state");
    let editor_key = format!("{name}_editor");
    let cached = temp::<Editor<View>>(ctx, &editor_key).filter(|e| e.baseline().uid == uid && !e.reopened(pass));
    let fresh = cached.is_none();
    let mut st = temp::<View>(ctx, &key).filter(|s| s.uid == uid && !fresh).unwrap_or(View {
        uid,
        handle: String::new(),
        sources: app.session.selection().iter().map(|h| h.hex()).collect::<Vec<_>>().join(" "),
        layout: app.session.space().layout().unwrap_or("GOST1").into(),
        at: [60.0, 130.0],
        direction: if name == "gostsectionview" { "top" } else { "front" }.into(),
        vector: [0.0, 0.0, 1.0],
        up: [0.0, 1.0, 0.0],
        scale: 1.0,
        hidden: true,
        section: name == "gostsectionview",
        offset: 15.0,
        label: if name == "gostsectionview" { "А" } else { "" }.into(),
        error: None,
        result: None,
    });
    let mut editor = cached.unwrap_or_else(|| Editor::new(pass, revision, selection.clone(), &st));
    let candidate = selected_owner(app, fresh || editor.selection != selection, &selection, "parameters");
    let owner_changed = candidate.as_ref().is_some_and(|h| h != &st.handle);
    editor.selection = selection;
    if editor.observe(pass, revision, owner_changed, &st) {
        if let Some(handle) = candidate {
            st.handle = handle;
        }
        if !st.handle.is_empty() {
            load_view(app, &mut st);
        }
        editor.accept(revision, &st);
    }
    let mut load = false;
    let mut apply = false;
    let mut close = false;
    egui::Window::new(if st.section { crate::tl!("GOST Associative Section") } else { crate::tl!("GOST Associative View") })
        .id(egui::Id::unique(name))
        .open(open)
        .default_size(egui::vec2(620.0, 600.0))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.8).show(ui, |ui| {
                editor.warning(ui);
                text(ui, crate::tl!("Existing object handle"), &mut st.handle);
                ui.horizontal(|ui| {
                    if ui.button(crate::tl!("Load selected view")).clicked() {
                        st.handle = selected(app);
                        load = true;
                    }
                    if ui.button(crate::tl!("Load")).clicked() {
                        load = true;
                    }
                    if ui.button(crate::tl!("New")).clicked() {
                        st.handle.clear();
                    }
                });
                text(ui, crate::tl!("Source body handles (space-separated)"), &mut st.sources);
                if ui.button(crate::tl!("Use selected bodies")).clicked() {
                    st.sources = app.session.selection().iter().map(|h| h.hex()).collect::<Vec<_>>().join(" ");
                }
                ui.add_enabled_ui(st.handle.trim().is_empty(), |ui| {
                    text(ui, crate::tl!("Layout name"), &mut st.layout);
                });
                xy(ui, crate::tl!("Insertion point"), &mut st.at);
                craft_ui::ComboBox::from_id_salt("projection_direction").selected_text(direction_label(&st.direction)).show_ui(ui, |ui| {
                    for (v, label) in [
                        ("front", crate::tl!("Front")),
                        ("top", crate::tl!("Top")),
                        ("left", crate::tl!("Left")),
                        ("right", crate::tl!("Right")),
                        ("rear", crate::tl!("Rear")),
                        ("bottom", crate::tl!("Bottom")),
                        ("isometric", crate::tl!("Isometric")),
                        ("custom", crate::tl!("Custom")),
                    ] {
                        ui.selectable_value(&mut st.direction, v.into(), label);
                    }
                });
                if st.direction == "custom" {
                    xyz(ui, crate::tl!("Direction toward observer"), &mut st.vector);
                    xyz(ui, crate::tl!("Up vector"), &mut st.up);
                }
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("Model units per paper millimetre"));
                    ui.add(egui::DragValue::new(&mut st.scale).range(0.000001..=1000000.0).speed(0.1));
                });
                ui.checkbox(&mut st.hidden, crate::tl!("Show hidden lines"));
                if st.section {
                    ui.horizontal(|ui| {
                        ui.label(crate::tl!("Cut plane offset (normal dot point)"));
                        ui.add(egui::DragValue::new(&mut st.offset));
                    });
                }
                text(ui, crate::tl!("View or section letter (optional)"), &mut st.label);
                if ui.add_enabled(!editor.conflict, egui::Button::new(crate::tl!("Apply drawing view"))).clicked() {
                    apply = true;
                }
                error(ui, &st.error);
                if let Some(v) = &st.result {
                    ui.label(format!("{}: {}", crate::tl!("Object"), v["handle"].as_str().unwrap_or("")));
                }
                if ui.button(crate::tl!("Close")).clicked() {
                    close = true;
                }
            });
        });
    if load {
        load_view(app, &mut st);
        editor.accept(app.session.state().map_or(0, |s| s.revision), &st);
    }
    if apply {
        let mut p = json!({"sources":st.sources.split_whitespace().collect::<Vec<_>>(),"scale":st.scale,"hidden":st.hidden,"label":if st.label.trim().is_empty(){Value::Null}else{json!(st.label.trim())}});
        if !st.handle.trim().is_empty() {
            p["handle"] = json!(st.handle.trim());
        } else {
            p["layout"] = json!(st.layout);
        }
        p["at"] = json!(st.at);
        if st.direction == "custom" {
            p["direction"] = json!(st.vector);
            p["up"] = json!(st.up);
        } else {
            p["direction"] = json!(st.direction);
        }
        if st.section {
            let n = match st.direction.as_str() {
                "front" => [0.0, -1.0, 0.0],
                "rear" => [0.0, 1.0, 0.0],
                "left" => [-1.0, 0.0, 0.0],
                "right" => [1.0, 0.0, 0.0],
                "top" => [0.0, 0.0, 1.0],
                "bottom" => [0.0, 0.0, -1.0],
                "isometric" => [1.0, -1.0, 1.0],
                _ => st.vector,
            };
            p["plane"] = json!({"normal":n,"offset":st.offset});
        }
        match app.run(if st.section { "gost.section.view" } else { "gost.projection" }, p) {
            Ok(v) => {
                st.handle = v["handle"].as_str().unwrap_or("").into();
                st.error = None;
                st.result = Some(v);
                editor.accept(app.session.state().map_or(0, |s| s.revision), &st);
            }
            Err(e) => st.error = Some(e),
        }
    }
    finish_editor(ctx, &key, &editor_key, st, editor, open, close);
    None
}
#[derive(Clone, PartialEq)]
struct Dimension {
    uid: u64,
    handle: String,
    view: String,
    sources: [String; 2],
    ids: [[u32; 2]; 2],
    edge: [bool; 2],
    kind: String,
    offset: f64,
    error: Option<String>,
    result: Option<Value>,
}
fn load_dimension(app: &mut CadApp, st: &mut Dimension) {
    match app.run("gost.projection.inspect", json!({"handle":st.handle})) {
        Ok(v) => {
            let r = &v["references"];
            if r.is_object() {
                st.view = r["view"].as_u64().map(|h| format!("{h:X}")).unwrap_or_default();
                for (index, key) in ["a", "b"].into_iter().enumerate() {
                    st.sources[index] = r[key]["source"].as_u64().map(|h| format!("{h:X}")).unwrap_or_default();
                    st.edge[index] = r[key]["edge"].is_array();
                    st.ids[index] = if st.edge[index] {
                        [
                            r[key]["edge"][0].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(1),
                            r[key]["edge"][1].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(2),
                        ]
                    } else {
                        [r[key]["vertex"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(1), 2]
                    };
                }
                st.kind = r["kind"].as_str().unwrap_or("horizontal").into();
                st.offset = r["offset"].as_f64().unwrap_or(10.0);
                st.error = v["issue"].as_str().map(str::to_string);
                st.result = Some(v);
            } else {
                st.error = Some("Select an associative view dimension".into());
            }
        }
        Err(e) => st.error = Some(e),
    }
}
fn dimension(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let Ok(doc) = app.session.state() else {
        no_drawing(ctx, crate::tl!("GOST / ESKD"), open);
        return None;
    };
    let (uid, revision) = (doc.uid, doc.revision);
    let pass = ctx.cumulative_pass_nr();
    let selection = selected(app);
    let cached = temp::<Editor<Dimension>>(ctx, "gost_view_dimension_editor").filter(|e| e.baseline().uid == uid && !e.reopened(pass));
    let fresh = cached.is_none();
    let mut st = temp::<Dimension>(ctx, "gost_view_dimension_state").filter(|s| s.uid == uid && !fresh).unwrap_or(Dimension {
        uid,
        handle: String::new(),
        view: selected(app),
        sources: [String::new(), String::new()],
        ids: [[1, 5], [2, 6]],
        edge: [false, false],
        kind: "horizontal".into(),
        offset: 10.0,
        error: None,
        result: None,
    });
    let mut editor = cached.unwrap_or_else(|| Editor::new(pass, revision, selection.clone(), &st));
    let candidate = selected_owner(app, fresh || editor.selection != selection, &selection, "references");
    let owner_changed = candidate.as_ref().is_some_and(|h| h != &st.handle);
    editor.selection = selection;
    if editor.observe(pass, revision, owner_changed, &st) {
        if let Some(handle) = candidate {
            st.handle = handle;
        }
        if !st.handle.is_empty() {
            load_dimension(app, &mut st);
        }
        editor.accept(revision, &st);
    }
    let mut apply = false;
    let mut load = false;
    let mut close = false;
    egui::Window::new(crate::tl!("GOST View Dimension"))
        .id(egui::Id::unique("gostviewdimension"))
        .open(open)
        .default_size(egui::vec2(620.0, 500.0))
        .show(ctx, |ui| {
            editor.warning(ui);
            text(ui, crate::tl!("Existing object handle"), &mut st.handle);
            ui.horizontal(|ui| {
                if ui.button(crate::tl!("Load selected dimension")).clicked() {
                    st.handle = selected(app);
                    load = true;
                }
                if ui.button(crate::tl!("Load")).clicked() {
                    load = true;
                }
                if ui.button(crate::tl!("New")).clicked() {
                    st.handle.clear();
                    st.result = None;
                }
            });
            text(ui, crate::tl!("Drawing view handle"), &mut st.view);
            for index in 0..2 {
                ui.push_id(index, |ui| {
                    ui.group(|ui| {
                        text(ui, crate::tl!("Source body handle"), &mut st.sources[index]);
                        ui.checkbox(&mut st.edge[index], crate::tl!("Anchor at cutting-edge intersection"));
                        ui.horizontal(|ui| {
                            ui.label(crate::tl!("Stable vertex ID"));
                            ui.add(egui::DragValue::new(&mut st.ids[index][0]).range(1..=u32::MAX));
                            if st.edge[index] {
                                ui.add(egui::DragValue::new(&mut st.ids[index][1]).range(1..=u32::MAX));
                            }
                        });
                        if ui.button(crate::tl!("Inspect source vertex IDs")).clicked() {
                            match app.run("gost.projection.inspect", json!({"handle":st.sources[index]})) {
                                Ok(v) => st.result = Some(v),
                                Err(e) => st.error = Some(e),
                            }
                        }
                    });
                });
            }
            craft_ui::ComboBox::from_id_salt("view_dimension_kind")
                .selected_text(match st.kind.as_str() {
                    "horizontal" => crate::tl!("Horizontal"),
                    "vertical" => crate::tl!("Vertical"),
                    _ => crate::tl!("Aligned"),
                })
                .show_ui(ui, |ui| {
                    for (v, l) in [("horizontal", crate::tl!("Horizontal")), ("vertical", crate::tl!("Vertical")), ("aligned", crate::tl!("Aligned"))]
                    {
                        ui.selectable_value(&mut st.kind, v.into(), l);
                    }
                });
            ui.horizontal(|ui| {
                ui.label(crate::tl!("Dimension offset (paper mm)"));
                ui.add(egui::DragValue::new(&mut st.offset));
            });
            if ui.add_enabled(!editor.conflict, egui::Button::new(crate::tl!("Apply associative dimension"))).clicked() {
                apply = true;
            }
            if let Some(v) = &st.result {
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    if let (Some(ids), Some(vertices)) = (v["ids"].as_array(), v["vertices"].as_array()) {
                        for (id, p) in ids.iter().zip(vertices) {
                            ui.label(format!("{id}: {p}"));
                        }
                    } else {
                        ui.label(format!("{}: {}", crate::tl!("Object"), v["handle"].as_str().unwrap_or("")));
                    }
                });
            }
            error(ui, &st.error);
            if ui.button(crate::tl!("Close")).clicked() {
                close = true;
            }
        });
    if load {
        load_dimension(app, &mut st);
        editor.accept(app.session.state().map_or(0, |s| s.revision), &st);
    }
    if apply {
        let anchor = |index: usize| {
            let mut p = json!({"source":st.sources[index]});
            if st.edge[index] {
                p["edge"] = json!(st.ids[index]);
            } else {
                p["vertex"] = json!(st.ids[index][0]);
            }
            p
        };
        let mut p = json!({"view":st.view,"a":anchor(0),"b":anchor(1),"kind":st.kind,"offset":st.offset});
        if !st.handle.is_empty() {
            p["handle"] = json!(st.handle);
        }
        match app.run("gost.projection.dimension", p) {
            Ok(v) => {
                st.handle = v["handle"].as_str().unwrap_or("").into();
                st.result = Some(v);
                st.error = None;
                editor.accept(app.session.state().map_or(0, |s| s.revision), &st);
            }
            Err(e) => st.error = Some(e),
        }
    }
    finish_editor(ctx, "gost_view_dimension_state", "gost_view_dimension_editor", st, editor, open, close);
    None
}
pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, name: &str, open: &mut bool) -> Action {
    match name {
        "gostsolid" => solid(app, ctx, open),
        "gostviewdimension" => dimension(app, ctx, open),
        _ => view(app, ctx, name, open),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_state(app: &CadApp) -> Solid {
        Solid {
            uid: app.session.state().unwrap().uid,
            handle: String::new(),
            mode: "extrusion".into(),
            min: [0.0; 3],
            max: [10.0; 3],
            profile: "0 0\n10 0\n10 10\n0 10".into(),
            z0: 0.0,
            z1: 10.0,
            vertices: String::new(),
            faces: String::new(),
            ids: String::new(),
            error: None,
            result: None,
        }
    }

    #[test]
    fn created_profile_can_be_edited_without_loading_or_changing_mode() {
        let mut app = CadApp::new(cadcraft_engine::extensions::new_session(), crate::Services::default());
        let mut st = body_state(&app);
        apply_solid(&mut app, &mut st).unwrap();
        let handle = st.handle.clone();
        let ids = st.ids.clone();
        assert!(!ids.is_empty());
        st.z1 = 25.0;
        apply_solid(&mut app, &mut st).unwrap();
        assert_eq!((st.handle.as_str(), st.ids.as_str(), st.mode.as_str()), (handle.as_str(), ids.as_str(), "extrusion"));
        assert_eq!(st.profile, "0 0\n10 0\n10 10\n0 10");
        let actual = app.run("gost.projection.inspect", json!({"handle":handle})).unwrap();
        assert!(actual["vertices"].as_array().unwrap().iter().any(|v| v["z"] == 25.0));
    }

    #[test]
    fn created_explicit_mesh_keeps_generated_identity_for_the_next_edit() {
        let mut app = CadApp::new(cadcraft_engine::extensions::new_session(), crate::Services::default());
        let mut st = body_state(&app);
        apply_solid(&mut app, &mut st).unwrap();
        load_solid(&mut app, &mut st);
        assert!(st.error.is_none());
        st.handle.clear();
        st.ids.clear();
        apply_solid(&mut app, &mut st).unwrap();
        let handle = st.handle.clone();
        let ids = st.ids.clone();
        assert!(!ids.is_empty());
        st.vertices =
            coordinates(&st.vertices, 3).unwrap().iter().map(|p| format!("{} {} {}", p[0], p[1], p[2] * 2.0)).collect::<Vec<_>>().join("\n");
        apply_solid(&mut app, &mut st).unwrap();
        assert_eq!((st.handle.as_str(), st.ids.as_str(), st.mode.as_str()), (handle.as_str(), ids.as_str(), "mesh"));
        let actual = app.run("gost.projection.inspect", json!({"handle":handle})).unwrap();
        assert!(actual["vertices"].as_array().unwrap().iter().any(|v| v["z"] == 20.0));
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    fn body(app: &mut CadApp, size: f64) -> String {
        app.run("gost.solid", json!({"box":{"min":[0,0,0],"max":[size,size,size]}})).unwrap()["handle"].as_str().unwrap().into()
    }

    fn frame(app: &mut CadApp, ctx: &egui::Context) {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            solid(app, ui.ctx(), &mut true);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn selected_body_refreshes_cleanly_but_never_retargets_a_dirty_mesh() {
        let mut app = CadApp::new(cadcraft_engine::extensions::new_session(), crate::Services::default());
        let a = body(&mut app, 10.0);
        let b = body(&mut app, 20.0);
        let ctx = egui::Context::default();
        app.session.set_selection(vec![cadcraft_doc::Handle::parse_hex(&a).unwrap()]);
        frame(&mut app, &ctx);
        assert_eq!(temp::<Solid>(&ctx, "gost_solid_state").unwrap().handle, a);
        app.session.set_selection(vec![cadcraft_doc::Handle::parse_hex(&b).unwrap()]);
        frame(&mut app, &ctx);
        let mut st = temp::<Solid>(&ctx, "gost_solid_state").unwrap();
        assert_eq!(st.handle, b);
        st.vertices.push_str("\n1 2 3");
        let draft = st.vertices.clone();
        set_temp(&ctx, "gost_solid_state", st);
        app.session.set_selection(vec![cadcraft_doc::Handle::parse_hex(&a).unwrap()]);
        frame(&mut app, &ctx);
        let st = temp::<Solid>(&ctx, "gost_solid_state").unwrap();
        assert_eq!(st.handle, b);
        assert_eq!(st.vertices, draft);
        assert!(temp::<Editor<Solid>>(&ctx, "gost_solid_editor").unwrap().conflict);
    }
}
