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
    #[serde(default)]
    extension_auto: bool,
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
fn settings(radius: f64, scale: f64, extension: f64) -> Result<(f64, bool)> {
    if !radius.is_finite() || radius <= 0.0 || radius > 1e9 || !scale.is_finite() || scale <= 0.0 || scale > 1e6 || !(3.0..=5.0).contains(&extension)
    {
        return Err(bad("gost.center", "invalid centre object settings"));
    }
    let length = radius / scale + extension;
    if !length.is_finite() || length * 8.0 > 16384.0 {
        return Err(bad("gost.center", "axis segment limit exceeded"));
    }
    Ok((length, 2.0 * radius / scale < 12.0))
}

pub(in crate::extensions) fn default_extension(radius: f64, scale: f64) -> Result<f64> {
    let (_, small) = settings(radius, scale, 3.0)?;
    // A 12..15 mm circle needs up to 4.5 mm extension to fit three >=5 mm
    // dashes and two >=3 mm dot intervals without moving the centre crossing.
    Ok(if small { 3.0 } else { (10.5 - radius / scale).max(3.0) })
}

fn pattern(length: f64) -> Result<(usize, f64, f64)> {
    if length < 10.5 - 1e-10 {
        return Err(bad("gost.center", "extension too short for the GOST dash pattern; omit extension for automatic sizing or increase it"));
    }
    // settings() bounds finite paper length to 2048 mm before this conversion.
    let count = ((length - 7.5) / 19.0).round().max(1.0) as usize;
    let n = count as f64;
    // Table 1: long dashes 5..30 mm, the complete interval including the dot
    // 3..5 mm. This interval is NOT a 3..5 mm gap on each side of the dot.
    let low = ((length - 30.0 * (n + 0.5)) / n).max(3.0);
    let high = ((length - 5.0 * (n + 0.5)) / n).min(5.0);
    if low > high + 1e-10 {
        return Err(bad("gost.center", "cannot fit a bounded GOST dash pattern"));
    }
    let interval = 4.0_f64.max(low).min(high.max(low));
    let dash = (length - n * interval) / (n + 0.5);
    Ok((count, dash, interval))
}

pub(in crate::extensions) fn entity_count(radius: f64, scale: f64, extension: f64) -> Result<usize> {
    let (length, small) = settings(radius, scale, extension)?;
    Ok(if small { 4 } else { 4 + 8 * pattern(length)?.0 })
}

/// A bounded paper-sized pattern shared by centre crosses and section symmetry axes.
pub(in crate::extensions) struct AxisPattern {
    length: f64,
    small: bool,
    periods: usize,
    dash: f64,
    gap: f64,
}
impl AxisPattern {
    pub(in crate::extensions) fn new(radius: f64, scale: f64, extension: f64) -> Result<Self> {
        let (length, small) = settings(radius, scale, extension)?;
        let (periods, dash, gap) = if small {
            (0, 0., 0.)
        } else {
            let (count, dash, interval) = pattern(length)?;
            (count, dash, (interval - 0.5) * 0.5)
        };
        Ok(Self { length, small, periods, dash, gap })
    }
    pub(in crate::extensions) fn count(&self) -> usize {
        if self.small { 2 } else { 2 + 4 * self.periods }
    }
    pub(in crate::extensions) fn append(&self, result: &mut Vec<(Common, EntityKind)>, center: Vec2, direction: Vec2, scale: f64) -> Result<()> {
        if !center.is_finite()
            || center.x.abs() > 1e9
            || center.y.abs() > 1e9
            || !direction.is_finite()
            || (direction.len() - 1.).abs() > 1e-10
            || !scale.is_finite()
            || scale <= 0.
            || scale > 1e6
        {
            return Err(bad("gost.center", "invalid native axis frame"));
        }
        for dir in [direction, -direction] {
            let mut add = |start: f64, end: f64| {
                let common = Common {
                    layer: if self.small { "GOST-THIN" } else { "GOST-CENTER" }.into(),
                    linetype: "Continuous".into(),
                    ..Default::default()
                };
                result.push((
                    common,
                    EntityKind::Line(Line { a: (center + dir * (start * scale)).to3(0.), b: (center + dir * (end * scale)).to3(0.) }),
                ));
            };
            if self.small {
                add(0., self.length);
                continue;
            }
            let mut end = self.dash * 0.5;
            add(0., end);
            for index in 0..self.periods {
                let dot_start = end + self.gap;
                let dot_end = dot_start + 0.5;
                add(dot_start, dot_end);
                let start = dot_end + self.gap;
                end = if index + 1 == self.periods { self.length } else { start + self.dash };
                add(start, end);
            }
        }
        Ok(())
    }
}

/// Append native centre crosses without an intermediate geometry allocation.
pub(in crate::extensions) fn geometry(result: &mut Vec<(Common, EntityKind)>, center: Vec2, radius: f64, scale: f64, extension: f64) -> Result<()> {
    let pattern = AxisPattern::new(radius, scale, extension)?;
    pattern.append(result, center, Vec2::X, scale)?;
    pattern.append(result, center, Vec2::Y, scale)
}
fn rebuild(d: &mut Drawing, h: Handle, r: &Record) -> Result<()> {
    let mut prepared = Vec::new();
    geometry(&mut prepared, Vec2::ZERO, r.radius, r.scale, r.extension)?;
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
pub(super) fn create(s: &mut Session, sources: Vec<Handle>, scale: f64, extension: Option<f64>) -> Result<Value> {
    let mut records = Vec::with_capacity(sources.len());
    let mut total = 0;
    for source in sources {
        let (center, radius) = circle(s.doc()?, source)?;
        let actual_extension = match extension {
            Some(value) => value,
            None => default_extension(radius, scale)?,
        };
        let record = Record { source, scale, extension: actual_extension, extension_auto: extension.is_none(), center, radius };
        total += entity_count(radius, scale, actual_extension)?;
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
            if r.extension_auto {
                r.extension = default_extension(radius, r.scale)?;
            }
            settings(r.radius, r.scale, r.extension)?;
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
