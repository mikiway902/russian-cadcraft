//! Serial, synchronous native model sessions for the separately installed exact worker.
//!
//! GUI callers must run these functions on a background job; CLI automation may block.
//! At most four immutable snapshots are retained. Preview triangles are display-only and
//! never supply engineering projections or measurements. The worker has deadlines and
//! bounded messages, not an OS memory sandbox; local concurrent path mutation is not sandboxed.

use crate::{model_preview, model_projection, model_section, model_worker};
use cadcraft_doc::{
    model_preview::Preview,
    model_projection::{Projection, SourceReference},
    model_section::{Section, SectionRequest},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

mod reference_results;
const PROTOCOL: &str = "cadcraft-model-worker/1";
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SESSIONS: usize = 4;
// Capabilities and load consume two frames. Renew before sending frame 256.
const RENEW_AT: u16 = 255;

#[derive(Debug, PartialEq, Eq)]
struct Fingerprint {
    sha256: String,
    bytes: u64,
}

struct Body {
    faces: usize,
    edges: usize,
}

struct Session {
    path: String,
    source: Fingerprint,
    metadata: Value,
    bodies: Vec<Body>,
    client: model_worker::Client,
    calls: u16,
    failed: Option<String>,
}

impl Session {
    fn call(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        self.calls += 1;
        self.client.call(method, params).map_err(|error| {
            let message = rpc_error(&error);
            if matches!(error, model_worker::Error::Transport(_) | model_worker::Error::Remote { code: -32603 | -32006, .. }) {
                self.failed = Some(message.clone());
            }
            message
        })
    }
}

static SESSIONS: OnceLock<Mutex<Vec<Session>>> = OnceLock::new();

fn sessions() -> Result<std::sync::MutexGuard<'static, Vec<Session>>, String> {
    SESSIONS.get_or_init(|| Mutex::new(Vec::new())).lock().map_err(|_| "model session lock is poisoned".into())
}

/// Explicitly select a file revision. A failed load cannot erase a prior valid snapshot.
/// Returns the complete validated worker metadata, plus its canonical absolute UTF-8 path.
pub fn load(path: &str) -> Result<Value, String> {
    let path = canonical_source(path)?;
    let source = fingerprint(Path::new(&path))?;
    let mut cache = sessions()?;
    if let Some(index) = cache.iter().position(|entry| entry.path == path && entry.source == source) {
        if cache[index].failed.is_none() && cache[index].calls < RENEW_AT {
            return Ok(cache[index].metadata.clone());
        }
        let replacement = import(&path, source)?;
        cache[index] = replacement;
        return Ok(cache[index].metadata.clone());
    }
    let entry = import(&path, source)?;
    let metadata = entry.metadata.clone();
    insert(&mut cache, entry);
    Ok(metadata)
}

/// Project the immutable selected snapshot, retaining exact parser quality/provenance.
/// Current source bytes must still match; a changed or absent file refuses new geometry.
pub fn view(path: &str, expected_sha: &str, direction: [f64; 3], up: [f64; 3]) -> Result<Projection, String> {
    validate_vectors(direction, up)?;
    // New drawing geometry must not silently use an obsolete on-disk revision.
    // Cached display previews remain available, but a source edit requires explicit refresh.
    with_session(path, expected_sha, |entry| {
        current_source(entry)?;
        let value = entry.call("v1.view", &json!({"source_sha256": expected_sha, "direction": direction, "up": up}))?;
        check_source(&value, &entry.source)?;
        let projection = model_projection::decode(&value).map_err(|error| error.to_string())?;
        if projection.prepare_reports.as_array().map(Vec::len) != Some(entry.bodies.len()) {
            return Err("invalid worker projection: body report count differs from loaded metadata".into());
        }
        for line in &projection.lines {
            let body = entry.bodies.get(line.placement).ok_or("invalid worker projection: placement outside loaded bodies")?;
            let valid = match line.source {
                SourceReference::Edge { edge, .. } => edge < body.edges,
                SourceReference::Silhouette { face } => face < body.faces,
            };
            if !valid {
                return Err("invalid worker projection: source outside loaded body topology".into());
            }
        }
        for face in &projection.unsupported {
            if entry.bodies.get(face.placement).is_none_or(|body| face.face >= body.faces) {
                return Err("invalid worker projection: unsupported face outside loaded topology".into());
            }
        }
        current_source(entry)?;
        Ok(projection)
    })
}

