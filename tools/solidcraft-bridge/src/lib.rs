//! Isolated native SolidCraft/STEP/IGES to raw analytic HLR worker; no GUI dependencies.
//! Coordinates and measures are normalized by the upstream importer to millimetres.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use solidcore_record::Vec3;
use solidcraft_geom::{hlr, section::CurveAccuracy};
use solidcraft_kernel::{
    StepImport,
    watch::{self, Watch},
};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

mod native;
mod preview;
mod references;

pub const UPSTREAM_REV: &str = "c5f47e501169c7f0f9e8191b2541e689e2f018e8";
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_REQUESTS: usize = 256;
pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_BODIES: usize = 32;
const MAX_TOPOLOGY: usize = 10_000;
const MAX_REFERENCES: usize = 100_000;
const MAX_DEPTH: usize = 32;
const MAX_TRACKS: usize = 4096;
const MAX_RAYS: usize = 500_000;
const WORK_SECONDS: u64 = 30;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid parameters: {0}")]
    Parameters(String),
    #[error("local path denied: {0}")]
    Path(String),
    #[error("file access failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("resource limit exceeded: {0}")]
    Limit(&'static str),
    #[error("model import failed: {0}")]
    Import(String),
    #[error("exact geometry unavailable: {0}")]
    Geometry(String),
    #[error("no model loaded; call v1.load first")]
    NoModel,
    #[error("loaded source fingerprint does not match request")]
    StaleModel,
    #[error("unsupported method: {0}")]
    Method(String),
    #[error("internal reusable-kernel failure; model was discarded")]
    Internal,
    #[error("response encoding failed: {0}")]
    Encoding(#[from] serde_json::Error),
}

impl Error {
    pub fn rpc(self) -> craft_jsonrpc::RpcError {
        let (code, kind) = match &self {
            Self::Parameters(_) => (-32602, "invalid_parameters"),
            Self::Method(_) => (-32601, "unknown_method"),
            Self::Path(_) => (-32001, "path_denied"),
            Self::Io(_) => (-32002, "file_access"),
            Self::Limit(_) => (-32003, "resource_limit"),
            Self::Import(_) => (-32004, "import_failed"),
            Self::Geometry(_) => (-32005, "geometry_unavailable"),
            Self::NoModel => (-32006, "no_model"),
            Self::StaleModel => (-32007, "stale_model"),
            Self::Internal | Self::Encoding(_) => (-32603, "internal_error"),
        };
        craft_jsonrpc::RpcError { code, message: self.to_string(), data: Some(json!({"kind": kind})) }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Fingerprint {
    pub sha256: String,
    pub bytes: usize,
}

fn fingerprint(bytes: &[u8]) -> Fingerprint {
    Fingerprint { sha256: format!("{:x}", Sha256::digest(bytes)), bytes: bytes.len() }
}

struct Budget {
    started: Instant,
    progress_calls: AtomicUsize,
    over_limit: AtomicBool,
}

impl Budget {
    fn new() -> Self {
        Self { started: Instant::now(), progress_calls: AtomicUsize::new(0), over_limit: AtomicBool::new(false) }
    }
    fn check(&self) -> Result<(), Error> {
        if self.cancelled() { Err(Error::Limit("cooperative geometry work/deadline")) } else { Ok(()) }
    }
}

impl Watch for Budget {
    fn cancelled(&self) -> bool {
        self.over_limit.load(Ordering::Relaxed) || self.started.elapsed() >= Duration::from_secs(WORK_SECONDS)
    }
    fn progress(&self, _stage: &'static str, _done: usize, total: usize) {
        if total > MAX_REFERENCES || self.progress_calls.fetch_add(1, Ordering::Relaxed) >= MAX_REFERENCES {
            self.over_limit.store(true, Ordering::Relaxed);
        }
    }
}

struct LoadedBody {
    body: solidcraft_kernel::Body,
    closed: bool,
    refused: bool,
}

struct Model {
    requested_path: PathBuf,
    canonical_path: PathBuf,
    format: &'static str,
    native: Option<references::Native>,
    fingerprint: Fingerprint,
    bodies: Vec<LoadedBody>,
    import_issues: usize,
}

pub struct Worker {
    roots: Vec<PathBuf>,
    model: Option<Model>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Load {
    path: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewQuery {
    source_sha256: String,
    direction: [f64; 3],
    up: [f64; 3],
    #[serde(default)]
    require_analytic: bool,
}

#[derive(Default, Serialize)]
struct Quality {
    unsupported_faces: usize,
    uncertain_pieces: usize,
    bounded_pieces: usize,
    sampled_pieces: usize,
    unsupported_trims: usize,
    import_issues: usize,
    open_bodies: usize,
    analytic_complete: bool,
}

fn params<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|e| Error::Parameters(e.to_string()))
}

impl Worker {
    /// Explicit allowed directories only; this is a local-file policy, not an OS sandbox.
    pub fn new(roots: Vec<PathBuf>) -> Result<Self, Error> {
        if roots.is_empty() || roots.len() > 8 {
            return Err(Error::Parameters("supply 1..8 explicit --allow-path directories".into()));
        }
        let roots = roots
            .into_iter()
            .map(|root| {
                let root = root.canonicalize()?;
                if !root.is_dir() {
                    return Err(Error::Path("allowed root must be a directory".into()));
                }
                Ok(root)
            })
            .collect::<Result<Vec<_>, Error>>()?;
        solidcraft_kernel::start();
        Ok(Self { roots, model: None })
    }

    /// Recover from an escaped upstream panic, and discard possibly inconsistent state.
    pub fn call(&mut self, method: &str, parameters: Value) -> Result<Value, Error> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.dispatch(method, parameters))) {
            Ok(result) => result,
            Err(_) => {
                self.model = None;
                Err(Error::Internal)
            }
        }
    }

    fn dispatch(&mut self, method: &str, parameters: Value) -> Result<Value, Error> {
        match method {
            "v1.capabilities" => {
                empty_params(parameters)?;
                Ok(json!({
                    "protocol": "cadcraft-model-worker/1", "stage": "prototype",
                    "upstream": {"revision": UPSTREAM_REV, "version": "0.45.0-rc.4"},
                    "methods": ["v1.capabilities", "v1.load", "v1.view", "v1.preview", "v1.source_status", "v1.references", "v1.resolve_point"],
                    "formats": ["sc", "step", "stp", "iges", "igs"],
                    "projection": "orthographic_raw_hlr", "curves": ["line", "arc", "cubic", "conic", "bezier"],
                    "units": units(), "placements": "all_imported_bodies_already_placed_including_hidden",
                    "notifications": "ignored_without_side_effects", "models_per_session": 1,
                    "native": {"reader": "upstream_document_migrations_and_timeline_evaluation",
                        "containers": ["solidcraft-container/1", "solvecraft-container/1"],
                        "model_cache": "never_trusted_or_required",
                        "assembly": "rigid_nested_occurrences_expressed_moves_and_settled_upstream_joint_placements",
                        "metadata": "saved_design_component_properties_and_explicit_body_material_assignments",
                        "identity": "project_local_ids_never_globally_unique; bounded_verified_same_path_semantic_continuity",
                        "unsupported": ["unsettled_or_conflicting_joint_assemblies", "external_fonts", "mesh_features",
                            "newer_unknown_geometry", "non_import_resource_blobs"]},
                    "references": {"protocol": "cadcraft-references/1", "token_bytes": references::MAX_TOKEN_BYTES,
                        "discovery_token_bytes_total": references::MAX_DISCOVERY_TOKEN_BYTES,
                        "discovery": "per_placement_exact_edges_and_faces_with_explicit_verification",
                        "fraction": "trimmed_edge_arc_length_start_to_end",
                        "native_continuity": "same_canonical_path_and_full_document_except_literal_independent_sizes_or_simple_extrude_distance/1",
                        "unverified_names": "snapshot_only_never_promoted_or_nearest_matched",
                        "step_iges": "payload_scoped_only"},
                    "limits": {"request_bytes": MAX_MESSAGE_BYTES, "response_bytes": MAX_RESPONSE_BYTES,
                        "requests": MAX_REQUESTS, "batch_items": 1, "file_bytes": MAX_FILE_BYTES,
                        "bodies": MAX_BODIES, "topology_items": MAX_TOPOLOGY, "nesting": MAX_DEPTH,
                        "native_container_entries": 10_000, "native_expanded_bytes": MAX_FILE_BYTES,
                        "native_joints": native::MAX_JOINTS, "native_links": native::MAX_JOINTS,
                        "references": MAX_REFERENCES, "tracks": MAX_TRACKS, "rays": MAX_RAYS,
                        "preview_triangles": preview::MAX_TRIANGLES, "preview_vertices": preview::MAX_VERTICES,
                        "cooperative_work_seconds": WORK_SECONDS, "supervised_hard_work_seconds": 40, "hlr_threads": 1},
                    "gaps": ["general_persistent_topology_remapping",
                        "perspective", "sections", "certified_freeform_silhouettes",
                        "hard_memory_sandbox", "original_step_iges_unit_declaration", "path_race_against_concurrent_local_directory_mutation"]
                }))
            }
            "v1.load" => self.load(params(parameters)?),
            "v1.view" => self.view(params(parameters)?),
            "v1.preview" => self.preview(params(parameters)?),
            "v1.references" => self.references(parameters),
            "v1.resolve_point" => self.resolve_point(parameters),
            "v1.source_status" => {
                empty_params(parameters)?;
                self.source_status()
            }
            _ => Err(Error::Method(method.into())),
        }
    }

    fn read_source(&self, path: &Path) -> Result<Vec<u8>, Error> {
        if !path.is_absolute() || path.as_os_str().len() > 4096 {
            return Err(Error::Path("supply an absolute path of at most 4096 bytes".into()));
        }
        let canonical = path.canonicalize()?;
        if !self.roots.iter().any(|root| canonical.starts_with(root)) {
            return Err(Error::Path("file is outside explicit allowed roots".into()));
        }
        let file = File::open(canonical)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(Error::Path("only regular local files are accepted".into()));
        }
        if metadata.len() > MAX_FILE_BYTES as u64 {
            return Err(Error::Limit("source file bytes"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(Error::Limit("source file bytes"));
        }
        Ok(bytes)
    }

    fn load(&mut self, request: Load) -> Result<Value, Error> {
        let extension = request
            .path
            .extension()
            .and_then(|x| x.to_str())
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| Error::Parameters("file extension must be SC/STEP/STP/IGES/IGS".into()))?;
        let is_step = match extension.as_str() {
            "step" | "stp" => true,
            "sc" | "iges" | "igs" => false,
            _ => return Err(Error::Parameters("file extension must be SC/STEP/STP/IGES/IGS".into())),
        };
        let bytes = self.read_source(&request.path)?;
        let source = fingerprint(&bytes);
        let canonical_path = request.path.canonicalize()?;
        if canonical_path.as_os_str().len() > 4096 {
            return Err(Error::Path("canonical project path exceeds 4096 bytes".into()));
        }
        if extension == "sc" {
            let loaded = native::load(&bytes, &source)?;
            ensure_response_size(&loaded.result)?;
            self.model = Some(Model {
                requested_path: request.path,
                canonical_path,
                format: "sc",
                native: Some(loaded.evaluated),
                fingerprint: source,
                bodies: loaded.bodies,
                import_issues: loaded.import_issues,
            });
            return Ok(loaded.result);
        }
        let text = std::str::from_utf8(&bytes).map_err(|e| Error::Import(format!("UTF-8/ASCII text required: {e}")))?;
        if is_step {
            preflight_step(text)?;
        }
        let budget = Arc::new(Budget::new());
        let import =
            watch::watching(budget.clone(), || if is_step { solidcraft_kernel::step_import(text) } else { solidcraft_kernel::iges_import(text) })
                .map_err(|e| if budget.cancelled() { Error::Limit("import work/deadline") } else { Error::Import(e.to_string()) })?;
        budget.check()?;
        validate_import(&import)?;
        let mut bodies = Vec::new();
        for (index, imported) in import.bodies.iter().enumerate() {
            budget.check()?;
            let measure = exact_measure(&imported.body, imported.closed);
            let record = imported.body.record();
            bodies.push(json!({"placement": index, "body": index,
                "source_item": imported.source_item, "face_origin": imported.face_origin,
                "identity_scope": source.sha256, "edge_identity": "result_local_only",
                "name": imported.name, "color": imported.color, "path": imported.path,
                "instance": imported.instance, "hidden": imported.hidden, "closed": imported.closed,
                "file_faces": imported.file_faces, "refused": imported.refused,
                "topology": {"faces": record.faces.len(), "edges": record.edges.len(),
                    "vertices": record.vertices.len(), "shells": record.shells.len()},
                "measure": measure}));
        }
        budget.check()?;
        let result = json!({"protocol": "cadcraft-model-worker/1", "source": source,
            "format": if is_step { "step" } else { "iges" }, "units": units(),
            "source_identity": if is_step { "payload_sha256_and_step_entity" } else { "payload_sha256_and_generated_step_entity_not_original_iges_identity" },
            "metadata": {"schema": import.schema, "system": import.system,
                "preprocessor": import.preprocessor, "description": import.description,
                "engineering_material": null, "density": null, "part_number": null},
            "bodies": bodies, "tree": import.tree, "tree_transform_convention": "column_major_parent_mm_do_not_reapply_to_bodies",
            "warnings": import.warnings, "report": import.report,
            "checks_settled": !import.report.checking()});
        ensure_response_size(&result)?;
        let import_issues = import.report.issues.len() + import.warnings.len();
        let bodies = import.bodies.into_iter().map(|b| LoadedBody { body: b.body, closed: b.closed, refused: b.refused.is_some() }).collect();
        self.model = Some(Model {
            requested_path: request.path,
            canonical_path,
            format: if is_step { "step" } else { "iges" },
            native: None,
            fingerprint: source,
            bodies,
            import_issues,
        });
        Ok(result)
    }

    fn source_status(&self) -> Result<Value, Error> {
        let model = self.model.as_ref().ok_or(Error::NoModel)?;
        let current = fingerprint(&self.read_source(&model.requested_path)?);
        Ok(json!({"loaded": model.fingerprint, "current": current,
            "changed": current != model.fingerprint, "identity_remap": "none"}))
    }

    fn view(&self, request: ViewQuery) -> Result<Value, Error> {
        let model = self.model.as_ref().ok_or(Error::NoModel)?;
        if request.source_sha256 != model.fingerprint.sha256 {
            return Err(Error::StaleModel);
        }
        let vector = |[x, y, z]: [f64; 3]| -> Result<Vec3, Error> {
            if [x, y, z].iter().any(|v| !v.is_finite() || v.abs() > 1e6) {
                return Err(Error::Parameters("view vectors must be finite, components at most 1e6".into()));
            }
            Ok(Vec3::new(x, y, z))
        };
        let view = hlr::View::new(vector(request.direction)?, vector(request.up)?).map_err(|e| Error::Parameters(e.to_string()))?;
        let options =
            hlr::Options { tangent_edges: true, freeform: false, trim_samples: 256, max_tracks: MAX_TRACKS, max_rays: MAX_RAYS, threads: 1 };
        let budget = Budget::new();
        let mut prepared = Vec::new();
        let mut placements = Vec::new();
        for (index, imported) in model.bodies.iter().enumerate() {
            budget.check()?;
            prepared.push(
                hlr::prepare_cancellable(imported.body.record(), &options, &|| budget.cancelled())
                    .map_err(|e| Error::Geometry(format!("body {index}: {e}")))?,
            );
            placements.push(hlr::Placement { body: index, rotation: [Vec3::X, Vec3::Y, Vec3::Z], translation: Vec3::ZERO });
        }
        let drawing =
            hlr::hidden_lines(&prepared, &placements, &view, &options, &|| budget.cancelled()).map_err(|e| Error::Geometry(e.to_string()))?;
        budget.check()?;
        let mut quality = Quality {
            unsupported_faces: drawing.unsupported.len(),
            unsupported_trims: prepared.iter().map(|p| p.report.unsupported_trims.len()).sum(),
            import_issues: model.import_issues,
            open_bodies: model.bodies.iter().filter(|b| !b.closed || b.refused).count(),
            ..Default::default()
        };
        for piece in drawing.lines.iter().flat_map(|line| &line.pieces) {
            if piece.visibility == hlr::Visibility::Uncertain {
                quality.uncertain_pieces += 1;
            }
            match piece.accuracy {
                CurveAccuracy::Analytic => {}
                CurveAccuracy::Bounded { .. } => quality.bounded_pieces += 1,
                CurveAccuracy::Sampled { .. } => quality.sampled_pieces += 1,
            }
        }
        quality.analytic_complete = quality.unsupported_faces == 0
            && quality.uncertain_pieces == 0
            && quality.bounded_pieces == 0
            && quality.sampled_pieces == 0
            && quality.unsupported_trims == 0
            && quality.import_issues == 0
            && quality.open_bodies == 0;
        if request.require_analytic && !quality.analytic_complete {
            return Err(Error::Geometry(format!("strict analytic view incomplete: {}", serde_json::to_string(&quality)?)));
        }
        let reports: Vec<_> = prepared.iter().map(|p| &p.report).collect();
        let result = json!({"source": model.fingerprint, "units": units(), "basis": {"x": view.x, "y": view.y, "direction": view.d},
            "scale": 1, "quality": quality, "prepare_reports": reports, "drawing": drawing,
            "source_mapping": "placement indexes v1.load bodies; edge/face are result-local record indices; face_origin when present maps faces to payload entities, not cross-rebuild topology",
            "visibility_contract": "raw_uncertain_and_covered_preserved_no_mesh_fallback"});
        ensure_response_size(&result)?;
        Ok(result)
    }
}

