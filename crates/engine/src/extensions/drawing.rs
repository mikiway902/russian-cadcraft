//! Persistent native polyhedral models, drawing views and dimension anchors. The kernel
//! performs real 3D clipping/occlusion; the DXF result is ordinary mesh/line/hatch/dimension data.
use super::data;
use crate::{
    Result, Session,
    cmd::{CommandSpec, bad, find_command},
};
use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Hatch, HatchLoop, Insert, Line, PolyfaceMesh, Space};
use cadcraft_geom::{
    PolyVertex, Vec2, Vec3,
    drafting3d::{Frame, Mesh, Plane},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};

pub(super) const EXTENSION: super::Extension = super::Extension {
    id: "drawing",
    version: 1,
    commands: specs,
    drawing_preset: None,
    document_changed: |session, event| after_command(session, event.before, event.uid, event.command),
    command_override: None,
    copy: Some(super::CopyHooks { tracks: owns_copy_links, apply: copied_entities }),
    clipboard: None,
};

fn owns_copy_links(drawing: &Drawing, handles: &[Handle]) -> bool {
    handles.iter().filter_map(|h| drawing.entity(*h)).any(|entity| match &entity.kind {
        EntityKind::Insert(insert) => insert.block.starts_with("GOST-PROJECTION-"),
        EntityKind::Dimension(_) => entity.common.xdata.iter().any(|tag| tag.code == 1001 && tag.value.eq_ignore_ascii_case("CADCRAFTGOST")),
        _ => false,
    })
}
const BODY: &str = "solid-mesh";
const VIEW: &str = "projection-view";
const DIM: &str = "projection-dimension";
const STALE_VIEW: &str = "ВИД НЕ АКТУАЛЕН";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    ids: Vec<u32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cut {
    normal: Vec3,
    offset: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct View {
    sources: Vec<Handle>,
    direction: Vec3,
    up: Vec3,
    scale: f64,
    hidden: bool,
    cut: Option<Cut>,
    label: Option<String>,
    issues: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor {
    source: Handle,
    #[serde(default)]
    vertex: Option<u32>,
    #[serde(default)]
    edge: Option<[u32; 2]>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Dimension {
    view: Handle,
    a: Anchor,
    b: Anchor,
    kind: String,
    offset: f64,
    #[serde(default)]
    unresolved: bool,
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
    CommandSpec::new("gost.solid","GOST Polyhedral Solid",solid).menu(&["Format","GOST","GOST Polyhedral Solid"]).params("{handle?: edit existing mesh, box?:{min:[x,y,z],max:[x,y,z]} | profile?:[[x,y]],z0?,z1? | vertices?:[[x,y,z]],faces?:[[1-based indices]]; ids?:[stable positive vertex IDs]}; closed planar convex faces (triangulate nonconvex faces), native DXF polyface, no curved-surface approximation"),
    CommandSpec::new("gost.projection","GOST Associative View",view).menu(&["Format","GOST","GOST Associative View"]).params("{handle?: edit view, sources?:[solid handles],layout?:paper tab,at?:[x,y],direction?:front|rear|top|bottom|left|right|isometric|[x,y,z],up?:[x,y,z],scale?:model units per paper mm,hidden?:bool,label?:selected-alphabet capital with optional decimal index|null}; actual multi-body hidden-line removal"),
    CommandSpec::new("gost.section.view","GOST Associative Section",section).menu(&["Format","GOST","GOST Associative Section"]).params("projection parameters plus plane:{normal:[x,y,z],offset:number}; keeps normal·point<=offset, view direction follows normal, true closed intersection loops and 3mm hatching"),
    CommandSpec::new("gost.projection.inspect","Inspect Associative View",inspect).params("{handle}; persistent sources, stable vertex IDs, view parameters, unresolved links").noundo(),
    CommandSpec::new("gost.projection.dimension","GOST View Dimension",dimension).menu(&["Format","GOST","GOST View Dimension"]).params("{handle?:edit existing dimension,view?:hex,a?:{source:hex,vertex:stable ID}|{source:hex,edge:[ID,ID]},b?:same,kind?:horizontal|vertical|aligned,offset?:paper mm}; new dimensions require view/a/b; aligned dimensions measure true source 3D length, not foreshortened isometric length"),
]
}
fn err(s: impl Into<String>) -> crate::EngineError {
    bad("gost.projection", s)
}
fn keys(p: &Value, allowed: &[&str]) -> Result<()> {
    let o = p.as_object().ok_or_else(|| err("parameters must be an object"))?;
    if o.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(err("unknown parameter"));
    }
    Ok(())
}
fn handle(p: &Value, key: &str) -> Result<Handle> {
    p.get(key).and_then(Value::as_str).and_then(Handle::parse_hex).ok_or_else(|| err(format!("{key}: hexadecimal handle required")))
}
fn optional_handle(p: &Value) -> Result<Option<Handle>> {
    p.get("handle").map(|_| handle(p, "handle")).transpose()
}
fn point3(v: &Value) -> Result<Vec3> {
    let a = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| err("expected [x,y,z]"))?;
    let n = |i| {
        a.get(i).and_then(Value::as_f64).filter(|n| n.is_finite() && n.abs() <= 1e9).ok_or_else(|| err("coordinates must be finite within +/-1e9"))
    };
    Ok(Vec3::new(n(0)?, n(1)?, n(2)?))
}
fn point2(v: &Value) -> Result<Vec2> {
    let a = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| err("expected [x,y]"))?;
    let n = |i| {
        a.get(i).and_then(Value::as_f64).filter(|n| n.is_finite() && n.abs() <= 1e9).ok_or_else(|| err("coordinates must be finite within +/-1e9"))
    };
    Ok(Vec2::new(n(0)?, n(1)?))
}
fn numeric(p: &Value, key: &str, default: f64, min: f64, max: f64) -> Result<f64> {
    match p.get(key) {
        None => Ok(default),
        Some(v) => {
            v.as_f64().filter(|n| n.is_finite() && *n >= min && *n <= max).ok_or_else(|| err(format!("{key}: finite value {min}..{max} required")))
        }
    }
}
fn flag(p: &Value, key: &str, default: bool) -> Result<bool> {
    p.get(key).map_or(Ok(default), |v| v.as_bool().ok_or_else(|| err(format!("{key}: boolean required"))))
}
fn atomic(s: &mut Session, f: impl FnOnce(&mut Session) -> Result<Value>) -> Result<Value> {
    let before = s.state()?.doc.clone();
    let result = f(s);
    if result.is_err() {
        s.state_mut()?.doc = before;
    }
    result
}
fn body(d: &Drawing, h: Handle) -> Result<(Mesh<'_>, Body)> {
    let e = d.entity(h).ok_or_else(|| err(format!("source {} is missing", h.hex())))?;
    let EntityKind::PolyfaceMesh(m) = &e.kind else { return Err(err("source must be a native polyface mesh")) };
    if m.vertices.len() > 512 || m.faces.len() > 1024 {
        return Err(err("mesh exceeds 512 vertices/1024 faces"));
    }
    let mut faces = Vec::with_capacity(m.faces.len());
    for f in &m.faces {
        let mut face = Vec::with_capacity(4);
        for i in f.iter().take_while(|i| **i != 0) {
            let index = usize::try_from(i.unsigned_abs()).ok().and_then(|i| i.checked_sub(1)).ok_or_else(|| err("invalid mesh face index"))?;
            face.push(index);
        }
        faces.push(face);
    }
    let mesh = Mesh { vertices: std::borrow::Cow::Borrowed(&m.vertices), faces };
    let record: Body = data::load(&e.common, BODY)?.unwrap_or_else(|| Body { ids: (1..=u32::try_from(m.vertices.len()).unwrap_or(0)).collect() });
    if record.ids.len() != mesh.vertices.len() || record.ids.contains(&0) || record.ids.iter().collect::<BTreeSet<_>>().len() != record.ids.len() {
        return Err(err("invalid stable vertex identities"));
    }
    Ok((mesh, record))
}
fn solid(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["handle", "box", "profile", "z0", "z1", "vertices", "faces", "ids"])?;
    let h = optional_handle(p)?;
    if h.is_some() && (p.get("vertices").is_some() || p.get("profile").is_some()) && p.get("ids").is_none() {
        return Err(err("explicit mesh/profile edits require stable vertex IDs"));
    }
    let modes = usize::from(p.get("box").is_some()) + usize::from(p.get("profile").is_some()) + usize::from(p.get("vertices").is_some());
    if modes != 1 {
        return Err(err("provide exactly one box, extrusion profile or explicit mesh"));
    }
    let mesh = if let Some(q) = p.get("box") {
        keys(q, &["min", "max"])?;
        Mesh::cuboid(point3(q.get("min").ok_or_else(|| err("box min required"))?)?, point3(q.get("max").ok_or_else(|| err("box max required"))?)?)
            .map_err(err)?
    } else if let Some(q) = p.get("profile") {
        let a = q.as_array().filter(|a| (3..=64).contains(&a.len())).ok_or_else(|| err("profile needs 3..64 points"))?;
        let points = a.iter().map(point2).collect::<Result<Vec<_>>>()?;
        Mesh::extrude(&points, numeric(p, "z0", 0.0, -1e9, 1e9)?, numeric(p, "z1", 10.0, -1e9, 1e9)?).map_err(err)?
    } else {
        let a = p["vertices"].as_array().filter(|a| a.len() <= 512).ok_or_else(|| err("vertices must contain at most 512 points"))?;
        let vertices = a.iter().map(point3).collect::<Result<Vec<_>>>()?;
        let f = p.get("faces").and_then(Value::as_array).filter(|a| a.len() <= 1024).ok_or_else(|| err("faces must contain at most 1024 faces"))?;
        let mut faces = Vec::with_capacity(f.len());
        for v in f {
            let a = v
                .as_array()
                .filter(|a| (3..=4).contains(&a.len()))
                .ok_or_else(|| err("native polyface faces have 3 or 4 vertices; triangulate longer polygons"))?;
            let row = a
                .iter()
                .map(|v| {
                    v.as_u64()
                        .and_then(|n| usize::try_from(n).ok())
                        .and_then(|n| n.checked_sub(1))
                        .ok_or_else(|| err("face indices are positive, 1-based integers"))
                })
                .collect::<Result<Vec<_>>>()?;
            faces.push(row);
        }
        let mesh = Mesh { vertices: vertices.into(), faces };
        mesh.validate().map_err(err)?;
        mesh
    };
    // Native POLYLINE faces support at most four corners. Triangulate longer convex cap faces.
    let mut native_faces = Vec::new();
    for face in &mesh.faces {
        let encode = |index: usize| i32::try_from(index + 1).map_err(|_| err("mesh index overflow"));
        if face.len() <= 4 {
            let mut f = [0; 4];
            for (slot, index) in f.iter_mut().zip(face) {
                *slot = encode(*index)?;
            }
            native_faces.push(f);
        } else {
            let first = *face.first().ok_or_else(|| err("empty face"))?;
            for (n, pair) in face.get(1..).unwrap_or_default().windows(2).enumerate() {
                if let [b, c] = pair {
                    // Negative indices hide the outgoing artificial fan edge in native POLYFACE.
                    let a_index = encode(first)?;
                    let c_index = encode(*c)?;
                    native_faces.push([
                        if n == 0 { a_index } else { -a_index },
                        encode(*b)?,
                        if n + 3 == face.len() { c_index } else { -c_index },
                        0,
                    ]);
                }
            }
        }
    }
    let old = h.map(|h| body(s.doc()?, h).map(|(_, r)| r)).transpose()?;
    let ids = if let Some(v) = p.get("ids") {
        let a = v.as_array().filter(|a| a.len() == mesh.vertices.len()).ok_or_else(|| err("ids must match the vertex count"))?;
        a.iter()
            .map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0).ok_or_else(|| err("vertex IDs must be positive u32 values")))
            .collect::<Result<Vec<_>>>()?
    } else if let Some(r) = old {
        if r.ids.len() != mesh.vertices.len() {
            return Err(err("topology changes require explicit stable vertex IDs"));
        }
        r.ids
    } else {
        (1..=u32::try_from(mesh.vertices.len()).map_err(|_| err("vertex count overflow"))?).collect()
    };
    if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
        return Err(err("duplicate vertex ID"));
    }
    let mut common = if let Some(h) = h {
        s.doc()?.entity(h).ok_or_else(|| err("body missing"))?.common.clone()
    } else {
        Common { layer: "GOST-OUTLINE".into(), ..Default::default() }
    };
    data::save(&mut common, BODY, &Body { ids })?;
    atomic(s, |s| {
        let native = EntityKind::PolyfaceMesh(PolyfaceMesh { vertices: mesh.vertices.into_owned(), faces: native_faces });
        let h = if let Some(h) = h {
            s.doc_mut()?.modify_entity(h, |e| {
                e.kind = native;
                e.common = common;
            })?;
            h
        } else {
            s.doc_mut()?.add(&Space::Model, common, native)?
        };
        Ok(json!({"handle":h.hex(),"representation":"closed-polyhedral-mesh","curvedSurfaces":false}))
    })
}
fn orientation(p: &Value, old: Option<&View>) -> Result<(Vec3, Vec3)> {
    let mut up = if p.get("direction").is_some() { Vec3::Z } else { old.map_or(Vec3::Z, |v| v.up) };
    let direction = match p.get("direction") {
        None => old.map_or(Vec3::new(0.0, -1.0, 0.0), |v| v.direction),
        Some(Value::String(name)) => match name.as_str() {
            "front" => Vec3::new(0.0, -1.0, 0.0),
            "rear" => Vec3::new(0.0, 1.0, 0.0),
            "left" => Vec3::new(-1.0, 0.0, 0.0),
            "right" => Vec3::new(1.0, 0.0, 0.0),
            "top" => {
                up = Vec3::new(0.0, 1.0, 0.0);
                Vec3::Z
            }
            "bottom" => {
                up = Vec3::new(0.0, -1.0, 0.0);
                -Vec3::Z
            }
            "isometric" => Vec3::new(1.0, -1.0, 1.0),
            _ => return Err(err("unsupported view direction")),
        },
        Some(v) => point3(v)?,
    };
    if let Some(v) = p.get("up") {
        up = point3(v)?;
    }
    Ok((direction, up))
}
fn view_insert(d: &Drawing, h: Handle) -> Result<&Insert> {
    let e = d.entity(h).ok_or_else(|| err("view missing"))?;
    let EntityKind::Insert(i) = &e.kind else { return Err(err("view must remain a block reference")) };
    let paper = d
        .space_of(h)
        .and_then(|space| space.layout().and_then(|name| d.layout(name)))
        .is_some_and(|l| cadcraft_render::paper::page_unit_mm(d, &l.page) == 1.0);
    if !paper
        || !i.insert.is_finite()
        || i.insert.x.abs() > 1e9
        || i.insert.y.abs() > 1e9
        || !i.rotation.is_finite()
        || i.rotation.abs() > 1e-9
        || !i.scale.is_finite()
        || (i.scale.x - 1.0).abs() > 1e-9
        || (i.scale.y - 1.0).abs() > 1e-9
        || i.rows != 1
        || i.cols != 1
    {
        return Err(err(
            "views require a millimetre paper layout and an unscaled, unrotated single INSERT; change drawing scale/direction through gost.projection properties so lettering and hatching keep their paper sizes",
        ));
    }
    Ok(i)
}
fn load_view(d: &Drawing, h: Handle) -> Result<View> {
    let e = d.entity(h).ok_or_else(|| err("view missing"))?;
    let r: View = data::load(&e.common, VIEW)?.ok_or_else(|| err("not an associative view"))?;
    validate_view(&r)?;
    view_insert(d, h)?;
    Ok(r)
}
fn validate_view(r: &View) -> Result<()> {
    if r.sources.is_empty() || r.sources.len() > 16 || !r.scale.is_finite() || r.scale < 1e-6 || r.scale > 1e6 {
        return Err(err("view needs 1..16 bodies and scale 0.000001..1000000"));
    }
    if r.sources.iter().collect::<BTreeSet<_>>().len() != r.sources.len() {
        return Err(err("duplicate sources"));
    }
    Frame::new(r.direction, r.up).map_err(err)?;
    if let Some(c) = &r.cut {
        Plane::new(c.normal, c.offset).map_err(err)?;
    }
    Ok(())
}
fn prepared(d: &Drawing, r: &View) -> Result<Vec<(Common, EntityKind)>> {
    validate_view(r)?;
    let frame = Frame::new(r.direction, r.up).map_err(err)?;
    let meshes = r.sources.iter().map(|h| body(d, *h).map(|v| v.0)).collect::<Result<Vec<_>>>()?;
    let cut = r.cut.as_ref().map(|c| Plane::new(c.normal, c.offset).map_err(err)).transpose()?;
    let p = cadcraft_geom::drafting3d::project(&meshes, frame, cut).map_err(err)?;
    if cut.is_some() && p.sections.iter().all(Vec::is_empty) {
        return Err(err("cutting plane must cross a body interior"));
    }
    let mut kinds = Vec::new();
    for (body, paths) in p.sections.iter().enumerate() {
        if paths.is_empty() {
            continue;
        }
        let loops = paths
            .iter()
            .map(|path| HatchLoop {
                edges: Vec::new(),
                vertices: path.iter().map(|p| PolyVertex::new(*p * (1.0 / r.scale))).collect(),
                outer: true,
                sources: Vec::new(),
            })
            .collect();
        kinds.push((
            Common { layer: "GOST-HATCH".into(), ..Default::default() },
            EntityKind::Hatch(Hatch {
                pattern: "GOST-METAL".into(),
                solid: false,
                loops,
                scale: 1.0,
                angle: if body % 2 == 0 { std::f64::consts::FRAC_PI_4 } else { -std::f64::consts::FRAC_PI_4 },
                associative: false,
                style: 0,
                elevation: 0.0,
                gradient: None,
                origin: Vec2::ZERO,
                background: None,
                pattern_lines: vec![cadcraft_doc::library::PatternLine { angle: 0.0, origin: (0.0, 0.0), delta: (0.0, 3.0), dashes: Vec::new() }],
            }),
        ));
    }
    for stroke in p.strokes {
        if stroke.hidden && !r.hidden {
            continue;
        }
        let common = Common {
            layer: if stroke.hidden { "GOST-HIDDEN" } else { "GOST-OUTLINE" }.into(),
            linetype: if stroke.hidden { "GOST-HIDDEN" } else { "Continuous" }.into(),
            ..Default::default()
        };
        kinds.push((common, EntityKind::Line(Line { a: (stroke.a * (1.0 / r.scale)).to3(0.0), b: (stroke.b * (1.0 / r.scale)).to3(0.0) })));
    }
    if let Some(label) = &r.label {
        let height = super::gost::standards::designation_height(d)?;
        let name = super::gost::annotations::view_caption(label, cut.is_some(), r.scale);
        let mut bounds: Option<cadcraft_geom::Bounds2> = None;
        let mut include = |p| {
            let point = cadcraft_geom::Bounds2::from_points([p]);
            bounds = Some(bounds.map_or(point, |b| b.union(&point)));
        };
        for (_, kind) in &kinds {
            match kind {
                EntityKind::Line(l) => {
                    include(l.a.xy());
                    include(l.b.xy());
                }
                EntityKind::Hatch(h) => {
                    for vertex in h.loops.iter().flat_map(|l| &l.vertices) {
                        include(vertex.p);
                    }
                }
                _ => {}
            }
        }
        let at = bounds.map_or(Vec2::new(0.0, 8.0), |b| Vec2::new((b.min.x + b.max.x) * 0.5, b.max.y + 8.0));
        kinds.push((Common { layer: "GOST-TEXT".into(), ..Default::default() }, super::gost::annotations::designation_text(at, name, height)));
    }
    Ok(kinds)
}
fn install(d: &mut Drawing, h: Handle, r: &View, kinds: Vec<(Common, EntityKind)>) -> Result<()> {
    let name = format!("GOST-PROJECTION-{}", h.hex());
    let mut block = Block::new(&name);
    block.units = 4;
    block.description = "CADCraftGOST associative polyhedral drawing view".into();
    for (common, kind) in kinds {
        block.entities.push(Entity { handle: d.new_handle(), common, kind });
    }
    let mut common = d.entity(h).ok_or_else(|| err("view missing"))?.common.clone();
    data::save(&mut common, VIEW, r)?;
    d.blocks.insert(name.clone(), Arc::new(block));
    d.modify_entity(h, |e| {
        e.common = common;
        if let EntityKind::Insert(i) = &mut e.kind {
            i.block = name;
        }
    })?;
    Ok(())
}
fn view(s: &mut Session, p: &Value) -> Result<Value> {
    view_inner(s, p, false)
}
fn section(s: &mut Session, p: &Value) -> Result<Value> {
    view_inner(s, p, true)
}
fn view_inner(s: &mut Session, p: &Value, is_section: bool) -> Result<Value> {
    keys(p, &["handle", "sources", "layout", "at", "direction", "up", "scale", "hidden", "label", "plane"])?;
    if s.doc()?.text_style("GOST").is_none() {
        return Err(err("GOST drawing profile required"));
    }
    let h = optional_handle(p)?;
    let old = h.map(|h| load_view(s.doc()?, h)).transpose()?;
    let sources = if let Some(v) = p.get("sources") {
        v.as_array()
            .filter(|a| !a.is_empty() && a.len() <= 16)
            .ok_or_else(|| err("1..16 sources required"))?
            .iter()
            .map(|v| v.as_str().and_then(Handle::parse_hex).ok_or_else(|| err("invalid source handle")))
            .collect::<Result<Vec<_>>>()?
    } else {
        old.as_ref().map(|r| r.sources.clone()).ok_or_else(|| err("sources required"))?
    };
    let (mut direction, mut up) = orientation(p, old.as_ref())?;
    let cut = if let Some(c) = p.get("plane") {
        keys(c, &["normal", "offset"])?;
        let normal = point3(c.get("normal").ok_or_else(|| err("plane normal required"))?)?;
        let offset = c.get("offset").and_then(Value::as_f64).ok_or_else(|| err("plane offset required"))?;
        let plane = Plane::new(normal, offset).map_err(err)?;
        direction = plane.normal;
        if p.get("up").is_none() && direction.cross(up).len() < 1e-6 {
            up = Vec3::new(0.0, 1.0, 0.0);
        }
        Some(Cut { normal: plane.normal, offset: plane.offset })
    } else {
        old.as_ref().and_then(|r| r.cut.clone())
    };
    if is_section && cut.is_none() {
        return Err(err("section plane required"));
    }
    let label = match p.get("label") {
        Some(Value::Null) => None,
        Some(Value::String(label)) => {
            super::gost::standards::validate_letter(s.doc()?, label, "gost.projection")?;
            Some(label.clone())
        }
        None => match old.as_ref() {
            Some(previous) => previous.label.clone(),
            None if is_section => Some(super::gost::standards::next_letter(s.doc()?, "gost.projection")?),
            None => None,
        },
        _ => return Err(err("label must be a selected-alphabet capital with an optional decimal index, or null")),
    };
    let r = View {
        sources,
        direction,
        up,
        scale: numeric(p, "scale", old.as_ref().map_or(1.0, |r| r.scale), 1e-6, 1e6)?,
        hidden: flag(p, "hidden", old.as_ref().is_none_or(|r| r.hidden))?,
        cut,
        label,
        issues: Vec::new(),
    };
    let layout = if let Some(h) = h {
        let current = s.doc()?.space_of(h).and_then(|s| s.layout().map(str::to_string)).ok_or_else(|| err("view must live on a paper sheet"))?;
        if p.get("layout").is_some_and(|v| v.as_str() != Some(current.as_str())) {
            return Err(err("layout is creation-only; moving an associative view to another sheet is not supported"));
        }
        current
    } else {
        p.get("layout").and_then(Value::as_str).or(s.space().layout()).unwrap_or("GOST1").to_string()
    };
    if s.doc()?.layout(&layout).is_none() {
        return Err(err("target paper layout missing"));
    }
    let at = p.get("at").map(point2).transpose()?;
    let kinds = prepared(s.doc()?, &r)?;
    atomic(s, |s| {
        for source in &r.sources {
            let e = s.doc()?.entity(*source).ok_or_else(|| err("source missing"))?;
            if data::load::<Body>(&e.common, BODY)?.is_none() {
                let (_, record) = body(s.doc()?, *source)?;
                let mut common = e.common.clone();
                data::save(&mut common, BODY, &record)?;
                s.doc_mut()?.modify_entity(*source, |e| e.common = common)?;
            }
        }
        let h = if let Some(h) = h {
            if let Some(at) = at {
                s.doc_mut()?.modify_entity(h, |e| {
                    if let EntityKind::Insert(i) = &mut e.kind {
                        i.insert = at.to3(0.0)
                    }
                })?;
            }
            h
        } else {
            s.doc_mut()?.add(
                &Space::Paper(layout.clone()),
                Common { layer: "GOST-OUTLINE".into(), ..Default::default() },
                EntityKind::Insert(Insert { insert: at.unwrap_or(Vec2::new(50.0, 100.0)).to3(0.0), ..Default::default() }),
            )?
        };
        install(s.doc_mut()?, h, &r, kinds)?;
        Ok(json!({"handle":h.hex(),"layout":layout,"parameters":r,"representation":"associative-polyhedral-view"}))
    })
}
fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["handle"])?;
    let h = handle(p, "handle")?;
    let e = s.doc()?.entity(h).ok_or_else(|| err("object missing"))?;
    if matches!(e.kind, EntityKind::PolyfaceMesh(_)) {
        let (mesh, r) = body(s.doc()?, h)?;
        mesh.validate().map_err(err)?;
        Ok(json!({"handle":h.hex(),"vertices":mesh.vertices,"faces":mesh.faces,"ids":r.ids,"representation":"closed-polyhedral-mesh"}))
    } else if let Some(r) = data::load::<Dimension>(&e.common, DIM)? {
        let issue = points(s.doc()?, &r).err().map(|e| e.to_string());
        Ok(json!({"handle":h.hex(),"references":r,"issue":issue}))
    } else {
        let r = load_view(s.doc()?, h)?;
        let transform = if let EntityKind::Insert(i) = &e.kind {
            json!({"at":[i.insert.x,i.insert.y],"rotation":i.rotation,"scale":i.scale})
        } else {
            Value::Null
        };
        Ok(
            json!({"handle":h.hex(),"parameters":r,"issues":r.issues,"transform":transform,"layout":s.doc()?.space_of(h).map(|s|s.tab_name().to_string())}),
        )
    }
}
fn anchor(mesh: &Mesh<'_>, ids: &Body, v: &View, i: &Insert, a: &Anchor) -> Result<(Vec2, Vec3)> {
    if !v.sources.contains(&a.source) {
        return Err(err("dimension source is not in this view"));
    }
    let vertex = |id: u32| {
        ids.ids
            .iter()
            .position(|v| *v == id)
            .and_then(|index| mesh.vertices.get(index).copied())
            .ok_or_else(|| err(format!("stable vertex {id} is unresolved")))
    };
    let p = match (a.vertex, a.edge) {
        (Some(id), None) => {
            let p = vertex(id)?;
            if v.cut.as_ref().is_some_and(|c| c.normal.dot(p) - c.offset > 1e-8) {
                return Err(err("anchor vertex was removed by the section plane"));
            }
            p
        }
        (None, Some([a, b])) => {
            let c = v.cut.as_ref().ok_or_else(|| err("edge intersection anchors require a section view"))?;
            let p = Plane::new(c.normal, c.offset).map_err(err)?;
            let ai = ids.ids.iter().position(|id| *id == a).ok_or_else(|| err("edge vertex is unresolved"))?;
            let bi = ids.ids.iter().position(|id| *id == b).ok_or_else(|| err("edge vertex is unresolved"))?;
            let mut normals = [None, None];
            let mut count = 0;
            for face in &mesh.faces {
                if face.iter().zip(face.iter().cycle().skip(1)).take(face.len()).any(|(&a, &b)| (a == ai && b == bi) || (a == bi && b == ai)) {
                    let vertices: Option<[Vec3; 3]> =
                        (|| Some([*mesh.vertices.get(*face.first()?)?, *mesh.vertices.get(*face.get(1)?)?, *mesh.vertices.get(*face.get(2)?)?]))();
                    if let Some([a, b, c]) = vertices {
                        if let Some(n) = normals.get_mut(count) {
                            *n = Some((b - a).cross(c - a).normalized());
                        }
                        count += 1;
                    }
                }
            }
            if count != 2 || matches!(normals,[Some(a),Some(b)] if (a-b).len()<1e-8) {
                return Err(err("cut anchor must reference a real noncoplanar mesh edge, not a diagonal"));
            }
            let a = vertex(a)?;
            let b = vertex(b)?;
            let da = p.normal.dot(a) - p.offset;
            let db = p.normal.dot(b) - p.offset;
            if da * db > 0.0 || (da - db).abs() < 1e-9 {
                return Err(err("edge does not cross section plane"));
            }
            a + (b - a) * (da / (da - db))
        }
        _ => return Err(err("anchor needs exactly one vertex or cutting-edge identity")),
    };
    let projected = Frame::new(v.direction, v.up).map_err(err)?.point(p) * (1.0 / v.scale);
    Ok((i.insert.xy() + Vec2::new(projected.x * i.scale.x, projected.y * i.scale.y).rotate(i.rotation), p))
}
fn points(d: &Drawing, r: &Dimension) -> Result<(Vec2, Vec2, f64, Space)> {
    let v = load_view(d, r.view)?;
    if !v.issues.is_empty() {
        return Err(err("view sources are unresolved"));
    }
    let i = view_insert(d, r.view)?;
    let (mesh, ids) = body(d, r.a.source)?;
    mesh.validate().map_err(err)?;
    let (a, source_a) = anchor(&mesh, &ids, &v, i, &r.a)?;
    let (b, source_b) = if r.a.source == r.b.source {
        anchor(&mesh, &ids, &v, i, &r.b)?
    } else {
        let (mesh, ids) = body(d, r.b.source)?;
        mesh.validate().map_err(err)?;
        anchor(&mesh, &ids, &v, i, &r.b)?
    };
    let projected = a.dist(b);
    if !projected.is_finite() || projected < 1e-8 {
        return Err(err("dimension anchors coincide in this projection"));
    }
    let factor = if r.kind == "aligned" { (source_b - source_a).len() / projected } else { v.scale / i.scale.x };
    Ok((a, b, factor, d.space_of(r.view).ok_or_else(|| err("view space missing"))?))
}
pub(super) fn dimension_factor(d: &Drawing, e: &Entity) -> Option<f64> {
    let r: Dimension = data::load(&e.common, DIM).ok().flatten()?;
    points(d, &r).ok().map(|(_, _, factor, _)| factor)
}
pub(super) fn link_findings(d: &Drawing) -> Vec<Value> {
    let mut findings = Vec::new();
    let mut count = 0;
    for e in d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())) {
        let issue = match &e.kind {
            EntityKind::Insert(i) if i.block.starts_with("GOST-PROJECTION-") => match load_view(d, e.handle) {
                Ok(r) => {
                    if r.sources.iter().any(|h| d.entity(*h).is_none()) {
                        Some("View source is missing; displayed geometry is not current".to_string())
                    } else {
                        None
                    }
                }
                Err(e) => Some(e.to_string()),
            },
            EntityKind::Dimension(_) => match data::load::<Dimension>(&e.common, DIM) {
                Ok(Some(r)) => points(d, &r)
                    .and_then(|p| if d.space_of(e.handle) == Some(p.3) { Ok(()) } else { Err(err("dimension and view are on different sheets")) })
                    .err()
                    .map(|e| e.to_string()),
                Ok(None) => continue,
                Err(e) => Some(e.to_string()),
            },
            _ => continue,
        };
        count += 1;
        if count > 1152 {
            findings.push(json!({"severity":"warning","code":"projection.checkLimit","message":"Projection-link check reached its object limit","actual":count,"expected":1152}));
            break;
        }
        if let Some(issue) = issue {
            findings.push(json!({"severity":"error","code":"projection.unresolved","message":issue,"location":e.handle.hex(),"handles":[e.handle.hex()],"space":d.space_of(e.handle).map(|s|s.tab_name().to_string()),"actual":"unresolved","expected":"live stable source geometry"}));
        }
    }
    findings
}
fn dim_geometry(d: &mut Drawing, h: Handle, r: &Dimension) -> Result<()> {
    let prepared = points(d, r).and_then(|points| {
        if d.space_of(h) == Some(points.3.clone()) { Ok(points) } else { Err(err("dimension and view must share the same paper layout")) }
    });
    let unresolved = prepared.is_err();
    if d.entity(h).is_some_and(|e| matches!(&e.kind,EntityKind::Dimension(dm) if dm.text.len()>4096)) {
        return Err(err("view dimension override text exceeds 4096 bytes"));
    }
    match prepared {
        Ok((a, b, factor, _)) => {
            let (kind, at) = match r.kind.as_str() {
                "horizontal" => (
                    cadcraft_doc::DimKind::Linear { rotation: 0.0 },
                    Vec2::new((a.x + b.x) * 0.5, if r.offset >= 0.0 { a.y.max(b.y) + r.offset } else { a.y.min(b.y) + r.offset }),
                ),
                "vertical" => (
                    cadcraft_doc::DimKind::Linear { rotation: std::f64::consts::FRAC_PI_2 },
                    Vec2::new(if r.offset >= 0.0 { a.x.max(b.x) + r.offset } else { a.x.min(b.x) + r.offset }, (a.y + b.y) * 0.5),
                ),
                "aligned" => {
                    let delta = b - a;
                    let n = Vec2::new(-delta.y, delta.x).normalized();
                    (cadcraft_doc::DimKind::Aligned, (a + b) * 0.5 + n * r.offset)
                }
                _ => return Err(err("dimension kind must be horizontal, vertical or aligned")),
            };
            d.modify_entity(h, |e| {
                if let EntityKind::Dimension(dm) = &mut e.kind {
                    dm.kind = kind;
                    dm.p13 = a.to3(0.0);
                    dm.p14 = b.to3(0.0);
                    dm.defpt = at.to3(0.0);
                    dm.block = None;
                    if r.unresolved {
                        if dm.text == "?" {
                            dm.text.clear();
                        } else if dm.text.starts_with("? ") {
                            dm.text.drain(..2);
                        }
                    }
                    dm.assoc.clear();
                    dm.overrides.insert("scale".into(), json!(1.0));
                    dm.overrides.insert("linearFactor".into(), json!(factor));
                }
            })?;
            let e = d.entity(h).ok_or_else(|| err("dimension missing"))?;
            if let EntityKind::Dimension(dm) = &e.kind {
                let st = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
                let g = cadcraft_render::dimension_geometry(dm, &st, 1.0);
                d.modify_entity(h, |e| {
                    if let EntityKind::Dimension(dm) = &mut e.kind {
                        dm.measurement = g.measurement;
                        if !dm.user_text_pos {
                            dm.text_mid = g.text_pos.to3(0.0);
                        }
                    }
                })?;
            }
        }
        Err(_) => {
            d.modify_entity(h, |e| {
                if let EntityKind::Dimension(dm) = &mut e.kind {
                    if !r.unresolved || !(dm.text == "?" || dm.text.starts_with("? ")) {
                        if dm.text.is_empty() {
                            dm.text.push('?');
                        } else {
                            dm.text.insert_str(0, "? ");
                        }
                    }
                    dm.block = None;
                }
            })?;
        }
    }
    if unresolved != r.unresolved {
        let mut record = r.clone();
        record.unresolved = unresolved;
        let mut common = d.entity(h).ok_or_else(|| err("dimension missing"))?.common.clone();
        data::save(&mut common, DIM, &record)?;
        d.modify_entity(h, |e| e.common = common)?;
    }
    Ok(())
}
fn dimension(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["handle", "view", "a", "b", "kind", "offset"])?;
    if s.doc()?.dim_style("GOST").is_none() || s.doc()?.text_style("GOST").is_none() {
        return Err(err("apply gost.setup before adding view dimensions"));
    }
    let h = optional_handle(p)?;
    let old = h
        .map(|h| -> Result<Dimension> {
            let e = s.doc()?.entity(h).ok_or_else(|| err("dimension missing"))?;
            if !matches!(e.kind, EntityKind::Dimension(_)) {
                return Err(err("not a dimension"));
            }
            data::load(&e.common, DIM)?.ok_or_else(|| err("not an associative view dimension"))
        })
        .transpose()?;
    let parse_anchor = |v: &Value| -> Result<Anchor> {
        keys(v, &["source", "vertex", "edge"])?;
        let id = |v: &Value| {
            v.as_u64().and_then(|v| u32::try_from(v).ok()).filter(|v| *v > 0).ok_or_else(|| err("stable vertex IDs must be positive u32 values"))
        };
        let vertex = v.get("vertex").filter(|v| !v.is_null()).map(id).transpose()?;
        let edge = v
            .get("edge")
            .filter(|v| !v.is_null())
            .map(|v| -> Result<[u32; 2]> {
                let a = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| err("edge requires two stable vertex IDs"))?;
                Ok([id(a.first().ok_or_else(|| err("edge vertex missing"))?)?, id(a.get(1).ok_or_else(|| err("edge vertex missing"))?)?])
            })
            .transpose()?;
        if vertex.is_some() == edge.is_some() {
            return Err(err("anchor requires exactly one vertex or edge"));
        }
        Ok(Anchor { source: handle(v, "source")?, vertex, edge })
    };
    let kind = match p.get("kind") {
        None => old.as_ref().map_or("horizontal", |r| r.kind.as_str()),
        Some(v) => v.as_str().ok_or_else(|| err("dimension kind must be a string"))?,
    };
    if !matches!(kind, "horizontal" | "vertical" | "aligned") {
        return Err(err("invalid dimension kind"));
    }
    let mut r = Dimension {
        view: if p.get("view").is_some() { handle(p, "view")? } else { old.as_ref().map(|r| r.view).ok_or_else(|| err("view required"))? },
        a: if let Some(a) = p.get("a") { parse_anchor(a)? } else { old.as_ref().map(|r| r.a).ok_or_else(|| err("anchor a required"))? },
        b: if let Some(b) = p.get("b") { parse_anchor(b)? } else { old.as_ref().map(|r| r.b).ok_or_else(|| err("anchor b required"))? },
        kind: kind.into(),
        offset: numeric(p, "offset", old.as_ref().map_or(10.0, |r| r.offset), -10000.0, 10000.0)?,
        unresolved: old.as_ref().is_some_and(|r| r.unresolved),
    };
    let (a, b, _, space) = points(s.doc()?, &r)?;
    if h.is_some_and(|h| s.doc().is_ok_and(|d| d.space_of(h) != Some(space.clone()))) {
        return Err(err("dimension and view must share a paper layout"));
    }
    if a.dist(b) < 1e-8 {
        return Err(err("dimension anchors coincide in this projection"));
    }
    atomic(s, |s| {
        let layout = find_command("layout.set").ok_or_else(|| err("layout command missing"))?.run;
        layout(s, &json!({"name":space.tab_name()}))?;
        let h = if let Some(h) = h {
            h
        } else {
            let run = find_command("dimlinear").ok_or_else(|| err("dimension command missing"))?.run;
            let out = run(s, &json!({"p1":[a.x,a.y],"p2":[b.x,b.y],"at":[a.x,b.y+10.0]}))?;
            out["handle"].as_str().and_then(Handle::parse_hex).ok_or_else(|| err("dimension returned no handle"))?
        };
        if old.is_none() {
            s.doc_mut()?.modify_entity(h, |e| {
                if let EntityKind::Dimension(dm) = &mut e.kind {
                    dm.style = "GOST".into();
                    dm.overrides.clear();
                }
            })?;
        }
        let mut common = s.doc()?.entity(h).ok_or_else(|| err("dimension missing"))?.common.clone();
        data::save(&mut common, DIM, &r)?;
        common.layer = "GOST-DIM".into();
        s.doc_mut()?.modify_entity(h, |e| e.common = common)?;
        dim_geometry(s.doc_mut()?, h, &r)?;
        r.unresolved = false;
        Ok(json!({"handle":h.hex(),"references":r,"associative":true}))
    })
}

