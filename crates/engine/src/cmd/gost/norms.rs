//! Read-only drafting measurements, not a conformity certificate. Geometry stays in its
//! owning space; block definitions and viewport projections are deliberately not flattened.

use std::borrow::Cow;
use std::collections::BTreeMap;

use cadcraft_doc::{DimKind, Dimension, Drawing, Entity, EntityKind, EntityStore, Handle, Prim};
use cadcraft_geom::{Arc, Bounds2, Circle, Line, Segment, Vec2, Vec3};
use cadcraft_render::LineRole;
use serde_json::{Value, json};

const MAX_ENTITIES: usize = 4096;
const MAX_PRIMITIVES: usize = 8192;
const MAX_PAIRS: usize = 100_000;
const MAX_FINDINGS: usize = 256;
const MAX_SPACES: usize = 64;
const EPS: f64 = 1e-7;
// Keeps fourth-order intersection arithmetic finite and submillimetre comparisons meaningful.
const MAX_COORD: f64 = 1e9;

#[derive(Clone, Copy)]
enum Curve {
    Line(Line),
    Circle(Circle),
    Arc(Arc),
}

impl Curve {
    fn bounds(self) -> Bounds2 {
        match self {
            Self::Line(l) => Bounds2::from_points([l.a, l.b]),
            Self::Circle(c) => c.bounds(),
            Self::Arc(a) => a.bounds(),
        }
    }

    fn endpoint(self, p: Vec2) -> bool {
        match self {
            Self::Line(l) => near(l.a, p) || near(l.b, p),
            Self::Arc(a) => near(a.start_point(), p) || near(a.end_point(), p),
            Self::Circle(_) => false,
        }
    }
}

struct Item {
    owner: Handle,
    curve: Curve,
    bounds: Bounds2,
    dimension: bool,
    // Only actual, visible linear dimension-line fragments participate in spacing.
    paper_scale: Option<f64>,
    anchors: Option<[Vec2; 2]>,
    stroke_ends: Option<[Vec2; 2]>,
}

impl Item {
    fn endpoint(&self, p: Vec2) -> bool {
        match self.stroke_ends {
            Some([a, b]) => near(a, p) || near(b, p),
            None => self.curve.endpoint(p),
        }
    }
}

#[derive(Default)]
struct Analysis {
    findings: Vec<Value>,
    incomplete: bool,
    exhausted: bool,
    entities: usize,
    primitives: usize,
    pairs: usize,
}

impl Analysis {
    fn emit(&mut self, finding: Value) {
        // Reserve one slot for a final, unambiguous incomplete-analysis warning.
        if self.findings.len() < MAX_FINDINGS - 1 {
            self.findings.push(finding);
        } else {
            self.incomplete = true;
            self.exhausted = true;
        }
    }

    fn omitted(&mut self, space: &str, owner: Handle, reason: &str) {
        self.incomplete = true;
        self.emit(finding(
            "warning",
            "geometry.scope",
            space,
            &[owner],
            Bounds2::from_points([Vec2::ZERO]),
            reason,
            (json!("not analysed"), json!("manual review or supported planar geometry")),
        ));
    }
}

/// Measurable drafting checks. `true` means some analysis was omitted or resource-limited;
/// a consumer must never present that result as a pass. Findings are stable in space/draw
/// order, with bounded storage, entity expansion, pair work and result counts.
pub(super) fn inspect(d: &Drawing) -> (Vec<Value>, bool) {
    let mut a = Analysis::default();
    a.emit(finding("info", "geometry.scope", "Model", &[], Bounds2::from_points([Vec2::ZERO]), "Drafting aid, not certification: planar native line/circle/arc/polyline centrelines and rendered dimension/extension strokes, excluding text and text marks. Spacing covers parallel visible linear/aligned dimension fragments. Model/layout spaces are independent; nested inserts, stored dimension blocks and viewport projections are not expanded. Model paper distances use effective dimension/annotation scale; layouts use paper units. Coordinates are limited to +/-1e9 drawing units in the XY plane at Z=0. Visibility, stroke width and occlusion are outside this check.", (json!("bounded geometric measurements"), json!("engineering review"))));
    inspect_space(d, &d.model, "Model", None, &mut a);
    for layout in d.layouts.iter().take(MAX_SPACES - 1) {
        if a.exhausted {
            break;
        }
        if layout.name.len() > 256 {
            a.incomplete = true;
            a.emit(finding(
                "warning",
                "geometry.scope",
                "Model",
                &[],
                Bounds2::from_points([Vec2::ZERO]),
                "A layout name exceeds 256 bytes; that layout is not analysed.",
                (json!("layout omitted"), json!("layout name at most 256 bytes")),
            ));
            continue;
        }
        let paper_mm = if layout.page.paper_mm || d.header.i64("MEASUREMENT", 0) == 1 { 1.0 } else { 25.4 };
        inspect_space(d, &layout.entities, &layout.name, Some(paper_mm), &mut a);
    }
    if d.layouts.len() >= MAX_SPACES {
        a.incomplete = true;
    }
    if a.incomplete {
        a.findings.push(finding("warning", "geometry.analysis_incomplete", "Model", &[], Bounds2::from_points([Vec2::ZERO]), "Analysis is incomplete: see scope warnings. Resource ceilings are 4096 entities, 8192 primitives, 100000 primitive pairs, 64 spaces and 256 findings. No passing or certified result is implied.", (json!({"entities": a.entities, "primitives": a.primitives, "pairs": a.pairs}), json!("complete analysis plus manual review"))));
    }
    (a.findings, a.incomplete)
}