fn empty_params(value: Value) -> Result<(), Error> {
    if value.as_object().is_some_and(|p| p.is_empty()) { Ok(()) } else { Err(Error::Parameters("expected an empty object".into())) }
}

fn units() -> Value {
    json!({"length": "mm", "area": "mm2", "volume": "mm3", "angle": "radian", "coordinate_system": "right_handed_z_up",
        "original_declaration": "not_exposed_by_selected_import_api"})
}

fn validate_import(import: &StepImport) -> Result<(), Error> {
    validate_bodies(import.bodies.iter().map(|b| &b.body))?;
    let mut stack: Vec<_> = import.tree.iter().map(|n| (n, 1usize)).collect();
    let mut visited = 0usize;
    while let Some((node, depth)) = stack.pop() {
        visited += 1;
        if depth > MAX_DEPTH || visited > MAX_REFERENCES {
            return Err(Error::Limit("assembly depth/work"));
        }
        stack.extend(node.children.iter().map(|n| (n, depth + 1)));
    }
    Ok(())
}

/// Limit Part 21 nesting and reference work before handing text to the upstream reader.
/// Quotes (including doubled apostrophes) and block comments cannot affect these counts.
fn preflight_step(text: &str) -> Result<(), Error> {
    let mut bytes = text.bytes().peekable();
    let mut quoted = false;
    let mut comment = false;
    let mut depth = 0usize;
    let mut references = 0usize;
    while let Some(b) = bytes.next() {
        if comment {
            if b == b'*' && bytes.peek() == Some(&b'/') {
                bytes.next();
                comment = false;
            }
        } else if quoted {
            if b == b'\'' {
                if bytes.peek() == Some(&b'\'') {
                    bytes.next();
                } else {
                    quoted = false;
                }
            }
        } else {
            match b {
                b'\'' => quoted = true,
                b'/' if bytes.peek() == Some(&b'*') => {
                    bytes.next();
                    comment = true;
                }
                b'(' => {
                    depth += 1;
                    if depth > MAX_DEPTH {
                        return Err(Error::Limit("STEP expression depth"));
                    }
                }
                b')' => {
                    depth = depth.checked_sub(1).ok_or_else(|| Error::Import("unbalanced STEP parentheses".into()))?;
                }
                b'#' => {
                    references += 1;
                    if references > MAX_REFERENCES {
                        return Err(Error::Limit("STEP references"));
                    }
                }
                _ => {}
            }
        }
    }
    if quoted || comment || depth != 0 {
        return Err(Error::Import("unterminated STEP string/comment/expression".into()));
    }
    Ok(())
}

