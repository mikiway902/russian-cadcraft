//! Native associative centre objects. Source handles persist through DXF; visible geometry
//! remains ordinary block lines for other readers. Copied source+axes pairs are remapped.
use super::data;
use crate::{Result, Session, cmd::bad};
use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Insert, Line};
use cadcraft_geom::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
const KIND: &str = "center-axes";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Record {
    source: Handle,
    scale: f64,
    extension: f64,
    center: Vec2,
    radius: f64,
}
fn circle(d: &Drawing, h: Handle) -> Result<(Vec2, f64)> {
    let e = d.entity(h).ok_or_else(|| bad("gost.center", "source no longer exists"))?;
    let (c, r) = match &e.kind {
        EntityKind::Circle(c) => (c.center.xy(), c.radius),
        EntityKind::Arc(a) => (a.center.xy(), a.radius),
        _ => return Err(bad("gost.center", "source is no longer a circle or arc")),
    };
    if !c.is_finite() || c.x.abs() > 1e9 || c.y.abs() > 1e9 || !r.is_finite() || r <= 0.0 || r > 1e9 {
        return Err(bad("gost.center", "invalid source geometry"));
    }
    Ok((c, r))
}
fn geometry(r: &Record) -> Result<Vec<(Common, EntityKind)>> {
    if !r.scale.is_finite() || r.scale <= 0.0 || r.scale > 10000.0 || !(3.0..=5.0).contains(&r.extension) {
        return Err(bad("gost.center", "invalid centre object settings"));
    }
    let length = r.radius / r.scale + r.extension;
    if !length.is_finite() || length * 8.0 > 16384.0 {
        return Err(bad("gost.center", "axis segment limit exceeded"));
    }
    let small = 2.0 * r.radius / r.scale < 12.0;
    let mut result = Vec::new();
    for dir in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
        let mut start = 0.0;
        let mut long = true;
        let mut end = if small { length } else { 5.0_f64.min(length) };
        loop {
            if length - end <= 8.0 {
                end = length;
            }
            let common =
                Common { layer: if small { "GOST-THIN" } else { "GOST-CENTER" }.into(), linetype: "Continuous".into(), ..Default::default() };
            result.push((common, EntityKind::Line(Line { a: (dir * (start * r.scale)).to3(0.0), b: (dir * (end * r.scale)).to3(0.0) })));
            if end >= length {
                break;
            }
            start = end + 3.0;
            long = !long;
            end = (start + if long { 15.0 } else { 1.0 }).min(length);
        }
    }
    Ok(result)
}
fn rebuild(d: &mut Drawing, h: Handle, r: &Record) -> Result<()> {
    let prepared = geometry(r)?;
    let e = d.entity(h).ok_or_else(|| bad("gost.center", "axes object missing"))?;
    let EntityKind::Insert(i) = &e.kind else { return Err(bad("gost.center", "axes must remain a block reference")) };
    let old = i.block.clone();
    let mut common = e.common.clone();
    data::save(&mut common, KIND, r)?;
    let name = format!("GOST-AXES-{}", h.hex());
    let mut b = Block::new(&name);
    b.units = 4;
    b.description = "CADCraftGOST associative centre axes".into();
    for (common, kind) in prepared {
        b.entities.push(Entity { handle: d.new_handle(), common, kind });
    }
    d.blocks.insert(name.clone(), Arc::new(b));
    d.modify_entity(h, |e| {
        e.common = common;
        if let EntityKind::Insert(i) = &mut e.kind {
            i.block = name;
            i.insert = r.center.to3(0.0);
            i.rotation = 0.0;
            i.scale = Vec3::new(1.0, 1.0, 1.0);
        }
    })?;
    if old.starts_with("GOST-AXES-")
        && !d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())).any(|e| matches!(&e.kind,EntityKind::Insert(i) if i.block==old))
    {
        d.blocks.remove(&old);
    }
    Ok(())
}
pub(super) fn create(s: &mut Session, sources: Vec<Handle>, scale: f64, extension: f64) -> Result<Value> {
    let mut records = Vec::with_capacity(sources.len());
    let mut total = 0;
    for source in sources {
        let (center, radius) = circle(s.doc()?, source)?;
        let record = Record { source, scale, extension, center, radius };
        total += geometry(&record)?.len();
        if total > 16384 {
            return Err(bad("gost.center", "aggregate axis segment limit exceeded"));
        }
        records.push(record);
    }
    let before = s.state()?.doc.clone();
    let result = (|| {
        let space = s.space();
        let mut handles = Vec::with_capacity(records.len());
        for r in records {
            let h = s.doc_mut()?.add(&space, Common { layer: "GOST-CENTER".into(), ..Default::default() }, EntityKind::Insert(Insert::default()))?;
            rebuild(s.doc_mut()?, h, &r)?;
            handles.push(h.hex());
        }
        Ok(json!({"handles":handles,"associative":true}))
    })();
    if result.is_err() {
        s.state_mut()?.doc = before;
    }
    result
}

