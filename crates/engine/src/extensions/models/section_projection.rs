//! Frozen paper projection constraints. Events translate native INSERTs only.
use super::{Drawing, EntityKind, Handle, Insert, Options, Result, SavedView, Session, Space, Vec2, err, load, projections, section_notation};
use cadcraft_geom::Bounds2;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    AlongTrace,
    Fixed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Constraint {
    pub(super) version: u32,
    pub(super) mode: Mode,
    pub(super) axis: [f64; 2],
    pub(super) offset: [f64; 2],
}

pub(super) struct SourceTrace {
    pub(super) origin: Vec2,
    pub(super) point: Vec2,
    pub(super) axis: Vec2,
}

pub(super) fn validate(c: &Constraint) -> Result<()> {
    let axis = Vec2::new(c.axis[0], c.axis[1]);
    if c.version != 1 || !axis.is_finite() || (axis.len() - 1.).abs() > 1e-10 || !c.offset.iter().all(|v| v.is_finite() && v.abs() <= 2e9) {
        return Err(err("invalid section projection constraint version, unit axis or offset"));
    }
    Ok(())
}

fn bounded(at: Vec2) -> Result<()> {
    if !at.is_finite() || at.x.abs() > 1e9 || at.y.abs() > 1e9 {
        return Err(err("section projection placement requires finite paper coordinates within +/-1e9"));
    }
    Ok(())
}

fn center(bounds: Bounds2) -> Result<Vec2> {
    if bounds.is_empty() || !bounds.min.is_finite() || !bounds.max.is_finite() {
        return Err(err("section projection requires finite native geometry bounds"));
    }
    let center = bounds.center();
    if !center.is_finite() {
        return Err(err("nonfinite section projection bounds center"));
    }
    Ok(center)
}

pub(super) fn place(source: &SourceTrace, source_scale: f64, target_bounds: Bounds2, opts: &mut Options, imposed: bool) -> Result<Constraint> {
    let SourceTrace { origin: source_origin, point: trace_point, axis } = *source;
    let mut constraint =
        Constraint { version: 1, mode: if imposed { Mode::Fixed } else { Mode::AlongTrace }, axis: [axis.x, axis.y], offset: [0., 0.] };
    validate(&constraint)?;
    bounded(source_origin)?;
    bounded(opts.at)?;
    if !trace_point.is_finite() || !source_scale.is_finite() || source_scale <= 0. || !opts.scale.is_finite() || opts.scale <= 0. {
        return Err(err("section projection requires finite trace and positive scales"));
    }
    if imposed && (opts.scale * source_scale - 1.).abs() > 1e-10 {
        return Err(err("imposed section projection requires the actual source scale"));
    }
    let native_center = center(target_bounds)?;
    let normal = axis.perp();
    let at = if imposed {
        // Folding preserves model coordinates along the source trace. Centering
        // both bboxes would slide a narrow/off-centre cap along a wider source.
        source_origin + normal * (trace_point - source_origin - native_center * (1. / opts.scale)).dot(normal)
    } else {
        let target_center = opts.at + native_center * (1. / opts.scale);
        opts.at + normal * (trace_point - target_center).dot(normal)
    };
    bounded(at)?;
    let offset = at - source_origin;
    constraint.offset = [offset.x, offset.y];
    validate(&constraint)?;
    opts.at = at;
    Ok(constraint)
}

fn handle(text: &str) -> Result<Handle> {
    Handle::parse_hex(text).filter(|h| h.0 != 0).ok_or_else(|| err("invalid section projection view handle"))
}

fn policy() -> crate::EngineError {
    err("section projection requires unchanged positive uniform native INSERT scales/orientation on the same millimetre paper layout")
}

fn unchanged(before: &Drawing, old: &SavedView, now: &Drawing, current: &SavedView) -> Result<(Vec2, Vec2)> {
    let (a, old_space, old_origin) = projections::placement(before, old)?;
    let (b, new_space, new_origin) = projections::placement(now, current)?;
    if old_space != new_space || a.block != b.block || (a.scale.x - b.scale.x).abs() > a.scale.x * 1e-10 {
        return Err(policy());
    }
    let old_base = before.block(&a.block).ok_or_else(policy)?.base;
    let new_base = now.block(&b.block).ok_or_else(policy)?.base;
    if old_base != new_base {
        return Err(policy());
    }
    bounded(old_origin)?;
    bounded(new_origin)?;
    Ok((old_origin, new_origin))
}

// A few floating ULPs accommodate subtraction at large origins without a
// percentage tolerance that would permit visible millimetre drift.
fn tolerance(points: &[Vec2]) -> f64 {
    let magnitude = points.iter().fold(1_f64, |a, p| a.max(p.x.abs()).max(p.y.abs())).min(2e9);
    1e-8 + 8. * f64::EPSILON * magnitude
}

