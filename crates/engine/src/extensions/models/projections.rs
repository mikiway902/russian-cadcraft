//! Frozen first-angle projection families. Events only move native INSERTs; they never project a source.
use super::{
    Common, Drawing, EntityKind, Handle, Insert, MAX_ENTITIES, MAX_VIEWS, Projection, Result, SavedView, Session, Space, Store, VIEW, Value, Vec2,
    Vec3, ViewLink, atomic, data, err, flag, hooks, install_view_options, json, keys, load, model, options, save, tag, text, vector,
};
use crate::cmd::CommandSpec;
use cadcraft_doc::Prim;
use cadcraft_geom::{Bounds2, Spline};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const MARKER: &str = "CADCRAFTGOST_PROJECTION_LINK";
const MAX_DEPTH: usize = 32;
const ALIGN_EPS: f64 = 1e-8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Link {
    version: u32,
    parent: Option<String>,
    original_parent: String,
    direction: String,
    status: String,
}
impl Link {
    fn vertical(&self) -> bool {
        matches!(self.direction.as_str(), "top" | "bottom")
    }
    fn validate(&self) -> Result<()> {
        if self.version != 1
            || handle(&self.original_parent).is_err()
            || !matches!(self.direction.as_str(), "top" | "bottom" | "left" | "right")
            || !matches!((self.status.as_str(), self.parent.as_deref()), ("linked", Some(_)) | ("missing_parent", None))
            || self.parent.as_deref().is_some_and(|h| handle(h).is_err())
        {
            return Err(err("invalid projection_link version, status, direction or handle"));
        }
        Ok(())
    }
}
fn link(v: &SavedView) -> Result<Option<Link>> {
    v.provenance
        .get("projection_link")
        .map(|v| {
            let link = Link::deserialize(v).map_err(|e| err(format!("invalid projection_link: {e}")))?;
            link.validate()?;
            Ok(link)
        })
        .transpose()
}
/// Clipboard only needs the active destination reference, never original_parent's audit value.
pub(super) fn projection_parent(view: &SavedView) -> Result<Option<Handle>> {
    link(view)?.and_then(|link| link.parent).map(|h| handle(&h)).transpose()
}
fn handle(s: &str) -> Result<Handle> {
    if s.is_empty() || s.len() > 16 || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(err("invalid projection view handle"));
    }
    Handle::parse_hex(s).filter(|h| h.0 != 0).ok_or_else(|| err("invalid projection view handle"))
}
fn same_source(a: &SavedView, b: &SavedView) -> bool {
    a.model_id == b.model_id && a.revision == b.revision && a.source == b.source && a.path == b.path
}
/// Structural validation also runs at Store load, including on drawings without live geometry.
/// A nonexistent active parent is resolved to missing_parent by the document event, never by a handle collision.
pub(super) fn validate_store_links(store: &Store) -> Result<()> {
    if store.views.len() > MAX_VIEWS {
        return Err(err("projection view count limit exceeded"));
    }
    let mut views = BTreeMap::new();
    for view in &store.views {
        if views.insert(handle(&view.handle)?, view).is_some() {
            return Err(err("duplicate projection view handles"));
        }
    }
    for view in &store.views {
        let mut seen = BTreeSet::new();
        let mut current = view;
        let mut depth = 0;
        while let Some(relation) = link(current)? {
            let Some(parent) = relation.parent else { break };
            if current.provenance.get("section").is_some() {
                return Err(err("a section cannot be an ordinary linked projection"));
            }
            if !seen.insert(handle(&current.handle)?) || depth >= MAX_DEPTH {
                return Err(err("cyclic projection graph or projection depth exceeds 32"));
            }
            depth += 1;
            let Some(parent) = views.get(&handle(&parent)?) else { break };
            if !same_source(current, parent) || parent.provenance.get("section").is_some() {
                return Err(err("projection parent belongs to a different source/revision or is a section"));
            }
            current = parent;
        }
    }
    Ok(())
}
fn marker(common: &Common) -> Result<bool> {
    let mut apps = common.xdata.iter().enumerate().filter(|(_, t)| t.code == 1001 && t.value == MARKER);
    let Some((start, _)) = apps.next() else { return Ok(false) };
    if apps.next().is_some() {
        return Err(err("duplicate projection relationship marker"));
    }
    let tags = &common.xdata[start + 1..];
    let end = tags.iter().position(|t| t.code == 1001).unwrap_or(tags.len());
    if end != 1 || tags[0].code != 1070 || tags[0].value != "1" {
        return Err(err("invalid projection relationship marker"));
    }
    Ok(true)
}
fn policy() -> crate::EngineError {
    err(
        "linked projection family refuses scale (including uniform scale), rotation, mirror, nonuniform transforms, layout moves and nested wrappers; recreate related views at the required scale",
    )
}
pub(super) fn placement<'a>(d: &'a Drawing, view: &SavedView) -> Result<(&'a Insert, Space, Vec2)> {
    let h = handle(&view.handle)?;
    let entity = d.entity(h).ok_or_else(|| err("projection view is not live at top level"))?;
    let saved: ViewLink = data::load(&entity.common, VIEW)?.ok_or_else(|| err("projection view association missing"))?;
    if handle(&saved.view)? != h || saved.model_id != view.model_id || saved.revision != view.revision {
        return Err(err("projection view identity differs from saved source"));
    }
    let EntityKind::Insert(i) = &entity.kind else { return Err(policy()) };
    if entity.common.extrusion != Vec3::Z
        || !i.insert.is_finite()
        || i.insert.z.abs() > ALIGN_EPS
        || !i.scale.is_finite()
        || i.scale.x <= 0.
        || i.scale.x < 1e-6
        || i.scale.x > 1e6
        || (i.scale.x - i.scale.y).abs() > i.scale.x * 1e-10
        || (i.scale.x - i.scale.z).abs() > i.scale.x * 1e-10
        || !i.rotation.is_finite()
        || i.rotation.sin().abs() + (i.rotation.cos() - 1.).abs() > 1e-10
        || i.rows != 1
        || i.cols != 1
        || i.clip.is_some()
    {
        return Err(policy());
    }
    let space = d.space_of(h).ok_or_else(policy)?;
    if !matches!(space, Space::Paper(_)) {
        return Err(err("projection relationships require a paper layout"));
    }
    let block = d.block(&i.block).ok_or_else(|| err("projection view block is missing"))?;
    if !block.base.is_finite() {
        return Err(err("nonfinite projection block base"));
    }
    Ok((i, space, i.insert.xy() - block.base.xy() * i.scale.x))
}
fn basis(v: &SavedView, direction: &str) -> Result<([f64; 3], [f64; 3])> {
    super::validate_basis(&v.basis)?;
    let read = |key| -> Result<Vec3> {
        let [x, y, z] = vector(&v.basis[key])?;
        Ok(Vec3::new(x, y, z))
    };
    let (x, y, d) = (read("x")?, read("y")?, read("direction")?);
    let (d, u) = match direction {
        "top" => (-y, d),
        "bottom" => (y, -d),
        "right" => (-x, y),
        "left" => (x, y),
        _ => return Err(err("projection direction must be top, bottom, left or right")),
    };
    Ok(([d.x, d.y, d.z], [u.x, u.y, u.z]))
}
fn request(p: &Value) -> Result<(&str, &str, f64)> {
    keys(p, &["parent", "direction", "gap", "hidden", "accept_incomplete"])?;
    let parent = text(p, "parent")?;
    handle(parent)?;
    let direction = text(p, "direction")?;
    if !matches!(direction, "top" | "bottom" | "left" | "right") {
        return Err(err("invalid projection direction"));
    }
    let gap = p
        .get("gap")
        .map_or(Some(20.), Value::as_f64)
        .filter(|v| v.is_finite() && (0.0..=10000.).contains(v))
        .ok_or_else(|| err("gap must be finite 0..10000 paper mm"))?;
    flag(p, "hidden", true)?;
    flag(p, "accept_incomplete", false)?;
    Ok((parent, direction, gap))
}
fn parent<'a>(d: &Drawing, store: &'a Store, h: &str) -> Result<&'a SavedView> {
    let h = handle(h)?;
    let v = store.views.iter().find(|v| handle(&v.handle).ok() == Some(h)).ok_or_else(|| err("missing saved projection parent"))?;
    let m = model(store, &v.model_id)?;
    if v.revision != m.revision || v.source != m.source {
        return Err(err("stale_source: projection parent is not the current linked revision"));
    }
    if v.provenance.get("section").is_some() {
        return Err(err("section parents cannot project the uncut source"));
    }
    placement(d, v)?;
    Ok(v)
}
/// Canonical ordinary view parameters for the exact worker. `at` is provisional on the free axis.
pub fn prepare(s: &Session, p: &Value) -> Result<Value> {
    let (h, direction, _) = request(p)?;
    let store = load(s.doc()?)?;
    validate_store_links(&store)?;
    validate_graph(s.doc()?, &store, true)?;
    let v = parent(s.doc()?, &store, h)?;
    let (i, space, origin) = placement(s.doc()?, v)?;
    let Space::Paper(layout) = space else { return Err(policy()) };
    let (direction, up) = basis(v, direction)?;
    let p = json!({"id":v.model_id,"layout":layout,"at":[origin.x,origin.y],"direction":direction,"up":up,
        "scale":1./i.scale.x,"hidden":flag(p,"hidden",true)?,"accept_incomplete":flag(p,"accept_incomplete",false)?});
    options(s, &p)?;
    Ok(p)
}
pub(super) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("gost.model.projected_view", "Create Linked Projected View", projected)
        .params("{parent,direction:top|bottom|left|right,gap?:20 paper mm,hidden?:true,accept_incomplete?:false}; native first-angle projection, same source and scale")]
}
fn projected(s: &mut Session, p: &Value) -> Result<Value> {
    let prepared = prepare(s, p)?;
    let store = load(s.doc()?)?;
    let m = model(&store, text(&prepared, "id")?)?;
    let (direction, up) = super::view_vectors(&prepared)?;
    let projection = (hooks()?.view)(&m.path, &m.source.sha256, direction, up).map_err(err)?;
    install(s, p, projection)
}
/// Complete exact host work through the ordinary guarded transaction. No worker I/O occurs here.
pub fn apply(s: &mut Session, p: &Value, projection: Projection) -> Result<Value> {
    s.execute_prepared("gost.model.projected_view", p, |s| install(s, p, projection))
}
fn install(s: &mut Session, p: &Value, projection: Projection) -> Result<Value> {
    let (h, direction, gap) = request(p)?;
    if projection.entities.len() > MAX_ENTITIES {
        return Err(err("projection entity count limit exceeded"));
    }
    let mut prepared = prepare(s, p)?;
    let store = load(s.doc()?)?;
    let v = parent(s.doc()?, &store, h)?;
    let (i, _, origin) = placement(s.doc()?, v)?;
    let parent_bounds = view_bounds(s.doc()?, i)?;
    let hidden = flag(p, "hidden", true)?;
    let mut child_bounds = Bounds2::EMPTY;
    for entity in &projection.entities {
        if !entity.covered && (hidden || entity.visibility != cadcraft_doc::model_projection::Visibility::Hidden) {
            child_bounds = child_bounds.union(&curve_bounds(&entity.kind)?);
        }
    }
    checked_bounds(child_bounds)?;
    let mut at = origin;
    match direction {
        "top" => at.y = parent_bounds.min.y - gap - child_bounds.max.y * i.scale.x,
        "bottom" => at.y = parent_bounds.max.y + gap - child_bounds.min.y * i.scale.x,
        "right" => at.x = parent_bounds.min.x - gap - child_bounds.max.x * i.scale.x,
        "left" => at.x = parent_bounds.max.x + gap - child_bounds.min.x * i.scale.x,
        _ => return Err(err("invalid projection direction")),
    }
    prepared["at"] = json!([at.x, at.y]);
    let opts = options(s, &prepared)?;
    let id = v.model_id.clone();
    let sha = v.source.sha256.clone();
    let relation =
        Link { version: 1, parent: Some(v.handle.clone()), original_parent: v.handle.clone(), direction: direction.into(), status: "linked".into() };
    let conservative_curve = |kind: &EntityKind| matches!(kind, EntityKind::Spline(s) if !quadratic_bezier(s));
    let conservative = projection.entities.iter().any(|e| conservative_curve(&e.kind))
        || s.doc()?.block(&i.block).is_some_and(|b| b.entities.iter().any(|e| conservative_curve(&e.kind)));
    let extra_gap_bound = if !conservative {
        0.
    } else if matches!(direction, "top" | "bottom") {
        parent_bounds.max.y - parent_bounds.min.y + (child_bounds.max.y - child_bounds.min.y) * i.scale.x
    } else {
        parent_bounds.max.x - parent_bounds.min.x + (child_bounds.max.x - child_bounds.min.x) * i.scale.x
    };
    let placement_audit = json!({"requested_gap_mm":gap,"gap_contract":"minimum_native_curve_gap",
        "bounds":if conservative {"analytic_conics_and_conservative_same_sign_rational_spline_control_hulls"} else {"analytic_native_curve_extrema"},
        "conservative_spline_margin":conservative,"conservative_extra_gap_upper_bound_mm":extra_gap_bound});
    atomic(s, |s| {
        let mut result = install_view_options(s, &id, &sha, &prepared, projection, opts, None)?;
        let h = handle(text(&result, "handle")?)?;
        let mut store = load(s.doc()?)?;
        let view = store.views.iter_mut().find(|v| handle(&v.handle).ok() == Some(h)).ok_or_else(|| err("new projection view disappeared"))?;
        view.provenance["projection_link"] = json!(relation);
        view.provenance["projection_placement"] = placement_audit.clone();
        s.doc_mut()?.modify_entity(h, |e| {
            e.common.xdata.push(tag(1001, MARKER));
            e.common.xdata.push(tag(1070, "1"));
        })?;
        validate_store_links(&store)?;
        validate_graph(s.doc()?, &store, true)?;
        save(s.doc_mut()?, &store)?;
        result["projection_link"] = json!(relation);
        result["projection_placement"] = placement_audit;
        Ok(result)
    })
}