fn stale_banner(d: &mut Drawing, h: Handle) -> Result<()> {
    let name = view_insert(d, h)?.block.clone();
    let b = d.block(&name).ok_or_else(|| err("cached view block missing"))?;
    if b.entities.len() > 16384 {
        return Err(err("cached view exceeds 16384 entities"));
    }
    let mut bounds: Option<cadcraft_geom::Bounds2> = None;
    let mut vertices = 0usize;
    for e in b.entities.iter() {
        match &e.kind {
            EntityKind::Text(t) if t.value == STALE_VIEW => continue,
            EntityKind::Text(t) if t.value.len() <= 1024 => {}
            EntityKind::Line(_) => {}
            EntityKind::Hatch(h) => {
                if h.loops.len() > 4096 {
                    return Err(err("cached section exceeds loop limit"));
                }
                for l in &h.loops {
                    vertices = vertices.saturating_add(l.point_count());
                }
                if vertices > 4096 {
                    return Err(err("cached section exceeds 4096 vertices"));
                }
            }
            _ => return Err(err("cached view contains unsupported edited geometry")),
        }
        let b = cadcraft_doc::entity_bounds(d, e, 0);
        if b.min.is_finite() && b.max.is_finite() {
            bounds = Some(bounds.map_or(b, |old| old.union(&b)));
        }
    }
    let at = bounds.map_or(Vec2::new(0.0, 8.0), |b| Vec2::new((b.min.x + b.max.x) * 0.5, b.max.y + 7.0)).to3(0.0);
    let marker = Entity {
        handle: d.new_handle(),
        common: Common { layer: "GOST-TEXT".into(), color: cadcraft_color::Color::Index(1), ..Default::default() },
        kind: EntityKind::Text(cadcraft_doc::Text {
            insert: at,
            align_pt: Some(at),
            height: 3.5,
            value: STALE_VIEW.into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "GOST".into(),
            halign: cadcraft_doc::HAlign::Center,
            valign: cadcraft_doc::VAlign::Bottom,
        }),
    };
    let b = Arc::make_mut(d.blocks.get_mut(&name).ok_or_else(|| err("cached view block missing"))?);
    let old: Vec<_> = b.entities.iter().filter(|e| matches!(&e.kind,EntityKind::Text(t) if t.value==STALE_VIEW)).map(|e| e.handle).collect();
    for h in old {
        b.entities.remove(h);
    }
    b.entities.push(marker);
    Ok(())
}

