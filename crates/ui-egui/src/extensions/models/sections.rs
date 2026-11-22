//! Cutting-plane controls use original body indices, never display triangles.
use super::{Act, Guard, Linked};
use cadcraft_doc::model_section::KeptSide;
use serde_json::{Value, json};

pub(super) struct State {
    model_id: String,
    revision: u64,
    source_sha256: String,
    source_view: String,
    designation: String,
    origin: [f64; 3],
    normal: [f64; 3],
    kept_side: KeptSide,
    presentation: &'static str,
    excluded: u32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            model_id: String::new(),
            revision: 0,
            source_sha256: String::new(),
            source_view: String::new(),
            designation: String::new(),
            origin: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
            kept_side: KeptSide::Positive,
            presentation: "cutaway",
            excluded: 0,
        }
    }
}
impl State {
    pub(super) fn params(&self, guard: &Guard, scale: f64, at: [f64; 2], hidden: bool, accept: bool) -> Result<Value, String> {
        if self.model_id != guard.id || self.revision != guard.revision || self.source_sha256 != guard.sha256 {
            return Err("Section selection belongs to another source revision".into());
        }
        let excluded: Vec<usize> = (0..32).filter(|i| self.excluded & (1_u32 << i) != 0).collect();
        let mut params = json!({"id":guard.id,"plane":{"origin":self.origin,"normal":self.normal},"kept_side":self.kept_side,
            "presentation":self.presentation,"exclude_bodies":excluded,"scale":scale,"at":at,"hidden":hidden,"accept_incomplete":accept});
        if !self.source_view.is_empty() {
            params["source_view"] = json!(self.source_view);
            if self.presentation == "imposed" {
                params["label"] = Value::Null;
            } else if !self.designation.trim().is_empty() {
                params["label"] = json!(self.designation.trim());
            }
        }
        Ok(params)
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui, model: &Linked) -> Option<Act> {
        if self.model_id != model.id || self.revision != model.revision || self.source_sha256 != model.source.sha256 {
            self.model_id.clone_from(&model.id);
            self.revision = model.revision;
            self.source_sha256.clone_from(&model.source.sha256);
            self.source_view.clear();
            self.designation.clear();
            self.excluded = 0;
        }
        let views = || {
            model
                .views
                .iter()
                .filter(|view| {
                    view["stale"] == false
                        && view["revision"].as_u64() == Some(model.revision)
                        && view["source"]["sha256"].as_str() == Some(model.source.sha256.as_str())
                        && view["provenance"].get("section").is_none()
                })
                .filter_map(|view| view["handle"].as_str())
        };
        if !self.source_view.is_empty() && !views().any(|view| view == self.source_view) {
            self.source_view.clear();
            self.designation.clear();
        }
        let mut action = None;
        ui.collapsing(crate::tl!("Exact model section"), |ui| {
            ui.label(crate::tl!("Section presentation"));
            let label = match self.presentation {
                "detached" => crate::tl!("Detached cross-section — thick contour"),
                "imposed" => crate::tl!("Imposed cross-section — thin contour"),
                _ => crate::tl!("Cutaway — retained geometry"),
            };
            craft_ui::ComboBox::from_id_salt("exact-section-presentation").selected_text(label).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.presentation, "cutaway", crate::tl!("Cutaway — retained geometry"));
                ui.selectable_value(&mut self.presentation, "detached", crate::tl!("Detached cross-section — thick contour"));
                ui.selectable_value(&mut self.presentation, "imposed", crate::tl!("Imposed cross-section — thin contour"));
            });
            if self.presentation != "cutaway" {
                ui.small(crate::tl!("Cross-sections contain only the cutting-plane regions; position them using the view insertion controls."));
            }
            if self.presentation == "imposed" {
                ui.small(crate::tl!("Imposed cross-sections do not erase the underlying view."));
            }
            ui.label(crate::tl!("Source ordinary view"));
            let source_label = if self.source_view.is_empty() { crate::tl!("No source — geometry only") } else { &self.source_view };
            craft_ui::ComboBox::from_id_salt("exact-section-source").selected_text(source_label).show_ui(ui, |ui| {
                if ui.selectable_label(self.source_view.is_empty(), crate::tl!("No source — geometry only")).clicked() {
                    self.source_view.clear();
                    self.designation.clear();
                }
                for view in views() {
                    if ui.selectable_label(self.source_view == view, view).clicked() && self.source_view != view {
                        self.source_view.clear();
                        self.source_view.push_str(view);
                        self.designation.clear();
                    }
                }
            });
            if self.source_view.is_empty() {
                ui.small(crate::tl!("Without a source view, only frozen exact geometry is created; cutting-plane notation is not linked."));
            } else {
                ui.small(crate::tl!("Use an ordinary view on the same millimetre sheet, with positive uniform unrotated scale and the cutting plane seen edge-on."));
                if self.presentation == "imposed" {
                    ui.small(crate::tl!("Non-symmetric imposed sections use source arrows without letters."));
                } else {
                    ui.label(crate::tl!("Section designation (optional)"));
                    ui.text_edit_singleline(&mut self.designation);
                    ui.small(crate::tl!("Leave blank to choose the next unused section designation."));
                }
            }
            ui.label(crate::tl!("Plane origin (mm)"));
            ui.horizontal(|ui| {
                for coordinate in &mut self.origin {
                    ui.add(egui::DragValue::new(coordinate).range(-1e6..=1e6).speed(1.0));
                }
            });
            ui.label(crate::tl!("Plane normal"));
            ui.horizontal(|ui| {
                for coordinate in &mut self.normal {
                    ui.add(egui::DragValue::new(coordinate).range(-1e6..=1e6).speed(0.1));
                }
            });
            let side = match self.kept_side {
                KeptSide::Positive => crate::tl!("Keep positive half-space"),
                KeptSide::Negative => crate::tl!("Keep negative half-space"),
            };
            craft_ui::ComboBox::from_id_salt("exact-section-side").selected_text(side).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.kept_side, KeptSide::Positive, crate::tl!("Keep positive half-space"));
                ui.selectable_value(&mut self.kept_side, KeptSide::Negative, crate::tl!("Keep negative half-space"));
            });
            let mut valid_body_count = false;
            if let Some(bodies) = model.metadata["bodies"].as_array() {
                valid_body_count = !bodies.is_empty() && bodies.len() <= 32;
                if valid_body_count {
                    ui.collapsing(crate::tl!("Omit bodies entirely (not uncut exceptions)"), |ui| {
                        for index in 0..bodies.len() {
                            let bit = 1_u32 << index;
                            let mut excluded = self.excluded & bit != 0;
                            if ui.checkbox(&mut excluded, format!("{} {}", crate::tl!("Body"), index)).changed() {
                                if excluded {
                                    self.excluded |= bit;
                                } else {
                                    self.excluded &= !bit;
                                }
                            }
                        }
                    });
                }
            }
            ui.small(crate::tl!("Hatching: 3 mm on paper; scale and insertion use the view controls above."));
            ui.small(crate::tl!("Exact section geometry stays frozen. Source-bound cutting-plane notation follows 2D source MOVE; no 3D source rebuild or symmetry inference."));
            let create = if self.presentation == "cutaway" { crate::tl!("Create exact section") } else { crate::tl!("Create exact cross-section") };
            if ui.add_enabled(valid_body_count, egui::Button::new(create)).clicked() {
                action = Some(Act::Section);
            }
        });
        action
    }
}