fn checked_bounds(b: Bounds2) -> Result<Bounds2> {
    if b.is_empty() || !b.min.is_finite() || !b.max.is_finite() {
        return Err(err("projection has no finite native curve bounds"));
    }
    Ok(b)
}
pub(super) fn curve_bounds(kind: &EntityKind) -> Result<Bounds2> {
    if !crate::finite::is_finite(kind) {
        return Err(err("nonfinite native projection curve"));
    }
    if !matches!(kind, EntityKind::Line(_) | EntityKind::Circle(_) | EntityKind::Arc(_) | EntityKind::Ellipse(_) | EntityKind::Spline(_)) {
        return Err(err("projection view bounds require native exact curves"));
    }
    let mut bounds = Bounds2::EMPTY;
    for prim in kind.prims() {
        let b = match prim {
            Prim::Seg(s) => s.bounds(),
            Prim::Circle(c) => c.bounds(),
            Prim::Ellipse(e) => e.bounds(),
            Prim::Spline(s) => spline_bounds(&s)?,
            _ => return Err(err("unsupported native projection curve bounds")),
        };
        bounds = bounds.union(&b);
    }
    checked_bounds(bounds)
}
pub(super) fn view_bounds(d: &Drawing, i: &Insert) -> Result<Bounds2> {
    let block = d.block(&i.block).ok_or_else(|| err("missing projection block"))?;
    if block.entities.len() > MAX_ENTITIES {
        return Err(err("projection entity count limit exceeded"));
    }
    let mut bounds = Bounds2::EMPTY;
    for entity in block.entities.iter() {
        if super::section_notation::mark_owner(&entity.common)?.is_some() {
            continue;
        }
        if entity.common.extrusion != Vec3::Z {
            return Err(err("projection bounds refuse non-XY native curves"));
        }
        let b = curve_bounds(&entity.kind)?;
        bounds = bounds.union(&Bounds2 {
            min: (b.min - block.base.xy()) * i.scale.x + i.insert.xy(),
            max: (b.max - block.base.xy()) * i.scale.x + i.insert.xy(),
        });
    }
    checked_bounds(bounds)
}