/// Cut the immutable selected snapshot by an exact half-space, then project its result.
/// Raw HLR quality is preserved for the caller's acceptance policy. A rejected cap or any
/// changed source refuses the entire result; neither an uncut view nor a preview is a fallback.
pub fn section(path: &str, expected_sha: &str, request: &SectionRequest) -> Result<Section, String> {
    model_section::validate_request(request).map_err(|error| error.to_string())?;
    with_session(path, expected_sha, |entry| {
        if request.exclude_bodies.iter().any(|i| *i >= entry.bodies.len()) {
            return Err("section exclusions must refer to original loaded body indices".into());
        }
        current_source(entry)?;
        let value = entry.call(
            "v1.section",
            &json!({
                "source_sha256": expected_sha, "plane": request.plane,
                "kept_side": request.kept_side, "direction": request.direction, "up": request.up,
                "exclude_bodies": request.exclude_bodies, "require_analytic": false,
            }),
        )?;
        check_source(&value, &entry.source)?;
        // Fixed-size scratch space: original face bounds validate only boolean ancestry.
        // The decoder uses returned section topology for every HLR edge/face reference.
        let faces: [usize; 32] = std::array::from_fn(|i| entry.bodies.get(i).map_or(0, |body| body.faces));
        let result = model_section::decode(&value, request, expected_sha, entry.source.bytes, &faces[..entry.bodies.len()])
            .map_err(|error| error.to_string())?;
        current_source(entry)?;
        Ok(result)
    })
}

/// Enumerate exact snapshot-scoped references, never drawing/preview indices.
pub fn references(path: &str, expected_sha: &str) -> Result<Value, String> {
    with_session(path, expected_sha, |entry| {
        current_source(entry)?;
        let value = entry.call("v1.references", &json!({"source_sha256":expected_sha}))?;
        reference_results::discovery(&value, entry)?;
        current_source(entry)?;
        Ok(value)
    })
}

/// Evaluate a trimmed exact edge at its arc-length fraction in source-world millimetres.
pub fn resolve_point(path: &str, expected_sha: &str, reference: &str, fraction: f64) -> Result<Value, String> {
    reference_results::request(reference, fraction)?;
    with_session(path, expected_sha, |entry| {
        current_source(entry)?;
        let value = entry.call("v1.resolve_point", &json!({"source_sha256":expected_sha,"reference":reference,"fraction":fraction}))?;
        reference_results::resolution(&value, entry, fraction)?;
        current_source(entry)?;
        Ok(value)
    })
}

/// Request a transient display-only preview of the same immutable snapshot.
pub fn preview(path: &str, expected_sha: &str, detail: u8) -> Result<Preview, String> {
    if !(1..=8).contains(&detail) {
        return Err("preview detail must be in 1..8".into());
    }
    with_session(path, expected_sha, |entry| {
        let value = entry.call("v1.preview", &json!({"source_sha256": expected_sha, "detail": detail}))?;
        check_source(&value, &entry.source)?;
        let preview = model_preview::from_worker(value, expected_sha).map_err(|error| error.to_string())?;
        if preview.detail != detail || preview.bodies.len() != entry.bodies.len() {
            return Err("invalid worker preview: detail or body count differs from request/load".into());
        }
        for (mesh, body) in preview.bodies.iter().zip(&entry.bodies) {
            let face_count =
                mesh.tri_face.iter().map(|&face| face as usize).chain(mesh.missing_faces.iter().copied()).max().map_or(0, |face| face + 1);
            if face_count != body.faces {
                return Err("invalid worker preview: face coverage differs from loaded topology".into());
            }
        }
        Ok(preview)
    })
}

