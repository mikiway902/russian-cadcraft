//! Exact DXF HATCH edge paths. Tessellation is a display operation, never stored geometry.
use super::{HatchLoop, Prim};
use cadcraft_geom::{Bounds2, Ellipse, Line, Mat3, Segment, Spline, Vec2, bulge_to_arc};
use serde::{Deserialize, Serialize};

/// A directed boundary edge. Ellipse parameters describe the CCW geometric arc; `ccw`
/// controls traversal, as for `Segment::Arc`. Spline traversal is its knot-domain order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HatchEdge {
    Segment(Segment),
    Ellipse { ellipse: Ellipse, ccw: bool },
    Spline(Spline),
}
impl HatchEdge {
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Segment(Segment::Line(line)) => line.a.is_finite() && line.b.is_finite(),
            Self::Segment(Segment::Arc { arc, .. }) => {
                arc.center.is_finite() && arc.radius.is_finite() && arc.radius > 0.0 && arc.start.is_finite() && arc.end.is_finite()
            }
            Self::Ellipse { ellipse, .. } => {
                ellipse.center.is_finite()
                    && ellipse.major.is_finite()
                    && ellipse.major.len() > 0.0
                    && ellipse.ratio.is_finite()
                    && ellipse.ratio > 0.0
                    && ellipse.ratio <= 1.0
                    && ellipse.start.is_finite()
                    && ellipse.end.is_finite()
            }
            Self::Spline(spline) => {
                spline.is_valid()
                    && spline.domain().0 < spline.domain().1
                    && spline.knots.iter().all(|v| v.is_finite())
                    && spline.control.iter().chain(&spline.fit).all(|p| p.is_finite())
                    && (spline.weights.is_empty() || spline.weights.len() == spline.control.len())
                    && spline.weights.iter().all(|v| v.is_finite())
            }
        }
    }
    pub fn primitive(&self) -> Prim {
        match self {
            Self::Segment(segment) => Prim::Seg(*segment),
            Self::Ellipse { ellipse, .. } => Prim::Ellipse(*ellipse),
            Self::Spline(spline) => Prim::Spline(spline.clone()),
        }
    }
    fn bounds(&self) -> Bounds2 {
        match self {
            Self::Segment(segment) => segment.bounds(),
            Self::Ellipse { ellipse, .. } => ellipse.bounds(),
            Self::Spline(spline) => spline.bounds(),
        }
    }
    fn metrics(&self, origin: Vec2) -> (f64, f64) {
        let [start, end] = self.endpoints();
        match self {
            Self::Segment(Segment::Line(line)) => ((line.a - origin).cross(line.b - origin), line.len()),
            Self::Segment(Segment::Arc { arc, ccw }) => {
                let signed_sweep = if *ccw { arc.sweep() } else { -arc.sweep() };
                ((arc.center - origin).cross(end - start) + arc.radius * arc.radius * signed_sweep, arc.len())
            }
            Self::Ellipse { ellipse, ccw } => {
                let sign = if *ccw { 1.0 } else { -1.0 };
                (
                    (ellipse.center - origin).cross(end - start) + sign * ellipse.major.cross(ellipse.minor()) * ellipse.sweep(),
                    ellipse.arc_length_from(ellipse.start, ellipse.sweep()),
                )
            }
            Self::Spline(spline) => {
                // Same span-aware numerical area policy as AREA on a native SPLINE.
                let bounds = spline.bounds();
                let points = spline.tessellate_spans((bounds.min.dist(bounds.max) * 1e-7).max(1e-12));
                let closing = points.last().zip(points.first()).map_or(0.0, |(b, a)| (*b - origin).cross(*a - origin));
                (cadcraft_geom::shoelace(&points) * 2.0 - closing, spline.checked_length().unwrap_or(f64::INFINITY))
            }
        }
    }
    fn endpoints(&self) -> [Vec2; 2] {
        match self {
            Self::Segment(segment) => [segment.start(), segment.end()],
            Self::Ellipse { ellipse, ccw } => {
                let mut ends = [ellipse.at_param(ellipse.start), ellipse.at_param(ellipse.end)];
                if !ccw {
                    ends.swap(0, 1);
                }
                ends
            }
            Self::Spline(spline) => {
                let (lo, hi) = spline.domain();
                [spline.eval(lo), spline.eval(hi)]
            }
        }
    }
    fn tessellate(&self, tol: f64, out: &mut Vec<Vec2>) {
        match self {
            Self::Segment(Segment::Line(line)) => out.extend([line.a, line.b]),
            Self::Segment(Segment::Arc { arc, ccw }) => {
                let start = out.len();
                arc.tessellate(tol, out);
                if !ccw && let Some(points) = out.get_mut(start..) {
                    points.reverse();
                }
            }
            Self::Ellipse { ellipse, ccw } => {
                let start = out.len();
                ellipse.tessellate(tol, out);
                if !ccw && let Some(points) = out.get_mut(start..) {
                    points.reverse();
                }
            }
            Self::Spline(spline) => spline.tessellate_into(tol, out),
        }
    }
    fn append_within(&self, tol: f64, budget: &mut usize, points: &mut Vec<Vec2>) {
        let [start, end] = self.endpoints();
        if points.last().is_some_and(|p| p.near(start, 1e-9)) {
            points.pop();
        }
        let before = points.len();
        if *budget > 0 {
            self.tessellate(tol, points);
            *budget = budget.saturating_sub(points.len() - before);
        } else {
            points.extend([start, end]);
        }
    }
    fn transform(&mut self, matrix: &Mat3, similarity: Option<f64>, mirror: bool) {
        match self {
            Self::Segment(Segment::Line(line)) => {
                line.a = matrix.apply(line.a);
                line.b = matrix.apply(line.b);
            }
            Self::Segment(Segment::Arc { arc, ccw }) => {
                if let Some(scale) = similarity {
                    let center = matrix.apply(arc.center);
                    let a = matrix.apply(arc.start_point()) - center;
                    let b = matrix.apply(arc.end_point()) - center;
                    let (start, end) = if mirror { (b.angle(), a.angle()) } else { (a.angle(), b.angle()) };
                    *arc = cadcraft_geom::Arc::new(center, arc.radius * scale, start, end);
                    *ccw ^= mirror;
                } else {
                    let ellipse = Ellipse { center: arc.center, major: Vec2::new(arc.radius, 0.0), ratio: 1.0, start: arc.start, end: arc.end }
                        .transformed(matrix);
                    *self = Self::Ellipse { ellipse, ccw: *ccw ^ mirror };
                }
            }
            Self::Ellipse { ellipse, ccw } => {
                *ellipse = ellipse.transformed(matrix);
                *ccw ^= mirror;
            }
            Self::Spline(spline) => {
                for point in spline.control.iter_mut().chain(&mut spline.fit) {
                    *point = matrix.apply(*point);
                }
                if let Some(tangent) = &mut spline.fit_opts.start_tangent {
                    *tangent = matrix.apply_vec(*tangent);
                }
                if let Some(tangent) = &mut spline.fit_opts.end_tangent {
                    *tangent = matrix.apply_vec(*tangent);
                }
            }
        }
    }
    pub fn point_count(&self) -> usize {
        match self {
            Self::Segment(_) | Self::Ellipse { .. } => 2,
            Self::Spline(spline) => spline.control.len().saturating_add(spline.fit.len()),
        }
    }
}
impl HatchLoop {
    fn polyline_segments(&self) -> impl Iterator<Item = Segment> + '_ {
        self.vertices.iter().zip(self.vertices.iter().cycle().skip(1)).take(self.vertices.len()).filter_map(|(a, b)| {
            if a.p.near(b.p, cadcraft_geom::EPS) && a.bulge.abs() < 1e-12 {
                return None;
            }
            Some(match bulge_to_arc(a.p, b.p, a.bulge) {
                Some((arc, ccw)) => Segment::Arc { arc, ccw },
                None => Segment::Line(Line::new(a.p, b.p)),
            })
        })
    }
    pub fn bounds(&self) -> Bounds2 {
        if self.edges.is_empty() {
            self.polyline_segments().fold(Bounds2::from_points(self.vertices.iter().map(|v| v.p)), |bounds, segment| bounds.union(&segment.bounds()))
        } else {
            self.edges.iter().fold(Bounds2::EMPTY, |bounds, edge| bounds.union(&edge.bounds()))
        }
    }
    /// Signed Green-integral area and perimeter. Analytic line/arc/ellipse terms;
    /// splines use the same span-aware numerical integration policy as native SPLINE inquiry.
    pub fn area_and_perimeter(&self) -> (f64, f64) {
        let origin = self.edges.first().map_or_else(|| self.vertices.first().map_or(Vec2::ZERO, |v| v.p), |edge| edge.endpoints()[0]);
        let mut total = (0.0, 0.0);
        let mut add = |edge: &HatchEdge| {
            let (twice_area, length) = edge.metrics(origin);
            total.0 += twice_area * 0.5;
            total.1 += length;
        };
        if self.edges.is_empty() {
            for segment in self.polyline_segments() {
                add(&HatchEdge::Segment(segment));
            }
        } else {
            for edge in &self.edges {
                add(edge);
            }
        }
        total
    }
    /// Boundary primitives stay analytic for snapping, selection and DXF exchange.
    pub fn primitives(&self) -> Vec<Prim> {
        if self.edges.is_empty() { self.polyline_segments().map(Prim::Seg).collect() } else { self.edges.iter().map(HatchEdge::primitive).collect() }
    }
    pub fn point_count(&self) -> usize {
        self.vertices.len().saturating_add(self.edges.iter().map(HatchEdge::point_count).sum::<usize>())
    }
    /// Shared display budget follows Polyline: after exhaustion, remaining curves use chords.
    /// The exact edge records are untouched, including hole boundaries.
    pub fn tessellate_within(&self, tol: f64, budget: &mut usize) -> Vec<Vec2> {
        let mut points = Vec::new();
        if self.edges.is_empty() {
            for segment in self.polyline_segments() {
                HatchEdge::Segment(segment).append_within(tol, budget, &mut points);
            }
            if points.is_empty() {
                points.extend(self.vertices.first().map(|v| v.p));
            }
        } else {
            for edge in &self.edges {
                edge.append_within(tol, budget, &mut points);
            }
        }
        points
    }
    pub fn transform(&mut self, matrix: &Mat3) {
        let similarity = matrix.similarity_scale();
        let mirror = matrix.is_mirroring();
        if similarity.is_none() && self.vertices.iter().any(|vertex| vertex.bulge != 0.0) {
            self.edges = self.polyline_segments().map(HatchEdge::Segment).collect();
            self.vertices = Vec::new();
        }
        for vertex in &mut self.vertices {
            vertex.p = matrix.apply(vertex.p);
            if mirror {
                vertex.bulge = -vertex.bulge;
            }
        }
        for edge in &mut self.edges {
            edge.transform(matrix, similarity, mirror);
        }
    }
}
