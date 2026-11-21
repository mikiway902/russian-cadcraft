//! Native documents use upstream migrations, evaluation and world placement, never preview/cache meshes.
use super::{
    Budget, Error, Fingerprint, LoadedBody, MAX_BODIES, MAX_DEPTH, MAX_FILE_BYTES, MAX_REFERENCES, exact_measure, preflight_step, units,
    validate_bodies,
};
use serde_json::{Value, json};
use solidcraft_doc::{Document, FeatureKind, Model, joints};
use solidcraft_format::container::{self, Limits, Reader};
use solidcraft_kernel::watch;
use std::{borrow::Cow, collections::BTreeMap, io::Cursor, sync::Arc};

// The upstream loop solver allocates dense matrices quadratic in its variable count.
// Bound its input before calling it; the supervisor also enforces the hard deadline.
pub(super) const MAX_JOINTS: usize = 32;
const MAX_ENTRIES: usize = 10_000;

pub(super) struct Loaded {
    pub bodies: Vec<LoadedBody>,
    pub import_issues: usize,
    pub result: Value,
    pub evaluated: super::references::Native,
}

pub(super) const CONTAINER_LIMITS: Limits = Limits {
    max_entries: MAX_ENTRIES,
    max_expanded: || MAX_FILE_BYTES,
    part_limit: |_| MAX_FILE_BYTES,
    level: |_, _| 1,
    check_size: |size| if size <= MAX_FILE_BYTES as u64 { Ok(()) } else { Err("source file bytes".into()) },
    cancelled: None,
};

fn invalid(error: impl std::fmt::Display) -> Error {
    Error::Import(error.to_string())
}

/// Count JSON structure independently of upstream deserialization/migration budgets.
fn preflight(value: &Value) -> Result<(), Error> {
    let mut stack = vec![(value, 0usize)];
    let mut visited = 0usize;
    while let Some((value, depth)) = stack.pop() {
        visited += 1;
        if visited > MAX_REFERENCES || depth > MAX_DEPTH {
            return Err(Error::Limit("native JSON depth/work"));
        }
        match value {
            Value::Array(values) => stack.extend(values.iter().map(|v| (v, depth + 1))),
            Value::Object(values) => stack.extend(values.values().map(|v| (v, depth + 1))),
            _ => {}
        }
        if stack.len() > MAX_REFERENCES {
            return Err(Error::Limit("native JSON work"));
        }
    }
    Ok(())
}

fn document_bytes(bytes: &[u8]) -> Result<(Cow<'_, [u8]>, Value), Error> {
    if !container::is_container(bytes) {
        return Ok((Cow::Borrowed(bytes), parse_json(bytes)?));
    }
    let mut reader = Reader::new(Cursor::new(bytes), CONTAINER_LIMITS).map_err(invalid)?;
    let manifest = parse_json(&reader.limited_part("container.json", 4096).map_err(invalid)?)?;
    if manifest != json!({"format": "solidcraft-container/1"}) && manifest != json!({"format": "solvecraft-container/1"}) {
        return Err(Error::Geometry("unsupported native container version/content; a drawing is not a model".into()));
    }
    let mut value = parse_json(&reader.document(MAX_FILE_BYTES).map_err(invalid)?)?;
    if let Some(features) = value.get_mut("features").and_then(Value::as_array_mut) {
        for feature in features {
            // Only authoritative STEP/record blobs have a geometry mapping here. All other
            // resource blobs are rejected below, not silently replaced by display meshes.
            if feature.get("step_blob").is_some() {
                if feature.get("step").and_then(Value::as_str).is_some_and(|s| !s.is_empty()) {
                    return Err(invalid("ambiguous native inline STEP and blob"));
                }
                let step = std::str::from_utf8(reader.blob(feature, "step_blob", "step").map_err(invalid)?).map_err(invalid)?;
                preflight_step(step)?;
                let step = Value::String(step.into());
                let object = feature.as_object_mut().ok_or_else(|| invalid("invalid native import feature"))?;
                object.remove("step_blob");
                object.insert("step".into(), step);
            }
            if feature.get("records_blob").is_some() {
                if feature.get("records").is_some_and(|r| !r.is_null()) {
                    return Err(invalid("ambiguous native inline records and blob"));
                }
                let records = parse_json(reader.blob(feature, "records_blob", "records").map_err(invalid)?)?;
                let object = feature.as_object_mut().ok_or_else(|| invalid("invalid native import feature"))?;
                object.remove("records_blob");
                object.insert("records".into(), records);
            }
        }
    }
    preflight(&value)?;
    // No unchecked resource may expand beyond the same policy as the original source.
    let text = serde_json::to_vec(&value)?;
    if text.len() > MAX_FILE_BYTES {
        return Err(Error::Limit("hydrated native document bytes"));
    }
    Ok((Cow::Owned(text), value))
}