// Bound the on-disk revision on both sides of exact projection without replacing the cache.
fn current_source(entry: &Session) -> Result<(), String> {
    let current = fingerprint(Path::new(&entry.path))?;
    require_digest(&current, &entry.source.sha256)?;
    if current.bytes != entry.source.bytes {
        return Err("stale_source: source byte count differs from linked revision; refresh explicitly".into());
    }
    Ok(())
}

/// Hash bounded regular-file bytes only: no worker, import, evaluation, or session mutation.
/// Missing, denied, nonregular and oversized sources return errors rather than fake status.
pub fn source_status(path: &str, expected_sha: &str, bytes: u64) -> Result<Value, String> {
    validate_digest(expected_sha)?;
    if bytes > MAX_FILE_BYTES {
        return Err("invalid loaded source byte count (maximum 8 MiB)".into());
    }
    let path = canonical_source(path)?;
    let current = fingerprint(Path::new(&path))?;
    Ok(json!({"loaded": {"sha256": expected_sha, "bytes": bytes},
        "current": {"sha256": current.sha256, "bytes": current.bytes},
        "changed": current.sha256 != expected_sha || current.bytes != bytes}))
}

fn with_session<T>(path: &str, expected_sha: &str, query: impl FnOnce(&mut Session) -> Result<T, String>) -> Result<T, String> {
    validate_digest(expected_sha)?;
    validate_path(path)?;
    let mut cache = sessions()?;
    // Check the supplied canonical snapshot path first: deletion must not lose cached geometry.
    let index = if let Some(index) = cache.iter().position(|entry| entry.path == path && entry.source.sha256 == expected_sha) {
        index
    } else {
        let canonical = canonical_source(path)?;
        if let Some(index) = cache.iter().position(|entry| entry.path == canonical && entry.source.sha256 == expected_sha) {
            index
        } else {
            let source = fingerprint(Path::new(&canonical))?;
            require_digest(&source, expected_sha)?;
            let entry = import(&canonical, source)?;
            insert(&mut cache, entry)
        }
    };
    if let Some(error) = &cache[index].failed {
        return Err(error.clone());
    }
    if cache[index].calls >= RENEW_AT {
        // Import separately; failed renewal never replaces the prior valid snapshot.
        let current = fingerprint(Path::new(&cache[index].path))?;
        require_digest(&current, expected_sha)?;
        if current.bytes != cache[index].source.bytes {
            return Err("stale_source: source byte count differs from loaded snapshot".into());
        }
        let entry = import(&cache[index].path, current)?;
        cache[index] = entry;
    }
    query(&mut cache[index])
}

fn insert(cache: &mut Vec<Session>, entry: Session) -> usize {
    if cache.len() == MAX_SESSIONS {
        cache.remove(0);
    }
    cache.push(entry);
    cache.len() - 1
}

fn import(path: &str, expected: Fingerprint) -> Result<Session, String> {
    let root = Path::new(path).parent().ok_or("model source has no parent directory")?;
    let mut client = model_worker::Client::spawn(&worker_program()?, root).map_err(|error| rpc_error(&error))?;
    let capabilities = client.call("v1.capabilities", &json!({})).map_err(|error| rpc_error(&error))?;
    validate_capabilities(&capabilities)?;
    let mut metadata = client.call("v1.load", &json!({"path": path})).map_err(|error| rpc_error(&error))?;
    check_source(&metadata, &expected)?;
    let bodies = validate_load(&metadata, path, &expected.sha256)?;
    metadata.as_object_mut().ok_or("invalid worker load object")?.insert("path".into(), Value::String(path.into()));
    Ok(Session { path: path.into(), source: expected, metadata, bodies, client, calls: 2, failed: None })
}