// A same-sign rational B-spline is contained by its control hull: its normalized
// basis coefficients are nonnegative and sum to one. This is conservative, not sampled.
// Quadratic Beziers (including exact conics with a zero/negative middle weight) use
// analytic rational extrema and a denominator proof instead of this general hull.
fn spline_bounds(s: &Spline) -> Result<Bounds2> {
    if !s.is_valid()
        || (!s.weights.is_empty() && s.weights.len() != s.control.len())
        || s.knots.iter().any(|v| !v.is_finite())
        || s.control.len() > MAX_ENTITIES
        || s.control.iter().any(|v| !v.is_finite())
        || s.domain().0 >= s.domain().1
    {
        return Err(err("invalid native spline for projection bounds"));
    }
    if quadratic_bezier(s) {
        return quadratic_bounds(s);
    }
    if !s.weights.is_empty() && !(s.weights.iter().all(|v| v.is_finite() && *v > 0.) || s.weights.iter().all(|v| v.is_finite() && *v < 0.)) {
        return Err(err("projection placement cannot prove a finite rational spline denominator for mixed or zero weights"));
    }
    checked_bounds(Bounds2::from_points(s.control.iter().copied()))
}
fn quadratic_bezier(s: &Spline) -> bool {
    s.degree == 2
        && s.control.len() == 3
        && s.knots.len() == 6
        && s.knots[..3].iter().all(|k| *k == s.knots[0])
        && s.knots[3..].iter().all(|k| *k == s.knots[5])
}
fn quadratic_roots(a: f64, b: f64, c: f64) -> Result<[Option<f64>; 2]> {
    if [a, b, c].iter().any(|v| !v.is_finite()) {
        return Err(err("nonfinite analytic rational extrema"));
    }
    let scale = a.abs().max(b.abs()).max(c.abs());
    if scale == 0. {
        return Ok([None, None]);
    }
    let (a, b, c) = (a / scale, b / scale, c / scale);
    if a == 0. {
        return Ok([if b == 0. { None } else { Some(-c / b) }, None]);
    }
    let discriminant = b * b - 4. * a * c;
    if discriminant < 0. {
        return Ok([None, None]);
    }
    let q = -0.5 * (b + discriminant.sqrt().copysign(b));
    if q == 0. {
        return Ok([Some(-b / (2. * a)), None]);
    }
    Ok([Some(q / a), Some(c / q)])
}
fn quadratic_bounds(s: &Spline) -> Result<Bounds2> {
    let weights = [s.weights.first().copied().unwrap_or(1.), s.weights.get(1).copied().unwrap_or(1.), s.weights.get(2).copied().unwrap_or(1.)];
    let scale = weights.iter().map(|w| w.abs()).fold(0., f64::max);
    if !scale.is_finite() || scale == 0. || weights.iter().any(|w| !w.is_finite()) {
        return Err(err("invalid rational conic weights"));
    }
    let w = weights.map(|w| w / scale);
    let polynomial = |c: [f64; 3]| [c[0] - 2. * c[1] + c[2], 2. * (c[1] - c[0]), c[0]];
    let denominator = polynomial(w);
    let value = |p: [f64; 3], t: f64| (p[0] * t + p[1]) * t + p[2];
    let mut minimum = w[0].min(w[2]);
    let mut maximum = w[0].max(w[2]);
    if denominator[0] != 0. {
        let t = -denominator[1] / (2. * denominator[0]);
        if (0.0..=1.0).contains(&t) {
            let w = value(denominator, t);
            minimum = minimum.min(w);
            maximum = maximum.max(w);
        }
    }
    if minimum <= 0. && maximum >= 0. {
        return Err(err("rational conic denominator reaches zero inside its native parameter domain"));
    }
    let x = polynomial([s.control[0].x * w[0], s.control[1].x * w[1], s.control[2].x * w[2]]);
    let y = polynomial([s.control[0].y * w[0], s.control[1].y * w[1], s.control[2].y * w[2]]);
    let evaluate = |t| Vec2::new(value(x, t) / value(denominator, t), value(y, t) / value(denominator, t));
    let mut bounds = Bounds2::from_points([s.control[0], s.control[2]]);
    for numerator in [x, y] {
        let [a, b, c] = numerator;
        let [d, e, f] = denominator;
        for t in quadratic_roots(a * e - b * d, 2. * (a * f - c * d), b * f - c * e)?.into_iter().flatten() {
            if (0.0..=1.0).contains(&t) {
                let point = evaluate(t);
                if !point.is_finite() {
                    return Err(err("nonfinite rational conic extreme"));
                }
                bounds.add(point);
            }
        }
    }
    checked_bounds(bounds)
}