fn validate_resources(value: &Value) -> Result<(), Error> {
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    // Informational application properties are opaque, not resource references.
                    if key == "metadata" || key == "properties" {
                        continue;
                    }
                    if key.ends_with("_blob") {
                        return Err(Error::Geometry(format!("native resource `{key}` is unavailable; no preview fallback")));
                    }
                    // A model-controlled font path would bypass the explicit local-file roots.
                    if key == "font" && !value.is_null() {
                        return Err(Error::Geometry("native custom fonts require external resources and are not loaded".into()));
                    }
                    stack.push(value);
                }
            }
            Value::Array(values) => stack.extend(values),
            _ => {}
        }
    }
    Ok(())
}

fn placements(doc: &Document) -> Result<(), Error> {
    // Unlike placements(), this upstream API refuses an incomplete/cyclic walk.
    let placements = doc.placements_below(0, &[], solidcraft_doc::IDENTITY).map_err(invalid)?;
    if placements.len() > MAX_REFERENCES || placements.iter().any(|(_, path, _)| path.len() > MAX_DEPTH) {
        return Err(Error::Limit("native assembly depth/work"));
    }
    Ok(())
}

/// Native timeline imports intentionally return while their independent checks run.
/// Wait before folding reports/heals; the worker supervisor supplies the hard deadline.
fn evaluate_settled(model: &mut Model, doc: &Document, budget: &Arc<Budget>) -> Result<(), Error> {
    for _ in 0..3 {
        watch::watching(budget.clone(), || model.evaluate(doc));
        budget.check()?;
        for report in model.results.iter().filter_map(|result| result.import_report.as_ref()) {
            if report.checks.wait().is_none() {
                return Err(Error::Geometry("native import checks were cancelled".into()));
            }
            budget.check()?;
        }
        let reevaluate = watch::watching(budget.clone(), || model.settle_import_checks());
        budget.check()?;
        if !reevaluate {
            return Ok(());
        }
    }
    Err(Error::Geometry("native import checks/heals did not settle".into()))
}

fn evaluate(doc: &mut Document, budget: &Arc<Budget>) -> Result<Model, Error> {
    if doc.assembly.joints.len() > MAX_JOINTS || doc.assembly.links.len() > MAX_JOINTS {
        return Err(Error::Limit("native joint/link count"));
    }
    let mut model = Model::default();
    evaluate_settled(&mut model, doc, budget)?;
    let params = joints::Params::new(doc);
    for movement in doc.occurrences.iter().flat_map(|o| &o.expressed_moves) {
        movement.eval(&params).map_err(invalid)?;
    }
    let shifts = doc.expressed_move_shifts();
    if !shifts.is_empty() {
        doc.apply_expressed_shifts(&shifts);
        evaluate_settled(&mut model, doc, budget)?;
    }
    // Match the native engine's bounded placement/evaluation feedback, using its public
    // document solver. Refuse errors and an unsettled last move instead of a stale shape.
    for pass in 0..3 {
        if doc.assembly.joints.is_empty() {
            break;
        }
        refresh_joint_inputs(doc, &model)?;
        let solution = joints::solve_holding_with_analysis(doc, &model.state(), &BTreeMap::new(), joints::Analysis::Placement);
        budget.check()?;
        if !solution.errors.is_empty() || !solution.conflicts.is_empty() {
            return Err(Error::Geometry(format!("native assembly joint solve failed: {}", serde_json::to_string(&solution)?)));
        }
        let before = doc.occurrences.clone();
        doc.apply_joint_solution(&solution);
        if before == doc.occurrences {
            break;
        }
        if pass == 2 {
            return Err(Error::Geometry("native assembly placement/evaluation did not settle".into()));
        }
        evaluate_settled(&mut model, doc, budget)?;
    }
    if !model.param_errors.is_empty() {
        return Err(Error::Geometry(format!("native parameter evaluation failed: {:?}", model.param_errors)));
    }
    for result in &model.results {
        if result.internal {
            return Err(Error::Geometry("native feature evaluator reported an internal failure".into()));
        }
        if let Some(error) = &result.error {
            return Err(Error::Geometry(format!("native feature {} failed: {error}", result.id)));
        }
        if result.stale {
            return Err(Error::Geometry("native timeline has stale results".into()));
        }
    }
    Ok(model)
}