fn worker_program() -> Result<PathBuf, String> {
    if let Some(program) = std::env::var_os("CADCRAFTGOST_MODEL_WORKER") {
        if program.is_empty() {
            return Err("CADCRAFTGOST_MODEL_WORKER must name an executable, not an empty path".into());
        }
        // Canonicalizing the explicit override prevents Command's bare-name PATH search.
        return PathBuf::from(program).canonicalize().map_err(|error| format!("cannot resolve CADCRAFTGOST_MODEL_WORKER: {error}"));
    }
    let executable = std::env::current_exe().map_err(|error| format!("cannot locate host executable: {error}"))?;
    let directory = executable.parent().ok_or("host executable has no parent directory")?;
    let name = if cfg!(windows) { "cadcraftgost-model-worker.exe" } else { "cadcraftgost-model-worker" };
    Ok(directory.join(name))
}

fn validate_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > 4096 {
        return Err("model source must be a nonempty UTF-8 path of at most 4096 bytes".into());
    }
    Ok(())
}

fn canonical_source(path: &str) -> Result<String, String> {
    validate_path(path)?;
    let canonical = Path::new(path).canonicalize().map_err(|error| format!("model source file access failed ({path}): {error}"))?;
    let text = canonical.to_str().ok_or("canonical model source path is not UTF-8")?;
    validate_path(text)?;
    source_format(text)?;
    Ok(text.into())
}

fn source_format(path: &str) -> Result<&'static str, String> {
    match Path::new(path).extension().and_then(|extension| extension.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("sc") => Ok("sc"),
        Some("step" | "stp") => Ok("step"),
        Some("iges" | "igs") => Ok("iges"),
        _ => Err("unsupported model source: select SC/STEP/STP/IGES/IGS".into()),
    }
}

fn fingerprint(path: &Path) -> Result<Fingerprint, String> {
    let io_error = |error| format!("model source file access failed ({}): {error}", path.display());
    // Reject ordinary nonregular paths before opening (notably FIFOs). This local policy
    // is not protection against concurrent hostile directory-entry replacement.
    let metadata = std::fs::metadata(path).map_err(io_error)?;
    check_file(&metadata)?;
    let mut file = File::open(path).map_err(io_error)?;
    check_file(&file.metadata().map_err(io_error)?)?;
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 8192];
    loop {
        // Read at most one byte past the bound, even if the file grows after metadata.
        let limit = (MAX_FILE_BYTES + 1 - bytes).min(buffer.len() as u64) as usize;
        let count = file.read(&mut buffer[..limit]).map_err(io_error)?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > MAX_FILE_BYTES {
            return Err("model source resource limit: file exceeds 8 MiB".into());
        }
        hasher.update(&buffer[..count]);
    }
    Ok(Fingerprint { sha256: format!("{:x}", hasher.finalize()), bytes })
}

fn check_file(metadata: &std::fs::Metadata) -> Result<(), String> {
    if !metadata.is_file() {
        return Err("model source path denied: only regular local files are accepted".into());
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err("model source resource limit: file exceeds 8 MiB".into());
    }
    Ok(())
}

fn validate_digest(digest: &str) -> Result<(), String> {
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
        return Err("invalid source SHA-256: expected 64 lowercase hexadecimal characters".into());
    }
    Ok(())
}

fn require_digest(source: &Fingerprint, expected: &str) -> Result<(), String> {
    if source.sha256 != expected {
        return Err(format!("stale_source: expected SHA-256 {expected}, current {}", source.sha256));
    }
    Ok(())
}

fn check_source(value: &Value, expected: &Fingerprint) -> Result<(), String> {
    if value.pointer("/source/sha256").and_then(Value::as_str) != Some(expected.sha256.as_str()) {
        return Err("stale_source: worker result belongs to another source snapshot".into());
    }
    if value.pointer("/source/bytes").and_then(Value::as_u64) != Some(expected.bytes) {
        return Err("invalid worker result: source byte count differs from loaded snapshot".into());
    }
    Ok(())
}