fn live(d: &Drawing, v: &SavedView) -> Result<bool> {
    let h = handle(&v.handle)?;
    let Some(e) = d.entity(h) else { return Ok(false) };
    let Some(identity) = data::load::<ViewLink>(&e.common, VIEW)? else { return Ok(false) };
    Ok(handle(&identity.view)? == h && identity.model_id == v.model_id && identity.revision == v.revision)
}
/// Validate live graph, optionally its alignment (translations are reconciled before the latter).
fn validate_graph(d: &Drawing, store: &Store, alignment: bool) -> Result<()> {
    validate_store_links(store)?;
    let by_handle: BTreeMap<_, _> = store.views.iter().map(|v| Ok((handle(&v.handle)?, v))).collect::<Result<_>>()?;
    let mut associated = BTreeSet::new();
    let mut active_handles = BTreeSet::new();
    for view in &store.views {
        if let Some(relation) = link(view)?
            && let Some(parent) = relation.parent
        {
            active_handles.insert(handle(&view.handle)?);
            active_handles.insert(handle(&parent)?);
        }
    }
    for entity in super::entities(d) {
        let marked = marker(&entity.common)?;
        let identity = data::load::<ViewLink>(&entity.common, VIEW)?;
        if marked {
            let identity = identity.as_ref().ok_or_else(|| err("projection marker without view identity"))?;
            let v = by_handle.get(&handle(&identity.view)?).ok_or_else(|| err("projection marker without saved view"))?;
            if link(v)?.is_none() || handle(&identity.view)? != entity.handle || identity.model_id != v.model_id || identity.revision != v.revision {
                return Err(err("projection marker differs from saved relationship/source"));
            }
        }
        if let Some(identity) = identity {
            let h = handle(&identity.view)?;
            if active_handles.contains(&h) && (h != entity.handle || d.entity(entity.handle).is_none()) {
                return Err(policy());
            }
            if by_handle.get(&h).is_some_and(|v| v.provenance.get("projection_link").is_some()) {
                if !associated.insert(h) || h != entity.handle || !marked {
                    return Err(err("duplicate, nested or detached projection view identity"));
                }
                if d.entity(entity.handle).is_none() && link(by_handle[&h])?.is_some_and(|r| r.parent.is_some()) {
                    return Err(policy());
                }
            }
        }
    }
    for child in &store.views {
        let Some(relation) = link(child)? else { continue };
        if !live(d, child)? {
            continue;
        }
        let Some(parent_handle) = relation.parent.as_deref() else { continue };
        let Some(parent) = by_handle.get(&handle(parent_handle)?) else { continue };
        if !live(d, parent)? {
            continue;
        }
        let (ci, cs, co) = placement(d, child)?;
        let (pi, ps, po) = placement(d, parent)?;
        if cs != ps || (ci.scale.x - pi.scale.x).abs() > pi.scale.x * 1e-10 {
            return Err(policy());
        }
        let (direction, up) = basis(parent, &relation.direction)?;
        let d = Vec3::new(direction[0], direction[1], direction[2]);
        let u = Vec3::new(up[0], up[1], up[2]);
        let x = d.cross(u);
        let y = x.cross(d);
        let cv = |key| -> Result<Vec3> {
            let [x, y, z] = vector(&child.basis[key])?;
            Ok(Vec3::new(x, y, z))
        };
        if (cv("direction")? - d).len() > 1e-8 || (cv("x")? - x).len() > 1e-8 || (cv("y")? - y).len() > 1e-8 {
            return Err(err("projection child basis differs from parent frame"));
        }
        if alignment && if relation.vertical() { (co.x - po.x).abs() > ALIGN_EPS } else { (co.y - po.y).abs() > ALIGN_EPS } {
            return Err(err("linked projection child may move only along its free paper axis"));
        }
    }
    // An active family cannot also occur through another INSERT of its native block.
    let mut family_blocks = BTreeSet::new();
    let mut family_handles = BTreeSet::new();
    for v in &store.views {
        if let Some(relation) = link(v)?
            && relation.parent.is_some()
            && live(d, v)?
        {
            for h in [Some(v.handle.as_str()), relation.parent.as_deref()].into_iter().flatten() {
                if let Some(v) = by_handle.get(&handle(h)?)
                    && live(d, v)?
                {
                    let (i, _, _) = placement(d, v)?;
                    family_blocks.insert(i.block.as_str());
                    family_handles.insert(handle(&v.handle)?);
                }
            }
        }
    }
    for entity in super::entities(d) {
        if let EntityKind::Insert(i) = &entity.kind
            && family_blocks.contains(i.block.as_str())
            && !family_handles.contains(&entity.handle)
        {
            return Err(policy());
        }
    }
    Ok(())
}

