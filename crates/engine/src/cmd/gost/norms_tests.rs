use super::*;
use cadcraft_doc::{DimMarks, Layout};

fn line(handle: u64, a: [f64; 2], b: [f64; 2]) -> Entity {
    Entity::new(Handle(handle), EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(a[0], a[1], 0.0), b: Vec3::new(b[0], b[1], 0.0) }))
}

fn circle(handle: u64, x: f64, r: f64) -> Entity {
    Entity::new(Handle(handle), EntityKind::Circle(cadcraft_doc::Circle { center: Vec3::new(x, 0.0, 0.0), radius: r }))
}

fn arc(handle: u64, start: f64, end: f64) -> Entity {
    Entity::new(Handle(handle), EntityKind::Arc(cadcraft_doc::Arc { center: Vec3::ZERO, radius: 10.0, start, end }))
}

fn dimension(handle: u64, y: f64, scale: f64) -> Entity {
    Entity::new(
        Handle(handle),
        EntityKind::Dimension(Dimension {
            kind: DimKind::Linear { rotation: 0.0 },
            defpt: Vec3::new(0.0, y, 0.0),
            text_mid: Vec3::ZERO,
            p13: Vec3::ZERO,
            p14: Vec3::new(100.0, 0.0, 0.0),
            p15: Vec3::ZERO,
            p16: Vec3::ZERO,
            text: " ".into(),
            style: "ISO-25".into(),
            measurement: 0.0,
            text_rotation: 0.0,
            user_text_pos: false,
            block: None,
            insertion_point: Vec3::ZERO,
            overrides: serde_json::from_value(json!({"scale": scale, "suppressExt1": true, "suppressExt2": true})).unwrap(),
            assoc: Vec::new(),
            marks: DimMarks::default(),
        }),
    )
}

fn drawing(entities: impl IntoIterator<Item = Entity>) -> Drawing {
    let mut d = Drawing::new_metric();
    for e in entities {
        d.model.push(e);
    }
    d
}

fn defects(findings: &[Value]) -> Vec<&Value> {
    findings.iter().filter(|f| f["code"] != "geometry.scope" && f["code"] != "geometry.analysis_incomplete").collect()
}

fn with_code<'a>(findings: &'a [Value], code: &str) -> Vec<&'a Value> {
    findings.iter().filter(|f| f["code"] == code).collect()
}

#[test]
fn line_overlap_is_positive_length_not_adjacent_or_bbox_overlap() {
    let d = drawing([line(0xA, [0.0, 0.0], [10.0, 0.0]), line(0xB, [10.0, 0.0], [20.0, 0.0]), line(0xC, [0.0, 1.0], [10.0, 1.0])]);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    assert!(defects(&findings).is_empty());
    let mut d = d;
    d.model.push(line(0xD, [5.0, 0.0], [8.0, 0.0]));
    let before = d.clone();
    let (findings, incomplete) = inspect(&d);
    assert_eq!(d, before, "inspection must not mutate the document");
    assert!(!incomplete);
    let overlaps = with_code(&findings, "geometry.overlap");
    assert_eq!(overlaps.len(), 1);
    assert_eq!(overlaps[0]["handles"], json!(["A", "D"]));
    assert_eq!(overlaps[0]["space"], "Model");
    assert_eq!(overlaps[0]["bounds"], json!([[0.0, 0.0], [10.0, 0.0]]));
    assert_eq!(overlaps[0]["actual"]["overlapDrawingUnits"], 3.0);
}

#[test]
fn reversed_duplicate_line_and_real_circular_overlaps_are_reported() {
    let d = drawing([
        line(1, [0.0, 50.0], [10.0, 50.0]),
        line(2, [10.0, 50.0], [0.0, 50.0]),
        circle(3, 0.0, 10.0),
        circle(4, 0.0, 10.0),
        circle(5, 1.0, 10.0),
        arc(6, 0.0, 1.0),
    ]);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    let duplicates = with_code(&findings, "geometry.duplicate");
    assert_eq!(duplicates.len(), 2);
    assert!(duplicates.iter().any(|f| f["handles"] == json!(["1", "2"])));
    assert!(duplicates.iter().any(|f| f["handles"] == json!(["3", "4"])));
    assert_eq!(with_code(&findings, "geometry.overlap").len(), 2);
}

#[test]
fn adjacent_arc_endpoints_are_not_overlaps_including_angle_wrap() {
    let d = drawing([arc(1, 5.0, 0.0), arc(2, 0.0, 1.0), arc(3, 1.0, 2.0)]);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    assert!(defects(&findings).is_empty());
    let d = drawing([arc(1, 5.0, 0.5), arc(2, 0.0, 1.0)]);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    let overlaps = with_code(&findings, "geometry.overlap");
    assert_eq!(overlaps.len(), 1);
    assert!((overlaps[0]["actual"]["overlapDrawingUnits"].as_f64().unwrap() - 5.0).abs() < EPS);
}

