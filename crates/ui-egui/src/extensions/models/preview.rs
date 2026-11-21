//! Read-only orbit preview: upstream triangles, existing camera math and CADCraft renderer.
//! The temporary drawing is never given to a Session, saved, measured or used for drafting.
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::channel;
use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, TryRecvError},
};
use std::time::Duration;

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, Drawing, EntityKind, Handle, PolyfaceMesh, Space, model_preview::Preview};
use cadcraft_geom::{Vec2, Vec3, drafting3d::Frame};
use cadcraft_render::DisplayList;
use egui::{PointerButton, Sense, vec2};

use crate::{
    canvas::{self, Xf},
    gpu,
};

type Device = egui_wgpu::wgpu::Device;

#[derive(Clone, Copy, PartialEq)]
struct Pose {
    yaw: f64,
    pitch: f64,
}
impl Default for Pose {
    fn default() -> Self {
        Self { yaw: -std::f64::consts::FRAC_PI_4, pitch: (1.0_f64 / 3.0_f64.sqrt()).asin() }
    }
}
impl Pose {
    fn frame(self) -> Result<Frame, String> {
        let direction = Vec3::new(self.pitch.cos() * self.yaw.cos(), self.pitch.cos() * self.yaw.sin(), self.pitch.sin());
        let up = if direction.z.abs() > 0.999_999 { Vec3::new(0.0, 1.0, 0.0) } else { Vec3::Z };
        Frame::new(direction, up)
    }
}

struct Input {
    source: Arc<Preview>,
    drawing: Drawing,
    handles: Vec<Handle>,
    origin: Vec3,
    radius: f64,
    pose: Pose,
}
impl Input {
    fn new(source: Arc<Preview>) -> Result<Self, String> {
        let first = source
            .bodies
            .iter()
            .flat_map(|b| b.positions.iter())
            .next()
            .copied()
            .ok_or_else(|| "Preview has no display vertices; inspect missing/refused faces.".to_string())?;
        let (mut min, mut max) = (first, first);
        for p in source.bodies.iter().flat_map(|b| b.positions.iter()) {
            if !p.is_finite() {
                return Err("Invalid preview coordinates".into());
            }
            min = Vec3::new(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z));
            max = Vec3::new(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z));
        }
        let origin = (min + max) * 0.5;
        let radius = (max - min).len() * 0.5;
        if !radius.is_finite() || radius <= 0.0 {
            return Err("Preview has no non-degenerate display extent".into());
        }
        let mut drawing = Drawing::new_metric();
        let mut handles = Vec::with_capacity(source.bodies.len());
        let pose = Pose::default();
        let frame = pose.frame()?;
        let colors = [Rgb(50, 160, 220), Rgb(220, 140, 50), Rgb(90, 180, 120), Rgb(180, 110, 210)];
        for body in &source.bodies {
            let faces = body
                .triangles
                .iter()
                .map(|tri| {
                    let mut face = [0_i32; 4];
                    for (slot, &i) in face.iter_mut().zip(tri) {
                        if usize::try_from(i).ok().is_none_or(|i| i >= body.positions.len()) {
                            return Err("Invalid preview triangle index".to_string());
                        }
                        *slot = i32::try_from(i).ok().and_then(|n| n.checked_add(1)).ok_or_else(|| "Preview index overflow".to_string())?;
                    }
                    Ok(face)
                })
                .collect::<Result<Vec<_>, String>>()?;
            let vertices = body
                .positions
                .iter()
                .map(|&src| {
                    let p = frame.point(src - origin);
                    Vec3::new(p.x, p.y, 0.0)
                })
                .collect();
            let kind = EntityKind::PolyfaceMesh(PolyfaceMesh { vertices, faces });
            let common = Common { color: Color::True(colors[body.placement % colors.len()]), ..Default::default() };
            handles.push(drawing.add(&Space::Model, common, kind).map_err(|e| e.to_string())?);
        }
        Ok(Self { source, drawing, handles, origin, radius, pose })
    }
}

