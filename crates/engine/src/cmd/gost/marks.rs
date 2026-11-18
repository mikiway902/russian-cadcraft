//! Original vector constructions from public ESKD conventions: ГОСТ 2.309-73
//! (roughness), ГОСТ 2.312-72 (weld indications), ГОСТ 2.311-68 (threads),
//! ГОСТ 2.308-2011 (tolerances). These are drafting objects, not manufacturing
//! verification, process-specific weld joint catalogues, or modeled 3D threads.

use std::sync::Arc as Shared;

use cadcraft_doc::{Arc, Block, Circle, Common, Drawing, Entity, EntityKind, GdtSymbol, HAlign, Handle, Insert, Line, Solid, Text, VAlign};
use cadcraft_geom::Vec2;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cmd::{CommandSpec, bad, find_command, point_req, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

const META: &str = "engineering-symbol";
const EDIT: &str = "gost.symbol.edit";
const INSPECT: &str = "gost.symbol.inspect";
const TOL: &str = "gost.tolerance";
const LIMITS: &[&str] = &[
    "Native 2D drafting geometry, not manufacturing verification or an associative attachment to a part.",
    "Thread geometry is a conventional symbolic representation, never a modeled 3D helix; basic metric profile only, without fits or runout.",
    "Welds support generic fillet/butt indications; process-specific standard/joint codes and inspection requirements must be specified separately by the author.",
];

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("gost.roughness", "GOST Roughness", roughness)
            .menu(&["Format", "GOST", "GOST Roughness"])
            .params("{at:[x,y], parameter?: Ra|Rz (Ra), value: positive micrometres, removal?: permitted|required|prohibited, lay?: parallel|perpendicular|crossed|multidirectional|circular|radial|null, direction?: degrees|null, allSurfaces?:bool, scale?:model units/paper mm, textHeight?:paper mm, rotation?:degrees} → {handle,block,kind,parameters,labels,limitations}")
            .interactive(|_| Ok(Box::new(CreateM::new("gost.roughness", "GOST.ROUGHNESS")))),
        CommandSpec::new("gost.weld", "GOST Weld", weld)
            .menu(&["Format", "GOST", "GOST Weld"])
            .params("{at:[x,y]: shelf/leader junction, type:fillet|butt, size:positive mm (fillet leg/butt thickness), length?:positive mm|null, pitch?:positive mm|null, side?:arrow|other, continuous?:bool (true; false requires length<pitch), field?:bool, allAround?:bool, leader?:[dx,dy] in local PAPER mm from junction (default [-12,-10]), scale?,textHeight?,rotation?} → native editable 2D symbol; no process-specific joint code inferred")
            .interactive(|_| Ok(Box::new(CreateM::new("gost.weld", "GOST.WELD")))),
        CommandSpec::new("gost.thread", "GOST Symbolic Thread", thread)
            .menu(&["Format", "GOST", "GOST Symbolic Thread"])
            .params("{at:[x,y]: axis origin, nominal:metric diameter mm, pitch:positive mm, length:mm >= pitch, side?:external|internal, view?:longitudinal|end|both, rotation?:axis degrees, scale?:annotation-only model units/paper mm, textHeight?:paper mm} → 2D conventional lines/circles/label, NOT a modeled 3D thread; physical diameter/length are not annotation-scaled")
            .interactive(|_| Ok(Box::new(CreateM::new("gost.thread", "GOST.THREAD")))),
        CommandSpec::new(EDIT, "Edit GOST Engineering Symbol", edit)
            .menu(&["Format", "GOST", "Edit GOST Engineering Symbol"])
            .params("{handle:hex (or one selected Insert), patch:{...kind-specific parameters}}; nullable optional fields clear with null; kind, at and transform are not patchable; stable handle/placement/transform/foreign xdata, copy-on-write block geometry")
            .interactive(|_| Ok(Box::new(PatchM))),
        CommandSpec::new(INSPECT, "Inspect GOST Engineering Symbol", inspect)
            .menu(&["Format", "GOST", "Inspect GOST Engineering Symbol"])
            .params("{handle?:hex (else one selected Insert)} → typed persistent parameters, native Insert transform, geometry count and labels; read-only")
            .noundo()
            .interactive(|_| Ok(Box::new(crate::cmd::machines::SelectRun::new(INSPECT, "GOST.SYMBOL.INSPECT")))),
        CommandSpec::new(TOL, "GOST Geometric Tolerance", tolerance)
            .menu(&["Format", "GOST", "GOST Geometric Tolerance"])
            .params("{at:[x,y],symbol:existing TOLERANCE characteristic,tol1:positive number|string|{value,diameter?:bool,material?:M},tol2?,datums?:[letter|{letter,material?:M}] (up to 3 distinct bases),line2?:{symbol,tol1,tol2?,datums?},projected?:bool,height?:positive mm,datumId?:uppercase base,rotation?:degrees}; raw text/style overrides are rejected; delegates native TOLERANCE using GOST text/dimension style, scale 1 in paper space")
            .interactive(|_| Ok(Box::new(CreateM::new(TOL, "GOST.TOLERANCE")))),
    ]
}

