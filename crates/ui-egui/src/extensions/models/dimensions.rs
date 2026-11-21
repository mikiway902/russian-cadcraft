//! Edge choices are exact source references; fractions are arc length, never mesh picks.
use super::{Act, Guard, Linked};
use serde_json::{Value, json};
use std::sync::Arc;

struct Edge {
    label: String,
    reference: String,
}

pub(super) struct State {
    guard: Option<Arc<Guard>>,
    edges: Vec<Edge>,
    selected: [usize; 2],
    fractions: [f64; 2],
    view: String,
    kind: &'static str,
    offset: f64,
}
impl Default for State {
    fn default() -> Self {
        Self { guard: None, edges: Vec::new(), selected: [0, 0], fractions: [0.0, 1.0], view: String::new(), kind: "aligned", offset: 10.0 }
    }
}
impl State {
    pub(super) fn invalidate(&mut self, models: &[Linked]) {
        if self
            .guard
            .as_ref()
            .is_some_and(|g| !models.iter().any(|m| m.id == g.id && m.revision == g.revision && m.path == g.path && m.source.sha256 == g.sha256))
        {
            *self = Self::default();
        }
    }
    pub(super) fn install(&mut self, guard: Arc<Guard>, mut report: Value) -> Result<(), String> {
        let Value::Array(placements) = report["placements"].take() else {
            return Err("Exact references have no placements".into());
        };
        let mut edges = Vec::new();
        for mut placement in placements {
            let index = placement["placement"].as_u64().ok_or_else(|| "Exact reference has no placement index".to_string())?;
            let Value::Array(source_edges) = placement["edges"].take() else {
                return Err("Exact placement has no edges".into());
            };
            for mut edge in source_edges {
                let edge_index = edge["index"].as_u64().ok_or_else(|| "Exact edge has no index".to_string())?;
                let name = edge["name"].as_str().unwrap_or("");
                let label = format!("{index}/{edge_index}: {name}");
                let Value::String(reference) = edge["reference"].take() else {
                    return Err("Exact edge has no reference".into());
                };
                edges.push(Edge { label, reference });
            }
        }
        self.guard = Some(guard);
        self.edges = edges;
        self.selected = [0, 0];
        Ok(())
    }
    pub(super) fn params(&self, guard: &Guard) -> Result<Value, String> {
        if self.guard.as_ref().is_none_or(|g| g.id != guard.id || g.revision != guard.revision || g.path != guard.path || g.sha256 != guard.sha256) {
            return Err("Load exact edges for the selected model first".into());
        }
        let anchors = [0, 1].map(|i| self.edges.get(self.selected[i]).map(|edge| json!({"reference":edge.reference,"fraction":self.fractions[i]})));
        let [Some(a), Some(b)] = anchors else {
            return Err("Select two exact edge anchors first".into());
        };
        if self.view.is_empty() {
            return Err("Select a current saved view first".into());
        }
        Ok(json!({"view":self.view,"a":a,"b":b,"kind":self.kind,"offset":self.offset}))
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui, model: &Linked) -> Option<Act> {
        let mut action = None;
        ui.collapsing(crate::tl!("Exact Model Dimension"), |ui| {
            ui.small(crate::tl!("Anchors use trimmed-edge arc-length fractions in source millimetres, not the display mesh."));
            if ui.button(crate::tl!("Load exact model edges")).clicked() {
                action = Some(Act::References);
            }
            let ready = self
                .guard
                .as_ref()
                .is_some_and(|g| g.id == model.id && g.revision == model.revision && g.sha256 == model.source.sha256 && g.path == model.path);
            ui.add_enabled_ui(ready && !self.edges.is_empty(), |ui| {
                let views = || model.views.iter().filter(|v| v["stale"] == false).filter_map(|v| v["handle"].as_str());
                if !views().any(|v| v == self.view) {
                    self.view.clear();
                    self.view.push_str(views().next().unwrap_or_default());
                }
                craft_ui::ComboBox::from_id_salt("exact-dimension-view").selected_text(&self.view).show_ui(ui, |ui| {
                    for view in views() {
                        if ui.selectable_label(self.view == view, view).clicked() {
                            self.view.clear();
                            self.view.push_str(view);
                        }
                    }
                });
                for i in 0..2 {
                    ui.push_id(i, |ui| {
                        ui.label(if i == 0 { crate::tl!("First exact anchor") } else { crate::tl!("Second exact anchor") });
                        let label = self.edges.get(self.selected[i]).map_or("", |edge| edge.label.as_str());
                        craft_ui::ComboBox::from_id_salt("edge").selected_text(label).show_ui(ui, |ui| {
                            for (index, edge) in self.edges.iter().enumerate() {
                                ui.selectable_value(&mut self.selected[i], index, &edge.label);
                            }
                        });
                        ui.add(egui::DragValue::new(&mut self.fractions[i]).range(0.0..=1.0).speed(0.01).prefix(crate::tl!("Arc-length fraction: ")));
                    });
                }
                craft_ui::ComboBox::from_id_salt("exact-dimension-kind")
                    .selected_text(match self.kind {
                        "horizontal" => crate::tl!("Horizontal"),
                        "vertical" => crate::tl!("Vertical"),
                        _ => crate::tl!("Aligned: true 3D distance"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.kind, "aligned", crate::tl!("Aligned: true 3D distance"));
                        ui.selectable_value(&mut self.kind, "horizontal", crate::tl!("Horizontal"));
                        ui.selectable_value(&mut self.kind, "vertical", crate::tl!("Vertical"));
                    });
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("Paper offset, mm"));
                    ui.add(egui::DragValue::new(&mut self.offset).range(-10000.0..=10000.0));
                });
                if ui.add_enabled(!self.view.is_empty(), egui::Button::new(crate::tl!("Create exact dimension"))).clicked() {
                    action = Some(Act::Dimension);
                }
            });
        });
        action
    }
}
