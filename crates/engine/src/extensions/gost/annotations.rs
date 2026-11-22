//! Original ESKD annotation geometry, not a 3D projection/section generator.
//! ГОСТ 2.305-2008 §§5.2, 5.8, 6.5–6.6; ГОСТ 2.303-68; ГОСТ 2.306-68.

use cadcraft_doc::{Common, EntityKind, HAlign, Handle, Solid, Text, VAlign};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use crate::cmd::helpers::line;
use crate::cmd::machines::SelectRun;
use crate::cmd::{CommandSpec, bad, find_command, point_req, size_param, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("gost.center", "GOST Center Axes", center)
            .menu(&["Format", "GOST", "GOST Center Axes"])
            .params("{handles?: circle/arc handles (else selection), scale?: model units per paper mm, extension?: paper mm (3..5; omitted: automatic 3..4.5 to fit the dash pattern)}; native associative axes follow source geometry and survive DXF")
            .interactive(|_| Ok(Box::new(SelectRun::new("gost.center", "GOST.CENTER")))),
        CommandSpec::new("gost.section", "GOST Cutting Plane", section)
            .menu(&["Format", "GOST", "GOST Cutting Plane"])
            .params("{p1:[x,y], p2:[x,y]: outer stroke ends, label?: selected-alphabet capital with optional decimal index (next unused designation when omitted), reverse?: bool, scale?: model units per paper mm (paper space: 1), caption?:[x,y]} → static cutting-plane marks, not model section geometry")
            .interactive(|_| Ok(Box::new(SectionM::default()))),
        CommandSpec::new("gost.view", "GOST View Designation", view)
            .menu(&["Format", "GOST", "GOST View Designation"])
            .params("{at:[x,y], label?: selected-alphabet capital with optional decimal index (next unused designation when omitted), arrowFrom?:[x,y], arrowTo?:[x,y] (both for source direction), scale?: model units per paper mm (paper space: 1), viewScale?: standard scale string, section?: bool}; designation only, no view geometry")
            .interactive(|_| Ok(Box::new(ViewM::default()))),
        CommandSpec::new("gost.hatch", "GOST Metal Hatch", hatch)
            .menu(&["Format", "GOST", "GOST Metal Hatch"])
            .params("{handles?: closed boundaries (else selection), points?: internal picks, spacing?: paper mm (1..10, default 3), angle?: 45|-45|30|-30|60|-60 degrees, scale?: model units per paper mm}; associative metal/general-material hatch")
            .interactive(|_| Ok(Box::new(SelectRun::new("gost.hatch", "GOST.HATCH")))),
    ]
}

fn ready(s: &Session, cmd: &str, layer: &str) -> Result<()> {
    let d = s.doc()?;
    let paper_mm = s.space().layout().and_then(|n| d.layout(n)).is_some_and(|l| cadcraft_render::paper::page_unit_mm(d, &l.page) == 1.0);
    if (!paper_mm && d.header.i64("INSUNITS", 0) != 4) || d.layer(layer).is_none() || d.text_style("GOST").is_none() {
        return Err(bad(cmd, "apply gost.setup to a millimetre drawing first"));
    }
    if d.layer(layer).is_some_and(|l| l.locked || !l.visible() || !l.plot) {
        return Err(bad(cmd, format!("layer {layer} must be unlocked, visible and printable")));
    }
    Ok(())
}

fn annotation_scale(s: &Session, p: &Value, cmd: &str) -> Result<f64> {
    if p.get("scale").is_some() {
        let scale = size_param(cmd, p, "scale", false)?.filter(|v| *v <= 10_000.0).ok_or_else(|| bad(cmd, "scale must be in (0,10000]"))?;
        if s.space().is_paper() && scale != 1.0 {
            return Err(bad(cmd, "paper-space annotations use paper millimetres: scale must be 1; use viewScale for the represented model scale"));
        }
        return Ok(scale);
    }
    if s.space().is_paper() {
        Ok(1.0)
    } else {
        let v = s.doc()?.dim_style("GOST").map_or(1.0, |d| d.scale);
        if v.is_finite() && v > 0.0 && v <= 10_000.0 { Ok(v) } else { Err(bad(cmd, "invalid GOST dimension scale; reapply gost.setup")) }
    }
}

