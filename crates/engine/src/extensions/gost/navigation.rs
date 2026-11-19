//! Navigate by live entity handles, not stale coordinates from a saved report.
use crate::{
    Result, Session,
    cmd::{CommandSpec, bad, find_command},
};
use cadcraft_doc::{Handle, Space, entity_bounds};
use cadcraft_geom::{Bounds2, Vec2};
use serde_json::{Value, json};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("gost.check.show", "Show GOST Finding", show)
            .params("{space:Model|layout name,handles:[hex]}; select current objects and fit their bounds; no drawing changes")
            .noundo(),
    ]
}
fn show(s: &mut Session, p: &Value) -> Result<Value> {
    let space = p.get("space").and_then(Value::as_str).ok_or_else(|| bad("gost.check.show", "space required"))?;
    let space = if space.eq_ignore_ascii_case("Model") {
        Space::Model
    } else {
        Space::Paper(s.doc()?.layout(space).ok_or_else(|| bad("gost.check.show", "layout no longer exists"))?.name.clone())
    };
    let raw = p
        .get("handles")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty() && a.len() <= 256)
        .ok_or_else(|| bad("gost.check.show", "1..256 handles required"))?;
    let store = s.doc()?.space(&space).ok_or_else(|| bad("gost.check.show", "space missing"))?;
    let mut handles = Vec::with_capacity(raw.len());
    let mut bounds = Bounds2::from_points([Vec2::ZERO]);
    for raw in raw {
        let h = raw.as_str().and_then(Handle::parse_hex).ok_or_else(|| bad("gost.check.show", "invalid handle"))?;
        let e = store.get(h).ok_or_else(|| bad("gost.check.show", format!("object {} no longer exists in the reported space", h.hex())))?;
        let b = entity_bounds(s.doc()?, e, 0);
        if ![b.min.x, b.min.y, b.max.x, b.max.y].into_iter().all(|x| x.is_finite() && x.abs() <= 1e9) {
            return Err(bad("gost.check.show", "object bounds are not finite/bounded"));
        }
        bounds = if handles.is_empty() { b } else { bounds.union(&b) };
        if !handles.contains(&h) {
            handles.push(h);
        }
    }
    let name = space.tab_name();
    let run = find_command("layout.set").ok_or_else(|| bad("gost.check.show", "layout command missing"))?.run;
    run(s, &json!({"name":name}))?;
    let (w, h) = s.viewport_px;
    let height = (bounds.height().max(bounds.width() * h.max(1.0) / w.max(1.0)) * 1.3).max(20.0);
    let run = find_command("view.set").ok_or_else(|| bad("gost.check.show", "view command missing"))?.run;
    run(s, &json!({"center":[(bounds.min.x+bounds.max.x)*0.5,(bounds.min.y+bounds.max.y)*0.5],"height":height}))?;
    s.set_selection(handles.clone());
    Ok(json!({"space":name,"handles":handles.into_iter().map(Handle::hex).collect::<Vec<_>>()}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finding_navigation_uses_live_geometry_and_keeps_drawing_clean() {
        let mut s = crate::extensions::new_session();
        let h = s.execute("line", &json!({"points":[[100,200],[150,200]]})).unwrap()["handles"][0].clone();
        let before = s.state().unwrap().doc.clone();
        let undo = s.state().unwrap().undo.len();
        s.execute("layout.set", &json!({"name":"GOST1"})).unwrap();
        s.execute("gost.check.show", &json!({"space":"Model","handles":[h.clone()]})).unwrap();
        assert_eq!(s.space(), Space::Model);
        assert_eq!(s.selection(), &[Handle::parse_hex(h.as_str().unwrap()).unwrap()]);
        assert!(std::sync::Arc::ptr_eq(&before, &s.state().unwrap().doc));
        assert_eq!(s.state().unwrap().undo.len(), undo);
        assert!((s.state().unwrap().view().center.x - 125.0).abs() < 1e-6);
        assert!(s.execute("gost.check.show", &json!({"space":"GOST1","handles":[h]})).is_err());
    }
}