fn refresh_joint_inputs(doc: &mut Document, model: &Model) -> Result<(), Error> {
    let params = joints::Params::new(doc);
    let state = model.state();
    let mut expressed = Vec::new();
    for (index, joint) in doc.assembly.joints.iter().enumerate().filter(|(_, j)| !j.exprs.is_empty()) {
        let (mut offset, angle, mut values) = joints::expressed(&params, joint).map_err(invalid)?;
        if joint.constraint == Some(joints::Constraint::Tangent) {
            offset += doc.origin_radius(&state, &joint.b).map_err(invalid)?;
        }
        joints::clamp(joint, &mut values);
        expressed.push((index, offset, angle, values));
    }
    for (index, offset, angle, values) in expressed {
        if let Some(joint) = doc.assembly.joints.get_mut(index) {
            (joint.offset, joint.angle, joint.values) = (offset, angle, values);
        }
    }
    let coupled = doc
        .assembly
        .links
        .iter()
        .enumerate()
        .filter_map(|(i, l)| joints::coupling_values(doc, &state, l).map(|v| (i, v)))
        .map(|(i, v)| v.map(|v| (i, v)).map_err(invalid))
        .collect::<Result<Vec<_>, _>>()?;
    for (index, (ratio, lift)) in coupled {
        if let Some(link) = doc.assembly.links.get_mut(index) {
            link.ratio = ratio;
            link.lift = lift;
        }
    }
    Ok(())
}

fn text_property<'a>(metadata: &'a solidcraft_doc::Metadata, key: &str) -> Option<&'a str> {
    metadata.0.get(key).and_then(Value::as_str)
}

