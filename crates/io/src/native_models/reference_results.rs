//! Fail-closed validation at the worker boundary; reference tokens remain opaque.
use super::{Body, Fingerprint, Session, Value, check_source, normalized_units, source_format, validate_digest};
use std::collections::BTreeSet;

const CONVENTION: &str = "trimmed_edge_arc_length_start_to_end";
const EVALUATION: &str = "upstream_exact_trimmed_edge_path_arc_length_no_mesh_or_fitting";
struct Snapshot<'a> {
    path: &'a str,
    source: &'a Fingerprint,
    bodies: &'a [Body],
}
impl<'a> From<&'a Session> for Snapshot<'a> {
    fn from(s: &'a Session) -> Self {
        Self { path: &s.path, source: &s.source, bodies: &s.bodies }
    }
}
fn native_identity(v: &Value) -> bool {
    v.as_object().is_some_and(|o| o.len() == 4)
        && v["component"].as_u64().is_some()
        && v["feature"].as_u64().is_some()
        && v["definition_body"].as_str().is_some_and(|s| !s.is_empty() && s.len() <= 8192)
        && v["occurrence_path"].as_array().is_some_and(|p| p.len() <= 32 && p.iter().all(|id| id.as_u64().is_some()))
}
fn invalid() -> String {
    "invalid worker exact reference result".into()
}
pub(super) fn request(reference: &str, fraction: f64) -> Result<(), String> {
    if reference.is_empty() || reference.len() > 8192 || !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
        return Err("reference requires 1..8192 UTF-8 bytes and fraction in 0..1".into());
    }
    Ok(())
}
fn common(v: &Value, session: &Snapshot<'_>) -> Result<(), String> {
    check_source(v, session.source)?;
    normalized_units(v)?;
    if v["protocol"] != "cadcraft-references/1" || v["canonical_path"] != session.path || v["fraction_convention"] != CONVENTION {
        return Err(invalid());
    }
    Ok(())
}
fn verification(v: &Value, native: bool) -> bool {
    if native { matches!(v.as_str(), Some("verified_upstream_history" | "unverified_upstream_name")) } else { v.as_str() == Some("payload_scoped") }
}
pub(super) fn discovery(v: &Value, session: &Session) -> Result<(), String> {
    discovery_for(v, &Snapshot::from(session))
}
fn discovery_for(v: &Value, session: &Snapshot<'_>) -> Result<(), String> {
    common(v, session)?;
    let native = source_format(session.path)? == "sc";
    if v["format"] != source_format(session.path)?
        || v["native_continuity_scheme"] != "same_path_full_document_except_literal_independent_sizes_or_simple_extrude_distance/1"
    {
        return Err(invalid());
    }
    let placements = v["placements"].as_array().ok_or_else(invalid)?;
    if placements.len() != session.bodies.len() {
        return Err(invalid());
    }
    let mut tokens = BTreeSet::new();
    let mut bytes = 0usize;
    for (index, (placement, body)) in placements.iter().zip(session.bodies).enumerate() {
        if placement["placement"].as_u64() != Some(index as u64)
            || placement["identity_scope"] != if native { "canonical_project_path_and_payload_baseline" } else { "payload_sha256_and_canonical_path" }
            || (native && !native_identity(&placement["native_identity"]))
            || (!native && !placement["native_identity"].is_null())
        {
            return Err(invalid());
        }
        for (key, count) in [("edges", body.edges), ("faces", body.faces)] {
            let items = placement[key].as_array().ok_or_else(invalid)?;
            if items.len() != count {
                return Err(invalid());
            }
            for (index, item) in items.iter().enumerate() {
                let token = item["reference"].as_str().ok_or_else(invalid)?;
                request(token, 0.0)?;
                bytes = bytes.checked_add(token.len()).ok_or_else(invalid)?;
                if bytes > 4 * 1024 * 1024
                    || !tokens.insert(token)
                    || item["index"].as_u64() != Some(index as u64)
                    || !verification(&item["verification"], native)
                    || item["cross_rebuild"].as_bool().is_none()
                    || (item["cross_rebuild"] == true && item["verification"] != "verified_upstream_history")
                    || (native && !item["name"].as_str().is_some_and(|s| !s.is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control)))
                    || (!native && (!item["name"].is_null() || item["cross_rebuild"] != false))
                {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(())
}
pub(super) fn resolution(v: &Value, session: &Session, fraction: f64) -> Result<(), String> {
    resolution_for(v, &Snapshot::from(session), fraction)
}
fn resolution_for(v: &Value, session: &Snapshot<'_>, fraction: f64) -> Result<(), String> {
    common(v, session)?;
    validate_digest(v["baseline_source_sha256"].as_str().ok_or_else(invalid)?)?;
    if v["fraction"].as_f64() != Some(fraction) {
        return Err(invalid());
    }
    let native = source_format(session.path)? == "sc";
    if v["identity_scope"] != if native { "canonical_project_path_and_payload_baseline" } else { "payload_sha256_and_canonical_path" } {
        return Err(invalid());
    }
    match v["status"].as_str() {
        Some("resolved") => {
            let p = v["point"].as_array().ok_or_else(invalid)?;
            let placement = v["placement"].as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(invalid)?;
            let body = session.bodies.get(placement).ok_or_else(invalid)?;
            let edge = v["edge"].as_u64().ok_or_else(invalid)?;
            let changed = v["baseline_source_sha256"] != session.source.sha256;
            if (native && !native_identity(&v["native_identity"])) || (!native && !v["native_identity"].is_null()) {
                return Err(invalid());
            }
            if p.len() != 3
                || p.iter().any(|x| !x.as_f64().is_some_and(f64::is_finite))
                || edge >= body.edges as u64
                || !verification(&v["verification"], native)
                || v["evaluation"] != EVALUATION
                || v["cross_rebuild"].as_bool() != Some(changed)
                || (changed && (!native || v["verification"] != "verified_upstream_history"))
            {
                return Err(invalid());
            }
        }
        Some("lost" | "ambiguous" | "suppressed" | "failed" | "unmeasurable") => {
            if v.get("point").is_some()
                || v.get("placement").is_some()
                || v.get("edge").is_some()
                || !v["reason"].as_str().is_some_and(|s| !s.is_empty() && s.len() <= 16384)
                || v["verification"] != "not_verified"
            {
                return Err(invalid());
            }
        }
        _ => return Err(invalid()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fractions_and_token_bounds_are_fail_closed() {
        for f in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
            assert!(request("opaque", f).is_err());
        }
        assert!(request("", 0.5).is_err());
        assert!(request(&"x".repeat(8193), 0.5).is_err());
        assert!(request("opaque", 0.0).is_ok());
        assert!(request("opaque", 1.0).is_ok());
    }
    fn resolved() -> Value {
        super::super::json!({"protocol":"cadcraft-references/1","source":{"sha256":"a".repeat(64),"bytes":10},
            "canonical_path":"/part.step","units":{"length":"mm","area":"mm2","volume":"mm3","angle":"radian",
                "coordinate_system":"right_handed_z_up","original_declaration":"not_exposed_by_selected_import_api"},
            "fraction":0.5,"fraction_convention":CONVENTION,"baseline_source_sha256":"a".repeat(64),"status":"resolved",
            "point":[1,2,3],"placement":0,"edge":0,"verification":"payload_scoped","identity_scope":"payload_sha256_and_canonical_path",
            "cross_rebuild":false,"evaluation":EVALUATION})
    }
    #[test]
    fn resolution_rejects_wrong_units_snapshot_fraction_topology_and_fake_continuity() {
        let source = Fingerprint { sha256: "a".repeat(64), bytes: 10 };
        let bodies = [Body { edges: 1, faces: 1 }];
        let session = Snapshot { path: "/part.step", source: &source, bodies: &bodies };
        let valid = resolved();
        assert!(resolution_for(&valid, &session, 0.5).is_ok());
        for (pointer, value) in [
            ("/units/length", super::super::json!("inch")),
            ("/source/sha256", super::super::json!("b".repeat(64))),
            ("/source/bytes", super::super::json!(11)),
            ("/point", super::super::json!([1, 2])),
            ("/point", super::super::json!([1, null, 3])),
            ("/fraction", super::super::json!(0.6)),
            ("/placement", super::super::json!(1)),
            ("/edge", super::super::json!(1)),
            ("/baseline_source_sha256", super::super::json!("b".repeat(64))),
            ("/cross_rebuild", super::super::json!(true)),
            ("/evaluation", super::super::json!("mesh")),
            ("/verification", super::super::json!("verified_upstream_history")),
        ] {
            let mut bad = valid.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(resolution_for(&bad, &session, 0.5).is_err(), "{pointer}");
        }
        for status in ["lost", "ambiguous", "suppressed", "failed", "unmeasurable"] {
            let mut lost = valid.clone();
            for key in ["point", "placement", "edge", "evaluation", "cross_rebuild"] {
                lost.as_object_mut().unwrap().remove(key);
            }
            lost["status"] = super::super::json!(status);
            lost["reason"] = super::super::json!("explicit source loss");
            lost["verification"] = super::super::json!("not_verified");
            assert!(resolution_for(&lost, &session, 0.5).is_ok(), "{status}");
            lost["point"] = super::super::json!([1, 2, 3]);
            assert!(resolution_for(&lost, &session, 0.5).is_err());
        }
    }
    #[test]
    fn discovery_requires_complete_unique_payload_scoped_topology() {
        let source = Fingerprint { sha256: "a".repeat(64), bytes: 10 };
        let bodies = [Body { edges: 1, faces: 1 }];
        let session = Snapshot { path: "/part.step", source: &source, bodies: &bodies };
        let mut v = resolved();
        v["format"] = super::super::json!("step");
        v["native_continuity_scheme"] = super::super::json!("same_path_full_document_except_literal_independent_sizes_or_simple_extrude_distance/1");
        v["placements"] = super::super::json!([{"placement":0,"native_identity":null,"identity_scope":"payload_sha256_and_canonical_path",
            "edges":[{"index":0,"reference":"edge","name":null,"verification":"payload_scoped","cross_rebuild":false}],
            "faces":[{"index":0,"reference":"face","name":null,"verification":"payload_scoped","cross_rebuild":false}]}]);
        assert!(discovery_for(&v, &session).is_ok());
        for (pointer, value) in [
            ("/placements/0/edges", super::super::json!([])),
            ("/placements/0/faces/0/reference", super::super::json!("edge")),
            ("/placements/0/edges/0/cross_rebuild", super::super::json!(true)),
            ("/placements/0/edges/0/verification", super::super::json!("verified_upstream_history")),
        ] {
            let mut bad = v.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(discovery_for(&bad, &session).is_err(), "{pointer}");
        }
    }
}