fn relevant_changes(before: &Drawing, d: &Drawing) -> Result<bool> {
    let mut any = false;
    let mut blocks = BTreeSet::new();
    for drawing in [before, d] {
        for e in super::entities(drawing) {
            let marked = marker(&e.common)?;
            any |= marked;
            if let EntityKind::Insert(i) = &e.kind
                && (marked || data::load::<ViewLink>(&e.common, VIEW)?.is_some())
            {
                blocks.insert(i.block.as_str());
            }
        }
    }
    if !any && blocks.is_empty() {
        return Ok(false);
    }
    // Include wrappers of native view blocks without decoding the Store or traversing geometry.
    let mut wrappers: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for drawing in [before, d] {
        for (name, block) in &drawing.blocks {
            for entity in block.entities.iter() {
                if let EntityKind::Insert(i) = &entity.kind {
                    wrappers.entry(i.block.as_str()).or_default().push(name.as_str());
                }
            }
        }
    }
    let mut queue: Vec<_> = blocks.iter().copied().collect();
    while let Some(child) = queue.pop() {
        if let Some(parents) = wrappers.get(child) {
            for parent in parents {
                if blocks.insert(*parent) {
                    queue.push(*parent);
                }
            }
        }
    }
    let mut changed = false;
    let mut old_inserts = BTreeMap::new();
    for e in super::entities(before) {
        if let EntityKind::Insert(i) = &e.kind
            && blocks.contains(i.block.as_str())
        {
            old_inserts.insert(e.handle, (i, &e.common, before.space_of(e.handle)));
        }
    }
    for e in super::entities(d) {
        if let EntityKind::Insert(i) = &e.kind
            && blocks.contains(i.block.as_str())
        {
            if let Some((old, common, space)) = old_inserts.remove(&e.handle) {
                changed |= i != old
                    || common.xdata != e.common.xdata
                    || space != d.space_of(e.handle)
                    || common.extrusion != e.common.extrusion
                    || before.block(&old.block).map(|b| b.base) != d.block(&i.block).map(|b| b.base);
            } else {
                changed = true;
            }
        }
    }
    Ok(changed
        || !old_inserts.is_empty()
        || !(Arc::ptr_eq(&before.foreign, &d.foreign) || super::record(before)?.map(|r| &r.tags) == super::record(d)?.map(|r| &r.tags)))
}