fn inspect_space(d: &Drawing, store: &EntityStore, space: &str, paper_mm: Option<f64>, a: &mut Analysis) {
    let mut items = Vec::new();
    let mut unsupported: BTreeMap<&str, (usize, Handle)> = BTreeMap::new();
    for e in store.iter() {
        if a.exhausted || a.entities >= MAX_ENTITIES || a.primitives >= MAX_PRIMITIVES {
            a.incomplete = true;
            a.exhausted = true;
            break;
        }
        a.entities += 1;
        if !planar(e) {
            a.omitted(space, e.handle, "Non-planar, tilted, non-finite or out-of-range geometry is not projected into a drafting defect.");
            continue;
        }
        match &e.kind {
            EntityKind::Line(l) => push_curve(&mut items, e.handle, Curve::Line(Line::new(l.a.xy(), l.b.xy())), space, a),
            EntityKind::Circle(c) if valid_radius(c.radius) => {
                push_curve(&mut items, e.handle, Curve::Circle(Circle::new(c.center.xy(), c.radius)), space, a);
            }
            EntityKind::Arc(c) if valid_radius(c.radius) && valid_number(c.start) && valid_number(c.end) => {
                push_curve(&mut items, e.handle, Curve::Arc(Arc::new(c.center.xy(), c.radius, c.start, c.end)), space, a);
            }
            EntityKind::Circle(_) | EntityKind::Arc(_) => a.omitted(space, e.handle, "Invalid circular radius or angle; geometry not analysed."),
            EntityKind::LwPolyline(p) if p.vertices.len() <= 256 && p.vertices.iter().all(|v| valid_point(v.p) && valid_number(v.bulge)) => {
                for prim in e.kind.prims() {
                    if let Prim::Seg(s) = prim {
                        let curve = match s {
                            Segment::Line(l) => Curve::Line(l),
                            Segment::Arc { arc, .. } => Curve::Arc(arc),
                        };
                        push_curve(&mut items, e.handle, curve, space, a);
                    }
                }
            }
            EntityKind::Polyline3d(p) if p.points.len() <= 256 => {
                for prim in e.kind.prims() {
                    if let Prim::Seg(Segment::Line(l)) = prim {
                        push_curve(&mut items, e.handle, Curve::Line(l), space, a);
                    }
                }
            }
            EntityKind::Dimension(dm) => dimension_items(d, e, dm, space, paper_mm, &mut items, a),
            // These have no contour in this deliberately geometry-only check.
            EntityKind::Point(_) | EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::AttDef(_) => {}
            // Native sheet geometry is checked structurally by profile::check_sheets,
            // not as part contours (stamp grid intersections are intentional).
            EntityKind::Insert(i)
                if paper_mm.is_some()
                    && i.block.starts_with("GOST-SHEET-")
                    && d.block(&i.block).is_some_and(|b| b.description.starts_with("GOST R 2.104-2023 form ")) => {}
            _ => {
                let entry = unsupported.entry(e.kind.type_name()).or_insert((0, e.handle));
                entry.0 += 1;
            }
        }
    }
    for (kind, (count, handle)) in unsupported {
        a.omitted(
            space,
            handle,
            &format!("{count} {kind} object(s) not expanded/analysed; nested blocks and unrelated definitions are never compared."),
        );
    }
    for (i, x) in items.iter().enumerate() {
        for y in items.iter().skip(i + 1) {
            if a.exhausted || a.pairs >= MAX_PAIRS {
                a.incomplete = true;
                a.exhausted = true;
                return;
            }
            a.pairs += 1;
            if x.owner == y.owner && (x.dimension || y.dimension) {
                continue;
            }
            let bounds = x.bounds.union(&y.bounds);
            if !x.dimension && !y.dimension {
                if let Some((code, actual)) = overlap(x.curve, y.curve) {
                    a.emit(finding(
                        "warning",
                        code,
                        space,
                        &[x.owner, y.owner],
                        bounds,
                        "Coincident geometry has a positive-length overlap; review duplicate or overdrawn contours.",
                        (json!({"overlapDrawingUnits": actual}), json!("no unintended positive-length overlap")),
                    ));
                }
                continue;
            }
            if let (Some(sx), Some(sy), Curve::Line(lx), Curve::Line(ly)) = (x.paper_scale, y.paper_scale, x.curve, y.curve)
                && let Some(distance) = parallel_distance(lx, ly)
            {
                // Different effective scales have no single paper-distance interpretation.
                if (sx - sy).abs() <= EPS * sx.max(sy) {
                    let mm = distance / sx;
                    if mm < 7.0 - EPS {
                        a.emit(finding(
                            "warning",
                            "dimension.spacing",
                            space,
                            &[x.owner, y.owner],
                            bounds,
                            "Parallel visible dimension lines are less than 7 mm apart on paper.",
                            (json!({"paperMm": mm, "drawingDistance": distance, "drawingUnitsPerPaperMm": sx}), json!({"minimumPaperMm": 7.0})),
                        ));
                    }
                } else {
                    a.incomplete = true;
                    a.emit(finding(
                        "warning",
                        "dimension.scale_mismatch",
                        space,
                        &[x.owner, y.owner],
                        bounds,
                        "Parallel dimension lines have different effective scales; their paper spacing needs manual review.",
                        (json!([sx, sy]), json!("a common paper scale")),
                    ));
                }
            }
            if let Some(p) = crossing(x, y) {
                a.emit(finding(
                    "warning",
                    "dimension.intersection",
                    space,
                    &[x.owner, y.owner],
                    bounds,
                    "A rendered dimension/extension stroke intersects another contour away from legitimate shared endpoints and extension anchors.",
                    (json!({"point": [p.x, p.y]}), json!("no unintended interior intersection")),
                ));
            }
        }
    }
}

