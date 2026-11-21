//! Cutting-plane controls use original body indices, never display triangles.
use super::{Act, Guard, Linked};
use cadcraft_doc::model_section::KeptSide;
use serde_json::{Value, json};

pub(super) struct State {
    model_id: String,
    revision: u64,
    origin: [f64; 3],
    normal: [f64; 3],
    kept_side: KeptSide,
    excluded: u32,
}
impl Default for State {
    fn default() -> Self {
        Self { model_id: String::new(), revision: 0, origin: [0.0; 3], normal: [0.0, 0.0, 1.0], kept_side: KeptSide::Positive, excluded: 0 }
    }
}
impl State {
    pub(super) fn params(&self, guard: &Guard, scale: f64, at: [f64; 2], hidden: bool, accept: bool) -> Result<Value, String> {
        if self.model_id != guard.id || self.revision != guard.revision {
            return Err("Section selection belongs to another source revision".into());
        }
        let excluded: Vec<usize> = (0..32).filter(|i| self.excluded & (1_u32 << i) != 0).collect();
        Ok(json!({"id":guard.id,"plane":{"origin":self.origin,"normal":self.normal},"kept_side":self.kept_side,
            "exclude_bodies":excluded,"scale":scale,"at":at,"hidden":hidden,"accept_incomplete":accept}))
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui, model: &Linked) -> Option<Act> {
        if self.model_id != model.id || self.revision != model.revision {
            self.model_id.clone_from(&model.id);
            self.revision = model.revision;
            self.excluded = 0;
        }
        let mut action = None;
        ui.collapsing(crate::tl!("Exact model section"), |ui| {
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
            ui.small(crate::tl!("Frozen exact geometry; cutting-plane labels are separate."));
            if ui.add_enabled(valid_body_count, egui::Button::new(crate::tl!("Create exact section"))).clicked() {
                action = Some(Act::Section);
            }
        });
        action
    }
}