/// Cheap marker/INSERT comparison precedes Store decoding. Unrelated edits do not parse its 32 MB payload.
pub fn after_command(s: &mut Session, before: Option<&Arc<Drawing>>, uid: Option<u64>) -> Result<()> {
    let Some(before) = before else { return Ok(()) };
    let state = s.state()?;
    if uid.is_some_and(|uid| uid != state.uid) || Arc::ptr_eq(before, &state.doc) {
        return Ok(());
    }
    let d = s.doc()?;
    if !relevant_changes(before, d)? {
        return Ok(());
    }
    let mut store = load(d)?;
    validate_graph(d, &store, false)?;
    let old_store = load(before)?;
    let old_views: BTreeMap<_, _> = old_store.views.iter().map(|v| Ok((handle(&v.handle)?, v))).collect::<Result<_>>()?;
    for view in &store.views {
        if let Some(old) = old_views.get(&handle(&view.handle)?)
            && live(before, old)?
            && live(d, view)?
            && link(old)? != link(view)?
        {
            return Err(err("an existing projection relationship and original_parent audit are immutable"));
        }
    }
    let mut updates = BTreeMap::<Handle, Vec2>::new();
    let mut missing = false;
    // Validate every surviving family transform before deriving any descendant movement.
    let mut active = BTreeSet::new();
    for v in &store.views {
        if let Some(relation) = link(v)?
            && let Some(parent) = relation.parent
            && live(d, v)?
        {
            active.insert(handle(&v.handle)?);
            active.insert(handle(&parent)?);
        }
    }
    for h in active {
        let Some(v) = store.views.iter().find(|v| handle(&v.handle).ok() == Some(h)) else { continue };
        if !live(d, v)? {
            continue;
        }
        let (new, space, _) = placement(d, v)?;
        if let Some(old) = old_views.get(&h)
            && live(before, old)?
        {
            let (old, old_space, _) = placement(before, old)?;
            if space != old_space || (new.scale.x - old.scale.x).abs() > old.scale.x * 1e-10 || new.block != old.block {
                return Err(policy());
            }
        }
    }
    fn shift(
        h: Handle,
        d: &Drawing,
        before: &Drawing,
        store: &Store,
        old: &BTreeMap<Handle, &SavedView>,
        updates: &mut BTreeMap<Handle, Vec2>,
        depth: usize,
    ) -> Result<Vec2> {
        if let Some(delta) = updates.get(&h) {
            return Ok(*delta);
        }
        if depth > MAX_DEPTH {
            return Err(err("projection depth exceeds 32"));
        }
        let v = store.views.iter().find(|v| handle(&v.handle).ok() == Some(h)).ok_or_else(|| err("missing graph view"))?;
        let (_, _, now) = placement(d, v)?;
        let Some(was) = old.get(&h) else {
            updates.insert(h, Vec2::ZERO);
            return Ok(Vec2::ZERO);
        };
        if !live(before, was)? {
            updates.insert(h, Vec2::ZERO);
            return Ok(Vec2::ZERO);
        }
        let (_, _, was) = placement(before, was)?;
        let own = now - was;
        let inherited = if let Some(relation) = link(v)?
            && let Some(p) = relation.parent
        {
            let ph = handle(&p)?;
            if let Some(pv) = store.views.iter().find(|v| handle(&v.handle).ok() == Some(ph))
                && live(d, pv)?
                && old.contains_key(&ph)
            {
                let total = shift(ph, d, before, store, old, updates, depth + 1)?;
                // Commands selecting both parent and child have already applied the common translation.
                let parent_old = old.get(&ph).ok_or_else(|| err("missing old projection parent"))?;
                let (_, _, po) = placement(before, parent_old)?;
                let (_, _, pn) = placement(d, pv)?;
                let actual = pn - po;
                let common = if (own - actual).len() <= ALIGN_EPS { actual } else { Vec2::ZERO };
                total - common
            } else {
                Vec2::ZERO
            }
        } else {
            Vec2::ZERO
        };
        let total = own + inherited;
        updates.insert(h, total);
        Ok(total)
    }
    for v in &store.views {
        if link(v)?.is_some_and(|r| r.parent.is_some()) && live(d, v)? {
            shift(handle(&v.handle)?, d, before, &store, &old_views, &mut updates, 0)?;
        }
    }
    let mut moves = Vec::new();
    for (h, total) in updates {
        let Some(v) = store.views.iter().find(|v| handle(&v.handle).ok() == Some(h)) else { continue };
        let Some(old) = old_views.get(&h) else { continue };
        if !live(before, old)? {
            continue;
        }
        let (_, _, now) = placement(d, v)?;
        let (_, _, was) = placement(before, old)?;
        let additional = total - (now - was);
        if additional.len() > 0. {
            moves.push((h, additional));
        }
    }
    let live_parents: BTreeSet<_> = store
        .views
        .iter()
        .filter_map(|v| match live(d, v) {
            Ok(true) => Some(handle(&v.handle)),
            Ok(false) => None,
            Err(e) => Some(Err(e)),
        })
        .collect::<Result<_>>()?;
    for v in &mut store.views {
        let Some(mut relation) = link(v)? else { continue };
        if let Some(parent) = relation.parent.as_deref()
            && !live_parents.contains(&handle(parent)?)
        {
            relation.parent = None;
            relation.status = "missing_parent".into();
            v.provenance["projection_link"] = json!(relation);
            missing = true;
        }
    }
    // The outer command transaction rolls this back with all original command edits on refusal.
    for (h, delta) in moves {
        s.doc_mut()?.modify_entity(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.insert.x += delta.x;
                i.insert.y += delta.y;
            }
        })?;
    }
    validate_graph(s.doc()?, &store, true)?;
    if missing {
        save(s.doc_mut()?, &store)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "projections_tests.rs"]
mod tests;
