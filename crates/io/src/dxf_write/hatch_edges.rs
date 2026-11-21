//! Native edge paths, including rational splines and directed elliptic arcs.
use super::W;
use cadcraft_doc::HatchEdge;
use cadcraft_geom::Segment;

fn angles(w: &mut W, start: f64, end: f64, ccw: bool) {
    let start = cadcraft_geom::norm_angle(start).to_degrees();
    let end = start + cadcraft_geom::ccw_sweep(start.to_radians(), end).to_degrees();
    let (start, end) = if ccw { (start, end) } else { (360.0 - end, 360.0 - start) };
    w.f(50, start);
    w.f(51, end);
    w.i(73, i64::from(ccw));
}
pub(super) fn write(w: &mut W, edge: &HatchEdge) {
    match edge {
        HatchEdge::Segment(Segment::Line(line)) => {
            w.i(72, 1);
            w.p2(10, line.a);
            w.p2(11, line.b);
        }
        HatchEdge::Segment(Segment::Arc { arc, ccw }) => {
            w.i(72, 2);
            w.p2(10, arc.center);
            w.f(40, arc.radius);
            angles(w, arc.start, arc.end, *ccw);
        }
        HatchEdge::Ellipse { ellipse, ccw } => {
            w.i(72, 3);
            w.p2(10, ellipse.center);
            w.p2(11, ellipse.major);
            w.f(40, ellipse.ratio);
            let angle = |parameter: f64| (ellipse.ratio * parameter.sin()).atan2(parameter.cos());
            let start = angle(ellipse.start);
            let end = if ellipse.is_full() { start + std::f64::consts::TAU } else { angle(ellipse.end) };
            angles(w, start, end, *ccw);
        }
        HatchEdge::Spline(spline) => {
            w.i(72, 4);
            w.i(94, spline.degree as i64);
            w.i(73, i64::from(!spline.weights.is_empty()));
            w.i(74, i64::from(spline.closed));
            w.i(95, spline.knots.len() as i64);
            w.i(96, spline.control.len() as i64);
            for knot in &spline.knots {
                w.f(40, *knot);
            }
            for (index, point) in spline.control.iter().enumerate() {
                w.p2(10, *point);
                if let Some(weight) = spline.weights.get(index) {
                    w.f(42, *weight);
                }
            }
            w.i(97, spline.fit.len() as i64);
            for point in &spline.fit {
                w.p2(11, *point);
            }
            if let Some(tangent) = spline.fit_opts.start_tangent {
                w.p2(12, tangent);
            }
            if let Some(tangent) = spline.fit_opts.end_tangent {
                w.p2(13, tangent);
            }
        }
    }
}