fn point(s: &Value, key: &str, cmd: &str) -> Result<Vec2> {
    let v = point_req(cmd, s, key)?;
    if !v.is_finite() || v.x.abs() > 1e9 || v.y.abs() > 1e9 {
        return Err(bad(cmd, format!("{key} must have finite coordinates within ±1e9")));
    }
    Ok(v)
}

fn letter<'a>(s: &Session, p: &'a Value, generated: &'a mut String, cmd: &str) -> Result<&'a str> {
    let t = match p.get("label") {
        None => {
            *generated = super::standards::next_letter(s.doc()?, cmd)?;
            return Ok(generated.as_str());
        }
        Some(Value::String(t)) => t.as_str(),
        _ => return Err(bad(cmd, "label must be a selected-alphabet capital with an optional decimal index")),
    };
    super::standards::validate_letter(s.doc()?, t, cmd)?;
    Ok(t)
}

fn flag(p: &Value, key: &str, cmd: &str) -> Result<bool> {
    match p.get(key) {
        None => Ok(false),
        Some(Value::Bool(v)) => Ok(*v),
        _ => Err(bad(cmd, format!("{key} must be boolean"))),
    }
}

fn add(s: &mut Session, kind: EntityKind, layer: &str, scale: f64) -> Result<Handle> {
    let space = s.space();
    let common = Common { layer: layer.into(), ltscale: scale, ..Common::default() };
    Ok(s.doc_mut()?.add(&space, common, kind)?)
}

/// Native MTEXT lowers numeric indices; unindexed captions retain native TEXT.
pub(in crate::extensions) fn designation_text(at: Vec2, value: String, height: f64) -> EntityKind {
    if value.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) {
        let mut contents = String::with_capacity(value.len() + 32);
        let mut chars = value.chars().peekable();
        while let Some(c) = chars.next() {
            contents.push(c);
            if c.is_uppercase() && chars.peek().is_some_and(char::is_ascii_digit) {
                contents.push_str("{\\A1;\\S^");
                while let Some(digit) = chars.next_if(char::is_ascii_digit) {
                    contents.push(digit);
                }
                contents.push_str(";}");
            }
        }
        return EntityKind::MText(cadcraft_doc::MText {
            insert: at.to3(0.0),
            height,
            width: 0.0,
            attach: 8,
            rotation: 0.0,
            style: "GOST".into(),
            contents,
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        });
    }
    EntityKind::Text(Text {
        insert: at.to3(0.0),
        align_pt: Some(at.to3(0.0)),
        height,
        value,
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "GOST".into(),
        halign: HAlign::Center,
        valign: VAlign::Bottom,
    })
}

