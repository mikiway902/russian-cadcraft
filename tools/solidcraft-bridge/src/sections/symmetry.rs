//! Native-curve reflection comparison. No sampled outlines or fitted shape recognition.
use super::{Budget, Error, Loop2, MAX_COORD, Region2, Seg2, Seg2Geom, Vec2, watch};
use serde::{Deserialize, Serialize};
use solidcraft_geom::{ArcMap, Vec2Geom};
use std::{cell::Cell, sync::Arc};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Report {
    axis: [f64; 2],
    center: [f64; 2],
    symmetric: bool,
    tolerance_mm: f64,
}

pub(super) fn validate_axis(axis: [f64; 2]) -> Result<(), Error> {
    if axis.iter().any(|v| !v.is_finite()) || (axis[0].hypot(axis[1]) - 1.).abs() > 1e-9 {
        return Err(Error::Parameters("symmetry_axis must be a finite unit direction in native cap XY (length tolerance 1e-9)".into()));
    }
    Ok(())
}

pub(super) fn classify(region: &Region2, axis: [f64; 2], budget: &Arc<Budget>) -> Result<Report, Error> {
    // Directional extrema, not the world-XY bbox midpoint: the latter need not
    // lie on an oblique symmetry axis of a non-centrally-symmetric figure.
    let length = axis[0].hypot(axis[1]);
    let along = Vec2::new(axis[0] / length, axis[1] / length);
    let normal = Vec2::new(-along.y, along.x);
    let (min, max) = outer_bounds(&region.outer, budget, along)?;
    let local_center = (min + max) * 0.5;
    let center = along * local_center.x + normal * local_center.y;
    if [center.x, center.y].iter().any(|v| !v.is_finite() || v.abs() > MAX_COORD) {
        return Err(Error::Geometry("section symmetry center is outside finite native cap coordinates".into()));
    }
    let scale = Cell::new(0_f64);
    let reflect = |p: Vec2| {
        let q = p - normal * (2. * (p - center).dot(normal));
        scale.set(scale.get().max(p.x.abs()).max(p.y.abs()).max(q.x.abs()).max(q.y.abs()));
        q
    };
    let arc = ArcMap { angle: 2. * axis[1].atan2(axis[0]), mirrored: true, scale: 1. };
    let mut reflect_loop = |ring: &Loop2| -> Result<Loop2, Error> {
        let mut segs = Vec::with_capacity(ring.segs.len());
        // Reflection reverses winding; reverse both loop order and each segment's
        // traversal to keep the original outer-CCW / hole-CW material convention.
        for segment in ring.segs.iter().rev() {
            budget.check()?;
            if let Seg2::Arc { center, radius, .. } = segment {
                scale.set(scale.get().max(center.x.abs() + radius).max(center.y.abs() + radius));
            }
            let reflected = segment.mapped(reflect, arc).reversed();
            // Reflected controls may legitimately lie outside the source's +/-1e6
            // box. They must still be finite before entering the exact boolean.
            reflected.check(f64::MAX).map_err(|why| Error::Geometry(format!("invalid reflected cap segment: {why}")))?;
            segs.push(reflected);
        }
        Ok(Loop2 { segs })
    };
    let reflected = Region2 { outer: reflect_loop(&region.outer)?, holes: region.holes.iter().map(&mut reflect_loop).collect::<Result<_, _>>()? };
    let tolerance_mm = (32. * f64::EPSILON * scale.get()).max(1e-9);
    if !tolerance_mm.is_finite() || tolerance_mm > 1e-6 {
        return Err(Error::Limit("section symmetry comparison tolerance"));
    }
    budget.check()?;
    let difference = watch::watching(budget.clone(), || {
        solidcraft_kernel::planar_regions_boolean(
            std::slice::from_ref(region),
            std::slice::from_ref(&reflected),
            solidcraft_kernel::BoolOp::Cut,
            tolerance_mm,
        )
    })
    .map_err(|e| {
        if budget.check().is_err() {
            Error::Limit("section symmetry boolean work/deadline")
        } else {
            Error::Geometry(format!("section cap reflection difference: {e}"))
        }
    })?;
    budget.check()?;
    // Reflection is involutive and preserves material measure: one empty directed
    // difference suffices, without a redundant reverse boolean.
    Ok(Report { axis, center: [center.x, center.y], symmetric: difference.is_empty(), tolerance_mm })
}