fn one() -> f64 {
    1.0
}
fn height() -> f64 {
    3.5
}
fn yes() -> bool {
    true
}
fn leader() -> [f64; 2] {
    [-12.0, -10.0]
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Symbol {
    Roughness(Roughness),
    Weld(Weld),
    Thread(Thread),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
enum Parameter {
    #[default]
    Ra,
    Rz,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
enum Removal {
    #[default]
    Permitted,
    Required,
    Prohibited,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Lay {
    Parallel,
    Perpendicular,
    Crossed,
    Multidirectional,
    Circular,
    Radial,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum WeldType {
    Fillet,
    Butt,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
enum WeldSide {
    #[default]
    Arrow,
    Other,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
enum ThreadSide {
    #[default]
    External,
    Internal,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
enum ThreadView {
    #[default]
    Longitudinal,
    End,
    Both,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Roughness {
    #[serde(default)]
    parameter: Parameter,
    value: f64,
    #[serde(default)]
    removal: Removal,
    #[serde(default)]
    lay: Option<Lay>,
    #[serde(default)]
    direction: Option<f64>,
    #[serde(default)]
    all_surfaces: bool,
    #[serde(default = "one")]
    scale: f64,
    #[serde(default = "height")]
    text_height: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Weld {
    #[serde(rename = "type")]
    weld_type: WeldType,
    size: f64,
    #[serde(default)]
    length: Option<f64>,
    #[serde(default)]
    pitch: Option<f64>,
    #[serde(default)]
    side: WeldSide,
    #[serde(default = "yes")]
    continuous: bool,
    #[serde(default)]
    field: bool,
    #[serde(default)]
    all_around: bool,
    #[serde(default = "leader")]
    leader: [f64; 2],
    #[serde(default = "one")]
    scale: f64,
    #[serde(default = "height")]
    text_height: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Thread {
    nominal: f64,
    pitch: f64,
    length: f64,
    #[serde(default)]
    side: ThreadSide,
    #[serde(default)]
    view: ThreadView,
    #[serde(default = "one")]
    scale: f64,
    #[serde(default = "height")]
    text_height: f64,
}

/// Bound structural work before cloning JSON or calling the native frame parser.
fn bounded(p: &Value, cmd: &str, depth: usize) -> Result<()> {
    if depth > 4 {
        return Err(bad(cmd, "parameter nesting exceeds four levels"));
    }
    match p {
        Value::Object(o) => {
            if o.len() > 20 || o.keys().any(|k| k.len() > 32) {
                return Err(bad(cmd, "too many or oversized parameter names"));
            }
            for v in o.values() {
                bounded(v, cmd, depth + 1)?;
            }
        }
        Value::Array(a) => {
            if a.len() > 3 {
                return Err(bad(cmd, "parameter arrays are limited to three elements"));
            }
            for v in a {
                bounded(v, cmd, depth + 1)?;
            }
        }
        Value::String(s) if s.len() > 256 => return Err(bad(cmd, "parameter strings are limited to 256 bytes")),
        _ => {}
    }
    Ok(())
}

fn number(cmd: &str, name: &str, v: f64, min: f64, max: f64) -> Result<()> {
    if !v.is_finite() || v < min || v > max {
        return Err(bad(cmd, format!("{name} must be finite and in [{min},{max}]")));
    }
    Ok(())
}
fn angle(p: &Value, key: &str, cmd: &str) -> Result<f64> {
    let n = match p.get(key) {
        None => 0.0,
        Some(v) => v.as_f64().ok_or_else(|| bad(cmd, format!("{key} must be a number")))?,
    };
    number(cmd, key, n, -36000.0, 36000.0)?;
    Ok(n.to_radians())
}
fn position(p: &Value, cmd: &str) -> Result<Vec2> {
    let at = point_req(cmd, p, "at")?;
    number(cmd, "at.x", at.x, -1e9, 1e9)?;
    number(cmd, "at.y", at.y, -1e9, 1e9)?;
    Ok(at)
}
fn ready(s: &Session, cmd: &str) -> Result<()> {
    let d = s.doc()?;
    let paper = s.space().layout().and_then(|n| d.layout(n)).is_some_and(|l| cadcraft_render::paper::page_unit_mm(d, &l.page) == 1.0);
    if (!paper && d.header.i64("INSUNITS", 0) != 4) || d.text_style("GOST").is_none() || d.dim_style("GOST").is_none() {
        return Err(bad(cmd, "apply gost.setup to a millimetre drawing first"));
    }
    for name in ["GOST-OUTLINE", "GOST-THIN", "GOST-TEXT", "GOST-DIM"] {
        if d.layer(name).is_none_or(|l| l.locked || !l.visible() || !l.plot) {
            return Err(bad(cmd, format!("{name} must exist and be unlocked, visible and printable")));
        }
    }
    Ok(())
}
fn defaults(s: &Session, p: &mut serde_json::Map<String, Value>) -> Result<()> {
    let style = s.doc()?.dim_style("GOST").ok_or_else(|| bad("gost.symbol", "GOST dimension style missing"))?;
    if !p.contains_key("scale") {
        p.insert("scale".into(), json!(if s.space().is_paper() { 1.0 } else { style.scale }));
    }
    if !p.contains_key("textHeight") {
        p.insert("textHeight".into(), json!(style.text_height));
    }
    Ok(())
}
impl Symbol {
    fn kind(&self) -> &'static str {
        match self {
            Self::Roughness(_) => "roughness",
            Self::Weld(_) => "weld",
            Self::Thread(_) => "thread",
        }
    }
    fn sizing(&self) -> (f64, f64) {
        match self {
            Self::Roughness(q) => (q.scale, q.text_height),
            Self::Weld(q) => (q.scale, q.text_height),
            Self::Thread(q) => (q.scale, q.text_height),
        }
    }
    fn validate(&self, cmd: &str) -> Result<()> {
        let (k, h) = self.sizing();
        number(cmd, "scale", k, 1e-6, 10000.0)?;
        if ![2.5, 3.5, 5.0, 7.0, 10.0, 14.0, 20.0].contains(&h) {
            return Err(bad(cmd, "textHeight must be a supported paper height: 2.5, 3.5, 5, 7, 10, 14 or 20 mm"));
        }
        match self {
            Self::Roughness(q) => {
                number(cmd, "value (micrometres)", q.value, 0.001, 10000.0)?;
                if let Some(v) = q.direction {
                    number(cmd, "direction", v, -360.0, 360.0)?;
                }
            }
            Self::Weld(q) => {
                number(cmd, "size", q.size, 0.01, 10000.0)?;
                for (name, v) in [("length", q.length), ("pitch", q.pitch)] {
                    if let Some(v) = v {
                        number(cmd, name, v, 0.01, 1e6)?;
                    }
                }
                if q.continuous && q.pitch.is_some() {
                    return Err(bad(cmd, "continuous welds cannot have an intermittent pitch"));
                }
                if !q.continuous && !matches!((q.length, q.pitch), (Some(l), Some(p)) if p > l) {
                    return Err(bad(cmd, "intermittent welds require positive length and pitch strictly greater than length"));
                }
                for v in q.leader {
                    number(cmd, "leader offset (paper mm)", v, -1000.0, 1000.0)?;
                }
                if q.leader[0].hypot(q.leader[1]) < 5.0 {
                    return Err(bad(cmd, "leader must be at least 5 paper mm long"));
                }
            }
            Self::Thread(q) => {
                number(cmd, "nominal", q.nominal, 0.1, 10000.0)?;
                number(cmd, "pitch", q.pitch, 0.01, 1000.0)?;
                number(cmd, "length", q.length, 0.01, 1e6)?;
                // ISO/GOST basic 60-degree metric profile: d3 = d - 1.226869 P.
                if q.nominal <= 1.226869 * q.pitch {
                    return Err(bad(cmd, "pitch leaves no positive basic metric thread root diameter"));
                }
                if q.length < q.pitch {
                    return Err(bad(cmd, "length must contain at least one complete pitch"));
                }
            }
        }
        Ok(())
    }
    fn parameters(&self) -> Result<Value> {
        let mut p = serde_json::to_value(self).map_err(|e| bad(INSPECT, e.to_string()))?;
        if let Some(o) = p.as_object_mut() {
            o.remove("kind");
        }
        Ok(p)
    }
}

fn decode(v: Value, cmd: &str) -> Result<Symbol> {
    let symbol: Symbol = serde_json::from_value(v).map_err(|e| bad(cmd, e.to_string()))?;
    symbol.validate(cmd)?;
    Ok(symbol)
}
fn decimal(v: f64) -> String {
    v.to_string().replace('.', ",")
}

struct Geometry {
    entities: Vec<(Common, EntityKind)>,
    labels: Vec<String>,
    font: cadcraft_fonts::TextFont,
    width_factor: f64,
}
impl Geometry {
    fn new(s: &Session) -> Result<Self> {
        let style = s.doc()?.text_style("GOST").ok_or_else(|| bad("gost.symbol", "GOST text style missing"))?;
        number("gost.symbol", "GOST text width factor", style.width_factor, 0.01, 100.0)?;
        Ok(Self { entities: Vec::new(), labels: Vec::new(), font: cadcraft_render::style_font(style), width_factor: style.width_factor })
    }
    fn push(&mut self, layer: &str, kind: EntityKind) {
        self.entities.push((Common { layer: layer.into(), ..Common::default() }, kind));
    }
    fn line(&mut self, a: Vec2, b: Vec2, thick: bool) {
        self.push(if thick { "GOST-OUTLINE" } else { "GOST-THIN" }, EntityKind::Line(Line { a: a.to3(0.0), b: b.to3(0.0) }));
    }
    fn circle(&mut self, center: Vec2, radius: f64, thick: bool) {
        self.push(if thick { "GOST-OUTLINE" } else { "GOST-THIN" }, EntityKind::Circle(Circle { center: center.to3(0.0), radius }));
    }
    fn text(&mut self, at: Vec2, value: String, h: f64) -> f64 {
        let width = cadcraft_fonts::text_width(&self.font, &value, h, self.width_factor);
        self.labels.push(value.clone());
        self.push(
            "GOST-TEXT",
            EntityKind::Text(Text {
                insert: at.to3(0.0),
                align_pt: None,
                height: h,
                value,
                rotation: 0.0,
                width_factor: self.width_factor,
                oblique: 0.0,
                style: "GOST".into(),
                halign: HAlign::Left,
                valign: VAlign::Baseline,
            }),
        );
        width
    }
    fn arrow(&mut self, tip: Vec2, towards: Vec2, size: f64) {
        let d = towards - tip;
        let u = d / d.len();
        let n = Vec2::new(-u.y, u.x);
        let a = tip + u * size + n * (size * 0.18);
        let b = tip + u * size - n * (size * 0.18);
        self.push("GOST-THIN", EntityKind::Solid(Solid { corners: [tip.to3(0.0), a.to3(0.0), b.to3(0.0), b.to3(0.0)] }));
    }
    fn roughness(&mut self, q: &Roughness) {
        let h = q.text_height * q.scale;
        let elbow = Vec2::new(h, h * 1.7320508075688772);
        self.line(Vec2::new(-h * 0.5, h * 0.8660254037844386), Vec2::ZERO, false);
        self.line(Vec2::ZERO, elbow, false);
        match q.removal {
            Removal::Permitted => {}
            Removal::Required => self.line(Vec2::new(-h * 0.5, h * 0.8660254037844386), Vec2::new(h * 0.5, h * 0.8660254037844386), false),
            Removal::Prohibited => self.circle(Vec2::new(0.0, h * 0.65), h * 0.325, false),
        }
        let value = format!(
            "{} {}",
            match q.parameter {
                Parameter::Ra => "Ra",
                Parameter::Rz => "Rz",
            },
            decimal(q.value)
        );
        let width = self.text(elbow + Vec2::new(h * 0.3, h * 0.35), value, h);
        self.line(elbow, elbow + Vec2::new(width + h * 0.8, 0.0), false);
        if q.all_surfaces {
            self.circle(elbow, h * 0.25, false);
        }
        if let Some(lay) = q.lay {
            let o = elbow + Vec2::new(h, -h * 0.8);
            let r = h * 0.25;
            match lay {
                Lay::Parallel => {
                    for dy in [-r * 0.5, r * 0.5] {
                        self.line(o + Vec2::new(-r, dy), o + Vec2::new(r, dy), false);
                    }
                }
                Lay::Perpendicular => {
                    self.line(o + Vec2::new(-r, -r), o + Vec2::new(r, -r), false);
                    self.line(o + Vec2::new(0.0, -r), o + Vec2::new(0.0, r), false);
                }
                Lay::Crossed => {
                    self.line(o + Vec2::new(-r, -r), o + Vec2::new(r, r), false);
                    self.line(o + Vec2::new(-r, r), o + Vec2::new(r, -r), false);
                }
                Lay::Multidirectional => {
                    self.line(o + Vec2::new(-r, -r), o + Vec2::new(-r, r), false);
                    self.line(o + Vec2::new(-r, r), o, false);
                    self.line(o, o + Vec2::new(r, r), false);
                    self.line(o + Vec2::new(r, r), o + Vec2::new(r, -r), false);
                }
                Lay::Circular => self.push(
                    "GOST-THIN",
                    EntityKind::Arc(Arc {
                        center: o.to3(0.0),
                        radius: r,
                        start: std::f64::consts::FRAC_PI_4,
                        end: 7.0 * std::f64::consts::FRAC_PI_4,
                    }),
                ),
                Lay::Radial => {
                    self.line(o + Vec2::new(-r, -r), o + Vec2::new(-r, r), false);
                    self.line(o + Vec2::new(-r, r), o + Vec2::new(0.0, r), false);
                    self.push(
                        "GOST-THIN",
                        EntityKind::Arc(Arc {
                            center: (o + Vec2::new(0.0, r * 0.5)).to3(0.0),
                            radius: r * 0.5,
                            start: -std::f64::consts::FRAC_PI_2,
                            end: std::f64::consts::FRAC_PI_2,
                        }),
                    );
                    self.line(o, o + Vec2::new(-r, 0.0), false);
                    self.line(o + Vec2::new(-r, 0.0), o + Vec2::new(r * 0.5, -r), false);
                }
            }
        }
        if let Some(a) = q.direction {
            let start = elbow + Vec2::new(width + 2.0 * h, -h);
            let end = start + Vec2::from_angle(a.to_radians()) * (1.5 * h);
            self.line(start, end, false);
            self.arrow(end, start, h * 0.5);
        }
    }
    fn weld(&mut self, q: &Weld) {
        let h = q.text_height * q.scale;
        let tip = Vec2::new(q.leader[0], q.leader[1]) * q.scale;
        self.line(tip, Vec2::ZERO, false);
        self.arrow(tip, Vec2::ZERO, h);
        let y = if q.side == WeldSide::Arrow { h * 0.35 } else { -h * 1.35 };
        let x = h * 0.8;
        let mut label = decimal(q.size);
        if let Some(l) = q.length {
            label.push('-');
            label.push_str(&decimal(l));
        }
        if let Some(p) = q.pitch {
            label.push('/');
            label.push_str(&decimal(p));
        }
        let width = self.text(Vec2::new(x + h * 1.4, y), label, h);
        let base = Vec2::new(x, if q.side == WeldSide::Arrow { 0.0 } else { -h });
        match q.weld_type {
            WeldType::Fillet => {
                self.line(base, base + Vec2::new(0.0, h), false);
                self.line(base + Vec2::new(0.0, h), base + Vec2::new(h, 0.0), false);
                self.line(base + Vec2::new(h, 0.0), base, false);
            }
            WeldType::Butt => {
                for dx in [0.0, h * 0.6] {
                    self.line(base + Vec2::new(dx, 0.0), base + Vec2::new(dx, h), false);
                }
            }
        }
        self.line(Vec2::ZERO, Vec2::new(x + 2.2 * h + width, 0.0), false);
        if q.all_around {
            self.circle(Vec2::ZERO, h * 0.3, false);
        }
        if q.field {
            let top = Vec2::new(0.0, 2.5 * h);
            self.line(Vec2::ZERO, top, false);
            self.line(top, top + Vec2::new(h, -h * 0.4), false);
            self.line(top + Vec2::new(h, -h * 0.4), top + Vec2::new(0.0, -h * 0.8), false);
        }
    }
    fn thread(&mut self, q: &Thread) {
        let h = q.text_height * q.scale;
        let major = q.nominal * 0.5;
        let minor = (q.nominal - if q.side == ThreadSide::External { 1.226869 } else { 1.082532 } * q.pitch) * 0.5;
        let outer_thick = q.side == ThreadSide::External;
        if q.view != ThreadView::End {
            for sign in [-1.0, 1.0] {
                self.line(Vec2::new(0.0, sign * major), Vec2::new(q.length, sign * major), outer_thick);
                self.line(Vec2::new(0.0, sign * minor), Vec2::new(q.length, sign * minor), !outer_thick);
            }
            self.line(Vec2::new(q.length, -major), Vec2::new(q.length, major), true);
            self.line(Vec2::new(0.0, -major), Vec2::new(0.0, major), true);
            self.axis(Vec2::new(-2.0 * q.scale, 0.0), q.length + 4.0 * q.scale, q.scale);
        }
        if q.view != ThreadView::Longitudinal {
            let center = if q.view == ThreadView::End { Vec2::ZERO } else { Vec2::new(-q.nominal - 5.0 * q.scale, 0.0) };
            let (thick, thin) = if outer_thick { (major, minor) } else { (minor, major) };
            self.circle(center, thick, true);
            self.push(
                "GOST-THIN",
                EntityKind::Arc(Arc { center: center.to3(0.0), radius: thin, start: 0.25, end: 0.25 + 1.5 * std::f64::consts::PI }),
            );
            let r = major + 2.0 * q.scale;
            self.axis(center - Vec2::new(r, 0.0), r * 2.0, q.scale);
            self.axis_along(center - Vec2::new(0.0, r), Vec2::new(0.0, 1.0), r * 2.0, q.scale);
        }
        self.text(Vec2::new(0.0, major + h), format!("M{}×{}", decimal(q.nominal), decimal(q.pitch)), h);
        if q.view == ThreadView::End {
            self.text(Vec2::new(0.0, -major - 2.0 * h), format!("l={}", decimal(q.length)), h);
        }
    }
    fn axis(&mut self, start: Vec2, length: f64, scale: f64) {
        self.axis_along(start, Vec2::new(1.0, 0.0), length, scale);
    }
    fn axis_along(&mut self, start: Vec2, direction: Vec2, length: f64, scale: f64) {
        // Bounded, native dash-dot segments; no dependence on global LTSCALE.
        let period = (length / 64.0).max(18.0 * scale);
        for i in 0..64 {
            let x = f64::from(i) * period;
            if x >= length {
                break;
            }
            self.line(start + direction * x, start + direction * (x + period * 0.7).min(length), false);
            let dot = x + period * 0.85;
            if dot < length {
                self.line(start + direction * dot, start + direction * (dot + period * 0.04).min(length), false);
            }
        }
    }
    fn build(s: &Session, symbol: &Symbol) -> Result<Self> {
        let mut g = Self::new(s)?;
        match symbol {
            Symbol::Roughness(q) => g.roughness(q),
            Symbol::Weld(q) => g.weld(q),
            Symbol::Thread(q) => g.thread(q),
        }
        // A broken user font/style must fail before a drawing mutation, not emit NaN geometry.
        if g.entities.iter().any(|(_, k)| match k {
            EntityKind::Text(t) => !cadcraft_fonts::text_width(&g.font, &t.value, t.height, t.width_factor).is_finite(),
            _ => false,
        }) {
            return Err(bad("gost.symbol", "GOST font produces non-finite text metrics"));
        }
        Ok(g)
    }
    fn block(self, d: &mut Drawing, name: &str, kind: &str) -> Block {
        let mut block = Block::new(name);
        block.units = 4;
        block.description = format!("Editable GOST {kind}; original native 2D construction");
        for (common, kind) in self.entities {
            block.entities.push(Entity { handle: d.new_handle(), common, kind });
        }
        block
    }
}

fn block_name(d: &Drawing) -> Result<String> {
    // At most N occupied names among N+1 candidates, including case variants.
    (0..=d.blocks.len())
        .map(|i| format!("GOST-MARK-{i:X}"))
        .find(|name| d.block(name).is_none())
        .ok_or_else(|| bad("gost.symbol", "no available block name"))
}
fn roughness(s: &mut Session, p: &Value) -> Result<Value> {
    create(s, p, "roughness")
}
fn weld(s: &mut Session, p: &Value) -> Result<Value> {
    create(s, p, "weld")
}
fn thread(s: &mut Session, p: &Value) -> Result<Value> {
    create(s, p, "thread")
}
fn create(s: &mut Session, p: &Value, kind: &str) -> Result<Value> {
    bounded(p, "gost.symbol", 0)?;
    let cmd = format!("gost.{kind}");
    ready(s, &cmd)?;
    let at = position(p, &cmd)?;
    let rotation = angle(p, "rotation", &cmd)?;
    let mut object = p.as_object().cloned().ok_or_else(|| bad(&cmd, "parameters must be an object"))?;
    object.remove("at");
    object.remove("rotation");
    if object.contains_key("kind") {
        return Err(bad(&cmd, "kind is determined by the command"));
    }
    defaults(s, &mut object)?;
    object.insert("kind".into(), json!(kind));
    let symbol = decode(Value::Object(object), &cmd)?;
    let geometry = Geometry::build(s, &symbol)?;
    let labels = geometry.labels.clone();
    let name = block_name(s.doc()?)?;
    let mut common = Common { layer: "GOST-DIM".into(), ..Common::default() };
    super::data::save(&mut common, META, &symbol)?;
    let space = s.space();
    if s.doc()?.space(&space).is_none() {
        return Err(bad(&cmd, "current drawing space is missing"));
    }
    let d = s.doc_mut()?;
    let block = geometry.block(d, &name, symbol.kind());
    let h = d.add(&space, common, EntityKind::Insert(Insert { block: name.clone(), insert: at.to3(0.0), rotation, ..Insert::default() }))?;
    d.blocks.insert(name.clone(), Shared::new(block));
    Ok(
        json!({"handle":h.hex(),"block":name,"kind":symbol.kind(),"parameters":symbol.parameters()?,"labels":labels,"representation":"native-2d-symbol","limitations":LIMITS}),
    )
}

fn selected(s: &Session, p: &Value, cmd: &str) -> Result<Handle> {
    let hs = targets(s, p)?;
    if let [h] = hs.as_slice() { Ok(*h) } else { Err(bad(cmd, "provide one engineering symbol handle or select one Insert")) }
}
fn load(s: &Session, h: Handle, cmd: &str) -> Result<(Common, Insert, Symbol)> {
    let e = s.doc()?.entity(h).ok_or_else(|| bad(cmd, "no such entity"))?;
    let EntityKind::Insert(insert) = &e.kind else {
        return Err(bad(cmd, "entity is not a native engineering symbol Insert"));
    };
    let symbol = super::data::load::<Symbol>(&e.common, META)?.ok_or_else(|| bad(cmd, "Insert has no supported engineering symbol payload"))?;
    symbol.validate(cmd)?;
    if s.doc()?.block(&insert.block).is_none() {
        return Err(bad(cmd, "symbol block definition is missing"));
    }
    Ok((e.common.clone(), insert.clone(), symbol))
}
fn referenced_elsewhere(d: &Drawing, h: Handle, name: &str) -> bool {
    d.model
        .iter()
        .chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
        .any(|e| e.handle != h && matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(name)))
        || d.blocks.values().flat_map(|b| b.entities.iter()).any(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(name)))
}
fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    bounded(p, EDIT, 0)?;
    ready(s, EDIT)?;
    let object = p.as_object().ok_or_else(|| bad(EDIT, "parameters must be an object"))?;
    if object.keys().any(|k| !matches!(k.as_str(), "handle" | "handles" | "patch")) {
        return Err(bad(EDIT, "expected handle and patch only"));
    }
    let h = selected(s, p, EDIT)?;
    let (mut common, mut insert, old) = load(s, h, EDIT)?;
    if s.doc()?.layer(&common.layer).is_some_and(|l| l.locked) {
        return Err(bad(EDIT, "symbol layer is locked"));
    }
    let patch = p.get("patch").and_then(Value::as_object).ok_or_else(|| bad(EDIT, "patch must be a parameter object"))?;
    if patch.is_empty() {
        return Err(bad(EDIT, "patch must contain at least one parameter"));
    }
    if patch.contains_key("kind") {
        return Err(bad(EDIT, "symbol kind cannot change"));
    }
    let mut value = serde_json::to_value(&old).map_err(|e| bad(EDIT, e.to_string()))?;
    let fields = value.as_object_mut().ok_or_else(|| bad(EDIT, "invalid symbol payload"))?;
    for (k, v) in patch {
        fields.insert(k.clone(), v.clone());
    }
    let symbol = decode(value, EDIT)?;
    let geometry = Geometry::build(s, &symbol)?;
    let labels = geometry.labels.clone();
    super::data::save(&mut common, META, &symbol)?;
    let can_reuse = insert.block.starts_with("GOST-MARK-") && !referenced_elsewhere(s.doc()?, h, &insert.block);
    let name = if can_reuse { insert.block.clone() } else { block_name(s.doc()?)? };
    insert.block = name.clone();
    let d = s.doc_mut()?;
    let block = geometry.block(d, &name, symbol.kind());
    // The Insert itself is edited, never erased/reinserted: all its affine and array
    // settings, attributes and foreign xdata survive. Shared copies keep their block.
    d.modify_entity(h, |e| {
        e.common = common;
        e.kind = EntityKind::Insert(insert);
    })?;
    d.blocks.insert(name.clone(), Shared::new(block));
    Ok(
        json!({"handle":h.hex(),"block":name,"kind":symbol.kind(),"parameters":symbol.parameters()?,"labels":labels,"representation":"native-2d-symbol","limitations":LIMITS}),
    )
}
fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    bounded(p, INSPECT, 0)?;
    let h = selected(s, p, INSPECT)?;
    let (_, insert, symbol) = load(s, h, INSPECT)?;
    let block = s.doc()?.block(&insert.block).ok_or_else(|| bad(INSPECT, "symbol block missing"))?;
    let labels: Vec<&str> =
        block.entities.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.as_str()) } else { None }).collect();
    Ok(
        json!({"handle":h.hex(),"block":insert.block,"kind":symbol.kind(),"parameters":symbol.parameters()?,"transform":{"at":[insert.insert.x,insert.insert.y,insert.insert.z],"rotation":insert.rotation.to_degrees(),"scale":[insert.scale.x,insert.scale.y,insert.scale.z],"rows":insert.rows,"cols":insert.cols},"geometryCount":block.entities.len(),"labels":labels,"representation":"native-2d-symbol","limitations":LIMITS}),
    )
}

fn tolerance_value(v: &Value, name: &str) -> Result<cadcraft_doc::GdtTol> {
    let t = match v {
        Value::String(text) if text.len() <= 64 && !text.chars().any(char::is_control) => crate::cmd::tolerance::parse_tol(text),
        Value::Number(n) => crate::cmd::tolerance::parse_tol(&n.to_string()),
        Value::Object(o) => {
            if o.keys().any(|k| !matches!(k.as_str(), "value" | "diameter" | "material")) {
                return Err(bad(TOL, format!("unknown {name} field")));
            }
            let value = o
                .get("value")
                .filter(|v| v.is_string() || v.is_number())
                .ok_or_else(|| bad(TOL, format!("{name}.value must be a scalar number or numeric string")))?;
            let mut t = tolerance_value(value, name)?;
            if let Some(d) = o.get("diameter") {
                t.diameter = d.as_bool().ok_or_else(|| bad(TOL, "diameter must be boolean"))?;
            }
            if let Some(m) = o.get("material") {
                t.material = Some(m.as_str().ok_or_else(|| bad(TOL, "material must be M"))?.to_string());
            }
            t
        }
        _ => return Err(bad(TOL, format!("{name} must contain a positive numeric tolerance"))),
    };
    let n = t.value.replace(',', ".").parse::<f64>().map_err(|_| bad(TOL, format!("{name} value must be numeric")))?;
    number(TOL, name, n, 1e-9, 1e6)?;
    if t.material.as_deref().is_some_and(|m| m != "M") {
        return Err(bad(TOL, "this GOST wrapper supports only maximum-material M; use native TOLERANCE for other conventions"));
    }
    Ok(t)
}
fn base(text: &str) -> bool {
    !text.is_empty() && text.chars().count() <= 3 && text.chars().all(|c| c.is_ascii_uppercase() || ('А'..='Я').contains(&c))
}
fn normalize_line(p: &Value) -> Result<Value> {
    let o = p.as_object().ok_or_else(|| bad(TOL, "tolerance line must be an object"))?;
    if o.keys().any(|k| !matches!(k.as_str(), "symbol" | "tol1" | "tol2" | "datums")) {
        return Err(bad(TOL, "unknown tolerance line field"));
    }
    let symbol = o.get("symbol").and_then(Value::as_str).ok_or_else(|| bad(TOL, "symbol is required"))?;
    if symbol.len() > 64 {
        return Err(bad(TOL, "characteristic name is too long"));
    }
    let characteristic = GdtSymbol::parse(symbol).filter(|s| s.is_characteristic()).ok_or_else(|| bad(TOL, "unknown geometric characteristic"))?;
    let t1 = tolerance_value(o.get("tol1").ok_or_else(|| bad(TOL, "tol1 is required"))?, "tol1")?;
    let form = matches!(characteristic, GdtSymbol::Straightness | GdtSymbol::Flatness | GdtSymbol::Circularity | GdtSymbol::Cylindricity);
    if t1.diameter
        && !matches!(
            characteristic,
            GdtSymbol::Position | GdtSymbol::Straightness | GdtSymbol::Parallelism | GdtSymbol::Perpendicularity | GdtSymbol::Angularity
        )
    {
        return Err(bad(TOL, "diameter zone is not supported for this characteristic"));
    }
    if t1.material.is_some()
        && matches!(
            characteristic,
            GdtSymbol::Flatness | GdtSymbol::Circularity | GdtSymbol::Cylindricity | GdtSymbol::CircularRunout | GdtSymbol::TotalRunout
        )
    {
        return Err(bad(TOL, "material condition is not appropriate for this characteristic"));
    }
    let mut out = json!({"symbol":symbol,"tol1":{"value":t1.value.replace('.',","),"diameter":t1.diameter,"material":t1.material}});
    // Native TOLERANCE interprets a null material as absent; preserve that contract.
    if let Some(v) = o.get("tol2") {
        let t = tolerance_value(v, "tol2")?;
        if t.diameter != t1.diameter || t.material != t1.material {
            return Err(bad(TOL, "the two tolerance values must use the same zone and material conventions"));
        }
        out["tol2"] = json!({"value":t.value.replace('.',","),"diameter":t.diameter,"material":t.material});
    }
    if let Some(ds) = o.get("datums") {
        let ds = ds.as_array().filter(|ds| ds.len() <= 3).ok_or_else(|| bad(TOL, "datums must be an array of up to three bases"))?;
        let mut seen = Vec::new();
        let mut normalized = Vec::new();
        for d in ds {
            let datum = match d {
                Value::String(v) if v.len() <= 32 => crate::cmd::tolerance::parse_datum(v),
                Value::Object(o) => {
                    if o.keys().any(|k| !matches!(k.as_str(), "letter" | "material")) {
                        return Err(bad(TOL, "unknown datum field"));
                    }
                    cadcraft_doc::GdtDatum {
                        letter: o.get("letter").and_then(Value::as_str).ok_or_else(|| bad(TOL, "datum letter is required"))?.into(),
                        material: o
                            .get("material")
                            .map(|v| v.as_str().map(str::to_owned).ok_or_else(|| bad(TOL, "datum material must be M")))
                            .transpose()?,
                    }
                }
                _ => return Err(bad(TOL, "invalid datum")),
            };
            if !base(&datum.letter) || datum.material.as_deref().is_some_and(|m| m != "M") || seen.contains(&datum.letter) {
                return Err(bad(TOL, "bases must be distinct uppercase Latin/Russian letters with optional M"));
            }
            seen.push(datum.letter.clone());
            normalized.push(json!({"letter":datum.letter,"material":datum.material}));
        }
        if !ds.is_empty() && form {
            return Err(bad(TOL, "form tolerances cannot reference datum bases"));
        }
        out["datums"] = json!(normalized);
    }
    Ok(out)
}
fn tolerance(s: &mut Session, p: &Value) -> Result<Value> {
    bounded(p, TOL, 0)?;
    ready(s, TOL)?;
    let o = p.as_object().ok_or_else(|| bad(TOL, "parameters must be an object"))?;
    if o.keys()
        .any(|k| !matches!(k.as_str(), "at" | "symbol" | "tol1" | "tol2" | "datums" | "line2" | "projected" | "height" | "datumId" | "rotation"))
    {
        return Err(bad(TOL, "unsupported parameter; raw text and style overrides bypass GOST validation"));
    }
    let at = position(p, TOL)?;
    angle(p, "rotation", TOL)?;
    let mut first = serde_json::Map::new();
    for key in ["symbol", "tol1", "tol2", "datums"] {
        if let Some(v) = p.get(key) {
            first.insert(key.into(), v.clone());
        }
    }
    let mut native = normalize_line(&Value::Object(first))?;
    native["at"] = json!([at.x, at.y]);
    if let Some(v) = p.get("line2") {
        native["line2"] = normalize_line(v)?;
    }
    let projected = match p.get("projected") {
        None => false,
        Some(v) => v.as_bool().ok_or_else(|| bad(TOL, "projected must be boolean"))?,
    };
    if let Some(v) = p.get("height") {
        let h = v.as_f64().ok_or_else(|| bad(TOL, "height must be positive mm"))?;
        number(TOL, "height", h, 0.001, 1e6)?;
        if !projected {
            return Err(bad(TOL, "height requires projected:true"));
        }
        native["height"] = json!(decimal(h));
    } else if projected {
        return Err(bad(TOL, "projected:true requires a positive height"));
    }
    native["projected"] = json!(projected);
    if let Some(v) = p.get("datumId") {
        let id = v.as_str().filter(|v| base(v)).ok_or_else(|| bad(TOL, "datumId must be uppercase base letters"))?;
        native["datumId"] = json!(id);
    }
    if let Some(v) = p.get("rotation") {
        native["rotation"] = v.clone();
    }
    let mut style = s.doc()?.dim_style("GOST").cloned().ok_or_else(|| bad(TOL, "GOST dimension style missing"))?;
    if !style.text_style.eq_ignore_ascii_case("GOST") || ![2.5, 3.5, 5.0, 7.0, 10.0, 14.0, 20.0].contains(&style.text_height) {
        return Err(bad(TOL, "GOST dimension style must use GOST text and a supported standard text height"));
    }
    number(TOL, "dimension scale", style.scale, 1e-6, 10000.0)?;
    number(TOL, "text gap", style.text_gap, 0.0, 100.0)?;
    let text_style = s.doc()?.text_style("GOST").ok_or_else(|| bad(TOL, "GOST text style missing"))?;
    number(TOL, "GOST text width factor", text_style.width_factor, 0.01, 100.0)?;
    if text_style.height != 0.0 {
        return Err(bad(TOL, "GOST text style must have automatic (zero) height for correct paper sizing"));
    }
    let mut added_style = None;
    if s.space().is_paper() && style.scale != 1.0 {
        style.name = "GOST-TOLERANCE-PAPER".into();
        style.scale = 1.0;
        if let Some(existing) = s.doc()?.dim_style(&style.name) {
            if existing != &style {
                return Err(bad(TOL, "GOST-TOLERANCE-PAPER style conflicts with the current GOST profile"));
            }
        } else {
            added_style = Some(style.clone());
        }
    }
    native["style"] = json!(style.name);
    let spec = find_command("tolerance").ok_or_else(|| bad(TOL, "native TOLERANCE command unavailable"))?;
    if let Some(st) = &added_style {
        s.doc_mut()?.dim_styles.push(st.clone());
    }
    let result = (spec.run)(s, &native);
    let mut result = match result {
        Ok(r) => r,
        Err(e) => {
            if let Some(st) = &added_style {
                s.doc_mut()?.dim_styles.retain(|v| v.name != st.name);
            }
            return Err(e);
        }
    };
    let h =
        result.get("handle").and_then(Value::as_str).and_then(Handle::parse_hex).ok_or_else(|| bad(TOL, "native TOLERANCE returned no handle"))?;
    s.doc_mut()?.modify_entity(h, |e| e.common.layer = "GOST-DIM".into())?;
    result["delegatedTo"] = json!("tolerance");
    result["style"] = json!(style.name);
    result["limitations"] = json!([
        "Native feature-control frame, not a geometric measurement or a certificate. Maximum-material M is supported; other material conventions use native TOLERANCE. Datum selection and zone suitability require engineering review."
    ]);
    Ok(result)
}

struct CreateM {
    cmd: &'static str,
    name: &'static str,
    at: Option<Vec2>,
}
impl CreateM {
    fn new(cmd: &'static str, name: &'static str) -> Self {
        Self { cmd, name, at: None }
    }
}
impl Interactive for CreateM {
    fn name(&self) -> &'static str {
        self.name
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.at.is_none() {
            Prompt::new("Specify engineering symbol origin", Accept::POINT)
        } else {
            Prompt::new("Enter symbol parameters as a JSON object (without at)", Accept::TEXT)
        }
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        match (self.at, input) {
            (None, Input::Point(at)) => self.at = Some(at),
            (Some(at), Input::Text(text)) => {
                if text.len() > 4096 {
                    return Err(bad(self.cmd, "parameter JSON exceeds 4096 bytes"));
                }
                let mut p: Value = serde_json::from_str(&text).map_err(|e| bad(self.cmd, e.to_string()))?;
                let o = p.as_object_mut().ok_or_else(|| bad(self.cmd, "parameters must be a JSON object"))?;
                o.insert("at".into(), json!([at.x, at.y]));
                let spec = find_command(self.cmd).ok_or_else(|| bad(self.cmd, "command unavailable"))?;
                (spec.run)(s, &p)?;
                return Ok(Step::Done);
            }
            _ => {}
        }
        Ok(Step::Continue)
    }
}
struct PatchM;
impl Interactive for PatchM {
    fn name(&self) -> &'static str {
        "GOST.SYMBOL.EDIT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Enter {handle,patch} JSON, or {patch} for one selected symbol", Accept::TEXT)
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        if let Input::Text(text) = input {
            if text.len() > 4096 {
                return Err(bad(EDIT, "patch JSON exceeds 4096 bytes"));
            }
            let p = serde_json::from_str(&text).map_err(|e| bad(EDIT, e.to_string()))?;
            edit(s, &p)?;
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }
}

#[cfg(test)]
#[path = "marks_tests.rs"]
mod tests;
