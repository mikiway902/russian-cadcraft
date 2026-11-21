//! Large ACAD_GROUP dictionaries must not be rescanned for each anonymous child (#862).
use super::RAW_TAG_VISITS;

fn anonymous_groups(n: usize) -> String {
    let mut dictionary = String::from("0\nDICTIONARY\n5\nC\n3\nACAD_GROUP\n350\nD\n0\nDICTIONARY\n5\nD\n330\nC\n");
    let mut groups = String::new();
    for i in 1..=n {
        let handle = format!("{:X}", 0x1000 + i);
        dictionary.push_str(&format!("3\n*A{i}\n350\n{handle}\n"));
        groups.push_str(&format!("0\nGROUP\n5\n{handle}\n330\nD\n300\n\n70\n1\n71\n1\n"));
    }
    format!(
        "0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n2A\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n{dictionary}{groups}0\nENDSEC\n0\nEOF\n"
    )
}

fn read_work(n: usize) -> usize {
    let dxf = anonymous_groups(n);
    RAW_TAG_VISITS.with(|count| count.set(0));
    let drawing = crate::read_dxf(dxf.as_bytes()).expect("read anonymous groups");
    let work = RAW_TAG_VISITS.with(std::cell::Cell::get);
    assert_eq!(drawing.groups.len(), n);
    for (i, group) in drawing.groups.iter().enumerate() {
        assert_eq!(group.name, format!("*A{}", i + 1));
        assert!(group.members.is_empty());
        assert!(group.description.is_empty());
        assert!(group.selectable);
    }
    work
}

#[test]
fn anonymous_group_dictionary_scans_scale_linearly() {
    // Same 16x workload as the former wall-clock test. Count actual raw header/dictionary tag
    // visits instead: a quadratic parent scan exceeds this independently of CPU contention.
    let small = read_work(2_500);
    let large = read_work(40_000);
    assert!(small > 0);
    assert!(large <= small.saturating_mul(20), "16x the groups required {small} -> {large} raw tag visits");
}