pub(super) fn copied_entities(d: &mut Drawing, pairs: &[(Handle, Handle)]) -> Result<()> {
    let mut map = std::collections::HashMap::with_capacity(pairs.len());
    for (old, new) in pairs {
        if map.insert(*old, *new).is_some() {
            return Err(err("copied associative identity is ambiguous"));
        }
    }
    for (_, h) in pairs {
        let e = d.entity(*h).ok_or_else(|| err("copied entity is missing"))?;
        let mut common = None;
        match &e.kind {
            EntityKind::Insert(i) if i.block.starts_with("GOST-PROJECTION-") => {
                if let Some(mut r) = data::load::<View>(&e.common, VIEW)? {
                    let mut changed = false;
                    for source in &mut r.sources {
                        if let Some(new) = map.get(source) {
                            *source = *new;
                            changed = true;
                        }
                    }
                    if changed {
                        let mut c = e.common.clone();
                        data::save(&mut c, VIEW, &r)?;
                        common = Some(c);
                    }
                }
            }
            EntityKind::Dimension(_) => {
                if let Some(mut r) = data::load::<Dimension>(&e.common, DIM)? {
                    let mut changed = false;
                    for source in [&mut r.view, &mut r.a.source, &mut r.b.source] {
                        if let Some(new) = map.get(source) {
                            *source = *new;
                            changed = true;
                        }
                    }
                    if changed {
                        let mut c = e.common.clone();
                        data::save(&mut c, DIM, &r)?;
                        common = Some(c);
                    }
                }
            }
            _ => {}
        }
        if let Some(common) = common {
            d.modify_entity(*h, |e| e.common = common)?;
        }
    }
    Ok(())
}