pub(super) fn load(bytes: &[u8], source: &Fingerprint) -> Result<Loaded, Error> {
    let budget = Arc::new(Budget::new());
    let (text, value) = document_bytes(bytes)?;
    validate_resources(&value)?;
    let mut doc = Document::from_json(std::str::from_utf8(&text).map_err(invalid)?).map_err(invalid)?;
    drop(value);
    drop(text);
    if doc.newer_file.is_some() {
        return Err(Error::Geometry("native document contains newer/unknown geometry; partial evaluation refused".into()));
    }
    for feature in &doc.features {
        match &feature.kind {
            FeatureKind::MeshImport { .. } => return Err(Error::Geometry("native mesh features are not exact geometry".into())),
            FeatureKind::Import { step, .. } => preflight_step(step)?,
            _ => {}
        }
    }
    placements(&doc)?;
    let model = evaluate(&mut doc, &budget)?;
    placements(&doc)?;
    let state = model.state();
    validate_bodies(state.bodies.iter().map(|b| &b.body))?;
    let frames = doc.body_frames_with_source(&state);
    if frames.len() > MAX_BODIES {
        return Err(Error::Limit("native placed body count"));
    }
    let world = watch::watching(budget.clone(), || solidcraft_doc::world_state(&doc, &state)).map_err(invalid)?;
    budget.check()?;
    validate_bodies(world.bodies.iter().map(|b| &b.body))?;
    if frames.len() != world.bodies.len() {
        return Err(Error::Geometry("native body placement mapping is incomplete".into()));
    }
    let mut bodies = Vec::with_capacity(world.bodies.len());
    let mut descriptions = Vec::with_capacity(world.bodies.len());
    for (index, (placed, (name, definition, component, path, transform))) in world.bodies.into_iter().zip(frames).enumerate() {
        budget.check()?;
        if placed.name != name {
            return Err(Error::Geometry("native body placement mapping disagrees".into()));
        }
        let record = placed.body.record();
        // A material region bounded by shells is a solid; an open face/wire has none.
        let closed = record.regions.iter().skip(1).any(|r| !r.void);
        let hidden = doc.visibility.hidden_bodies.contains(&name) || doc.visibility.hidden_occurrences.iter().any(|p| path.starts_with(p));
        let assignment = doc.materials.get(&definition);
        let density = assignment.and_then(|m| doc.material_density(m));
        let component_metadata = doc.components.iter().find(|c| c.id == component).map(|c| &c.metadata).unwrap_or(&doc.metadata);
        let occurrence_names: Vec<_> = path.iter().filter_map(|id| doc.occurrences.iter().find(|o| o.id == *id).map(|o| &o.name)).collect();
        descriptions.push(json!({"placement": index, "body": index,
            "source_item": null, "face_origin": null, "file_faces": null,
            "identity_scope": source.sha256, "edge_identity": "result_local_only",
            "native_identity": {"component": component, "occurrence_path": path,
                "feature": placed.feature, "definition_body": definition},
            "name": name, "color": placed.body.color(), "path": occurrence_names,
            "instance": occurrence_names.last(), "hidden": hidden, "suppressed": false,
            "closed": closed, "refused": null,
            "part_number": text_property(component_metadata, "part_number"),
            "designation": text_property(component_metadata, "designation"),
            "engineering_material": assignment, "density_g_cm3": density,
            "component_static_material": text_property(component_metadata, "material"),
            "source_transform": transform,
            "topology": {"faces": record.faces.len(), "edges": record.edges.len(),
                "vertices": record.vertices.len(), "shells": record.shells.len()},
            "measure": exact_measure(&placed.body, closed)}));
        bodies.push(LoadedBody { body: placed.body, closed, refused: false });
    }
    let components: Vec<_> = doc
        .components
        .iter()
        .map(|c| {
            json!({"definition": c,
        "part_number": text_property(&c.metadata, "part_number"),
        "designation": text_property(&c.metadata, "designation"),
        "material": text_property(&c.metadata, "material")})
        })
        .collect();
    let features: Vec<_> = doc
        .features
        .iter()
        .enumerate()
        .map(|(index, f)| {
            json!({"id": f.id, "name": f.name,
        "component": f.component, "suppressed": f.suppressed,
        "rolled_back": doc.marker.is_some_and(|m| index >= m)})
        })
        .collect();
    let reports: Vec<_> = model
        .results
        .iter()
        .map(|r| {
            json!({"feature": r.id, "skipped": r.skipped,
        "warning": r.warning, "import_report": r.import_report})
        })
        .collect();
    let warnings: Vec<_> = model.results.iter().filter_map(|r| r.warning.as_ref()).collect();
    let import_issues = warnings.len() + model.results.iter().filter_map(|r| r.import_report.as_ref()).map(|r| r.issues.len()).sum::<usize>();
    if model.results.iter().filter_map(|r| r.import_report.as_ref()).any(|r| r.checking()) {
        return Err(Error::Geometry("native import checks have not settled".into()));
    }
    budget.check()?;
    let result = json!({"protocol": "cadcraft-model-worker/1", "source": source, "format": "sc", "units": units(),
        "source_identity": "payload_sha256_and_native_document_ids_not_cross_rebuild_topology",
        "metadata": {"schema": doc.format, "name": doc.name, "source_units": doc.units,
            "properties": doc.metadata, "part_number": text_property(&doc.metadata, "part_number"),
            "designation": text_property(&doc.metadata, "designation"),
            "engineering_material": null, "density": null,
            "material_assignments": doc.materials, "user_materials": doc.user_materials},
        "bodies": descriptions, "tree": {"kind": "native_components_and_occurrences", "components": components,
            "occurrences": doc.occurrences, "assembly": doc.assembly},
        "tree_transform_convention": "column_major_parent_mm_do_not_reapply_to_bodies",
        "features": features, "visibility": doc.visibility,
        "material_assignments": doc.materials, "user_materials": doc.user_materials,
        "hidden_contract": "body_and_occurrence_flags_reported_full_saved_visibility_retained_all_bodies_participate_in_views",
        "warnings": warnings, "report": reports, "checks_settled": true,
        "native_model_cache": "ignored_timeline_evaluated"});
    Ok(Loaded { bodies, import_issues, result, evaluated: super::references::Native { document: doc, model } })
}

/// Reject oversized structure before serde can allocate input-sized arrays or object maps.
fn parse_json(bytes: &[u8]) -> Result<Value, Error> {
    let (mut quoted, mut escaped, mut scalar) = (false, false, false);
    let (mut depth, mut tokens) = (0usize, 0usize);
    for &byte in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => {
                quoted = true;
                scalar = false;
                tokens += 1;
            }
            b'[' | b'{' => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(Error::Limit("native JSON depth"));
                }
                scalar = false;
                tokens += 1;
            }
            b']' | b'}' => {
                depth = depth.checked_sub(1).ok_or_else(|| invalid("unbalanced native JSON"))?;
                scalar = false;
            }
            b',' | b':' | b' ' | b'\n' | b'\r' | b'\t' => scalar = false,
            _ if !scalar => {
                scalar = true;
                tokens += 1;
            }
            _ => {}
        }
        if tokens > MAX_REFERENCES {
            return Err(Error::Limit("native JSON tokens"));
        }
    }
    if quoted || depth != 0 {
        return Err(invalid("unterminated native JSON"));
    }
    let value = serde_json::from_slice(bytes).map_err(invalid)?;
    preflight(&value)?;
    Ok(value)
}