pub(super) fn center(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "gost.center";
    ready(s, cmd, "GOST-CENTER")?;
    let scale = annotation_scale(s, p, cmd)?;
    let ext = size_param(cmd, p, "extension", false)?;
    if ext.is_some_and(|value| !(3.0..=5.0).contains(&value)) {
        return Err(bad(cmd, "extension must be 3..5 paper mm"));
    }
    let hs = targets(s, p)?;
    if hs.is_empty() || hs.len() > 256 {
        return Err(bad(cmd, "select 1..256 circles or arcs"));
    }
    // Validate the whole selection before adding anything.
    let mut circles = Vec::with_capacity(hs.len());
    for h in hs {
        let e = s.doc()?.space(&s.space()).and_then(|st| st.get(h)).ok_or_else(|| bad(cmd, "object is not in the current space"))?;
        let (c, r) = match &e.kind {
            EntityKind::Circle(c) => (c.center.xy(), c.radius),
            EntityKind::Arc(a) => (a.center.xy(), a.radius),
            _ => return Err(bad(cmd, "only circles and circular arcs are supported")),
        };
        if !c.is_finite() || !r.is_finite() || r <= 0.0 || r > 1e9 {
            return Err(bad(cmd, "invalid circle geometry"));
        }
        circles.push((h, c, r));
    }
    let work = circles.iter().try_fold(0.0, |work, (_, _, radius)| -> Result<f64> {
        let extension = match ext {
            Some(value) => value,
            None => super::axes::default_extension(*radius, scale)?,
        };
        Ok(work + (radius / scale + extension) * 2.0 + 4.0)
    })?;
    if work > 16_384.0 {
        return Err(bad(cmd, "axis segment limit exceeded; select fewer objects or use the intended drawing scale"));
    }
    if circles.iter().any(|(_, _, r)| 2.0 * r / scale < 12.0) {
        ready(s, cmd, "GOST-THIN")?;
    }
    super::axes::create(s, circles.into_iter().map(|(h, _, _)| h).collect(), scale, ext)
}

/// 15-degree included angle and a 5 mm head (ГОСТ 2.305 fig.14).
fn arrow(s: &mut Session, tip: Vec2, direction: Vec2, k: f64) -> Result<Vec<String>> {
    let base = tip - direction * (5.0 * k);
    let half_width = 5.0 * k * (7.5_f64.to_radians()).tan();
    let normal = Vec2::new(-direction.y, direction.x);
    let a = base + normal * half_width;
    let b = base - normal * half_width;
    let solid = EntityKind::Solid(Solid { corners: [tip.to3(0.0), a.to3(0.0), b.to3(0.0), b.to3(0.0)] });
    Ok(vec![add(s, line(tip - direction * (10.0 * k), tip), "GOST-THIN", k)?.hex(), add(s, solid, "GOST-THIN", k)?.hex()])
}

fn section(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "gost.section";
    ready(s, cmd, "GOST-SECTION")?;
    ready(s, cmd, "GOST-THIN")?;
    ready(s, cmd, "GOST-TEXT")?;
    let k = annotation_scale(s, p, cmd)?;
    let a = point(p, "p1", cmd)?;
    let b = point(p, "p2", cmd)?;
    if a.dist(b) < 25.0 * k {
        return Err(bad(cmd, "stroke ends must be at least 25 paper mm apart"));
    }
    let mut generated = String::new();
    let label = letter(s, p, &mut generated, cmd)?;
    let height = super::standards::designation_height(s.doc()?)?;
    let reverse = flag(p, "reverse", cmd)?;
    let caption = if p.get("caption").is_some() { Some(point(p, "caption", cmd)?) } else { None };
    let dir = (b - a).normalized();
    let mut normal = Vec2::new(-dir.y, dir.x);
    if reverse {
        normal = -normal;
    }
    // Two open 10 mm strokes, never a solid line through the object.
    let mut hs =
        vec![add(s, line(a, a + dir * (10.0 * k)), "GOST-SECTION", k)?.hex(), add(s, line(b - dir * (10.0 * k), b), "GOST-SECTION", k)?.hex()];
    let extent = height * if label.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) { 1.25 } else { 1.0 };
    for at in [a + dir * (2.5 * k), b - dir * (2.5 * k)] {
        hs.extend(arrow(s, at, normal, k)?);
        hs.push(add(s, designation_text(at - normal * ((13.0 + extent) * k), label.into(), height * k), "GOST-TEXT", k)?.hex());
    }
    let value = format!("{label}—{label}");
    if let Some(at) = caption {
        hs.push(add(s, designation_text(at, value.clone(), height * k), "GOST-TEXT", k)?.hex());
    }
    Ok(json!({"handles": hs, "designation": value, "associative": false,
        "warning": "Cutting-plane notation only. Place terminal strokes outside the object contour; model section geometry is not generated."}))
}