struct Built {
    input: Input,
    list: DisplayList,
    mesh: Option<gpu::MeshData>,
    pose: Pose,
}
fn build(mut input: Input, pose: Pose, device: Option<Device>) -> Result<Built, String> {
    if input.pose != pose {
        let frame = pose.frame()?;
        for (body, &handle) in input.source.bodies.iter().zip(&input.handles) {
            let mut mesh_found = false;
            input
                .drawing
                .modify_entity(handle, |entity| {
                    if let EntityKind::PolyfaceMesh(mesh) = &mut entity.kind {
                        mesh_found = true;
                        for (dst, &src) in mesh.vertices.iter_mut().zip(&body.positions) {
                            let p = frame.point(src - input.origin);
                            *dst = Vec3::new(p.x, p.y, 0.0);
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            if !mesh_found {
                return Err("Invalid preview entity type".into());
            }
        }
        input.pose = pose;
    }
    let list = cadcraft_render::build(&input.drawing, &Space::Model, &Default::default());
    // All preview entities use explicit, opaque RGB. No theme-dependent colour-7 or masks.
    let mesh = device.map(|device| {
        let mut mesh = gpu::build_mesh(&list, Vec2::ZERO, Rgb(0, 0, 0), false);
        mesh.upload_now(&device);
        mesh
    });
    Ok(Built { input, list, mesh, pose })
}

pub(super) struct Scene {
    input: Option<Input>,
    list: DisplayList,
    slot: gpu::MeshSlot,
    pending: Option<Receiver<Result<Built, String>>>,
    rendered: Pose,
    target: Pose,
    radius: f64,
    zoom: f64,
    pan: Vec2,
    error: Option<String>,
    uploaded: bool,
    gpu_key: Option<u64>,
    pub(super) quality: serde_json::Value,
}
impl Scene {
    /// Called on the model background job, not on the UI thread.
    pub(super) fn new(mut preview: Preview, device: Option<Device>) -> Result<Self, String> {
        let quality = std::mem::take(&mut preview.quality);
        let input = Input::new(Arc::new(preview))?;
        let radius = input.radius;
        let built = build(input, Pose::default(), device)?;
        let uploaded = built.mesh.is_some();
        Ok(Self {
            input: Some(built.input),
            list: built.list,
            slot: Arc::new(Mutex::new(built.mesh)),
            pending: None,
            rendered: built.pose,
            target: built.pose,
            radius,
            zoom: 1.0,
            pan: Vec2::ZERO,
            error: None,
            uploaded,
            gpu_key: None,
            quality,
        })
    }
    fn poll(&mut self, ctx: &egui::Context, device: Option<Device>) {
        let answer = self.pending.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(answer) => Some(answer),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("Preview rebuild ended without a result".into())),
        });
        if let Some(answer) = answer {
            self.pending = None;
            match answer {
                Ok(built) => {
                    self.rendered = built.pose;
                    self.input = Some(built.input);
                    if self.rendered == self.target {
                        self.list = built.list;
                        self.uploaded = built.mesh.is_some();
                        if let Ok(mut slot) = self.slot.lock() {
                            *slot = built.mesh;
                        } else {
                            self.error = Some("Preview GPU upload state is unavailable".into());
                        }
                        self.gpu_key = None;
                    }
                }
                Err(e) => self.error = Some(e),
            }
        }
        if self.pending.is_none()
            && self.rendered != self.target
            && self.error.is_none()
            && let Some(input) = self.input.take()
        {
            let target = self.target;
            #[cfg(not(target_arch = "wasm32"))]
            {
                let (tx, rx) = channel();
                match std::thread::Builder::new().name("model-preview-view".into()).spawn(move || {
                    let _ = tx.send(build(input, target, device));
                }) {
                    Ok(_) => self.pending = Some(rx),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = (input, target, device);
                self.error = Some("Native model preview is unavailable in the browser".into());
            }
        }
        if self.pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui, bg: egui::Color32, device: Option<Device>, gpu_enabled: bool, next_gpu_key: &mut u64) {
        ui.horizontal(|ui| {
            if ui.button(crate::tl!("Fit")).clicked() {
                self.zoom = 1.0;
                self.pan = Vec2::ZERO;
            }
            if ui.button(crate::tl!("Isometric")).clicked() {
                self.target = Pose::default();
            }
            if ui.button(crate::tl!("Front")).clicked() {
                self.target = Pose { yaw: -std::f64::consts::FRAC_PI_2, pitch: 0.0 };
            }
            if ui.button(crate::tl!("Top")).clicked() {
                self.target = Pose { yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2 };
            }
        });
        ui.label(crate::tl!("Display mesh only; never use it for dimensions or projections."));
        ui.small(crate::tl!("Drag to orbit; right-drag to pan; wheel to zoom."));
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(64.0), 320.0), Sense::drag());
        let scale = f64::from(rect.width().min(rect.height())) * 0.42 / self.radius * self.zoom;
        if ui.ctx().current_pass_index() == 0 {
            let delta = response.drag_delta();
            if response.dragged_by(PointerButton::Secondary) {
                self.pan += Vec2::new(-f64::from(delta.x) / scale, f64::from(delta.y) / scale);
            } else if response.dragged_by(PointerButton::Primary) {
                self.target.yaw -= f64::from(delta.x) * 0.008;
                self.target.pitch = (self.target.pitch + f64::from(delta.y) * 0.008).clamp(-1.553_343, 1.553_343);
            }
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                self.zoom = (self.zoom * (f64::from(scroll) * 0.002).exp()).clamp(0.05, 100.0);
            }
        }
        self.poll(ui.ctx(), device);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, bg);
        let xf = Xf { rect, center: self.pan, scale, twist: 0.0 };
        if gpu_enabled && self.uploaded {
            let center = rect.center();
            let key = *self.gpu_key.get_or_insert_with(|| {
                *next_gpu_key = next_gpu_key.wrapping_add(1);
                *next_gpu_key
            });
            painter.add(egui_wgpu::Callback::new_paint_callback(
                rect,
                gpu::CanvasCallback {
                    key,
                    slot: self.slot.clone(),
                    offset: [-self.pan.x as f32, -self.pan.y as f32],
                    scale: scale as f32,
                    center: [center.x, center.y],
                    layer: gpu::LAYER_MODEL_PREVIEW,
                    xform: gpu::IDENTITY_XFORM,
                    dash: 0.0,
                    stencil: gpu::Stencil::Off,
                },
            ));
        } else {
            canvas::draw_list(&painter, &xf, &self.list, Rgb(bg.r(), bg.g(), bg.b()), false);
        }
        if self.pending.is_some() {
            ui.spinner();
            ui.label(crate::tl!("Updating model preview…"));
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::RED, error);
        }
    }
}
