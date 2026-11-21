//! Linked-model UI. All kernel work runs off-thread; only guarded command completion mutates
//! the active document. Preview geometry is transient and never becomes drafting geometry.
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::channel;
use std::sync::{
    Arc,
    mpsc::{Receiver, TryRecvError},
};
use std::time::{Duration, Instant};

use cadcraft_doc::{Space, model_projection::Projection};
use cadcraft_engine::{extensions::models, model_io};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    CadApp, Pick,
    extensions::{DialogSpec, ExtensionDescriptor},
    managers::Action,
};

mod dimensions;
mod preview;

pub(super) const DESCRIPTOR: ExtensionDescriptor = ExtensionDescriptor {
    id: "models",
    dialogs: &[DialogSpec { commands: &["gost.model.list", "gost.model.import"], name: "gostmodels", render: dialog }],
};

#[derive(Deserialize)]
struct Source {
    sha256: String,
    bytes: u64,
}
#[derive(Deserialize)]
struct Linked {
    id: String,
    revision: u64,
    path: String,
    source: Source,
    metadata: Value,
    views: Vec<Value>,
}
struct Guard {
    id: String,
    revision: u64,
    path: String,
    sha256: String,
}
impl Linked {
    fn guard(&self) -> Arc<Guard> {
        Arc::new(Guard { id: self.id.clone(), revision: self.revision, path: self.path.clone(), sha256: self.source.sha256.clone() })
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Direction {
    #[default]
    Front,
    Rear,
    Top,
    Bottom,
    Left,
    Right,
    Isometric,
}
impl Direction {
    const ALL: [Self; 7] = [Self::Front, Self::Rear, Self::Top, Self::Bottom, Self::Left, Self::Right, Self::Isometric];
    fn id(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Rear => "rear",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
            Self::Isometric => "isometric",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Front => crate::tl!("Front"),
            Self::Rear => crate::tl!("Rear"),
            Self::Top => crate::tl!("Top"),
            Self::Bottom => crate::tl!("Bottom"),
            Self::Left => crate::tl!("Left"),
            Self::Right => crate::tl!("Right"),
            Self::Isometric => crate::tl!("Isometric"),
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Import,
    Refresh,
    View,
    Preview,
    Status,
    References,
    Dimension,
}
impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Import => crate::tl!("Importing 3D model…"),
            Self::Refresh => crate::tl!("Refreshing model source…"),
            Self::View => crate::tl!("Projecting exact model…"),
            Self::Preview => crate::tl!("Loading 3D preview…"),
            Self::Status => crate::tl!("Checking model source…"),
            Self::References => crate::tl!("Loading exact model edges…"),
            Self::Dimension => crate::tl!("Resolving exact model dimension…"),
        }
    }
}
enum Outcome {
    Loaded(Value),
    View { projection: Box<Projection>, params: Value },
    Preview(Box<preview::Scene>),
    Status(Value),
    References(Value),
    Dimension { params: Value, resolutions: [Value; 2] },
}
struct Job {
    uid: u64,
    document_revision: Option<u64>,
    space: Option<(Space, Option<cadcraft_doc::Handle>)>,
    guard: Option<Arc<Guard>>,
    kind: Kind,
    started: Instant,
    rx: Receiver<Result<Outcome, String>>,
}
enum Act {
    Browse,
    Import,
    Refresh,
    Preview,
    Status,
    View,
    Remove,
    ReloadList,
    References,
    Dimension,
}