// Seg2Geom::bounds samples noncircular curves at 1e-3, which can move the axis
// enough to break a real native ellipse. Bound the exact native parameterization
// by its endpoints and derivative roots instead; no polyline or conic fitting.
fn outer_bounds(ring: &Loop2, budget: &Budget, along: Vec2) -> Result<(Vec2, Vec2), Error> {
    let mut min = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut max = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut finite = true;
    let mut add = |p: Vec2| {
        finite &= p.x.is_finite() && p.y.is_finite();
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    };
    let normal = Vec2::new(-along.y, along.x);
    let rotation = ArcMap { angle: -along.y.atan2(along.x), mirrored: false, scale: 1. };
    for segment in &ring.segs {
        budget.check()?;
        let projected;
        let segment = if along == Vec2::new(1., 0.) {
            segment
        } else {
            projected = segment.mapped(|p| Vec2::new(p.dot(along), p.dot(normal)), rotation);
            &projected
        };
        match segment {
            Seg2::Line { a, b } => {
                add(*a);
                add(*b);
            }
            Seg2::Arc { .. } => {
                let bounds = segment.bounds(); // Analytic cardinal extrema for arcs.
                add(bounds.min);
                add(bounds.max);
            }
            _ => {
                add(segment.start());
                add(segment.end());
                let coordinates: [fn(Vec2) -> f64; 2] = [|p| p.x, |p| p.y];
                for coordinate in coordinates {
                    let mut derivative = [0.; solidcore_record::MAX_BEZIER_CTRL];
                    let degree = match *segment {
                        Seg2::Conic { a, apex, b, w } => {
                            // N'D-ND' in quadratic Bernstein form (constant factor omitted).
                            derivative[0] = w * (coordinate(apex) - coordinate(a));
                            derivative[1] = (coordinate(b) - coordinate(a)) * 0.5;
                            derivative[2] = w * (coordinate(b) - coordinate(apex));
                            2
                        }
                        Seg2::Cubic { p0, p1, p2, p3 } => {
                            derivative[0] = coordinate(p1) - coordinate(p0);
                            derivative[1] = coordinate(p2) - coordinate(p1);
                            derivative[2] = coordinate(p3) - coordinate(p2);
                            2
                        }
                        Seg2::Bezier { n, ref p } => {
                            for (i, pair) in p[..=usize::from(n)].windows(2).enumerate() {
                                derivative[i] = coordinate(pair[1]) - coordinate(pair[0]);
                            }
                            usize::from(n) - 1
                        }
                        Seg2::Line { .. } | Seg2::Arc { .. } => {
                            return Err(Error::Geometry("native line/arc extrema must use their analytic bounds".into()));
                        }
                    };
                    if derivative[..=degree].iter().any(|v| !v.is_finite()) {
                        return Err(Error::Geometry("section symmetry native derivative is not finite".into()));
                    }
                    let mut roots = [0.; solidcore_record::MAX_BEZIER_CTRL];
                    let count = roots_between(&derivative[..=degree], &mut roots);
                    for t in &roots[..count] {
                        add(native_point(segment, *t));
                    }
                }
            }
        }
    }
    if !finite || [min.x, min.y, max.x, max.y].iter().any(|v| !v.is_finite()) {
        return Err(Error::Geometry("section symmetry outer bounds are not finite".into()));
    }
    Ok((min, max))
}

fn native_point(segment: &Seg2, t: f64) -> Vec2 {
    if let Seg2::Bezier { n, p } = segment {
        // Upstream point_at builds subdivision vectors even when only a point is
        // needed. The bounded-degree de Casteljau evaluation needs only this stack.
        let mut work = *p;
        for remaining in (1..=usize::from(*n)).rev() {
            for i in 0..remaining {
                work[i] = work[i] * (1. - t) + work[i + 1] * t;
            }
        }
        work[0]
    } else {
        segment.point_at(t)
    }
}