fn push_curve(items: &mut Vec<Item>, owner: Handle, curve: Curve, space: &str, a: &mut Analysis) {
    if a.primitives >= MAX_PRIMITIVES {
        a.incomplete = true;
        a.exhausted = true;
        return;
    }
    if !valid_curve(curve) {
        a.omitted(space, owner, "A contour segment is degenerate, non-finite or outside safe coordinate/radius limits.");
        return;
    }
    a.primitives += 1;
    items.push(Item { owner, curve, bounds: curve.bounds(), dimension: false, paper_scale: None, anchors: None, stroke_ends: None });
}

fn dimension_items(d: &Drawing, e: &Entity, dm: &Dimension, space: &str, paper_mm: Option<f64>, items: &mut Vec<Item>, a: &mut Analysis) {
    if dm.block.is_some() {
        a.omitted(space, e.handle, "Dimension uses a stored anonymous geometry block; nested/stored block geometry is not expanded.");
        return;
    }
    if d.dim_styles.len() > 256 || d.text_styles.len() > 256 || d.scales.len() > 1000 || !bounded_dimension(dm) || e.common.anno_scales.len() > 64 {
        a.omitted(space, e.handle, "Dimension/style input exceeds bounded rendering limits or contains unusable numbers.");
        return;
    }
    let default_style = cadcraft_doc::DimStyle::default();
    let base = d.dim_style(&dm.style).unwrap_or(&default_style);
    if !bounded_style(base) {
        a.omitted(space, e.handle, "Dimension style is non-finite or exceeds bounded rendering limits.");
        return;
    }
    let mut dimension = Cow::Borrowed(dm);
    if !e.common.anno_scales.is_empty() {
        let view = if paper_mm.is_some() { 1.0 } else { d.current_anno_scale().factor() };
        match cadcraft_doc::anno_draw(e, view, paper_mm.is_some() || d.anno_all_visible()) {
            cadcraft_doc::AnnoDraw::Scaled(f) if f > 0.0 && f <= 1e6 => {
                dimension.to_mut().overrides.insert("scale".into(), json!(f));
            }
            _ => {
                a.omitted(space, e.handle, "Annotative dimension is hidden at the current scale or its scale is unusable.");
                return;
            }
        }
    }
    let style = base.with_overrides(&dimension.overrides);
    if !bounded_style(&style) {
        a.omitted(space, e.handle, "Overridden dimension style exceeds bounded rendering limits.");
        return;
    }
    if d.text_style(&style.text_style).is_some_and(|s| s.name.len() > 256 || s.font.len() > 256 || s.big_font.len() > 256) {
        a.omitted(space, e.handle, "Dimension font name exceeds bounded rendering limits.");
        return;
    }
    let font = cadcraft_render::dim_text(d, &style);
    if ![font.fixed_height, font.width_factor, font.oblique].into_iter().all(valid_number) {
        a.omitted(space, e.handle, "Dimension font metrics contain unusable numbers.");
        return;
    }
    let dimscale = d.header.f64("DIMSCALE", 1.0);
    let scale = style.effective_scale(dimscale);
    if scale > 1e6 || style.text_height.abs() * scale > 1e6 {
        a.omitted(space, e.handle, "Effective dimension scale exceeds safe rendering limits.");
        return;
    }
    let paper_scale = match paper_mm {
        Some(mm) => Some(1.0 / mm),
        None => millimetres_per_unit(d.header.i64("INSUNITS", 0)).map(|mm| scale / mm),
    };
    if paper_scale.is_none() {
        a.omitted(space, e.handle, "Model drawing units are unspecified/unsupported: paper-mm dimension spacing cannot be measured.");
    }
    let axis = match dimension.kind {
        DimKind::Linear { rotation } => Some(Vec2::from_angle(rotation)),
        DimKind::Aligned => Some((dimension.p14.xy() - dimension.p13.xy()).normalized()),
        _ => None,
    };
    let g = cadcraft_render::dimension_geometry_with(&dimension, base, dimscale, &font);
    let mut invalid = false;
    let mut count = 0;
    for (i, line) in g.lines.iter().enumerate() {
        if g.text_marks.contains(&i) {
            continue;
        }
        let stroke_ends = line.first().copied().zip(line.last().copied()).map(|(p, q)| [p, q]);
        for w in line.windows(2) {
            let (Some(&p), Some(&q)) = (w.first(), w.get(1)) else { continue };
            let curve = Curve::Line(Line::new(p, q));
            if !valid_curve(curve) {
                invalid = true;
                continue;
            }
            if a.primitives >= MAX_PRIMITIVES || count >= 2048 {
                a.incomplete = true;
                a.exhausted = true;
                return;
            }
            let is_dimension_line = g.line_roles.get(i) == Some(&LineRole::Dim)
                && axis.is_some_and(|u| u.cross((q - p).normalized()).abs() < EPS && u.cross(p - dimension.defpt.xy()).abs() < EPS);
            a.primitives += 1;
            count += 1;
            items.push(Item {
                owner: e.handle,
                curve,
                bounds: curve.bounds(),
                dimension: true,
                paper_scale: if is_dimension_line { paper_scale } else { None },
                anchors: (g.line_roles.get(i) == Some(&LineRole::Ext)).then_some([dimension.p13.xy(), dimension.p14.xy()]),
                stroke_ends,
            });
        }
    }
    if invalid {
        a.omitted(space, e.handle, "Some rendered dimension strokes were non-finite, degenerate or outside safe coordinate limits.");
    }
}

