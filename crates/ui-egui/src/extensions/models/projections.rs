//! First-angle related-view controls; all source/projection work remains in the engine/host.
use super::{Act, Linked};
use serde_json::{Value, json};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Direction {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}
impl Direction {
    const ALL: [Self; 4] = [Self::Top, Self::Bottom, Self::Left, Self::Right];
    fn id(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Top => crate::tl!("Top view — below parent"),
            Self::Bottom => crate::tl!("Bottom view — above parent"),
            Self::Left => crate::tl!("Left view — right of parent"),
            Self::Right => crate::tl!("Right view — left of parent"),
        }
    }
}

pub(super) struct State {
    parent: String,
    direction: Direction,
    gap: f64,
}
impl Default for State {
    fn default() -> Self {
        Self { parent: String::new(), direction: Direction::default(), gap: 20.0 }
    }
}
impl State {
    pub(super) fn params(&self, hidden: bool, accept: bool) -> Result<Value, String> {
        if self.parent.is_empty() {
            return Err("Select a current exact parent view first".into());
        }
        Ok(json!({"parent":self.parent,"direction":self.direction.id(),"gap":self.gap,"hidden":hidden,"accept_incomplete":accept}))
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui, model: &Linked) -> Option<Act> {
        let mut action = None;
        ui.collapsing(crate::tl!("Projection-linked view"), |ui| {
            let views = || {
                model.views.iter().filter(|v| v["stale"] == false && v["provenance"].get("section").is_none()).filter_map(|v| v["handle"].as_str())
            };
            if !views().any(|view| view == self.parent) {
                self.parent.clear();
                self.parent.push_str(views().next().unwrap_or_default());
            }
            ui.label(crate::tl!("Parent exact view"));
            craft_ui::ComboBox::from_id_salt("projection-link-parent").selected_text(&self.parent).show_ui(ui, |ui| {
                for view in views() {
                    if ui.selectable_label(self.parent == view, view).clicked() {
                        self.parent.clear();
                        self.parent.push_str(view);
                    }
                }
            });
            craft_ui::ComboBox::from_id_salt("projection-link-direction").selected_text(self.direction.label()).show_ui(ui, |ui| {
                for direction in Direction::ALL {
                    ui.selectable_value(&mut self.direction, direction, direction.label());
                }
            });
            ui.horizontal(|ui| {
                ui.label(crate::tl!("Paper gap, mm"));
                ui.add(egui::DragValue::new(&mut self.gap).range(0.0..=10000.0).speed(1.0));
            });
            ui.small(crate::tl!("Inherits the parent scale and sheet; first-angle projection."));
            if ui.add_enabled(!self.parent.is_empty(), egui::Button::new(crate::tl!("Create linked projected view"))).clicked() {
                action = Some(Act::ProjectedView);
            }
        });
        action
    }
}
