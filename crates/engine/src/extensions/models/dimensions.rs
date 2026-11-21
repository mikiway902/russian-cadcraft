//! Native dimensions anchored to exact source-world edge evaluations, not projected curves.
//! Document events only reproject cached points; changing a source never evaluates worker I/O.
use super::{
    Arc, CommandSpec, Common, Deserialize, Drawing, Entity, EntityKind, Handle, Insert, Result, SavedView, Serialize, Session, Space, Store, VIEW,
    Value, Vec2, Vec3, ViewLink, absolute, atomic, data, digest, entities, err, hooks, json, keys, load, model, text,
};
use crate::cmd::find_command;
use cadcraft_doc::DimKind;

pub(super) const DIM: &str = "linked-model-dimension";
const MAX_DIMENSIONS: usize = 1024;
const EVALUATION: &str = "upstream_exact_trimmed_edge_path_arc_length_no_mesh_or_fitting";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor {
    reference: String,
    fraction: f64,
    world: [f64; 3],
    audit: Value,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Link {
    version: u32,
    pub(super) model_id: String,
    pub(super) view: String,
    revision: u64,
    pub(super) source_sha256: String,
    a: Anchor,
    b: Anchor,
    kind: String,
    offset: f64,
    status: String,
    annotation_before_loss: Option<String>,
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("gost.model.references", "Exact Model References", references).params("{id}; exact edge/face reference discovery with honest persistence scope").noundo(),
        CommandSpec::new("gost.model.resolve_point", "Resolve Exact Model Point", resolve_point).params("{id,reference,fraction}; exact trimmed-edge arc-length fraction, source-world millimetres").noundo(),
        CommandSpec::new("gost.model.dimension", "Exact Model Dimension", dimension).params("{handle?:edit,view?:hex,a?:{reference,fraction},b?:same,kind?:horizontal|vertical|aligned,offset?:signed paper mm}; new dimensions need live view/a/b; aligned measures true source-world distance"),
        CommandSpec::new("gost.model.dimension.inspect", "Inspect Exact Model Dimension", inspect).params("{handle}; cached exact anchors, audit, current status and annotation").noundo(),
    ]
}
fn handle(p: &Value, key: &str) -> Result<Handle> {
    Handle::parse_hex(text(p, key)?).ok_or_else(|| err(format!("{key}: hexadecimal entity handle required")))
}
fn reference(p: &Value) -> Result<(&str, f64)> {
    keys(p, &["reference", "fraction"])?;
    let token = text(p, "reference")?;
    let fraction = p["fraction"].as_f64().filter(|f| f.is_finite() && (0.0..=1.0).contains(f)).ok_or_else(|| err("fraction must be in 0..1"))?;
    if token.len() > 8192 {
        return Err(err("reference exceeds 8192 bytes"));
    }
    Ok((token, fraction))
}
fn references(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["id"])?;
    let store = load(s.doc()?)?;
    let m = model(&store, text(p, "id")?)?;
    (hooks()?.references)(&m.path, &m.source.sha256).map_err(err)
}
fn resolve_point(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["id", "reference", "fraction"])?;
    let anchor = json!({"reference":p["reference"],"fraction":p["fraction"]});
    let (token, fraction) = reference(&anchor)?;
    let store = load(s.doc()?)?;
    let m = model(&store, text(p, "id")?)?;
    (hooks()?.resolve_point)(&m.path, &m.source.sha256, token, fraction).map_err(err)
}
fn old_link(d: &Drawing, p: &Value) -> Result<Option<(Handle, Link)>> {
    p.get("handle")
        .map(|_| {
            let h = handle(p, "handle")?;
            let e = d.entity(h).ok_or_else(|| err("dimension not found"))?;
            if !matches!(e.kind, EntityKind::Dimension(_)) {
                return Err(err("not a native dimension"));
            }
            let link = data::load::<Link>(&e.common, DIM)?.ok_or_else(|| err("not an exact model dimension"))?;
            validate(&link)?;
            Ok((h, link))
        })
        .transpose()
}
fn live_view<'a>(d: &Drawing, store: &'a Store, h: Handle) -> Result<&'a SavedView> {
    let v = store.views.iter().find(|v| v.handle == h.hex()).ok_or_else(|| err("missing saved exact model view"))?;
    let m = model(store, &v.model_id)?;
    if v.revision != m.revision || v.source != m.source {
        return Err(err("stale_source: frozen view is not the current linked revision"));
    }
    let e = d.entity(h).ok_or_else(|| err("model view is missing"))?;
    let link: ViewLink = data::load(&e.common, VIEW)?.ok_or_else(|| err("view association is missing"))?;
    if link.view != v.handle || link.model_id != v.model_id || link.revision != v.revision {
        return Err(err("view identity differs from saved source"));
    }
    let EntityKind::Insert(i) = &e.kind else {
        return Err(err("model view is not an INSERT"));
    };
    if e.common.extrusion != Vec3::Z {
        return Err(err("exact dimension refuses non-XY view extrusion"));
    }
    transform(d, i)?;
    Ok(v)
}
fn transform(d: &Drawing, i: &Insert) -> Result<()> {
    if !i.rotation.is_finite()
        || !i.insert.is_finite()
        || !i.scale.is_finite()
        || i.scale.x <= 1e-12
        || (i.scale.x - i.scale.y).abs() > i.scale.x * 1e-10
        || (i.scale.x - i.scale.z).abs() > i.scale.x * 1e-10
        || i.rows != 1
        || i.cols != 1
        || i.clip.is_some()
    {
        return Err(err("exact dimension requires a finite positive uniform, unarrayed, unclipped view INSERT"));
    }
    let block = d.block(&i.block).ok_or_else(|| err("view block is missing"))?;
    if !block.base.is_finite() {
        return Err(err("view block base is nonfinite"));
    }
    Ok(())
}
pub(super) fn validate(r: &Link) -> Result<()> {
    if r.version != 1
        || r.revision == 0
        || Handle::parse_hex(&r.view).is_none()
        || r.model_id.is_empty()
        || !digest(&r.source_sha256)
        || !matches!(r.kind.as_str(), "horizontal" | "vertical" | "aligned")
        || !r.offset.is_finite()
        || r.offset.abs() > 10000.
        || r.status.len() > 16384
        || r.annotation_before_loss.as_ref().is_some_and(|s| s.len() > 4096)
    {
        return Err(err("invalid exact dimension link"));
    }
    for a in [&r.a, &r.b] {
        if a.reference.is_empty()
            || a.reference.len() > 8192
            || !a.fraction.is_finite()
            || !(0.0..=1.0).contains(&a.fraction)
            || a.world.iter().any(|n| !n.is_finite())
        {
            return Err(err("invalid exact dimension anchor"));
        }
        let world = evaluated(&a.audit, &a.reference, a.fraction, &r.source_sha256)?;
        if world != a.world {
            return Err(err("exact dimension cached point differs from audit"));
        }
    }
    Ok(())
}
fn evaluated(v: &Value, token: &str, fraction: f64, sha: &str) -> Result<[f64; 3]> {
    if v["status"] != "resolved" {
        return Err(err(format!("exact anchor unresolved: {} ({})", v["status"], v["reason"])));
    }
    if token.is_empty()
        || token.len() > 8192
        || v["protocol"] != "cadcraft-references/1"
        || v["source"]["sha256"] != sha
        || v["fraction"].as_f64() != Some(fraction)
        || v["fraction_convention"] != "trimmed_edge_arc_length_start_to_end"
        || v["evaluation"] != EVALUATION
        || !v["baseline_source_sha256"].as_str().is_some_and(digest)
        || !matches!(v["verification"].as_str(), Some("payload_scoped" | "verified_upstream_history" | "unverified_upstream_name"))
        || v["placement"].as_u64().is_none()
        || v["edge"].as_u64().is_none()
    {
        return Err(err("invalid exact point audit"));
    }
    let changed = v["baseline_source_sha256"] != sha;
    let native = v["identity_scope"] == "canonical_project_path_and_payload_baseline";
    if (!native && v["identity_scope"] != "payload_sha256_and_canonical_path")
        || v["cross_rebuild"].as_bool() != Some(changed)
        || (changed && (!native || v["verification"] != "verified_upstream_history"))
        || (native && (!v["native_identity"].is_object() || v["verification"] == "payload_scoped"))
        || (!native && (!v["native_identity"].is_null() || v["verification"] != "payload_scoped"))
        || !v["canonical_path"].as_str().is_some_and(absolute)
        || v["source"]["bytes"].as_u64().is_none()
    {
        return Err(err("invalid exact point source identity/continuity audit"));
    }
    for (key, expected) in [
        ("length", "mm"),
        ("area", "mm2"),
        ("volume", "mm3"),
        ("angle", "radian"),
        ("coordinate_system", "right_handed_z_up"),
        ("original_declaration", "not_exposed_by_selected_import_api"),
    ] {
        if v["units"][key] != expected {
            return Err(err("exact point units are not source-world millimetres"));
        }
    }
    super::vector(&v["point"])
}
fn points(d: &Drawing, store: &Store, r: &Link) -> Result<(Vec2, Vec2, f64, Space)> {
    validate(r)?;
    let h = Handle::parse_hex(&r.view).ok_or_else(|| err("invalid view handle"))?;
    let v = live_view(d, store, h)?;
    if v.model_id != r.model_id || v.revision != r.revision || v.source.sha256 != r.source_sha256 {
        return Err(err("dimension belongs to a different view revision"));
    }
    let EntityKind::Insert(i) = &d.entity(h).ok_or_else(|| err("missing view"))?.kind else {
        return Err(err("missing view INSERT"));
    };
    let x = super::vector(&v.basis["x"])?;
    let y = super::vector(&v.basis["y"])?;
    let base = d.block(&i.block).ok_or_else(|| err("missing view block"))?.base;
    let project = |p: [f64; 3]| {
        let dot = |axis: [f64; 3]| p[0] * axis[0] + p[1] * axis[1] + p[2] * axis[2];
        let p = Vec2::new((dot(x) - base.x) * i.scale.x, (dot(y) - base.y) * i.scale.y);
        let (sin, cos) = i.rotation.sin_cos();
        Vec2::new(p.x * cos - p.y * sin + i.insert.x, p.x * sin + p.y * cos + i.insert.y)
    };
    let a = project(r.a.world);
    let b = project(r.b.world);
    let projected = a.dist(b);
    let world_a = Vec3::new(r.a.world[0], r.a.world[1], r.a.world[2]);
    let world_b = Vec3::new(r.b.world[0], r.b.world[1], r.b.world[2]);
    let measure = match r.kind.as_str() {
        "horizontal" => (b.x - a.x).abs(),
        "vertical" => (b.y - a.y).abs(),
        _ => projected,
    };
    let factor = if r.kind == "aligned" { (world_b - world_a).len() / projected } else { 1.0 / i.scale.x };
    if !projected.is_finite() || measure < 1e-8 || !factor.is_finite() || factor <= 0. {
        return Err(err("dimension is degenerate in this projection"));
    }
    Ok((a, b, factor, d.space_of(h).ok_or_else(|| err("view is not in a drawing space"))?))
}
pub(super) fn dimension_factor(d: &Drawing, e: &Entity) -> Option<f64> {
    let r: Link = data::load(&e.common, DIM).ok().flatten()?;
    points(d, &load(d).ok()?, &r).ok().map(|p| p.2)
}
fn request_parameters(d: &Drawing, p: &Value) -> Result<(Option<(Handle, Link)>, Handle, Value, Value)> {
    keys(p, &["handle", "view", "a", "b", "kind", "offset"])?;
    let old = old_link(d, p)?;
    let view = if p.get("view").is_some() {
        handle(p, "view")?
    } else {
        old.as_ref().and_then(|(_, r)| Handle::parse_hex(&r.view)).ok_or_else(|| err("view required"))?
    };
    let anchor = |key: &str, old_anchor: Option<&Anchor>| -> Result<Value> {
        let v = p
            .get(key)
            .cloned()
            .or_else(|| old_anchor.map(|a| json!({"reference":a.reference,"fraction":a.fraction})))
            .ok_or_else(|| err(format!("anchor {key} required")))?;
        reference(&v)?;
        Ok(v)
    };
    let a = anchor("a", old.as_ref().map(|(_, r)| &r.a))?;
    let b = anchor("b", old.as_ref().map(|(_, r)| &r.b))?;
    Ok((old, view, a, b))
}
fn dimension(s: &mut Session, p: &Value) -> Result<Value> {
    let (old, view, a, b) = request_parameters(s.doc()?, p)?;
    let replace = old.is_none() || ["view", "a", "b"].iter().any(|key| p.get(key).is_some());
    let resolutions = if replace {
        let store = load(s.doc()?)?;
        let v = live_view(s.doc()?, &store, view)?;
        let run = hooks()?.resolve_point;
        let (at, af) = reference(&a)?;
        let (bt, bf) = reference(&b)?;
        Some([run(&v.path, &v.source.sha256, at, af).map_err(err)?, run(&v.path, &v.source.sha256, bt, bf).map_err(err)?])
    } else {
        None
    };
    install_dimension(s, p, resolutions)
}
/// Complete two host-evaluated anchors in the ordinary guarded transaction. The caller must
/// verify document UID; source SHA, saved view revision and both audit payloads are checked here.
pub fn apply_dimension(s: &mut Session, p: &Value, resolutions: [Value; 2]) -> Result<Value> {
    s.execute_prepared("gost.model.dimension", p, |s| install_dimension(s, p, Some(resolutions)))
}
fn install_dimension(s: &mut Session, p: &Value, resolutions: Option<[Value; 2]>) -> Result<Value> {
    let (old, view, a, b) = request_parameters(s.doc()?, p)?;
    if old.is_none() && (s.doc()?.dim_style("GOST").is_none() || s.doc()?.text_style("GOST").is_none()) {
        return Err(err("a GOST drawing preset is required for exact model dimensions"));
    }
    let kind = p
        .get("kind")
        .map(|v| v.as_str().ok_or_else(|| err("kind must be a string")))
        .transpose()?
        .unwrap_or_else(|| old.as_ref().map_or("horizontal", |(_, r)| r.kind.as_str()));
    if !matches!(kind, "horizontal" | "vertical" | "aligned") {
        return Err(err("unsupported dimension kind"));
    }
    let offset = p
        .get("offset")
        .map(|v| v.as_f64().ok_or_else(|| err("offset must be finite paper millimetres")))
        .transpose()?
        .unwrap_or_else(|| old.as_ref().map_or(10., |(_, r)| r.offset));
    if !offset.is_finite() || offset.abs() > 10000. {
        return Err(err("offset exceeds signed paper range -10000..10000"));
    }
    let store = load(s.doc()?)?;
    let mut r = if let Some([ar, br]) = resolutions {
        let v = live_view(s.doc()?, &store, view)?;
        let anchor = |p: &Value, audit: Value| -> Result<Anchor> {
            let (reference, fraction) = reference(p)?;
            let world = evaluated(&audit, reference, fraction, &v.source.sha256)?;
            if audit["canonical_path"] != v.path || audit["source"]["bytes"].as_u64() != Some(v.source.bytes) {
                return Err(err("exact anchor source path/byte count mismatch"));
            }
            if (audit["identity_scope"] == "canonical_project_path_and_payload_baseline") != (v.metadata["format"] == "sc") {
                return Err(err("exact anchor identity scope differs from the saved source format"));
            }
            Ok(Anchor { reference: reference.into(), fraction, world, audit })
        };
        Link {
            version: 1,
            model_id: v.model_id.clone(),
            view: view.hex(),
            revision: v.revision,
            source_sha256: v.source.sha256.clone(),
            a: anchor(&a, ar)?,
            b: anchor(&b, br)?,
            kind: kind.into(),
            offset,
            status: "resolved".into(),
            annotation_before_loss: old.as_ref().and_then(|(_, r)| r.annotation_before_loss.clone()),
        }
    } else {
        old.as_ref().map(|(_, r)| r.clone()).ok_or_else(|| err("new dimension requires evaluated anchors"))?
    };
    r.kind = kind.into();
    r.offset = offset;
    validate(&r)?;
    let prepared = points(s.doc()?, &store, &r);
    if old.is_none() || p.get("view").is_some() || p.get("a").is_some() || p.get("b").is_some() {
        prepared.as_ref().map_err(|e| err(e.to_string()))?;
    }
    let space = if let Ok((_, _, _, space)) = &prepared {
        space.clone()
    } else {
        s.doc()?.space_of(old.as_ref().ok_or_else(|| err("dimension missing"))?.0).ok_or_else(|| err("dimension space missing"))?
    };
    if old.as_ref().is_some_and(|(h, _)| s.doc().is_ok_and(|d| d.space_of(*h) != Some(space.clone()))) {
        return Err(err("dimension and view must share the same drawing space"));
    }
    atomic(s, |s| {
        let h = if let Some((h, _)) = old {
            h
        } else {
            if entities(s.doc()?).filter(|e| data::load::<Link>(&e.common, DIM).ok().flatten().is_some()).count() >= MAX_DIMENSIONS {
                return Err(err("exact dimension count limit exceeded"));
            }
            let (a, b, _, _) = prepared.as_ref().map_err(|e| err(e.to_string()))?;
            let layout = find_command("layout.set").ok_or_else(|| err("layout command unavailable"))?.run;
            layout(s, &json!({"name":space.tab_name()}))?;
            let run = find_command("dimlinear").ok_or_else(|| err("dimension command unavailable"))?.run;
            let out = run(s, &json!({"p1":[a.x,a.y],"p2":[b.x,b.y],"at":[a.x,b.y+10.]}))?;
            let h = handle(&out, "handle")?;
            s.doc_mut()?.modify_entity(h, |e| {
                if let EntityKind::Dimension(dm) = &mut e.kind {
                    dm.style = "GOST".into();
                    dm.overrides.clear();
                }
            })?;
            h
        };
        geometry(s.doc_mut()?, h, &store, &mut r)?;
        describe(s.doc()?, h, &r)
    })
}
fn marker_matches(text: &str, original: &str) -> bool {
    if original.is_empty() { text == "?" } else { text.strip_prefix("? ") == Some(original) }
}
fn annotation_bound(text: &str, r: &Link) -> Result<()> {
    if text.len() > 4096 && !r.annotation_before_loss.as_ref().is_some_and(|original| marker_matches(text, original)) {
        return Err(err("dimension annotation exceeds 4096 bytes"));
    }
    Ok(())
}
fn mark_lost(dm: &mut cadcraft_doc::Dimension, r: &mut Link, status: String) {
    r.status = status;
    if !r.annotation_before_loss.as_ref().is_some_and(|original| marker_matches(&dm.text, original)) {
        r.annotation_before_loss = Some(dm.text.clone());
        dm.text = if dm.text.is_empty() { "?".into() } else { format!("? {}", dm.text) };
    }
    dm.block = None;
}
/// Block wrappers are not drawing-space view placements. Keep their associations for audit,
/// but never present an old paper dimension as a current measurement after PASTEBLOCK.
pub(super) fn nested_copy(e: &Entity) -> Result<Option<(Common, cadcraft_doc::Dimension)>> {
    let EntityKind::Dimension(dm) = &e.kind else { return Ok(None) };
    let Some(mut r) = data::load::<Link>(&e.common, DIM)? else { return Ok(None) };
    validate(&r)?;
    annotation_bound(&dm.text, &r)?;
    let mut dm = dm.clone();
    mark_lost(&mut dm, &mut r, "exact dimension view is nested; a drawing-space view placement is required".into());
    let mut common = e.common.clone();
    data::save(&mut common, DIM, &r)?;
    Ok(Some((common, dm)))
}
fn geometry(d: &mut Drawing, h: Handle, store: &Store, r: &mut Link) -> Result<()> {
    let e = d.entity(h).ok_or_else(|| err("dimension missing"))?;
    let EntityKind::Dimension(dm) = &e.kind else {
        return Err(err("not a dimension"));
    };
    annotation_bound(&dm.text, r)?;
    let prepared = points(d, store, r)
        .and_then(|p| if d.space_of(h) == Some(p.3.clone()) { Ok(p) } else { Err(err("dimension and view are on different sheets")) });
    match prepared {
        Ok((a, b, factor, _)) => {
            let (kind, at) = match r.kind.as_str() {
                "horizontal" => (
                    DimKind::Linear { rotation: 0. },
                    Vec2::new((a.x + b.x) * 0.5, if r.offset >= 0. { a.y.max(b.y) + r.offset } else { a.y.min(b.y) + r.offset }),
                ),
                "vertical" => (
                    DimKind::Linear { rotation: std::f64::consts::FRAC_PI_2 },
                    Vec2::new(if r.offset >= 0. { a.x.max(b.x) + r.offset } else { a.x.min(b.x) + r.offset }, (a.y + b.y) * 0.5),
                ),
                _ => {
                    let delta = b - a;
                    (DimKind::Aligned, (a + b) * 0.5 + Vec2::new(-delta.y, delta.x).normalized() * r.offset)
                }
            };
            d.modify_entity(h, |e| {
                if let EntityKind::Dimension(dm) = &mut e.kind {
                    dm.kind = kind;
                    dm.p13 = a.to3(0.);
                    dm.p14 = b.to3(0.);
                    dm.defpt = at.to3(0.);
                    dm.block = None;
                    dm.insertion_point = Vec3::ZERO;
                    dm.assoc.clear();
                    if let Some(original) = r.annotation_before_loss.take()
                        && marker_matches(&dm.text, &original)
                    {
                        dm.text = original;
                    }
                    dm.overrides.insert("linearFactor".into(), json!(factor));
                    dm.overrides.insert("scale".into(), json!(1.));
                }
            })?;
            let e = d.entity(h).ok_or_else(|| err("dimension missing"))?;
            if let EntityKind::Dimension(dm) = &e.kind {
                let st = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
                let g = cadcraft_render::dimension_geometry(dm, &st, 1.);
                d.modify_entity(h, |e| {
                    if let EntityKind::Dimension(dm) = &mut e.kind {
                        dm.measurement = g.measurement;
                        if !dm.user_text_pos {
                            dm.text_mid = g.text_pos.to3(0.);
                        }
                    }
                })?;
            }
            r.status = "resolved".into();
        }
        Err(error) => {
            d.modify_entity(h, |e| {
                if let EntityKind::Dimension(dm) = &mut e.kind {
                    mark_lost(dm, r, error.to_string());
                }
            })?;
        }
    }
    let mut common = d.entity(h).ok_or_else(|| err("dimension missing"))?.common.clone();
    data::save(&mut common, DIM, r)?;
    d.modify_entity(h, |e| e.common = common)?;
    Ok(())
}
fn describe(d: &Drawing, h: Handle, r: &Link) -> Result<Value> {
    let e = d.entity(h).ok_or_else(|| err("dimension missing"))?;
    let prepared = points(d, &load(d)?, r);
    let status = prepared.as_ref().err().map_or("resolved".into(), ToString::to_string);
    let paper_measurement = if let EntityKind::Dimension(dm) = &e.kind { Some(dm.measurement) } else { None };
    let measurement = prepared.as_ref().ok().map(|(a, b, factor, _)| {
        let paper = match r.kind.as_str() {
            "horizontal" => (b.x - a.x).abs(),
            "vertical" => (b.y - a.y).abs(),
            _ => a.dist(*b),
        };
        paper * factor
    });
    Ok(
        json!({"handle":h.hex(),"references":r,"resolved":prepared.is_ok(),"status":status,"measurement":measurement,"paper_measurement":paper_measurement,"measurement_units":"mm","dimension":e.kind,"associative":"cached-exact-world-points-no-automatic-source-rebuild"}),
    )
}
fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["handle"])?;
    let (h, r) = old_link(s.doc()?, p)?.ok_or_else(|| err("dimension handle required"))?;
    describe(s.doc()?, h, &r)
}
pub(super) fn after_command(s: &mut Session, before: Option<&Arc<Drawing>>, uid: Option<u64>) -> Result<()> {
    let Some(before) = before else { return Ok(()) };
    let state = s.state()?;
    if uid.is_some_and(|uid| uid != state.uid) || Arc::ptr_eq(before, &state.doc) {
        return Ok(());
    }
    let d = s.doc()?;
    let mut dimensions = Vec::new();
    for e in d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())) {
        if matches!(e.kind, EntityKind::Dimension(_))
            && let Some(r) = data::load::<Link>(&e.common, DIM)?
        {
            validate(&r)?;
            if dimensions.len() >= MAX_DIMENSIONS {
                return Err(err("exact dimension count limit exceeded"));
            }
            dimensions.push((e.handle, r));
        }
    }
    if dimensions.is_empty() {
        return Ok(());
    }
    let store = load(d)?;
    let old_store = load(before)?;
    dimensions.retain(|(h, r)| {
        let Some(view) = Handle::parse_hex(&r.view) else { return true };
        let changed = |h| match (before.entity(h), d.entity(h)) {
            (Some(old), Some(new)) => !Arc::ptr_eq(old, new),
            (None, None) => false,
            _ => true,
        };
        let source_changed = match (model(&old_store, &r.model_id), model(&store, &r.model_id)) {
            (Ok(old), Ok(new)) => old.revision != new.revision || old.source != new.source,
            (Err(_), Err(_)) => false,
            _ => true,
        };
        let block_base = |drawing: &Drawing| {
            drawing.entity(view).and_then(|e| match &e.kind {
                EntityKind::Insert(i) => drawing.block(&i.block).map(|b| b.base),
                _ => None,
            })
        };
        changed(*h) || changed(view) || source_changed || block_base(before) != block_base(d)
    });
    for (h, mut r) in dimensions {
        geometry(s.doc_mut()?, h, &store, &mut r)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "dimensions_tests.rs"]
mod tests;