fn view(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "gost.view";
    ready(s, cmd, "GOST-TEXT")?;
    let k = annotation_scale(s, p, cmd)?;
    let at = point(p, "at", cmd)?;
    let mut generated = String::new();
    let label = letter(s, p, &mut generated, cmd)?;
    let height = super::standards::designation_height(s.doc()?)?;
    let mut value = if flag(p, "section", cmd)? { format!("{label}—{label}") } else { label.into() };
    if let Some(v) = p.get("viewScale") {
        let scale = v.as_str().ok_or_else(|| bad(cmd, "viewScale must be a standard scale string"))?;
        if ![
            "1:1", "1:2", "1:2.5", "1:4", "1:5", "1:10", "1:15", "1:20", "1:25", "1:40", "1:50", "1:75", "1:100", "1:200", "1:400", "1:500", "1:800",
            "1:1000", "2:1", "2.5:1", "4:1", "5:1", "10:1", "20:1", "40:1", "50:1", "100:1",
        ]
        .contains(&scale)
        {
            return Err(bad(cmd, "viewScale is not a supported ГОСТ 2.302-68 scale"));
        }
        value.push_str(" (");
        value.push_str(&scale.replace('.', ","));
        value.push(')');
    }
    let source = if p.get("arrowFrom").is_some() || p.get("arrowTo").is_some() {
        ready(s, cmd, "GOST-THIN")?;
        let from = point(p, "arrowFrom", cmd)?;
        let to = point(p, "arrowTo", cmd)?;
        if from.dist(to) < 10.0 * k {
            return Err(bad(cmd, "source direction arrow must be at least 10 paper mm long"));
        }
        Some((from, to))
    } else {
        None
    };
    let h = add(s, designation_text(at, value, height * k), "GOST-TEXT", k)?;
    let mut handles = vec![h.hex()];
    if let Some((from, to)) = source {
        let dir = (to - from).normalized();
        handles.extend(arrow(s, to, dir, k)?);
        if from.dist(to) > 10.0 * k {
            handles.push(add(s, line(from, to - dir * (10.0 * k)), "GOST-THIN", k)?.hex());
        }
        handles.push(add(s, designation_text(from - dir * ((height * 0.6 + 3.0) * k), label.into(), height * k), "GOST-TEXT", k)?.hex());
    }
    Ok(json!({"handle": h.hex(), "handles": handles, "directionArrow": source.is_some(),
        "warning": "Designation only; no view geometry is generated. If this view is outside direct projection, provide its source direction arrow. Main views in direct projection do not require a name."}))
}

fn hatch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "gost.hatch";
    ready(s, cmd, "GOST-HATCH")?;
    let scale = annotation_scale(s, p, cmd)?;
    let spacing = size_param(cmd, p, "spacing", false)?.unwrap_or(3.0);
    if !(1.0..=10.0).contains(&spacing) {
        return Err(bad(cmd, "spacing must be 1..10 paper mm"));
    }
    let angle = match p.get("angle") {
        None => 45.0,
        Some(v) => v.as_f64().filter(|v| [30.0, 45.0, 60.0].contains(&v.abs())).ok_or_else(|| bad(cmd, "angle must be ±30, ±45 or ±60 degrees"))?,
    };
    if p.get("handles").and_then(Value::as_array).is_some_and(|a| a.len() > 256)
        || p.get("points").and_then(Value::as_array).is_some_and(|a| a.len() > 256)
    {
        return Err(bad(cmd, "at most 256 boundary objects or internal picks"));
    }
    let mut params = p.as_object().cloned().ok_or_else(|| bad(cmd, "parameters must be an object"))?;
    params.insert("pattern".into(), json!("ANSI31"));
    params.insert("scale".into(), json!(1.0));
    params.insert("angle".into(), json!(0.0));
    params.insert("associative".into(), json!(true));
    // Reuse the native boundary tracer, islands and associativity, without nesting an undo step.
    let run = find_command("hatch").ok_or_else(|| bad(cmd, "native HATCH command unavailable"))?.run;
    let result = run(s, &Value::Object(params))?;
    let handles = result.get("handles").and_then(Value::as_array).ok_or_else(|| bad(cmd, "HATCH returned no boundary"))?;
    for raw in handles {
        let h = raw.as_str().and_then(Handle::parse_hex).ok_or_else(|| bad(cmd, "invalid hatch handle"))?;
        s.doc_mut()?.modify_entity(h, |e| {
            e.common = Common { layer: "GOST-HATCH".into(), ..Common::default() };
            if let EntityKind::Hatch(h) = &mut e.kind {
                h.pattern = "GOST-METAL".into();
                h.scale = scale;
                h.angle = angle.to_radians();
                h.pattern_lines =
                    vec![cadcraft_doc::library::PatternLine { angle: 0.0, origin: (0.0, 0.0), delta: (0.0, spacing), dashes: Vec::new() }];
            }
        })?;
    }
    Ok(json!({"handles": handles, "spacingPaperMm": spacing, "angle": angle,
        "manualChecks": ["Keep hatch direction and spacing consistent for the same part; distinguish adjacent parts.", "Use 30° or 60° when 45° hatching coincides with contours or axes; special materials and narrow sections require separate treatment."]}))
}

