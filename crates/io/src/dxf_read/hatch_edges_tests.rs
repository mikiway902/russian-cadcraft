use cadcraft_doc::{Common, Drawing, EntityKind, Hatch, HatchEdge, HatchLoop, Space};
use cadcraft_geom::{Arc, Ellipse, Line, Mat3, Segment, Spline, Vec2, point_in_polygon};

fn hatch(loops: Vec<HatchLoop>) -> Hatch {
    Hatch {
        pattern: "SOLID".into(),
        solid: true,
        loops,
        scale: 1.0,
        angle: 0.0,
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::ZERO,
        background: None,
        pattern_lines: Vec::new(),
    }
}
fn ring(edges: Vec<HatchEdge>, outer: bool) -> HatchLoop {
    HatchLoop { vertices: Vec::new(), edges, outer, sources: Vec::new() }
}
fn roundtrip(hatch: &Hatch) -> Hatch {
    let mut drawing = Drawing::new_metric();
    drawing.add(&Space::Model, Common::default(), EntityKind::Hatch(hatch.clone())).unwrap();
    let text = crate::write_dxf(&drawing);
    let reopened = crate::read_dxf(text.as_bytes()).unwrap();
    let entity = reopened.model.iter().next().unwrap();
    let EntityKind::Hatch(hatch) = &entity.kind else { panic!("exact hatch was lost: {:?}", entity.kind) };
    hatch.clone()
}
fn quarter_circle(center: Vec2, angle: f64) -> Spline {
    let matrix = Mat3::translate(center).then_before(Mat3::rotate(angle));
    Spline {
        degree: 2,
        knots: vec![0., 0., 0., 1., 1., 1.],
        control: [Vec2::new(1., 0.), Vec2::new(1., 1.), Vec2::new(0., 1.)].into_iter().map(|p| matrix.apply(p)).collect(),
        weights: vec![1., std::f64::consts::FRAC_1_SQRT_2, 1.],
        ..Spline::default()
    }
}
fn nested() -> Hatch {
    let center = Vec2::new(20., 20.);
    let island = (0..4).map(|i| HatchEdge::Spline(quarter_circle(center, f64::from(i) * std::f64::consts::FRAC_PI_2))).collect();
    hatch(vec![
        ring(vec![HatchEdge::Ellipse { ellipse: Ellipse::full(center, Vec2::new(10., 0.), 0.5), ccw: true }], true),
        ring(vec![HatchEdge::Segment(Segment::Arc { arc: Arc::new(center, 2., 0., std::f64::consts::TAU), ccw: false })], false),
        ring(island, true),
    ])
}
#[test]
fn nested_ellipse_circle_and_rational_island_preserve_geometry_and_visible_holes() {
    let original = nested();
    let reopened = roundtrip(&original);
    assert!(reopened.loops.iter().all(|ring| ring.vertices.is_empty()));
    let HatchEdge::Spline(quarter) = &reopened.loops[2].edges[0] else { panic!("rational boundary must stay a spline") };
    assert_eq!(quarter.degree, 2);
    assert_eq!(quarter.knots, [0., 0., 0., 1., 1., 1.]);
    assert!((quarter.eval(0.5).dist(Vec2::new(20., 20.)) - 1.).abs() < 1e-12);
    let loops = cadcraft_render::hatch_fill_loops(&reopened, 1e-4);
    let filled = |point| loops.iter().filter(|ring| point_in_polygon(ring, point)).count() % 2 == 1;
    assert!(filled(Vec2::new(25., 20.)), "ellipse material must remain visible");
    assert!(!filled(Vec2::new(21.5, 20.)), "the round bore must remain empty");
    assert!(filled(Vec2::new(20., 20.)), "the rational circular island must remain visible");
    let areas: Vec<_> = reopened.loops.iter().map(|ring| ring.area_and_perimeter().0).collect();
    assert!((areas[0] - 50. * std::f64::consts::PI).abs() < 1e-10);
    assert!((areas[1] + 4. * std::f64::consts::PI).abs() < 1e-10);
    assert!((areas[2] - std::f64::consts::PI).abs() < 1e-5);
    let twice = roundtrip(&reopened);
    assert!((twice.loops[0].area_and_perimeter().0 - areas[0]).abs() < 1e-10);
}
#[test]
fn clockwise_trimmed_ellipse_uses_geometric_dxf_angles_not_curve_parameters() {
    let ellipse = Ellipse { center: Vec2::new(7., 11.), major: Vec2::new(10., 3.), ratio: 0.35, start: 0.31, end: 1.83 };
    let boundary = ring(
        vec![
            HatchEdge::Ellipse { ellipse, ccw: false },
            HatchEdge::Segment(Segment::Line(Line::new(ellipse.at_param(ellipse.start), ellipse.at_param(ellipse.end)))),
        ],
        true,
    );
    let original = hatch(vec![boundary]);
    let reopened = roundtrip(&original);
    let HatchEdge::Ellipse { ellipse: actual, ccw } = &reopened.loops[0].edges[0] else { panic!("trimmed ellipse must stay analytic") };
    assert!(!ccw);
    assert!(actual.at_param(actual.start).near(ellipse.at_param(ellipse.start), 1e-10));
    assert!(actual.at_param(actual.end).near(ellipse.at_param(ellipse.end), 1e-10));
    assert!((original.loops[0].area_and_perimeter().0 - reopened.loops[0].area_and_perimeter().0).abs() < 1e-9);
    let before = original.loops[0].bounds();
    let after = reopened.loops[0].bounds();
    assert!(before.min.near(after.min, 1e-10) && before.max.near(after.max, 1e-10));
}
#[test]
fn mirrored_nonuniform_transform_keeps_exact_boundaries_and_area_without_chords() {
    let mut entity = EntityKind::Hatch(nested());
    let matrix = Mat3::translate(Vec2::new(90., -20.)).then_before(Mat3::scale(-2., 3.));
    entity.transform(&matrix);
    let EntityKind::Hatch(transformed) = entity else { panic!("expected hatch") };
    let reopened = roundtrip(&transformed);
    let outer = &reopened.loops[0];
    let bounds = outer.bounds();
    assert!(bounds.min.near(Vec2::new(30., 25.), 1e-9));
    assert!(bounds.max.near(Vec2::new(70., 55.), 1e-9));
    assert!((outer.area_and_perimeter().0 + 300. * std::f64::consts::PI).abs() < 1e-8);
    assert!(reopened.loops.iter().all(|ring| ring.vertices.is_empty()));
    assert!(matches!(reopened.loops[1].edges[0], HatchEdge::Ellipse { .. }));
    let loops = cadcraft_render::hatch_fill_loops(&reopened, 1e-4);
    let hole = matrix.apply(Vec2::new(21.5, 20.));
    assert_eq!(loops.iter().filter(|ring| point_in_polygon(ring, hole)).count() % 2, 0);
}
#[test]
fn ambiguous_boundary_representations_and_region_curve_paths_are_rejected() {
    let ring = &nested().loops[0];
    let mut mixed = serde_json::to_value(ring).unwrap();
    mixed["vertices"] = serde_json::json!([{"p":{"x":0.,"y":0.},"bulge":0.}]);
    assert!(serde_json::from_value::<HatchLoop>(mixed).is_err());
    assert!(serde_json::from_value::<cadcraft_doc::Region>(serde_json::json!({"loops":[ring]})).is_err());
}

#[test]
fn nonuniform_legacy_bulges_become_analytic_ellipse_edges() {
    let vertices =
        vec![cadcraft_geom::PolyVertex::with_bulge(Vec2::new(-2., 0.), -1.), cadcraft_geom::PolyVertex::with_bulge(Vec2::new(2., 0.), -1.)];
    let mut entity = EntityKind::Hatch(hatch(vec![HatchLoop { vertices, edges: Vec::new(), outer: true, sources: Vec::new() }]));
    entity.transform(&Mat3::scale(-2., 3.));
    let EntityKind::Hatch(transformed) = entity else { panic!("expected hatch") };
    let reopened = roundtrip(&transformed);
    let ring = &reopened.loops[0];
    assert!(ring.vertices.is_empty());
    assert!(ring.edges.iter().all(|edge| matches!(edge, HatchEdge::Ellipse { .. })));
    assert!((ring.area_and_perimeter().0 - 24. * std::f64::consts::PI).abs() < 1e-10);
    assert!(ring.bounds().min.near(Vec2::new(-4., -6.), 1e-10));
    assert!(ring.bounds().max.near(Vec2::new(4., 6.), 1e-10));
}
