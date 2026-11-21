//! DXF edge paths stay analytic. No fitting or polygonization of authoritative boundaries.
use cadcraft_doc::HatchEdge;
use cadcraft_geom::{Arc, Ellipse, Line, Segment, Spline, Vec2};
use craft_dxf::Tag;

fn take<'a>(tags: &'a [Tag], index: &mut usize, code: i32) -> Option<&'a Tag> {
    let tag = tags.get(*index).filter(|tag| tag.code == code)?;
    *index += 1;
    Some(tag)
}
fn number(tags: &[Tag], index: &mut usize, code: i32) -> Option<f64> {
    let value = take(tags, index, code)?.f64();
    value.is_finite().then_some(value)
}
fn count(tags: &[Tag], index: &mut usize, code: i32) -> Option<usize> {
    usize::try_from(take(tags, index, code)?.i64()).ok().filter(|count| *count <= 100_000)
}
fn point(tags: &[Tag], index: &mut usize, code: i32) -> Option<Vec2> {
    Some(Vec2::new(number(tags, index, code)?, number(tags, index, code + 10)?))
}
fn angles(tags: &[Tag], index: &mut usize) -> Option<(f64, f64, bool)> {
    let start = number(tags, index, 50)?.to_radians();
    let end = number(tags, index, 51)?.to_radians();
    let ccw = take(tags, index, 73)?.i64() != 0;
    // DXF clockwise arcs use complementary angles, with start/end exchanged.
    Some(if ccw { (start, end, true) } else { (std::f64::consts::TAU - end, std::f64::consts::TAU - start, false) })
}
pub(super) fn read(tags: &[Tag], index: &mut usize) -> Option<Vec<HatchEdge>> {
    let count = count(tags, index, 93)?;
    let mut edges = Vec::new();
    let mut stored_points = 0usize;
    for _ in 0..count {
        let kind = take(tags, index, 72)?.i64();
        let edge = match kind {
            1 => HatchEdge::Segment(Segment::Line(Line::new(point(tags, index, 10)?, point(tags, index, 11)?))),
            2 => {
                let center = point(tags, index, 10)?;
                let radius = number(tags, index, 40)?;
                let (start, end, ccw) = angles(tags, index)?;
                HatchEdge::Segment(Segment::Arc { arc: Arc::new(center, radius, start, end), ccw })
            }
            3 => {
                let center = point(tags, index, 10)?;
                let major = point(tags, index, 11)?;
                let ratio = number(tags, index, 40)?;
                let (start, end, ccw) = angles(tags, index)?;
                // HATCH ellipse angles are geometric angles, unlike ELLIPSE's parameters.
                let parameter = |angle: f64| (angle.sin() / ratio).atan2(angle.cos());
                HatchEdge::Ellipse { ellipse: Ellipse { center, major, ratio, start: parameter(start), end: parameter(end) }, ccw }
            }
            4 => HatchEdge::Spline(spline(tags, index, &mut stored_points)?),
            _ => return None,
        };
        if !edge.is_valid() {
            return None;
        }
        edges.push(edge);
    }
    Some(edges)
}
fn spline(tags: &[Tag], index: &mut usize, stored_points: &mut usize) -> Option<Spline> {
    let (mut degree, mut knot_count, mut controls) = (None, None, None);
    let (mut rational, mut periodic) = (false, false);
    let mut knot_values = Vec::new();
    while let Some(tag) = tags.get(*index).filter(|tag| matches!(tag.code, 94 | 73 | 74 | 95 | 96 | 40)) {
        match tag.code {
            94 => degree = Some(count(tags, index, 94)?),
            73 => rational = take(tags, index, 73)?.i64() != 0,
            74 => periodic = take(tags, index, 74)?.i64() != 0,
            95 => knot_count = Some(count(tags, index, 95)?),
            96 => controls = Some(count(tags, index, 96)?),
            40 => {
                if knot_values.len() >= 100_000 {
                    return None;
                }
                knot_values.push(number(tags, index, 40)?);
            }
            _ => return None,
        }
    }
    let degree = degree?;
    let controls = controls?;
    if knot_count? != knot_values.len() {
        return None;
    }
    *stored_points = stored_points.checked_add(knot_values.len())?.checked_add(controls)?;
    if *stored_points > 1_000_000 {
        return None;
    }
    let mut spline = Spline { degree, knots: knot_values, closed: periodic, ..Spline::default() };
    for _ in 0..controls {
        spline.control.push(point(tags, index, 10)?);
        if rational {
            spline.weights.push(number(tags, index, 42)?);
        }
    }
    // DXF 2010+ writes group 97 even for zero fit points. Older files may omit it;
    // the last edge's following group 97 can be the path's source-object count.
    if tags.get(*index).is_some_and(|tag| tag.code == 97) {
        let fit_count = tags.get(*index)?.i64();
        let next = tags.get(*index + 1).map(|tag| tag.code);
        if fit_count == 0 || next == Some(11) {
            let fit_count = count(tags, index, 97)?;
            *stored_points = stored_points.checked_add(fit_count)?;
            if *stored_points > 1_000_000 {
                return None;
            }
            for _ in 0..fit_count {
                spline.fit.push(point(tags, index, 11)?);
            }
        }
    }
    if tags.get(*index).is_some_and(|tag| tag.code == 12) {
        spline.fit_opts.start_tangent = Some(point(tags, index, 12)?);
    }
    if tags.get(*index).is_some_and(|tag| tag.code == 13) {
        spline.fit_opts.end_tangent = Some(point(tags, index, 13)?);
    }
    Some(spline)
}

#[cfg(test)]
#[path = "hatch_edges_tests.rs"]
mod tests;