fn normalized_units(value: &Value) -> Result<(), String> {
    for (key, expected) in [
        ("length", "mm"),
        ("area", "mm2"),
        ("volume", "mm3"),
        ("angle", "radian"),
        ("coordinate_system", "right_handed_z_up"),
        ("original_declaration", "not_exposed_by_selected_import_api"),
    ] {
        if value.get("units").and_then(|units| units.get(key)).and_then(Value::as_str) != Some(expected) {
            return Err(format!("unsupported model worker normalized units: {key}"));
        }
    }
    Ok(())
}

fn validate_capabilities(value: &Value) -> Result<(), String> {
    if value.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
        return Err("unsupported model worker protocol (require cadcraft-model-worker/1)".into());
    }
    normalized_units(value)?;
    for (key, required) in [
        ("methods", &["v1.capabilities", "v1.load", "v1.view", "v1.preview", "v1.source_status", "v1.references", "v1.resolve_point"][..]),
        ("formats", &["sc", "step", "stp", "iges", "igs"][..]),
        ("curves", &["line", "arc", "cubic", "conic", "bezier"][..]),
    ] {
        let advertised = value.get(key).and_then(Value::as_array).ok_or_else(|| format!("invalid model worker capabilities: {key}"))?;
        if required.iter().any(|item| !advertised.iter().any(|entry| entry.as_str() == Some(*item))) {
            return Err(format!("model worker lacks required {key}"));
        }
    }
    for (key, expected) in [
        ("protocol", "cadcraft-references/1"),
        ("discovery", "per_placement_exact_edges_and_faces_with_explicit_verification"),
        ("fraction", "trimmed_edge_arc_length_start_to_end"),
        ("native_continuity", "same_canonical_path_and_full_document_except_literal_independent_sizes_or_simple_extrude_distance/1"),
        ("unverified_names", "snapshot_only_never_promoted_or_nearest_matched"),
        ("step_iges", "payload_scoped_only"),
    ] {
        if value["references"][key].as_str() != Some(expected) {
            return Err(format!("unsupported model worker reference contract: {key}"));
        }
    }
    if value["references"]["token_bytes"].as_u64() != Some(8192)
        || value["references"]["discovery_token_bytes_total"].as_u64() != Some(4 * 1024 * 1024)
    {
        return Err("unsupported model worker reference limits".into());
    }
    if value.get("projection").and_then(Value::as_str) != Some("orthographic_raw_hlr")
        || value.get("placements").and_then(Value::as_str) != Some("all_imported_bodies_already_placed_including_hidden")
        || value.get("models_per_session").and_then(Value::as_u64) != Some(1)
    {
        return Err("unsupported model worker projection/placement/session contract".into());
    }
    for (key, expected) in [("requests", 256), ("file_bytes", MAX_FILE_BYTES), ("bodies", 32), ("topology_items", 10_000)] {
        if value.get("limits").and_then(|limits| limits.get(key)).and_then(Value::as_u64) != Some(expected) {
            return Err(format!("unsupported model worker limit: {key}"));
        }
    }
    Ok(())
}

fn validate_load(value: &Value, path: &str, sha: &str) -> Result<Vec<Body>, String> {
    if value.get("protocol").and_then(Value::as_str) != Some(PROTOCOL)
        || value.get("format").and_then(Value::as_str) != Some(source_format(path)?)
        || value.get("checks_settled").and_then(Value::as_bool) != Some(true)
        || value.get("tree_transform_convention").and_then(Value::as_str) != Some("column_major_parent_mm_do_not_reapply_to_bodies")
    {
        return Err("invalid worker load protocol/format/settled placement contract".into());
    }
    normalized_units(value)?;
    let descriptions = value
        .get("bodies")
        .and_then(Value::as_array)
        .filter(|bodies| !bodies.is_empty() && bodies.len() <= 32)
        .ok_or("invalid worker load body count (require 1..32)")?;
    let mut bodies = Vec::with_capacity(descriptions.len());
    let mut topology = 0_u64;
    for (index, body) in descriptions.iter().enumerate() {
        if body.get("placement").and_then(Value::as_u64) != Some(index as u64)
            || body.get("body").and_then(Value::as_u64) != Some(index as u64)
            || body.get("identity_scope").and_then(Value::as_str) != Some(sha)
            || body.get("edge_identity").and_then(Value::as_str) != Some("result_local_only")
            || body.get("closed").and_then(Value::as_bool).is_none()
            || body.get("hidden").and_then(Value::as_bool).is_none()
        {
            return Err("invalid worker load body identity/placement/flags".into());
        }
        let count = |key| {
            body.get("topology")
                .and_then(|counts| counts.get(key))
                .and_then(Value::as_u64)
                .filter(|&n| n <= 10_000)
                .ok_or_else(|| format!("invalid worker body topology: {key}"))
        };
        let (faces, edges, vertices, shells) = (count("faces")?, count("edges")?, count("vertices")?, count("shells")?);
        topology += faces + edges + vertices + shells;
        if topology > 10_000 {
            return Err("worker load resource limit: total topology items".into());
        }
        bodies.push(Body { faces: faces as usize, edges: edges as usize });
    }
    Ok(bodies)
}