fn bounded_dimension(dm: &Dimension) -> bool {
    [dm.defpt, dm.text_mid, dm.p13, dm.p14, dm.p15, dm.p16, dm.insertion_point].into_iter().all(valid_vec3)
        && dm.text.len() <= 256
        && dm.style.len() <= 256
        && dm.assoc.len() <= 64
        && dm.marks.breaks.len() <= 16
        && dm.overrides.len() <= 32
        && dm.overrides.iter().all(|(k, v)| k.len() <= 64 && bounded_value(v, 0))
        && dm.marks.breaks.iter().all(|b| valid_vec3(b.at) && valid_number(b.size))
        && dm.marks.oblique.is_none_or(valid_number)
        && dm.marks.jog_line.is_none_or(valid_vec3)
        && dm.marks.jogged.as_ref().is_none_or(|j| valid_vec3(j.center) && valid_vec3(j.jog))
        && dm.marks.inspect.as_ref().is_none_or(|i| i.label.len() <= 128 && i.rate.len() <= 128)
        && valid_number(dm.text_rotation)
        && match dm.kind {
            DimKind::Linear { rotation } => valid_number(rotation),
            _ => true,
        }
}

fn bounded_style(st: &cadcraft_doc::DimStyle) -> bool {
    if [
        &st.name,
        &st.arrow_block,
        &st.arrow_block1,
        &st.arrow_block2,
        &st.text_style,
        &st.post,
        &st.alt_post,
        &st.decimal_separator,
        &st.leader_arrow_block,
        &st.dim_linetype,
        &st.ext_linetype1,
        &st.ext_linetype2,
    ]
    .into_iter()
    .any(|s| s.len() > 256)
    {
        return false;
    }
    // All style fields are scalars/strings. Serialization also exposes non-finite floats as
    // null, which is rejected here before passing values to font/geometry generation.
    serde_json::to_value(st).ok().is_some_and(|v| v.as_object().is_some_and(|o| o.values().all(|v| !v.is_null() && bounded_value(v, 0))))
}