pub(super) fn after_command(s: &mut Session, before: Option<&Arc<Drawing>>, uid: Option<u64>) -> Result<()> {
    let Some(before) = before else { return Ok(()) };
    let st = s.state()?;
    if uid.is_some_and(|u| u != st.uid) || Arc::ptr_eq(before, &st.doc) {
        return Ok(());
    }
    let d = s.doc()?;
    let mut changes = Vec::new();
    let mut removals = Vec::new();
    let mut count = 0;
    for e in d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())) {
        let EntityKind::Insert(i) = &e.kind else { continue };
        if !i.block.starts_with("GOST-AXES-") {
            continue;
        }
        if count == 0 && d.layouts.iter().fold(d.model.len(), |n, l| n.saturating_add(l.entities.len())) > 20_000 {
            return Err(bad("gost.center", "associative axes require at most 20000 entities"));
        }
        count += 1;
        if count > 1024 {
            return Err(bad("gost.center", "at most 1024 centre objects"));
        }
        let mut r: Record = data::load(&e.common, KIND)?.ok_or_else(|| bad("gost.center", "axes metadata missing"))?;
        let copied = before.entity(e.handle).is_none() && i.block != format!("GOST-AXES-{}", e.handle.hex());
        if copied {
            let mut candidates = d
                .model
                .iter()
                .chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
                .filter(|c| before.entity(c.handle).is_none() && d.space_of(c.handle) == d.space_of(e.handle))
                .filter_map(|c| {
                    circle(d, c.handle)
                        .ok()
                        .filter(|(center, radius)| center.near(i.insert.xy(), 1e-7) && (*radius - r.radius).abs() < 1e-7)
                        .map(|_| c.handle)
                });
            if let Some(source) = candidates.next() {
                if candidates.next().is_some() {
                    return Err(bad("gost.center", "copied axes have ambiguous source circles"));
                }
                r.source = source;
            } else if i.insert.xy() != r.center {
                return Err(bad("gost.center", "copy centre axes together with their source circle/arc"));
            }
        }
        if d.entity(r.source).is_none() {
            removals.push(e.handle);
            continue;
        }
        if d.space_of(r.source) != d.space_of(e.handle) {
            return Err(bad("gost.center", "source and axes must share a drawing space"));
        }
        let (center, radius) = circle(d, r.source)?;
        let transformed =
            i.insert.xy() != center || i.rotation != 0.0 || i.scale != Vec3::new(1.0, 1.0, 1.0) || i.rows != 1 || i.cols != 1 || i.clip.is_some();
        if center != r.center || radius != r.radius || transformed || copied {
            r.center = center;
            r.radius = radius;
            geometry(&r)?;
            changes.push((e.handle, r));
        }
    }
    if !changes.is_empty() || !removals.is_empty() {
        let d = s.doc_mut()?;
        for h in removals {
            let block = d.entity(h).and_then(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.block.clone()) } else { None });
            d.remove_entity(h);
            if let Some(block) = block
                && block == format!("GOST-AXES-{}", h.hex())
                && !d
                    .model
                    .iter()
                    .chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
                    .any(|e| matches!(&e.kind,EntityKind::Insert(i) if i.block==block))
            {
                d.blocks.remove(&block);
            }
        }
        for (h, r) in changes {
            rebuild(d, h, &r)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "axes_tests.rs"]
mod tests;