fn validate_vectors(direction: [f64; 3], up: [f64; 3]) -> Result<(), String> {
    if direction.iter().chain(&up).any(|value| !value.is_finite() || value.abs() > 1e6) {
        return Err("view vectors must be finite with components at most 1e6".into());
    }
    let cross =
        [direction[1] * up[2] - direction[2] * up[1], direction[2] * up[0] - direction[0] * up[2], direction[0] * up[1] - direction[1] * up[0]];
    if cross.iter().all(|&value| value == 0.0) {
        return Err("view direction and up must be nonzero and nonparallel".into());
    }
    Ok(())
}

fn rpc_error(error: &model_worker::Error) -> String {
    match error {
        model_worker::Error::Remote { code, message, data } => format!("model worker RPC {code}: {message}; data={data:?}"),
        _ => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn load_result() -> Value {
        json!({"protocol": PROTOCOL, "source":{"sha256":SHA,"bytes":42}, "format":"step",
            "units":{"length":"mm","area":"mm2","volume":"mm3","angle":"radian",
                "coordinate_system":"right_handed_z_up","original_declaration":"not_exposed_by_selected_import_api"},
            "checks_settled":true,"tree_transform_convention":"column_major_parent_mm_do_not_reapply_to_bodies",
            "bodies":[{"placement":0,"body":0,"identity_scope":SHA,"edge_identity":"result_local_only",
                "closed":true,"hidden":false,"topology":{"faces":6,"edges":12,"vertices":8,"shells":1}}]})
    }

    #[test]
    fn handshake_requires_protocol_methods_units_and_matching_limits() {
        let valid = json!({"protocol":PROTOCOL,"units":load_result()["units"],
            "methods":["v1.capabilities","v1.load","v1.view","v1.preview","v1.source_status","v1.references","v1.resolve_point"],
            "formats":["sc","step","stp","iges","igs"],
            "curves":["line","arc","cubic","conic","bezier"],
            "projection":"orthographic_raw_hlr",
            "placements":"all_imported_bodies_already_placed_including_hidden",
            "models_per_session":1,
            "references":{"protocol":"cadcraft-references/1","token_bytes":8192,"discovery_token_bytes_total":4*1024*1024,
                "discovery":"per_placement_exact_edges_and_faces_with_explicit_verification","fraction":"trimmed_edge_arc_length_start_to_end",
                "native_continuity":"same_canonical_path_and_full_document_except_literal_independent_sizes_or_simple_extrude_distance/1",
                "unverified_names":"snapshot_only_never_promoted_or_nearest_matched","step_iges":"payload_scoped_only"},
            "limits":{"requests":256,"file_bytes":MAX_FILE_BYTES,"bodies":32,"topology_items":10_000}});
        assert!(validate_capabilities(&valid).is_ok());
        for (pointer, replacement) in [
            ("/protocol", json!("cadcraft-model-worker/2")),
            ("/methods", json!(["v1.load", "v1.view"])),
            ("/units/length", json!("inch")),
            ("/limits/requests", json!(128)),
            ("/models_per_session", json!(2)),
            ("/references/protocol", json!("other")),
            ("/references/fraction", json!("curve_parameter")),
            ("/references/token_bytes", json!(16384)),
        ] {
            let mut invalid = valid.clone();
            if let Some(field) = invalid.pointer_mut(pointer) {
                *field = replacement;
            }
            assert!(validate_capabilities(&invalid).is_err(), "{pointer}");
        }
    }

    #[test]
    fn load_contract_rejects_identity_units_counts_and_unsettled_checks() {
        let valid = load_result();
        assert!(validate_load(&valid, "/selected.STP", SHA).is_ok());
        for (pointer, replacement) in [
            ("/bodies/0/identity_scope", json!("other")),
            ("/bodies/0/placement", json!(1)),
            ("/bodies/0/topology/faces", json!(10_000)),
            ("/units/length", json!("inch")),
            ("/checks_settled", json!(false)),
            ("/bodies", json!([])),
            ("/bodies", json!(vec![valid["bodies"][0].clone(); 33])),
        ] {
            let mut invalid = valid.clone();
            if let Some(field) = invalid.pointer_mut(pointer) {
                *field = replacement;
            }
            assert!(validate_load(&invalid, "/selected.step", SHA).is_err(), "{pointer}");
        }
    }

    #[test]
    fn source_identity_requires_digest_and_byte_count() {
        let expected = Fingerprint { sha256: SHA.into(), bytes: 42 };
        assert!(check_source(&load_result(), &expected).is_ok());
        let mut changed = load_result();
        changed["source"]["bytes"] = json!(43);
        assert!(check_source(&changed, &expected).is_err());
        assert!(require_digest(&expected, &"0".repeat(64)).is_err_and(|error| error.starts_with("stale_source:")));
        assert!(validate_digest(SHA).is_ok());
        assert!(validate_digest(&SHA.to_uppercase()).is_err());
        assert!(validate_path(&"a".repeat(4096)).is_ok());
        assert!(validate_path(&"я".repeat(2049)).is_err());
    }

    #[test]
    fn invalid_query_arguments_fail_before_worker_access() {
        assert!(validate_vectors([0.0; 3], [0.0, 0.0, 1.0]).is_err());
        assert!(validate_vectors([1.0, 0.0, 0.0], [2.0, 0.0, 0.0]).is_err());
        assert!(validate_vectors([f64::NAN, 0.0, 1.0], [0.0, 1.0, 0.0]).is_err());
        assert!(validate_vectors([1.0, 1.0, 1.0], [0.0, 0.0, 1.0]).is_ok());
        assert!(preview("missing.step", SHA, 0).is_err());
        assert!(source_status("missing.step", SHA, MAX_FILE_BYTES + 1).is_err());
    }

    #[test]
    fn source_status_detects_same_size_edits_empty_files_and_missing_sources() {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let path = std::env::temp_dir().join(format!(
            "cadcraftgost-source-status-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        let cleanup = Cleanup(path);
        let source = cleanup.0.join("model.step");
        let path = source.to_str().unwrap();
        std::fs::write(&source, b"abc").unwrap();
        let initial = source_status(path, SHA, 3).unwrap();
        let linked = initial["current"]["sha256"].as_str().unwrap();
        assert_eq!(source_status(path, linked, 3).unwrap()["changed"], false);
        std::fs::write(&source, b"xyz").unwrap();
        let changed = source_status(path, linked, 3).unwrap();
        assert_eq!(changed["current"]["bytes"], 3);
        assert_eq!(changed["changed"], true);
        std::fs::write(&source, b"").unwrap();
        let empty = source_status(path, linked, 3).unwrap();
        assert_eq!(empty["current"]["bytes"], 0);
        assert_eq!(empty["changed"], true);
        std::fs::remove_file(&source).unwrap();
        assert!(source_status(path, linked, 3).is_err());
    }
}