fn bounded_value(v: &Value, depth: usize) -> bool {
    if depth > 3 {
        return false;
    }
    match v {
        Value::String(s) => s.len() <= 256,
        Value::Number(n) => n.as_f64().is_some_and(valid_number),
        Value::Array(v) => v.len() <= 16 && v.iter().all(|v| bounded_value(v, depth + 1)),
        Value::Object(o) => o.len() <= 16 && o.iter().all(|(k, v)| k.len() <= 64 && bounded_value(v, depth + 1)),
        _ => true,
    }
}

fn valid_number(n: f64) -> bool {
    n.is_finite() && n.abs() <= MAX_COORD
}
fn valid_point(p: Vec2) -> bool {
    valid_number(p.x) && valid_number(p.y)
}
fn valid_vec3(p: Vec3) -> bool {
    valid_point(p.xy()) && valid_number(p.z) && p.z.abs() <= EPS
}
fn valid_radius(r: f64) -> bool {
    valid_number(r) && r > EPS
}
fn near(a: Vec2, b: Vec2) -> bool {
    a.dist(b) <= EPS
}

fn planar(e: &Entity) -> bool {
    if !(e.common.extrusion - Vec3::Z).is_finite()
        || (e.common.extrusion - Vec3::Z).len() > EPS
        || !valid_number(e.common.thickness)
        || e.common.thickness.abs() > EPS
    {
        return false;
    }
    match &e.kind {
        EntityKind::Line(l) => valid_vec3(l.a) && valid_vec3(l.b),
        EntityKind::Circle(c) => valid_vec3(c.center),
        EntityKind::Arc(c) => valid_vec3(c.center),
        EntityKind::LwPolyline(p) => valid_number(p.elevation) && p.elevation.abs() <= EPS,
        EntityKind::Polyline3d(p) => p.points.len() <= 256 && p.points.iter().copied().all(valid_vec3),
        _ => true,
    }
}

fn valid_curve(c: Curve) -> bool {
    match c {
        Curve::Line(l) => valid_point(l.a) && valid_point(l.b) && l.len() > EPS,
        Curve::Circle(c) => valid_point(c.center) && valid_radius(c.radius),
        Curve::Arc(c) => valid_point(c.center) && valid_radius(c.radius) && valid_number(c.start) && valid_number(c.end) && c.len() > EPS,
    }
}

fn millimetres_per_unit(units: i64) -> Option<f64> {
    match units {
        1 => Some(25.4),
        2 => Some(304.8),
        4 => Some(1.0),
        5 => Some(10.0),
        6 => Some(1000.0),
        7 => Some(1e6),
        10 => Some(914.4),
        13 => Some(0.001),
        14 => Some(100.0),
        _ => None,
    }
}

fn parallel_distance(a: Line, b: Line) -> Option<f64> {
    let u = a.dir();
    if u.cross(b.dir()).abs() > 1e-10 {
        return None;
    }
    let (b0, b1) = ((b.a - a.a).dot(u), (b.b - a.a).dot(u));
    let overlap = a.len().min(b0.max(b1)) - 0.0_f64.max(b0.min(b1));
    (overlap > EPS).then(|| u.cross(b.a - a.a).abs())
}