struct SizeWriter {
    written: usize,
}
impl std::io::Write for SizeWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.written = self.written.saturating_add(bytes.len());
        if self.written > MAX_RESPONSE_BYTES - 1024 {
            return Err(std::io::Error::other("response byte limit"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn ensure_response_size(value: &Value) -> Result<(), Error> {
    serde_json::to_writer(SizeWriter { written: 0 }, value).map_err(|_| Error::Limit("response bytes"))
}

fn validate_bodies<'a>(bodies: impl Iterator<Item = &'a solidcraft_kernel::Body>) -> Result<(), Error> {
    let mut topology = 0usize;
    let mut count = 0usize;
    for body in bodies {
        count += 1;
        if count > MAX_BODIES {
            return Err(Error::Limit("imported body count"));
        }
        if body.is_mesh() {
            return Err(Error::Geometry("mesh-only imported bodies are not supported".into()));
        }
        let record = body.record();
        for n in [record.faces.len(), record.edges.len(), record.vertices.len(), record.shells.len()] {
            topology = topology.checked_add(n).ok_or(Error::Limit("topology count overflow"))?;
        }
        if topology > MAX_TOPOLOGY {
            return Err(Error::Limit("total topology items"));
        }
    }
    if count == 0 {
        return Err(Error::Import("no exact B-rep bodies imported".into()));
    }
    Ok(())
}

fn exact_measure(body: &solidcraft_kernel::Body, closed: bool) -> Value {
    solidcraft_kernel::measure(body)
        .map_err(|e| Error::Geometry(e.to_string()))
        .and_then(|m| {
            if m.measured_by == "mesh" {
                return Err(Error::Geometry("mesh measures are not exact-surface measures".into()));
            }
            if !m.volume.is_finite() || !m.area.is_finite() || !m.centroid.is_finite() || !m.bbox.min.is_finite() || !m.bbox.max.is_finite() {
                return Err(Error::Geometry("non-finite exact-surface measure".into()));
            }
            Ok(json!({"status": "ok", "value": m, "volume_valid": closed,
                "provenance": "numerical_integration_on_exact_surfaces"}))
        })
        .unwrap_or_else(|e| json!({"status": "error", "message": e.to_string()}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn step_preflight_bounds_nesting_but_not_quoted_text() {
        assert!(preflight_step("#1=('((#',/* )# */'it''s okay');").is_ok());
        assert!(matches!(preflight_step(&"(".repeat(MAX_DEPTH + 1)), Err(Error::Limit(_))));
        assert!(preflight_step("#1=(").is_err());
        assert!(preflight_step("/* open").is_err());
    }
}