pub(crate) struct State {
    cache: Option<(u64, u64)>,
    rows: Vec<Linked>,
    selected: String,
    path: String,
    error: Option<String>,
    job: Option<Job>,
    scene: Option<Box<preview::Scene>>,
    scene_guard: Option<Arc<Guard>>,
    status: Option<(Arc<Guard>, Value)>,
    gpu_key: u64,
    direction: Direction,
    scale: f64,
    at: [f64; 2],
    hidden: bool,
    accept_incomplete: bool,
    dimensions: dimensions::State,
}
impl Default for State {
    fn default() -> Self {
        Self {
            cache: None,
            rows: Vec::new(),
            selected: String::new(),
            path: String::new(),
            error: None,
            job: None,
            scene: None,
            scene_guard: None,
            status: None,
            gpu_key: 0,
            direction: Direction::default(),
            scale: 1.0,
            at: [50.0, 100.0],
            hidden: false,
            accept_incomplete: false,
            dimensions: dimensions::State::default(),
        }
    }
}
impl State {
    fn clear_document(&mut self) {
        self.cache = None;
        self.rows.clear();
        self.selected.clear();
        self.scene = None;
        self.scene_guard = None;
        self.status = None;
        self.dimensions = dimensions::State::default();
    }
    fn update(&mut self, app: &mut CadApp) -> Result<(), String> {
        let key = app.session.state().map(|s| (s.uid, s.revision)).map_err(|e| e.to_string())?;
        if self.cache == Some(key) {
            return Ok(());
        }
        if self.cache.is_some_and(|old| old.0 != key.0) {
            self.clear_document();
        }
        self.cache = Some(key);
        let mut report = app.session.execute("gost.model.list", &json!({})).map_err(|e| e.to_string())?;
        self.rows = serde_json::from_value(report["models"].take()).map_err(|e| e.to_string())?;
        if !self.rows.iter().any(|m| m.id == self.selected) {
            self.selected = self.rows.first().map(|m| m.id.clone()).unwrap_or_default();
        }
        if self.scene_guard.as_ref().is_some_and(|g| !self.current(g)) {
            self.scene = None;
            self.scene_guard = None;
        }
        if self.status.as_ref().is_some_and(|(g, _)| !self.current(g)) {
            self.status = None;
        }
        self.dimensions.invalidate(&self.rows);
        Ok(())
    }
    fn current(&self, guard: &Guard) -> bool {
        self.rows.iter().any(|m| m.id == guard.id && m.revision == guard.revision && m.source.sha256 == guard.sha256 && m.path == guard.path)
    }
    fn selected_guard(&self) -> Result<Arc<Guard>, String> {
        self.rows.iter().find(|m| m.id == self.selected).map(Linked::guard).ok_or_else(|| "Select a linked model first".into())
    }
    fn start(
        &mut self,
        app: &CadApp,
        kind: Kind,
        guard: Option<Arc<Guard>>,
        work: impl FnOnce() -> Result<Outcome, String> + Send + 'static,
    ) -> Result<(), String> {
        if self.job.is_some() {
            return Err("A model operation is already running".into());
        }
        let state = app.session.state().map_err(|e| e.to_string())?;
        let uid = state.uid;
        let document_revision = matches!(kind, Kind::View | Kind::Dimension).then_some(state.revision);
        let space = matches!(kind, Kind::View | Kind::Dimension).then(|| (state.space.clone(), state.mspace));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, rx) = channel();
            std::thread::Builder::new()
                .name("linked-model".into())
                .spawn(move || {
                    let _ = tx.send(work());
                })
                .map_err(|e| e.to_string())?;
            self.job = Some(Job { uid, document_revision, space, guard, kind, started: Instant::now(), rx });
            self.error = None;
            Ok(())
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (uid, document_revision, space, guard, kind, work);
            Err("Native model operations are unavailable in the browser".into())
        }
    }
    fn preview(&mut self, app: &CadApp) -> Result<(), String> {
        let hooks = model_io::hooks().ok_or_else(|| "Native model worker is unavailable".to_string())?;
        let guard = self.selected_guard()?;
        let captured = guard.clone();
        let device = app.canvas.gpu_device.clone();
        self.start(app, Kind::Preview, Some(guard), move || {
            let preview = (hooks.preview)(&captured.path, &captured.sha256, 4)?;
            preview::Scene::new(preview, device).map(|scene| Outcome::Preview(Box::new(scene)))
        })
    }
    fn dispatch(&mut self, app: &mut CadApp, act: Act) -> Result<(), String> {
        let hooks = model_io::hooks().ok_or_else(|| "Native model worker is unavailable".to_string())?;
        match act {
            Act::Import => {
                let path = self.path.clone();
                self.start(app, Kind::Import, None, move || (hooks.load)(&path).map(Outcome::Loaded))
            }
            Act::Refresh => {
                let guard = self.selected_guard()?;
                let captured = guard.clone();
                self.start(app, Kind::Refresh, Some(guard), move || (hooks.load)(&captured.path).map(Outcome::Loaded))
            }
            Act::Preview => self.preview(app),
            Act::Status => {
                let guard = self.selected_guard()?;
                let captured = guard.clone();
                let bytes =
                    self.rows.iter().find(|m| m.id == guard.id).map(|m| m.source.bytes).ok_or_else(|| "Linked model not found".to_string())?;
                self.start(app, Kind::Status, Some(guard), move || {
                    (hooks.source_status)(&captured.path, &captured.sha256, bytes).map(Outcome::Status)
                })
            }
            Act::View => {
                let guard = self.selected_guard()?;
                let captured = guard.clone();
                let params = json!({"id":guard.id,"direction":self.direction.id(),"scale":self.scale,"at":self.at,"hidden":self.hidden,"accept_incomplete":self.accept_incomplete});
                let (direction, up) = models::view_vectors(&params).map_err(|e| e.to_string())?;
                self.start(app, Kind::View, Some(guard), move || {
                    (hooks.view)(&captured.path, &captured.sha256, direction, up)
                        .map(|projection| Outcome::View { projection: Box::new(projection), params })
                })
            }
            Act::References => {
                let guard = self.selected_guard()?;
                let captured = guard.clone();
                self.start(app, Kind::References, Some(guard), move || (hooks.references)(&captured.path, &captured.sha256).map(Outcome::References))
            }
            Act::Dimension => {
                let guard = self.selected_guard()?;
                let params = self.dimensions.params(&guard)?;
                let captured = guard.clone();
                self.start(app, Kind::Dimension, Some(guard), move || {
                    let resolve = |anchor: &Value| {
                        let token = anchor["reference"].as_str().ok_or_else(|| "Missing exact edge reference".to_string())?;
                        let fraction = anchor["fraction"].as_f64().ok_or_else(|| "Missing exact edge fraction".to_string())?;
                        (hooks.resolve_point)(&captured.path, &captured.sha256, token, fraction)
                    };
                    let resolutions = [resolve(&params["a"])?, resolve(&params["b"])?];
                    Ok(Outcome::Dimension { params, resolutions })
                })
            }
            Act::Remove => {
                app.session.execute("gost.model.remove", &json!({"id":self.selected})).map_err(|e| e.to_string())?;
                self.cache = None;
                Ok(())
            }
            Act::ReloadList => {
                self.cache = None;
                self.update(app)
            }
            Act::Browse => Ok(()),
        }
    }
}