fn overlap(a: Curve, b: Curve) -> Option<(&'static str, f64)> {
    match (a, b) {
        (Curve::Line(a), Curve::Line(b)) => {
            let u = a.dir();
            if u.cross(b.a - a.a).abs() > EPS || u.cross(b.b - a.a).abs() > EPS {
                return None;
            }
            let (b0, b1) = ((b.a - a.a).dot(u), (b.b - a.a).dot(u));
            let length = a.len().min(b0.max(b1)) - 0.0_f64.max(b0.min(b1));
            if length <= EPS {
                return None;
            }
            let duplicate = (near(a.a, b.a) && near(a.b, b.b)) || (near(a.a, b.b) && near(a.b, b.a));
            Some((if duplicate { "geometry.duplicate" } else { "geometry.overlap" }, length))
        }
        (Curve::Circle(a), Curve::Circle(b)) if near(a.center, b.center) && (a.radius - b.radius).abs() <= EPS => {
            Some(("geometry.duplicate", std::f64::consts::TAU * a.radius))
        }
        (Curve::Arc(a), Curve::Arc(b)) if near(a.center, b.center) && (a.radius - b.radius).abs() <= EPS => {
            let angle = arc_overlap(a, b);
            if angle * a.radius <= EPS {
                return None;
            }
            let duplicate = near(a.start_point(), b.start_point()) && near(a.end_point(), b.end_point()) && (a.sweep() - b.sweep()).abs() <= EPS;
            Some((if duplicate { "geometry.duplicate" } else { "geometry.overlap" }, angle * a.radius))
        }
        (Curve::Circle(c), Curve::Arc(a)) | (Curve::Arc(a), Curve::Circle(c)) if near(c.center, a.center) && (c.radius - a.radius).abs() <= EPS => {
            Some(("geometry.overlap", a.len()))
        }
        _ => None,
    }
}

fn arc_overlap(a: Arc, b: Arc) -> f64 {
    let (a0, a1) = (a.start, a.start + a.sweep());
    [-std::f64::consts::TAU, 0.0, std::f64::consts::TAU]
        .into_iter()
        .map(|shift| {
            let b0 = b.start + shift;
            (a1.min(b0 + b.sweep()) - a0.max(b0)).max(0.0)
        })
        .sum()
}

fn crossing(x: &Item, y: &Item) -> Option<Vec2> {
    let legitimate = |p| {
        x.endpoint(p)
            || y.endpoint(p)
            || x.anchors.is_some_and(|anchors| anchors.into_iter().any(|a| near(a, p)))
            || y.anchors.is_some_and(|anchors| anchors.into_iter().any(|a| near(a, p)))
    };
    let accepted = |p| valid_point(p) && !legitimate(p);
    match (x.curve, y.curve) {
        (Curve::Line(l), Curve::Circle(c)) | (Curve::Circle(c), Curve::Line(l)) => {
            cadcraft_geom::line_circle(&l, &c).into_iter().find_map(|(p, t)| ((0.0..=1.0).contains(&t) && accepted(p)).then_some(p))
        }
        (Curve::Line(a), Curve::Line(b)) => {
            let p = if overlap(x.curve, y.curve).is_some() {
                let u = a.dir();
                let lo = 0.0_f64.max((b.a - a.a).dot(u).min((b.b - a.a).dot(u)));
                let hi = a.len().min((b.a - a.a).dot(u).max((b.b - a.a).dot(u)));
                Some(a.a + u * ((lo + hi) / 2.0))
            } else {
                cadcraft_geom::line_line(&a, &b)
            };
            p.filter(|&p| accepted(p))
        }
        (Curve::Line(l), Curve::Arc(arc)) | (Curve::Arc(arc), Curve::Line(l)) => {
            cadcraft_geom::intersect_segments(&Segment::Line(l), &Segment::Arc { arc, ccw: true }).into_iter().find(|&p| accepted(p))
        }
        _ => None,
    }
}

fn finding(severity: &str, code: &str, space: &str, handles: &[Handle], bounds: Bounds2, message: &str, comparison: (Value, Value)) -> Value {
    let (actual, expected) = comparison;
    let mut handles = handles.to_vec();
    handles.sort_unstable();
    handles.dedup();
    let handles: Vec<_> = handles.into_iter().map(Handle::hex).collect();
    json!({
        "severity": severity, "code": code, "location": format!("{}:{}", space, handles.join(",")),
        "message": message, "actual": actual, "expected": expected, "handles": handles, "space": space,
        "bounds": [[bounds.min.x, bounds.min.y], [bounds.max.x, bounds.max.y]],
    })
}

#[cfg(test)]
#[path = "norms_tests.rs"]
mod tests;