#[test]
fn rendered_dimension_crossings_are_not_merely_bounding_box_intersections() {
    let d = drawing([dimension(0xA, 10.0, 1.0), line(0xB, [50.0, 5.0], [50.0, 15.0])]);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    let crossings = with_code(&findings, "dimension.intersection");
    assert_eq!(crossings.len(), 1);
    assert_eq!(crossings[0]["handles"], json!(["A", "B"]));
    assert_eq!(crossings[0]["actual"]["point"], json!([50.0, 10.0]));
    for endpoint in [[50.0, 10.0], [50.0, 9.0]] {
        let d = drawing([dimension(0xA, 10.0, 1.0), line(0xB, [50.0, 5.0], endpoint)]);
        let (findings, incomplete) = inspect(&d);
        assert!(!incomplete);
        assert!(with_code(&findings, "dimension.intersection").is_empty());
    }
}

#[test]
fn crossings_with_circles_and_arcs_use_actual_curves() {
    let d = drawing([dimension(1, 5.0, 1.0), circle(2, 50.0, 10.0)]);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    assert_eq!(with_code(&findings, "dimension.intersection").len(), 1);
    let mut c = arc(2, 0.0, std::f64::consts::PI);
    if let EntityKind::Arc(a) = &mut c.kind {
        a.center.x = 50.0;
    }
    let d = drawing([dimension(1, 5.0, 1.0), c]);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    assert_eq!(with_code(&findings, "dimension.intersection").len(), 1);
}

#[test]
fn extension_contour_anchor_is_legitimate_even_inside_extended_stroke() {
    let mut dim = dimension(1, 10.0, 1.0);
    if let EntityKind::Dimension(dm) = &mut dim.kind {
        dm.overrides.insert("suppressExt1".into(), json!(false));
        dm.overrides.insert("extOffset".into(), json!(-1.0));
    }
    let d = drawing([dim, line(2, [-5.0, 0.0], [5.0, 0.0])]);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    assert!(with_code(&findings, "dimension.intersection").is_empty());
}

#[test]
fn dimension_break_gap_is_not_reported_as_a_crossing() {
    let mut dim = dimension(1, 10.0, 1.0);
    if let EntityKind::Dimension(dm) = &mut dim.kind {
        dm.marks.breaks.push(cadcraft_doc::DimBreak { at: Vec3::new(50.0, 10.0, 0.0), size: 4.0, auto: false });
    }
    let d = drawing([dim, line(2, [50.0, 5.0], [50.0, 15.0])]);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    assert!(with_code(&findings, "dimension.intersection").is_empty());
}

#[test]
fn parallel_dimension_spacing_uses_effective_scale_and_positive_projection_overlap() {
    let d = drawing([dimension(1, 10.0, 2.0), dimension(2, 22.0, 2.0)]);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    let close = with_code(&findings, "dimension.spacing");
    assert_eq!(close.len(), 1);
    assert_eq!(close[0]["actual"]["paperMm"], 6.0);
    assert_eq!(close[0]["expected"]["minimumPaperMm"], 7.0);
    let d = drawing([dimension(1, 10.0, 2.0), dimension(2, 24.0, 2.0)]);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    assert!(with_code(&findings, "dimension.spacing").is_empty());
    let mut separate = dimension(2, 12.0, 1.0);
    if let EntityKind::Dimension(dm) = &mut separate.kind {
        dm.p13.x = 110.0;
        dm.p14.x = 210.0;
    }
    let d = drawing([dimension(1, 10.0, 1.0), separate]);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    assert!(with_code(&findings, "dimension.spacing").is_empty());
}

#[test]
fn annotative_spacing_and_paper_layout_spacing_use_their_own_scale() {
    let mut d = drawing([dimension(1, 10.0, 1.0), dimension(2, 22.0, 1.0)]);
    d.header.set_str("CANNOSCALE", "1:2");
    for h in [Handle(1), Handle(2)] {
        assert!(d.model.modify(h, |e| e.common.anno_scales.push(cadcraft_doc::AnnoScale::new("1:2", 1.0, 2.0))));
    }
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    assert_eq!(with_code(&findings, "dimension.spacing")[0]["actual"]["paperMm"], 6.0);
    let mut paper = Layout::new("Sheet", 1);
    paper.page.paper_mm = true;
    paper.entities.push(dimension(3, 10.0, 20.0));
    paper.entities.push(dimension(4, 16.0, 20.0));
    d.layouts.push(paper);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    let paper = with_code(&findings, "dimension.spacing").into_iter().find(|f| f["space"] == "Sheet").unwrap();
    assert_eq!(paper["actual"]["paperMm"], 6.0);
}