fn aligned(c: &Constraint, delta: Vec2, tolerance: f64) -> bool {
    match c.mode {
        Mode::AlongTrace => delta.dot(Vec2::new(c.axis[0], c.axis[1]).perp()).abs() <= tolerance,
        Mode::Fixed => delta.len() <= tolerance,
    }
}

fn paper(d: &Drawing, space: &Space) -> Result<()> {
    let Space::Paper(name) = space else { return Err(policy()) };
    let layout = d.layout(name).ok_or_else(policy)?;
    if cadcraft_render::paper::page_unit_mm(d, &layout.page) != 1. {
        return Err(policy());
    }
    Ok(())
}

fn pair(d: &Drawing, source: &SavedView, target: &SavedView, c: &Constraint, source_scale: f64) -> Result<(Vec2, Vec2)> {
    let (si, ss, so) = projections::placement(d, source)?;
    let (ti, ts, to) = projections::placement(d, target)?;
    if ss != ts
        || (si.scale.x - source_scale).abs() > source_scale * 1e-10
        || (c.mode == Mode::Fixed && (ti.scale.x - si.scale.x).abs() > si.scale.x * 1e-10)
    {
        return Err(policy());
    }
    paper(d, &ss)?;
    bounded(so)?;
    bounded(to)?;
    Ok((so, to))
}

pub(super) fn after_command(s: &mut Session, before: Option<&Arc<Drawing>>, uid: Option<u64>) -> Result<()> {
    let Some(before) = before else { return Ok(()) };
    let state = s.state()?;
    if uid.is_some_and(|uid| uid != state.uid) || Arc::ptr_eq(before, &state.doc) {
        return Ok(());
    }
    let d = s.doc()?;
    if !section_notation::relevant_changes(before, d)? {
        return Ok(());
    }
    let current = load(d)?;
    let previous = load(before)?;
    let views: BTreeMap<_, _> = current.views.iter().map(|v| Ok((handle(&v.handle)?, v))).collect::<Result<_>>()?;
    let old: BTreeMap<_, _> = previous.views.iter().map(|v| Ok((handle(&v.handle)?, v))).collect::<Result<_>>()?;
    let mut moves = Vec::new();
    for target in &current.views {
        let Some(relation) = section_notation::link(target)? else { continue };
        let (Some(source_handle), Some(c)) = (relation.source_view.as_deref(), relation.projection.as_ref()) else { continue };
        let sh = handle(source_handle)?;
        let th = handle(&target.handle)?;
        // Notation owns deletion/EXPLODE cleanup and immutable orphan audit.
        if d.entity(sh).is_none() || d.entity(th).is_none() {
            continue;
        }
        let source = views.get(&sh).ok_or_else(|| err("section projection source record missing"))?;
        let (source_origin, target_origin) = pair(d, source, target, c, relation.source_scale)?;
        let mut additional = Vec2::ZERO;
        if let (Some(old_source), Some(old_target)) = (old.get(&sh), old.get(&th))
            && before.entity(sh).is_some()
            && before.entity(th).is_some()
        {
            let (was_source, _) = unchanged(before, old_source, d, source)?;
            let (was_target, _) = unchanged(before, old_target, d, target)?;
            let p = source_origin - was_source;
            let child = target_origin - was_target;
            let eps = tolerance(&[source_origin, target_origin, was_source, was_target]);
            if p != Vec2::ZERO && child == Vec2::ZERO {
                additional = p;
            } else if !aligned(c, child - p, eps) {
                return Err(err("linked section may move only along its source trace; imposed sections are fixed to their source"));
            }
        }
        let final_origin = target_origin + additional;
        bounded(final_origin)?;
        let offset = Vec2::new(c.offset[0], c.offset[1]);
        if !aligned(c, final_origin - source_origin - offset, tolerance(&[final_origin, source_origin, offset])) {
            return Err(err("section projection origin differs from its immutable source constraint"));
        }
        if additional != Vec2::ZERO {
            // Preflight the actual INSERT coordinate as well as the block origin.
            let EntityKind::Insert(Insert { insert, .. }) = &d.entity(th).ok_or_else(policy)?.kind else { return Err(policy()) };
            bounded(insert.xy() + additional)?;
            moves.push((th, additional));
        }
    }
    // All affected relations are checked before touching any INSERT. The normal
    // guarded transaction also rolls back downstream notation/audit refusals.
    for (h, delta) in moves {
        s.doc_mut()?.modify_entity(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.insert.x += delta.x;
                i.insert.y += delta.y;
            }
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "section_projection_tests.rs"]
mod tests;