/// Poll once per real frame, including while the manager is closed. No kernel work here.
pub(crate) fn tick(app: &mut CadApp, ctx: &egui::Context) {
    let mut state = std::mem::take(&mut app.models);
    let uid = app.session.state().ok().map(|s| s.uid);
    if state.cache.is_some_and(|key| Some(key.0) != uid) {
        state.clear_document();
    }
    let answer = state.job.as_ref().and_then(|job| match job.rx.try_recv() {
        Ok(value) => Some(value),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(Err("Model operation ended without a result".into())),
    });
    if let Some(answer) = answer
        && let Some(job) = state.job.take()
    {
        let result = (|| -> Result<(), String> {
            let current = app.session.state().map_err(|e| e.to_string())?;
            if current.uid != job.uid {
                return Err("Model result belongs to another document; nothing was applied".into());
            }
            if job.document_revision.is_some_and(|r| current.revision != r) {
                return Err("Drawing changed during projection; nothing was applied".into());
            }
            if job.space.as_ref().is_some_and(|(space, mspace)| current.space != *space || current.mspace != *mspace) {
                return Err("Target space changed during projection; nothing was applied".into());
            }
            state.cache = None;
            state.update(app)?;
            if job.guard.as_ref().is_some_and(|g| !state.current(g)) {
                return Err("Linked model changed during the operation; nothing was applied".into());
            }
            match answer? {
                Outcome::Loaded(metadata) => {
                    let refresh = job.guard.as_ref().map(|g| (g.id.as_str(), g.sha256.as_str()));
                    let report = models::apply_loaded(&mut app.session, metadata, refresh).map_err(|e| e.to_string())?;
                    state.selected = report["id"].as_str().ok_or_else(|| "Model result has no ID".to_string())?.into();
                    state.cache = None;
                    state.update(app)?;
                    state.preview(app)?;
                }
                Outcome::View { projection, params } => {
                    let guard = job.guard.as_ref().ok_or_else(|| "Projection has no source guard".to_string())?;
                    models::apply_view(&mut app.session, &guard.id, &guard.sha256, &params, *projection).map_err(|e| e.to_string())?;
                    state.cache = None;
                }
                Outcome::Preview(scene) => {
                    let guard = job.guard.ok_or_else(|| "Preview has no source guard".to_string())?;
                    if state.selected != guard.id {
                        return Err("Model selection changed during preview; request a new preview".into());
                    }
                    state.scene = Some(scene);
                    state.scene_guard = Some(guard);
                }
                Outcome::Status(value) => {
                    let guard = job.guard.ok_or_else(|| "Source status has no guard".to_string())?;
                    state.status = Some((guard, value));
                }
                Outcome::References(report) => {
                    let guard = job.guard.ok_or_else(|| "Exact edges have no source guard".to_string())?;
                    if state.selected != guard.id {
                        return Err("Model selection changed while loading exact edges; request them again".into());
                    }
                    state.dimensions.install(guard, report)?;
                }
                Outcome::Dimension { params, resolutions } => {
                    models::dimensions::apply_dimension(&mut app.session, &params, resolutions).map_err(|e| e.to_string())?;
                    state.cache = None;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            app.set_status(error.clone());
            state.error = Some(error);
        }
    }
    if state.job.is_some() {
        ctx.request_repaint_after(Duration::from_millis(30));
    }
    app.models = state;
}

fn metadata(ui: &mut egui::Ui, value: &Value, depth: usize, left: &mut usize) {
    if *left == 0 {
        ui.small(crate::tl!("Display limit reached; expand fewer branches."));
        return;
    }
    *left -= 1;
    if depth > 32 {
        ui.small(crate::tl!("Metadata nesting limit reached."));
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if *left == 0 {
                    break;
                }
                ui.push_id(key, |ui| {
                    egui::CollapsingHeader::new(key).show(ui, |ui| metadata(ui, value, depth + 1, left));
                });
                *left = left.saturating_sub(1);
            }
        }
        Value::Array(array) => {
            for (i, value) in array.iter().enumerate() {
                if *left == 0 {
                    break;
                }
                ui.push_id(i, |ui| {
                    egui::CollapsingHeader::new(i.to_string()).show(ui, |ui| metadata(ui, value, depth + 1, left));
                });
                *left = left.saturating_sub(1);
            }
        }
        Value::String(text) => {
            let end = text.char_indices().nth(2048).map_or(text.len(), |(i, _)| i);
            ui.monospace(&text[..end]);
            if end < text.len() {
                ui.small(crate::tl!("Text display truncated."));
            }
            if ui.small_button(crate::tl!("Copy value")).clicked() {
                ui.ctx().copy_text(text.clone());
            }
        }
        _ => {
            ui.monospace(value.to_string());
        }
    }
}

fn dialog(app: &mut CadApp, ctx: &egui::Context, _: &str, open: &mut bool) -> Action {
    let mut state = std::mem::take(&mut app.models);
    if let Err(error) = state.update(app) {
        state.rows.clear();
        state.error = Some(error);
    }
    let mut action = None;
    let available = model_io::hooks().is_some();
    let busy = state.job.is_some();
    let bg = crate::theme::Tokens::get().canvas;
    let device = app.canvas.gpu_device.clone();
    let gpu_enabled = app.canvas.gpu.is_some();
    egui::Window::new(crate::tl!("Linked 3D Models"))
        .id(egui::Id::unique("gostmodels"))
        .open(open)
        .default_size(egui::vec2(1080.0, 780.0))
        .vscroll(true)
        .show(ctx, |ui| {
            if !available {
                ui.colored_label(egui::Color32::YELLOW, crate::tl!("Native model worker is unavailable."));
            }
            ui.add_enabled_ui(available && !busy, |ui| {
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("Source file"));
                    ui.add(egui::TextEdit::singleline(&mut state.path).desired_width(600.0));
                    if ui.add_enabled(app.services.pick_model.is_some(), egui::Button::new(crate::tl!("Browse…"))).clicked() {
                        action = Some(Act::Browse);
                    }
                    if ui.add_enabled(!state.path.trim().is_empty(), egui::Button::new(crate::tl!("Import 3D model"))).clicked() {
                        action = Some(Act::Import);
                    }
                });
            });
            if let Some(job) = &state.job {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(job.kind.label());
                    ui.label(format!("{:.1} s", job.started.elapsed().as_secs_f64()));
                });
            }
            if let Some(error) = &state.error {
                ui.colored_label(egui::Color32::RED, error);
            }
            ui.separator();
            ui.columns(2, |columns| {
                columns[0].label(crate::tl!("Linked models"));
                egui::ScrollArea::vertical().id_salt("model-list").max_height(190.0).show(&mut columns[0], |ui| {
                    for model in &state.rows {
                        let name = model.path.rsplit(['/', '\\']).next().unwrap_or(&model.path);
                        if ui.selectable_label(state.selected == model.id, format!("{}: {}", model.id, name)).on_hover_text(&model.path).clicked() {
                            state.selected = model.id.clone();
                            if state.scene_guard.as_ref().is_some_and(|g| g.id != state.selected) {
                                state.scene = None;
                                state.scene_guard = None;
                            }
                        }
                    }
                });
                columns[0].add_enabled_ui(available && !busy && !state.selected.is_empty(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(crate::tl!("3D preview")).clicked() {
                            action = Some(Act::Preview);
                        }
                        if ui.button(crate::tl!("Check source")).clicked() {
                            action = Some(Act::Status);
                        }
                        if ui.button(crate::tl!("Refresh source")).clicked() {
                            action = Some(Act::Refresh);
                        }
                        if ui.button(crate::tl!("Remove link")).clicked() {
                            action = Some(Act::Remove);
                        }
                    });
                    ui.separator();
                    ui.label(crate::tl!("Exact drawing view"));
                    craft_ui::ComboBox::from_id_salt("exact-view-direction").selected_text(state.direction.label()).show_ui(ui, |ui| {
                        for direction in Direction::ALL {
                            ui.selectable_value(&mut state.direction, direction, direction.label());
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(crate::tl!("Scale 1:"));
                        ui.add(egui::DragValue::new(&mut state.scale).range(0.01..=10000.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label(crate::tl!("Insertion point"));
                        for coordinate in &mut state.at {
                            ui.add(egui::DragValue::new(coordinate).speed(1.0));
                        }
                    });
                    let target = app
                        .session
                        .state()
                        .ok()
                        .map(|s| match &s.space {
                            Space::Model => crate::tl!("Model").to_string(),
                            Space::Paper(name) => name.clone(),
                        })
                        .unwrap_or_default();
                    ui.label(format!("{}: {}", crate::tl!("Target space"), target));
                    ui.checkbox(&mut state.hidden, crate::tl!("Show hidden lines"));
                    ui.checkbox(&mut state.accept_incomplete, crate::tl!("Accept incomplete geometry for manual review"));
                    if ui.button(crate::tl!("Create exact view")).clicked() {
                        action = Some(Act::View);
                    }
                    if let Some(model) = state.rows.iter().find(|m| m.id == state.selected)
                        && let Some(dimension_action) = state.dimensions.show(ui, model)
                    {
                        action = Some(dimension_action);
                    }
                });
                if let Some((guard, status)) = &state.status
                    && guard.id == state.selected
                {
                    match status["changed"].as_bool() {
                        Some(true) => {
                            columns[0].colored_label(egui::Color32::YELLOW, crate::tl!("Source changed; saved views were not rebuilt."));
                        }
                        Some(false) => {
                            columns[0].label(crate::tl!("Source fingerprint is unchanged."));
                        }
                        None => {
                            columns[0].colored_label(egui::Color32::RED, crate::tl!("Invalid source status response."));
                        }
                    }
                }
                if let Some(model) = state.rows.iter().find(|m| m.id == state.selected) {
                    columns[1].label(format!("{}: {}", crate::tl!("Linked revision"), model.revision));
                    columns[1].small(&model.source.sha256);
                    columns[1].label(format!("{}: {}", crate::tl!("Saved views"), model.views.len()));
                }
                if let Some(scene) = &mut state.scene {
                    scene.show(&mut columns[1], bg, device.clone(), gpu_enabled, &mut state.gpu_key);
                }
                if let Some(scene) = &state.scene {
                    columns[1].collapsing(crate::tl!("Preview quality"), |ui| metadata(ui, &scene.quality, 0, &mut 128));
                }
                if let Some(model) = state.rows.iter().find(|m| m.id == state.selected) {
                    egui::ScrollArea::vertical().id_salt("model-metadata").max_height(240.0).show(&mut columns[1], |ui| {
                        ui.collapsing(crate::tl!("Source metadata and assembly structure"), |ui| metadata(ui, &model.metadata, 0, &mut 512));
                        ui.collapsing(crate::tl!("Saved view quality and provenance"), |ui| {
                            for (i, view) in model.views.iter().enumerate() {
                                ui.push_id(i, |ui| {
                                    ui.collapsing(view["handle"].as_str().unwrap_or("unresolved"), |ui| metadata(ui, view, 0, &mut 512));
                                });
                            }
                        });
                    });
                }
            });
            if ui.button(crate::tl!("Reload model list")).clicked() {
                action = Some(Act::ReloadList);
            }
        });
    app.models = state;
    if let Some(Act::Browse) = action {
        let picked = app.pick_then(Pick::Model, |app, path| {
            app.models.path = path;
            Ok(Value::Null)
        });
        if let Some(Err(error)) = picked {
            app.models.error = Some(error);
        }
    } else if let Some(action) = action {
        let mut state = std::mem::take(&mut app.models);
        if let Err(error) = state.dispatch(app, action) {
            state.error = Some(error);
        }
        app.models = state;
        ctx.request_repaint();
    }
    None
}