#[derive(Default)]
struct SectionM {
    first: Option<Vec2>,
    second: Option<Vec2>,
}
impl Interactive for SectionM {
    fn name(&self) -> &'static str {
        "GOST.SECTION"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.first, self.second) {
            (None, _) => Prompt::new("Specify first outer cutting-plane stroke end", Accept::POINT),
            (Some(p), None) => Prompt::new("Specify last outer cutting-plane stroke end", Accept::POINT).base_opt(Some(p)),
            _ => Prompt::new("Enter section designation, or press Enter for the next unused designation", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.first, self.second, i) {
            (None, _, Input::Point(p)) => self.first = Some(p),
            (Some(_), None, Input::Point(p)) => self.second = Some(p),
            (Some(a), Some(b), Input::Text(t)) => {
                let params = if t.trim().is_empty() {
                    json!({"p1":[a.x,a.y], "p2":[b.x,b.y]})
                } else {
                    json!({"p1":[a.x,a.y], "p2":[b.x,b.y], "label":t.trim()})
                };
                section(s, &params)?;
                return Ok(Step::Done);
            }
            (Some(a), Some(b), Input::Enter) => {
                section(s, &json!({"p1":[a.x,a.y], "p2":[b.x,b.y]}))?;
                return Ok(Step::Done);
            }
            _ => {}
        }
        Ok(Step::Continue)
    }
}

#[derive(Default)]
struct ViewM {
    points: Vec<Vec2>,
}
impl Interactive for ViewM {
    fn name(&self) -> &'static str {
        "GOST.VIEW"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.points.len() {
            0 => Prompt::new("Specify view direction arrow tail", Accept::POINT),
            1 => Prompt::new("Specify view direction arrow tip", Accept::POINT).base_opt(self.points.first().copied()),
            2 => Prompt::new("Specify view designation position", Accept::POINT),
            _ => Prompt::new("Enter view designation, or press Enter for the next unused designation", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.points.len() < 3 {
            if let Input::Point(p) = i {
                self.points.push(p);
            }
            return Ok(Step::Continue);
        }
        let label = match i {
            Input::Text(t) => {
                if t.trim().is_empty() {
                    None
                } else {
                    Some(t.trim().to_owned())
                }
            }
            Input::Enter => None,
            _ => return Ok(Step::Continue),
        };
        if let [from, to, at] = self.points.as_slice() {
            let mut params = json!({"arrowFrom":[from.x,from.y], "arrowTo":[to.x,to.y], "at":[at.x,at.y]});
            if let Some(label) = label {
                params["label"] = Value::String(label);
            }
            view(s, &params)?;
        }
        Ok(Step::Done)
    }
}

#[cfg(test)]
#[path = "annotations_tests.rs"]
mod tests;