fn entity_changed(before: &Drawing, current: &Drawing, h: Handle) -> bool {
    match (before.entity(h), current.entity(h)) {
        (Some(old), Some(new)) => !Arc::ptr_eq(old, new),
        (None, None) => false,
        _ => true,
    }
}
pub(super) fn after_command(s: &mut Session, before: Option<&Arc<Drawing>>, uid: Option<u64>, command: &str) -> Result<()> {
    let Some(before) = before else { return Ok(()) };
    let state = s.state()?;
    if uid.is_some_and(|u| u != state.uid) || Arc::ptr_eq(before, &state.doc) {
        return Ok(());
    }
    let d = s.doc()?;
    let mut views = Vec::new();
    let mut dimensions = Vec::new();
    let mut inspected = 0usize;
    for e in d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())) {
        if let EntityKind::Insert(i) = &e.kind
            && i.block.starts_with("GOST-PROJECTION-")
        {
            inspected += 1;
            if inspected > 128 {
                return Err(err("at most 128 associative views"));
            }
            let r = load_view(d, e.handle)?;
            let changed = before.entity(e.handle).is_none_or(|old| data::load::<View>(&old.common, VIEW).ok().flatten().as_ref() != Some(&r))
                || r.sources.iter().any(|h| match (before.entity(*h), d.entity(*h)) {
                    (Some(old), Some(new)) => !Arc::ptr_eq(old, new) && old.kind != new.kind,
                    (None, None) => false,
                    _ => true,
                });
            // These handlers already installed current geometry in the original transaction.
            if changed && !command.eq_ignore_ascii_case("gost.projection") && !command.eq_ignore_ascii_case("gost.section.view") {
                views.push((e.handle, r));
            }
        }
        if matches!(e.kind, EntityKind::Dimension(_))
            && let Some(r) = data::load::<Dimension>(&e.common, DIM)?
        {
            if dimensions.len() >= 1024 {
                return Err(err("at most 1024 associative view dimensions"));
            }
            dimensions.push((e.handle, r));
        }
    }
    for (h, mut r) in views {
        let missing: Vec<_> = r
            .sources
            .iter()
            .filter(|h| s.doc().is_ok_and(|d| d.entity(**h).is_none()))
            .map(|h| format!("Source {} deleted; view is not current", h.hex()))
            .collect();
        if missing.is_empty() {
            r.issues.clear();
            let kinds = prepared(s.doc()?, &r)?;
            install(s.doc_mut()?, h, &r, kinds)?;
        } else {
            r.issues = missing;
            let mut common = s.doc()?.entity(h).ok_or_else(|| err("view missing"))?.common.clone();
            data::save(&mut common, VIEW, &r)?;
            s.doc_mut()?.modify_entity(h, |e| e.common = common)?;
            stale_banner(s.doc_mut()?, h)?;
            for issue in &r.issues {
                s.echo(issue.clone());
            }
        }
    }
    for (h, r) in dimensions {
        if entity_changed(before, s.doc()?, r.view)
            || entity_changed(before, s.doc()?, h)
            || entity_changed(before, s.doc()?, r.a.source)
            || (r.a.source != r.b.source && entity_changed(before, s.doc()?, r.b.source))
        {
            dim_geometry(s.doc_mut()?, h, &r)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod tests;
