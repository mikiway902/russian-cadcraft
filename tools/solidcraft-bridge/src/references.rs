//! Versioned, snapshot-scoped exact references. Native rebuilds are deliberately narrower than
//! upstream picking: no nearest-point, piece recovery, or geometry-derived name is reattached.
use super::{Budget, Error, Model, Worker, ensure_response_size, params, units};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use solidcraft_doc::{
    Document, FeatureKind, Model as NativeModel, ModelBody, Operation,
    drawing::dimension::{EdgePick, Reference},
    naming, persistent_name,
};
use solidcraft_kernel::watch;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

pub(super) const MAX_TOKEN_BYTES: usize = 8192;
pub(super) const MAX_DISCOVERY_TOKEN_BYTES: usize = super::MAX_RESPONSE_BYTES / 4;
const MAX_NAME_BYTES: usize = 1024;
const PREFIX: &str = "cadcraft-reference/1:";

pub(super) struct Native {
    // Retain real evaluated NameCells and their lineage/origin chains, not reconstructed names.
    pub document: Document,
    pub model: NativeModel,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Identity {
    component: u64,
    occurrence_path: Vec<u64>,
    feature: u64,
    definition_body: String,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Edge,
    Face,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Token {
    version: u8,
    source_sha256: String,
    canonical_path: PathBuf,
    format: String,
    placement: usize,
    kind: Kind,
    index: usize,
    native_identity: Option<Identity>,
    name: Option<String>,
    verified: bool,
    lineage: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Discovery {
    source_sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resolve {
    source_sha256: String,
    reference: String,
    fraction: f64,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME_BYTES && !name.chars().any(char::is_control)
}
fn encode(token: &Token, total: &mut usize) -> Result<String, Error> {
    let text = format!("{PREFIX}{}", serde_json::to_string(token)?);
    if text.len() > MAX_TOKEN_BYTES {
        return Err(Error::Limit("reference token bytes"));
    }
    *total = total.checked_add(text.len()).ok_or(Error::Limit("discovery token bytes"))?;
    if *total > MAX_DISCOVERY_TOKEN_BYTES {
        return Err(Error::Limit("discovery token bytes"));
    }
    Ok(text)
}
fn decode(text: &str) -> Result<Token, Error> {
    let bad = || Error::Parameters("invalid v1 reference token".into());
    if text.len() > MAX_TOKEN_BYTES {
        return Err(Error::Limit("reference token bytes"));
    }
    let token: Token = serde_json::from_str(text.strip_prefix(PREFIX).ok_or_else(bad)?).map_err(|_| bad())?;
    if token.version != 1
        || token.source_sha256.len() != 64
        || !token.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || !token.canonical_path.is_absolute()
        || token.canonical_path.as_os_str().len() > 4096
        || token.placement >= super::MAX_BODIES
        || token.index >= super::MAX_TOPOLOGY
        || !matches!(token.format.as_str(), "sc" | "step" | "iges")
        || token.name.as_deref().is_some_and(|n| !valid_name(n))
        || token.lineage.as_ref().is_some_and(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        || token.native_identity.as_ref().is_some_and(|i| {
            i.occurrence_path.len() > super::MAX_DEPTH || i.occurrence_path.contains(&0) || !valid_name(&i.definition_body) || i.feature == 0
        })
        || (token.format == "sc") != token.native_identity.is_some()
    {
        return Err(bad());
    }
    Ok(token)
}

fn verification(native: bool, verified: bool) -> &'static str {
    if !native {
        "payload_scoped"
    } else if verified {
        "verified_upstream_history"
    } else {
        "unverified_upstream_name"
    }
}

// Explicit semantic continuity scheme v1. Only literal independent primitive sizes or a simple
// sketch extrusion's literal distance may change. Every other saved document field must agree.
// This includes native local ids, names, kind, sketch curve ids, components and occurrence frames.
// A canonical path is the project boundary: SC has no document UUID. Not a general name matcher.
fn lineage(native: &Native, feature: u64) -> Result<Option<String>, Error> {
    let doc = &native.document;
    let Some(f) = doc.feature(feature) else { return Ok(None) };
    let positive = |s: &str| s.parse::<f64>().is_ok_and(|n| n.is_finite() && n > 1e-6);
    let fields: &[&str] = match &f.kind {
        FeatureKind::Cylinder { radius, height, operation: Operation::NewBody, references, .. }
            if references.is_empty() && positive(radius) && positive(height) =>
        {
            &["radius", "height"]
        }
        FeatureKind::Box { length, width, height, operation: Operation::NewBody, .. } if positive(length) && positive(width) && positive(height) => {
            &["length", "width", "height"]
        }
        FeatureKind::Extrude { extent, operation: Operation::NewBody, thin: None, targets, .. }
            if targets.is_empty()
                && positive(&extent.distance)
                && extent.distance2.is_none()
                && extent.start_offset.is_none()
                && !extent.through_all
                && extent.taper.is_none()
                && extent.to.is_none()
                && extent.from.is_none() =>
        {
            &["extent", "distance"]
        }
        _ => return Ok(None),
    };
    let mut value = serde_json::to_value(doc)?;
    let Some(features) = value.get_mut("features").and_then(Value::as_array_mut) else { return Ok(None) };
    let Some(saved) = features.iter_mut().find(|v| v["id"].as_u64() == Some(feature)) else { return Ok(None) };
    let kind = saved;
    if fields == ["extent", "distance"] {
        kind["extent"]["distance"] = json!("<literal-distance>");
    } else {
        for field in fields {
            kind[*field] = json!("<literal-size>");
        }
    }
    Ok(Some(format!("{:x}", Sha256::digest(serde_json::to_vec(&value)?))))
}

fn edge_verified(body: &ModelBody, index: usize, name: &str, counts: &BTreeMap<&str, usize>) -> bool {
    persistent_name::EdgeName::parse(name).is_ok_and(|n| n.tie.is_none())
        && counts.get(name) == Some(&1)
        && body
            .mesh()
            .edge_faces
            .get(index)
            .is_some_and(|faces| !faces.is_empty() && faces.iter().all(|f| naming::face_name_exact(body, *f as usize)))
}

impl Worker {
    pub(super) fn references(&self, parameters: Value) -> Result<Value, Error> {
        let request: Discovery = params(parameters)?;
        let model = self.model.as_ref().ok_or(Error::NoModel)?;
        if request.source_sha256 != model.fingerprint.sha256 {
            return Err(Error::StaleModel);
        }
        let budget = Arc::new(Budget::new());
        let result = watch::watching(budget.clone(), || discover(model, &budget))?;
        budget.check()?;
        ensure_response_size(&result)?;
        Ok(result)
    }

    pub(super) fn resolve_point(&self, parameters: Value) -> Result<Value, Error> {
        let request: Resolve = params(parameters)?;
        if !request.fraction.is_finite() || !(0.0..=1.0).contains(&request.fraction) {
            return Err(Error::Parameters("fraction must be finite and in 0..1".into()));
        }
        let model = self.model.as_ref().ok_or(Error::NoModel)?;
        if request.source_sha256 != model.fingerprint.sha256 {
            return Err(Error::StaleModel);
        }
        let token = decode(&request.reference)?;
        let budget = Arc::new(Budget::new());
        let result = watch::watching(budget.clone(), || resolve(model, &token, request.fraction, &budget))?;
        budget.check()?;
        ensure_response_size(&result)?;
        Ok(result)
    }
}

fn discover(model: &Model, budget: &Budget) -> Result<Value, Error> {
    let mut placements = Vec::with_capacity(model.bodies.len());
    let mut token_bytes = 0usize;
    let mut lineages = BTreeMap::new();
    if let Some(native) = &model.native {
        let state = native.model.state();
        let frames = native.document.body_frames_with_source(&state);
        if frames.len() != model.bodies.len() {
            return Err(Error::Geometry("native reference placement mapping changed".into()));
        }
        for (placement, (_, definition, component, path, _)) in frames.into_iter().enumerate() {
            budget.check()?;
            let candidates: Vec<_> =
                state.bodies.iter().filter(|b| b.definition_name() == definition && native.document.model_body_component(b) == component).collect();
            let [body] = candidates.as_slice() else { return Err(Error::Geometry("native reference body scope is ambiguous".into())) };
            let identity = Identity { component, occurrence_path: path.clone(), feature: body.feature, definition_body: definition };
            if let std::collections::btree_map::Entry::Vacant(entry) = lineages.entry(body.feature) {
                entry.insert(lineage(native, body.feature)?);
            }
            let lineage = lineages.get(&body.feature).cloned().flatten();
            let names = naming::edge_names(body);
            let mut counts = BTreeMap::new();
            for name in &names {
                *counts.entry(name.as_str()).or_insert(0) += 1;
            }
            // Validate the binding once per body/placement, then enumerate its actual NameCell
            // table. Picking every index clones placed edges/rebuilds every name O(E²).
            if let Some(name) = names.first() {
                let captured = native
                    .document
                    .edge_reference(&native.model, &EdgePick::Index { body: body.name.clone(), index: 0 }, &path)
                    .map_err(|e| Error::Geometry(e.to_string()))?;
                if captured.name != *name || captured.body != body.name || captured.origin_feature != body.feature || captured.path != path {
                    return Err(Error::Geometry("native edge naming scope disagrees with the evaluated body".into()));
                }
            }
            let mut edges = Vec::new();
            for (index, name) in names.iter().enumerate() {
                budget.check()?;
                if !valid_name(name) {
                    return Err(Error::Limit("native reference name bytes"));
                }
                let verified = edge_verified(body, index, name, &counts);
                let token = Token {
                    version: 1,
                    source_sha256: model.fingerprint.sha256.clone(),
                    canonical_path: model.canonical_path.clone(),
                    format: model.format.into(),
                    placement,
                    kind: Kind::Edge,
                    index,
                    native_identity: Some(identity.clone()),
                    name: Some(name.clone()),
                    verified,
                    lineage: if verified { lineage.clone() } else { None },
                };
                edges.push(json!({"index": index, "reference": encode(&token, &mut token_bytes)?, "name": name,
                    "verification": verification(true, verified), "cross_rebuild": verified && token.lineage.is_some()}));
            }
            let face_names = naming::face_names(body);
            let mut face_counts = BTreeMap::new();
            for name in face_names.iter() {
                *face_counts.entry(name.as_str()).or_insert(0usize) += 1;
            }
            if let Some(name) = face_names.first() {
                let captured = native
                    .document
                    .face_reference(&native.model, &EdgePick::Index { body: body.name.clone(), index: 0 }, &path)
                    .map_err(|e| Error::Geometry(e.to_string()))?;
                if captured.name != *name || captured.body != body.name || captured.origin_feature != body.feature || captured.path != path {
                    return Err(Error::Geometry("native face naming scope disagrees with the evaluated body".into()));
                }
            }
            let mut faces = Vec::new();
            for (index, name) in face_names.iter().enumerate() {
                budget.check()?;
                if !valid_name(name) {
                    return Err(Error::Limit("native reference name bytes"));
                }
                let verified = naming::face_name_exact(body, index) && face_counts.get(name.as_str()) == Some(&1);
                let token = Token {
                    version: 1,
                    source_sha256: model.fingerprint.sha256.clone(),
                    canonical_path: model.canonical_path.clone(),
                    format: model.format.into(),
                    placement,
                    kind: Kind::Face,
                    index,
                    native_identity: Some(identity.clone()),
                    name: Some(name.clone()),
                    verified,
                    lineage: if verified { lineage.clone() } else { None },
                };
                faces.push(json!({"index": index, "reference": encode(&token, &mut token_bytes)?, "name": name,
                    "verification": verification(true, verified), "cross_rebuild": verified && token.lineage.is_some()}));
            }
            placements
                .push(json!({"placement": placement, "native_identity": identity, "identity_scope": "canonical_project_path_and_payload_baseline",
                "edges": edges, "faces": faces}));
        }
    } else {
        for (placement, body) in model.bodies.iter().enumerate() {
            budget.check()?;
            let mut items = |kind, count| -> Result<Vec<Value>, Error> {
                let mut refs = Vec::with_capacity(count);
                for index in 0..count {
                    budget.check()?;
                    let token = Token {
                        version: 1,
                        source_sha256: model.fingerprint.sha256.clone(),
                        canonical_path: model.canonical_path.clone(),
                        format: model.format.into(),
                        placement,
                        kind,
                        index,
                        native_identity: None,
                        name: None,
                        verified: false,
                        lineage: None,
                    };
                    refs.push(json!({"index": index, "reference": encode(&token, &mut token_bytes)?, "name": null,
                        "verification": "payload_scoped", "cross_rebuild": false}));
                }
                Ok(refs)
            };
            placements.push(json!({"placement": placement, "native_identity": null, "identity_scope": "payload_sha256_and_canonical_path",
                "edges": items(Kind::Edge, body.body.record().edges.len())?, "faces": items(Kind::Face, body.body.record().faces.len())?}));
        }
    }
    Ok(json!({"protocol": "cadcraft-references/1", "source": model.fingerprint, "canonical_path": model.canonical_path,
        "format": model.format, "units": units(), "fraction_convention": "trimmed_edge_arc_length_start_to_end",
        "native_continuity_scheme": "same_path_full_document_except_literal_independent_sizes_or_simple_extrude_distance/1",
        "placements": placements}))
}

fn resolution(model: &Model, token: &Token, fraction: f64) -> Value {
    json!({"protocol": "cadcraft-references/1", "source": model.fingerprint,
        "baseline_source_sha256": token.source_sha256, "canonical_path": model.canonical_path,
        "native_identity": token.native_identity, "verification": "not_verified",
        "identity_scope": if model.native.is_some() { "canonical_project_path_and_payload_baseline" } else { "payload_sha256_and_canonical_path" },
        "units": units(), "fraction": fraction, "fraction_convention": "trimmed_edge_arc_length_start_to_end"})
}

fn resolve(model: &Model, token: &Token, fraction: f64, budget: &Budget) -> Result<Value, Error> {
    let finish = |status: &str, reason: &str| {
        let mut result = resolution(model, token, fraction);
        result["status"] = json!(status);
        result["reason"] = json!(reason);
        result
    };
    if token.canonical_path != model.canonical_path || token.format != model.format {
        return Ok(finish("lost", "reference belongs to a different canonical project path or source format"));
    }
    if token.kind != Kind::Edge {
        return Ok(finish("unmeasurable", "resolve_point requires an edge reference, not a face"));
    }
    let changed = token.source_sha256 != model.fingerprint.sha256;
    let state = model.native.as_ref().map(|native| native.model.state());
    let (body, index, frame, verified, placement) = if let Some(native) = &model.native {
        let Some(identity) = &token.native_identity else { return Ok(finish("lost", "native identity is absent")) };
        let doc = &native.document;
        let matches: Vec<_> = doc.features.iter().enumerate().filter(|(_, f)| f.id == identity.feature).collect();
        let [(feature_index, feature)] = matches.as_slice() else {
            return Ok(finish(if matches.is_empty() { "lost" } else { "ambiguous" }, "native feature identity is missing or collides"));
        };
        if feature.suppressed || doc.marker.is_some_and(|m| *feature_index >= m) {
            return Ok(finish("suppressed", "native source feature is suppressed or rolled back"));
        }
        if native.model.result(identity.feature).is_some_and(|r| r.error.is_some() || r.internal || r.stale) {
            return Ok(finish("failed", "native source evaluation failed"));
        }
        if changed && (!token.verified || token.lineage.is_none() || token.lineage != lineage(native, identity.feature)?) {
            return Ok(finish("lost", "native semantic lineage is unverified or changed outside the supported literal parameter edit"));
        }
        let Some(state) = state.as_ref() else { return Ok(finish("failed", "evaluated native state is absent")) };
        let bodies: Vec<_> = state
            .bodies
            .iter()
            .filter(|b| {
                b.definition_name() == identity.definition_body && b.feature == identity.feature && doc.model_body_component(b) == identity.component
            })
            .collect();
        let [body] = bodies.as_slice() else {
            return Ok(finish(if bodies.len() > 1 { "ambiguous" } else { "lost" }, "native definition body is missing or collides"));
        };
        let frames = doc.body_frames_with_source(state);
        let placements: Vec<_> = frames
            .iter()
            .enumerate()
            .filter(|(_, (_, definition, component, path, _))| {
                definition == &identity.definition_body && component == &identity.component && path == &identity.occurrence_path
            })
            .collect();
        let [(placement, _)] = placements.as_slice() else {
            return Ok(finish(if placements.len() > 1 { "ambiguous" } else { "lost" }, "native occurrence path is missing or collides"));
        };
        if !changed && *placement != token.placement {
            return Ok(finish("lost", "snapshot placement scope disagrees"));
        }
        let frame = match doc.body_frame_at(body, &identity.occurrence_path) {
            Ok(frame) => frame,
            Err(_) => return Ok(finish("lost", "native occurrence path is no longer valid")),
        };
        let names = naming::edge_names(body);
        let Some(wanted) = token.name.as_deref() else { return Ok(finish("lost", "native edge name is absent")) };
        // Intentionally stricter than upstream pick recovery: only the exact persistent name.
        let indices: Vec<_> = names.iter().enumerate().filter(|(_, name)| persistent_name::same_edge(name, wanted)).map(|(i, _)| i).collect();
        let [index] = indices.as_slice() else {
            return Ok(finish(if indices.len() > 1 { "ambiguous" } else { "lost" }, "exact native edge name is missing or ambiguous"));
        };
        if !changed && *index != token.index {
            return Ok(finish("lost", "snapshot edge index disagrees with its native name"));
        }
        let mut counts = BTreeMap::new();
        for name in &names {
            *counts.entry(name.as_str()).or_insert(0) += 1;
        }
        let verified = edge_verified(body, *index, &names[*index], &counts);
        let reference = doc
            .edge_reference(&native.model, &EdgePick::Index { body: body.name.clone(), index: *index }, &identity.occurrence_path)
            .map_err(|e| Error::Geometry(e.to_string()))?;
        if !doc.finds_one(&native.model, Reference::Edge(&reference)) || (changed && !verified) {
            return Ok(finish("lost", "native reference no longer has one verified exact lineage"));
        }
        (&body.body, *index, frame, verified, *placement)
    } else {
        if changed {
            return Ok(finish("lost", "STEP/IGES topology indices are strictly payload-scoped"));
        }
        let Some(body) = model.bodies.get(token.placement) else { return Ok(finish("lost", "placement is absent")) };
        if body.refused {
            return Ok(finish("failed", "upstream refused this imported body"));
        }
        (&body.body, token.index, solidcraft_doc::IDENTITY, false, token.placement)
    };
    budget.check()?;
    if index >= body.record().edges.len() {
        return Ok(finish("lost", "exact edge is absent"));
    }
    let path = match solidcraft_kernel::edge_path(body, index) {
        Ok(path) => path,
        Err(error) => return Ok(finish("unmeasurable", &error.to_string())),
    };
    let point = match solidcraft_kernel::along_path(&path, fraction) {
        Ok((point, _)) => solidcraft_doc::apply_point(&frame, point),
        Err(error) => return Ok(finish("unmeasurable", &error.to_string())),
    };
    if !point.is_finite() {
        return Ok(finish("failed", "exact edge evaluator returned a nonfinite point"));
    }
    let mut result = resolution(model, token, fraction);
    result["status"] = json!("resolved");
    result["point"] = json!([point.x, point.y, point.z]);
    result["placement"] = json!(placement);
    result["edge"] = json!(index);
    result["verification"] = json!(verification(model.native.is_some(), verified));
    result["cross_rebuild"] = json!(changed);
    result["evaluation"] = json!("upstream_exact_trimmed_edge_path_arc_length_no_mesh_or_fitting");
    Ok(result)
}