fn evaluate(coefficients: &[f64], t: f64) -> f64 {
    let mut work = [0.; solidcore_record::MAX_BEZIER_CTRL];
    work[..coefficients.len()].copy_from_slice(coefficients);
    for remaining in (1..coefficients.len()).rev() {
        for i in 0..remaining {
            work[i] = work[i] * (1. - t) + work[i + 1] * t;
        }
    }
    work[0]
}

// Isolate a Bernstein polynomial's roots by its derivative's roots. Each interval
// is monotone; bisection is numerical root solving, not geometry sampling. The
// degree is at most eight, with fixed-size stack storage and bounded work.
fn roots_between(coefficients: &[f64], roots: &mut [f64; solidcore_record::MAX_BEZIER_CTRL]) -> usize {
    if coefficients.len() < 2 || coefficients.iter().all(|c| *c == 0.) {
        return 0;
    }
    let mut derivative = [0.; solidcore_record::MAX_BEZIER_CTRL];
    for (i, pair) in coefficients.windows(2).enumerate() {
        derivative[i] = pair[1] - pair[0];
    }
    let mut critical = [0.; solidcore_record::MAX_BEZIER_CTRL];
    let count = roots_between(&derivative[..coefficients.len() - 1], &mut critical);
    let mut lo = 0.;
    let mut value_lo = coefficients[0];
    let mut found = 0;
    for hi in critical[..count].iter().copied().chain(std::iter::once(1.)) {
        let value_hi = evaluate(coefficients, hi);
        if value_lo != 0. && value_hi != 0. && value_lo.is_sign_negative() != value_hi.is_sign_negative() {
            let (mut left, mut right) = (lo, hi);
            for _ in 0..64 {
                let middle = (left + right) * 0.5;
                if evaluate(coefficients, middle).is_sign_negative() == value_lo.is_sign_negative() {
                    left = middle;
                } else {
                    right = middle;
                }
            }
            roots[found] = (left + right) * 0.5;
            found += 1;
        }
        if value_hi == 0. && hi < 1. {
            roots[found] = hi;
            found += 1;
        }
        lo = hi;
        value_lo = value_hi;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    #[test]
    fn native_conic_extrema_are_independent_of_asymmetric_segmentation() {
        let center = Vec2::new(13., -4.);
        let mut start = 0.23_f64;
        let mut segs = Vec::new();
        for sweep in [0.7_f64, 1.1, 2., TAU - 3.8] {
            let end = start + sweep;
            let mid = start + sweep * 0.5;
            let w = (sweep * 0.5).cos();
            let point = |t: f64| center + Vec2::new(7. * t.cos(), 3. * t.sin());
            segs.push(Seg2::Conic { a: point(start), apex: center + Vec2::new(7. * mid.cos() / w, 3. * mid.sin() / w), b: point(end), w });
            start = end;
        }
        let (min, max) = outer_bounds(&Loop2 { segs }, &Budget::new(), Vec2::new(1., 0.)).unwrap();
        assert!((min.x - 6.).abs() < 1e-12 && (max.x - 20.).abs() < 1e-12, "{min:?} {max:?}");
        assert!((min.y + 7.).abs() < 1e-12 && (max.y + 1.).abs() < 1e-12, "{min:?} {max:?}");
    }

    #[test]
    fn cubic_and_high_degree_native_bounds_use_derivative_extrema() {
        let cubic = Seg2::Cubic { p0: Vec2::ZERO, p1: Vec2::new(1., 2.), p2: Vec2::new(2., 2.), p3: Vec2::new(3., 0.) };
        let mut p = [Vec2::ZERO; solidcore_record::MAX_BEZIER_CTRL];
        for (i, y) in [0., 2., 2., 2., 0.].into_iter().enumerate() {
            p[i] = Vec2::new(i as f64, y);
        }
        for (segment, expected_x, expected_y) in [(cubic, 3., 1.5), (Seg2::Bezier { n: 4, p }, 4., 1.75)] {
            let (min, max) = outer_bounds(&Loop2 { segs: vec![segment] }, &Budget::new(), Vec2::new(1., 0.)).unwrap();
            assert_eq!(min, Vec2::ZERO);
            assert!((max.x - expected_x).abs() < 1e-12 && (max.y - expected_y).abs() < 1e-12, "{max:?}");
        }
    }
}