#[test]
fn spaces_never_share_geometry_pairs_or_navigation_handles() {
    let mut d = drawing([line(1, [0.0, 0.0], [10.0, 0.0])]);
    let mut a = Layout::new("Sheet A", 1);
    a.entities.push(line(2, [0.0, 0.0], [10.0, 0.0]));
    let mut b = Layout::new("Sheet B", 2);
    b.entities.push(line(3, [0.0, 0.0], [10.0, 0.0]));
    b.entities.push(line(4, [0.0, 0.0], [10.0, 0.0]));
    d.layouts = vec![a, b];
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    let duplicates = with_code(&findings, "geometry.duplicate");
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates[0]["handles"], json!(["3", "4"]));
    assert_eq!(duplicates[0]["space"], "Sheet B");
}

#[test]
fn bounded_work_and_findings_are_deterministic_and_never_pass_truncation() {
    let d = drawing((1_u32..=600).map(|i| line(u64::from(i), [0.0, f64::from(i)], [10.0, f64::from(i)])));
    let first = inspect(&d);
    assert_eq!(first, inspect(&d));
    assert!(first.1);
    assert!(first.0.len() <= MAX_FINDINGS);
    let last = first.0.last().unwrap();
    assert_eq!(last["code"], "geometry.analysis_incomplete");
    assert_eq!(last["actual"]["pairs"], MAX_PAIRS);
    let d = drawing((1..=100).map(|i| line(i, [0.0, 0.0], [10.0, 0.0])));
    let (findings, incomplete) = inspect(&d);
    assert!(incomplete);
    assert_eq!(findings.len(), MAX_FINDINGS);
    assert_eq!(findings.last().unwrap()["code"], "geometry.analysis_incomplete");
}

#[test]
fn hostile_and_stored_geometry_reports_scope_instead_of_a_false_pass() {
    let mut stored = dimension(2, 10.0, 1.0);
    if let EntityKind::Dimension(dm) = &mut stored.kind {
        dm.block = Some("*D1".into());
    }
    let mut tilted = line(3, [0.0, 0.0], [10.0, 0.0]);
    tilted.common.extrusion = Vec3::new(1.0, 0.0, 1.0);
    let mut hostile = dimension(4, 10.0, 1.0);
    if let EntityKind::Dimension(dm) = &mut hostile.kind {
        dm.defpt.x = f64::NAN;
    }
    let d = drawing([line(1, [f64::MAX, 0.0], [f64::INFINITY, 0.0]), stored, tilted, hostile]);
    let (findings, incomplete) = inspect(&d);
    assert!(incomplete);
    assert!(with_code(&findings, "geometry.scope").len() >= 5);
    assert!(with_code(&findings, "geometry.duplicate").is_empty());
    assert!(serde_json::to_string(&findings).is_ok());
}

#[test]
fn spacing_follows_named_style_then_drawing_dimscale_and_physical_units() {
    for (style_scale, header_scale, expected) in [(2.0, 9.0, 6.0), (0.0, 3.0, 4.0)] {
        let mut d = drawing([dimension(1, 10.0, 1.0), dimension(2, 22.0, 1.0)]);
        for h in [Handle(1), Handle(2)] {
            assert!(d.model.modify(h, |e| {
                if let EntityKind::Dimension(dm) = &mut e.kind {
                    dm.overrides.remove("scale");
                }
            }));
        }
        d.dim_styles.iter_mut().find(|s| s.name == "ISO-25").unwrap().scale = style_scale;
        d.header.set_f64("DIMSCALE", header_scale);
        let (findings, incomplete) = inspect(&d);
        assert!(!incomplete);
        assert_eq!(with_code(&findings, "dimension.spacing")[0]["actual"]["paperMm"], expected);
    }
    let mut d = drawing([dimension(1, 1.0, 1.0), dimension(2, 1.2, 1.0)]);
    d.header.set_i64("INSUNITS", 1);
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    let mm = with_code(&findings, "dimension.spacing")[0]["actual"]["paperMm"].as_f64().unwrap();
    assert!((mm - 5.08).abs() < EPS);
}

#[test]
fn angular_dimension_tessellation_vertices_are_not_legitimate_endpoints() {
    let mut dim = dimension(1, 10.0, 1.0);
    if let EntityKind::Dimension(dm) = &mut dim.kind {
        dm.kind = DimKind::Angular3P;
        dm.defpt = Vec3::new(10.0, 10.0, 0.0);
        dm.p13 = Vec3::new(10.0, 0.0, 0.0);
        dm.p14 = Vec3::new(0.0, 10.0, 0.0);
    }
    let mut d = drawing([dim]);
    let entity = d.model.get(Handle(1)).unwrap();
    let EntityKind::Dimension(dm) = &entity.kind else {
        panic!("dimension fixture");
    };
    let geometry = cadcraft_render::dimension_in(&d, dm);
    let vertex = geometry.lines.iter().find(|l| l.len() > 3).unwrap().get(1).copied().unwrap();
    d.model.push(line(2, [vertex.x * 0.9, vertex.y * 0.9], [vertex.x * 1.1, vertex.y * 1.1]));
    let (findings, incomplete) = inspect(&d);
    assert!(!incomplete);
    assert!(!with_code(&findings, "dimension.intersection").is_empty());
}
