//! Linked exact-source models and frozen, editable native drawing views.
//! Display previews never enter this module; source refresh never regenerates a view.
use super::data;
use crate::{
    Result, Session,
    cmd::{CommandSpec, bad},
    model_io,
};
use cadcraft_doc::model_projection::{Accuracy, Projection, Quality, Visibility};
use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Insert, RawTag, Space, foreign::ForeignObject};
use cadcraft_geom::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{self, Write},
    sync::Arc,
};

mod clipboard;
pub mod dimensions;
pub mod projections;
mod section_notation;
pub mod sections;

const RECORD: &str = "CADCRAFT_GOST_LINKED_MODELS";
const VIEW: &str = "linked-model-view";
const PIECE: &str = "linked-model-piece";
const MAX_BYTES: usize = 32 << 20;
const MAX_METADATA_BYTES: usize = 4 << 20;
const MAX_MODELS: usize = 128;
const MAX_VIEWS: usize = 512;
const MAX_ENTITIES: usize = 100_000;

pub(super) const EXTENSION: super::Extension = super::Extension {
    id: "models",
    version: 1,
    commands: specs,
    drawing_preset: None,
    document_changed: |s, e| {
        projections::after_command(s, e.before, e.uid)?;
        sections::after_command(s, e.before, e.uid)?;
        section_notation::after_command(s, e.before, e.uid)?;
        dimensions::after_command(s, e.before, e.uid)
    },
    command_override: None,
    copy: Some(clipboard::COPY_HOOKS),
    clipboard: Some(clipboard::HOOKS),
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    version: u32,
    next_id: u64,
    models: Vec<Model>,
    views: Vec<SavedView>,
}
impl Default for Store {
    fn default() -> Self {
        Self { version: 1, next_id: 1, models: Vec::new(), views: Vec::new() }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    sha256: String,
    bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Model {
    id: String,
    revision: u64,
    path: String,
    source: Source,
    metadata: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewLink {
    model_id: String,
    view: String,
    revision: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedView {
    handle: String,
    model_id: String,
    revision: u64,
    path: String,
    source: Source,
    metadata: Value,
    parameters: Value,
    basis: Value,
    quality: Quality,
    provenance: Value,
    warnings: Vec<String>,
}

fn err(message: impl Into<String>) -> crate::EngineError {
    bad("gost.model", message)
}
fn keys(p: &Value, allowed: &[&str]) -> Result<()> {
    let p = p.as_object().ok_or_else(|| err("parameters must be an object"))?;
    if p.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(err("unknown parameter"));
    }
    Ok(())
}
fn text<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| err(format!("{key}: nonempty string required")))
}
fn flag(p: &Value, key: &str, default: bool) -> Result<bool> {
    p.get(key).map_or(Ok(default), |v| v.as_bool().ok_or_else(|| err(format!("{key}: boolean required"))))
}
fn digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn absolute(path: &str) -> bool {
    // Understand both native host spellings when opening a drawing on another platform.
    let b = path.as_bytes();
    !path.contains('\0')
        && path.len() <= 32_768
        && (path.starts_with('/')
            || path.starts_with("\\\\")
            || (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && b"/\\".contains(&b[2])))
}
struct CappedWriter<W> {
    sink: W,
    remaining: usize,
}
impl<W: Write> Write for CappedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::other("serialized model record limit exceeded"));
        }
        self.sink.write_all(bytes)?;
        self.remaining -= bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.sink.flush()
    }
}
fn checked_metadata(metadata: &Value) -> Result<(String, Source)> {
    serde_json::to_writer(CappedWriter { sink: io::sink(), remaining: MAX_METADATA_BYTES }, metadata).map_err(|e| err(e.to_string()))?;
    let path = text(metadata, "path")?;
    if !absolute(path) {
        return Err(err("host must supply a canonical absolute source path"));
    }
    let source: Source = serde_json::from_value(metadata.get("source").cloned().ok_or_else(|| err("source metadata missing"))?)
        .map_err(|e| err(format!("invalid source metadata: {e}")))?;
    if !digest(&source.sha256) || source.bytes == 0 || source.bytes > 8 << 20 {
        return Err(err("invalid source fingerprint or size"));
    }
    if metadata.get("protocol").and_then(Value::as_str) != Some("cadcraft-model-worker/1")
        || !matches!(metadata.get("format").and_then(Value::as_str), Some("sc" | "step" | "iges"))
        || !metadata.get("bodies").and_then(Value::as_array).is_some_and(|a| !a.is_empty() && a.len() <= 32)
        || !metadata.get("metadata").is_some_and(Value::is_object)
        || metadata.get("tree").is_none()
    {
        return Err(err("incomplete or unsupported loaded model metadata"));
    }
    for (key, expected) in [("length", "mm"), ("area", "mm2"), ("volume", "mm3"), ("angle", "radian"), ("coordinate_system", "right_handed_z_up")] {
        if metadata.get("units").and_then(|v| v.get(key)).and_then(Value::as_str) != Some(expected) {
            return Err(err("loaded model units must be normalized millimetres/radians"));
        }
    }
    Ok((path.into(), source))
}
fn record(d: &Drawing) -> Result<Option<&ForeignObject>> {
    let mut entries = d.foreign.dictionary.iter().filter(|(key, _)| key.eq_ignore_ascii_case(RECORD));
    let Some((_, handle)) = entries.next() else { return Ok(None) };
    if entries.next().is_some() {
        return Err(err("duplicate linked-model dictionary entries"));
    }
    let mut objects = d.foreign.objects.iter().filter(|o| o.handle == *handle);
    let object =
        objects.next().filter(|o| o.kind.eq_ignore_ascii_case("XRECORD")).ok_or_else(|| err("linked-model entry must reference an XRECORD"))?;
    if objects.next().is_some() {
        return Err(err("duplicate linked-model object handles"));
    }
    Ok(Some(object))
}
fn load(d: &Drawing) -> Result<Store> {
    let Some(record) = record(d)? else { return Ok(Store::default()) };
    if record.tags.len() > 200_000 {
        return Err(err("linked-model XRECORD has too many chunks"));
    }
    let mut text = String::new();
    for chunk in record.tags.iter().filter(|t| t.code == 1) {
        if chunk.value.len() > 255 || chunk.value.len() > MAX_BYTES - text.len() {
            return Err(err("oversized linked-model XRECORD"));
        }
        text.push_str(&chunk.value);
    }
    let store: Store = serde_json::from_str(&text).map_err(|e| err(format!("invalid linked-model XRECORD: {e}")))?;
    validate_store(&store)?;
    Ok(store)
}
fn validate_store(store: &Store) -> Result<()> {
    if store.version != 1 || store.next_id == 0 || store.models.len() > MAX_MODELS || store.views.len() > MAX_VIEWS {
        return Err(err("unsupported linked-model version or record count"));
    }
    let mut ids = BTreeSet::new();
    for m in &store.models {
        let n = m.id.parse::<u64>().map_err(|_| err("invalid local model ID"))?;
        let (path, source) = checked_metadata(&m.metadata)?;
        if n == 0
            || n >= store.next_id
            || n.to_string() != m.id
            || !ids.insert(m.id.as_str())
            || m.revision == 0
            || m.path != path
            || m.source != source
        {
            return Err(err("inconsistent linked-model record"));
        }
    }
    let mut views = BTreeSet::new();
    for v in &store.views {
        let (path, source) = checked_metadata(&v.metadata)?;
        if Handle::parse_hex(&v.handle).is_none()
            || !views.insert(&v.handle)
            || !ids.contains(v.model_id.as_str())
            || v.revision == 0
            || v.revision > model(store, &v.model_id)?.revision
            || v.path != path
            || v.source != source
            || !v.provenance.get("lines").and_then(Value::as_array).is_some_and(|a| a.len() <= MAX_ENTITIES)
            || !v.provenance.get("unsupported").is_some_and(Value::is_array)
            || v.provenance.get("prepare_reports").is_none()
            || v.provenance.get("drawing_stats").is_none()
            || text(&v.provenance, "source_mapping").is_err()
            || text(&v.provenance, "visibility_contract").is_err()
            || v.parameters.get("id").and_then(Value::as_str) != Some(v.model_id.as_str())
        {
            return Err(err("inconsistent saved model view"));
        }
        validate_basis(&v.basis)?;
    }
    projections::validate_store_links(store)?;
    section_notation::validate_store(store)
}
fn tag(code: i32, value: impl Into<String>) -> RawTag {
    RawTag { code, value: value.into() }
}
fn save(d: &mut Drawing, store: &Store) -> Result<()> {
    // Never overwrite an unreadable/unknown record, including during asynchronous completion.
    load(d)?;
    validate_store(store)?;
    let mut encoded = CappedWriter { sink: Vec::new(), remaining: MAX_BYTES };
    serde_json::to_writer(&mut encoded, store).map_err(|e| err(e.to_string()))?;
    let text = String::from_utf8(encoded.sink).map_err(|e| err(e.to_string()))?;
    let existing = record(d)?.map(|o| o.handle);
    let handle = existing.unwrap_or_else(|| d.new_handle());
    let owner = d.foreign.targets.iter().find(|(_, role)| role.0 == "NOD" && role.1.is_empty()).map(|(h, _)| *h);
    let owner = owner.unwrap_or_else(|| d.new_handle());
    let mut tags = vec![tag(5, handle.hex()), tag(330, owner.hex()), tag(100, "AcDbXrecord"), tag(280, "1")];
    let mut rest = text.as_str();
    while !rest.is_empty() {
        let mut end = rest.len().min(240);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let (chunk, tail) = rest.split_at(end);
        tags.push(tag(1, chunk));
        rest = tail;
    }
    let foreign = Arc::make_mut(&mut d.foreign);
    foreign.targets.insert(owner, ("NOD".into(), String::new()));
    if let Some(object) = foreign.objects.iter_mut().find(|o| o.handle == handle) {
        object.tags = tags;
    } else {
        foreign.objects.push(ForeignObject { handle, kind: "XRECORD".into(), tags });
        foreign.dictionary.push((RECORD.into(), handle));
    }
    Ok(())
}
fn atomic(s: &mut Session, apply: impl FnOnce(&mut Session) -> Result<Value>) -> Result<Value> {
    let before = s.state()?.doc.clone();
    let result = apply(s);
    if result.is_err() {
        s.state_mut()?.doc = before;
    }
    result
}
fn model<'a>(store: &'a Store, id: &str) -> Result<&'a Model> {
    store.models.iter().find(|m| m.id == id).ok_or_else(|| err("linked model ID not found"))
}
fn hooks() -> Result<&'static model_io::ModelHooks> {
    model_io::hooks().ok_or_else(|| err("native model service is not available in this host"))
}
fn entities(d: &Drawing) -> impl Iterator<Item = &Entity> {
    d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())).chain(d.blocks.values().flat_map(|b| b.entities.iter())).map(Arc::as_ref)
}
fn dependent_views(d: &Drawing, id: &str) -> Result<Vec<String>> {
    let mut handles = Vec::new();
    for e in entities(d) {
        if let Some(link) = data::load::<ViewLink>(&e.common, VIEW)?
            && link.model_id == id
        {
            handles.push(e.handle.hex());
        }
    }
    Ok(handles)
}
fn describe(d: &Drawing, store: &Store, m: &Model) -> Result<Value> {
    let mut views = Vec::new();
    for e in entities(d) {
        if let Some(link) = data::load::<ViewLink>(&e.common, VIEW)?
            && link.model_id == m.id
        {
            let saved = store
                .views
                .iter()
                .find(|v| v.handle == link.view && v.model_id == m.id && v.revision == link.revision)
                .ok_or_else(|| err("model view metadata is missing or inconsistent"))?;
            views.push(json!({"handle":e.handle.hex(),"saved_handle":saved.handle,"stale":saved.revision != m.revision,
                "revision":saved.revision,"source":saved.source,"path":saved.path,"quality":saved.quality,
                "basis":saved.basis,"parameters":saved.parameters,"warnings":saved.warnings,
                "metadata":saved.metadata,"provenance":saved.provenance}));
        }
    }
    Ok(json!({"id":m.id,"revision":m.revision,"path":m.path,"source":m.source,"metadata":m.metadata,"views":views}))
}
fn specs() -> Vec<CommandSpec> {
    let mut commands = vec![
        CommandSpec::new("gost.model.import", "Import Linked Model", import).params("{path}; explicit native/STEP/IGES source; no preview geometry"),
        CommandSpec::new("gost.model.list", "Linked 3D Models", list).menu(&["Format", "GOST", "Linked 3D Models"]).params("{}; linked source metadata and frozen-view staleness").noundo(),
        CommandSpec::new("gost.model.inspect", "Inspect Linked Model", inspect).params("{id}; full source metadata, quality and provenance").noundo(),
        CommandSpec::new("gost.model.status", "Linked Model Source Status", status).params("{id}; compare disk fingerprint without reloading").noundo(),
        CommandSpec::new("gost.model.refresh", "Refresh Linked Model Source", refresh).params("{id,path?}; explicitly replace linked revision, never regenerate saved views"),
        CommandSpec::new("gost.model.remove", "Remove Linked Model", remove).params("{id}; refuses any dependent native view"),
        CommandSpec::new("gost.model.view", "Create Exact Model View", view).params("{id,layout?,at?:[x,y],direction?:front|rear|top|bottom|left|right|isometric|[x,y,z],up?:[x,y,z],scale?:model mm per paper mm,hidden?:bool,accept_incomplete?:false}; frozen editable native curves, no mesh fallback"),
    ];
    commands.extend(dimensions::specs());
    commands.extend(sections::specs());
    commands.extend(projections::specs());
    commands
}
pub(super) fn dimension_factor(d: &Drawing, e: &Entity) -> Option<f64> {
    dimensions::dimension_factor(d, e)
}
fn import(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["path"])?;
    load(s.doc()?)?;
    let metadata = (hooks()?.load)(text(p, "path")?).map_err(err)?;
    install_loaded(s, metadata, None)
}
fn refresh(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["id", "path"])?;
    let store = load(s.doc()?)?;
    let id = text(p, "id")?;
    let old = model(&store, id)?;
    let path = if p.get("path").is_some() { text(p, "path")? } else { &old.path };
    let metadata = (hooks()?.load)(path).map_err(err)?;
    install_loaded(s, metadata, Some((id, &old.source.sha256)))
}
/// Apply a background host load exactly once, with normal command guards and Undo/Redo.
/// The caller must verify the captured document UID before invoking this on its UI thread.
/// Refresh supplies (local ID, expected current linked SHA); a superseded revision is rejected.
pub fn apply_loaded(s: &mut Session, metadata: Value, refresh: Option<(&str, &str)>) -> Result<Value> {
    let command = if refresh.is_some() { "gost.model.refresh" } else { "gost.model.import" };
    let params = json!({"path":metadata.get("path"),"id":refresh.map(|r| r.0)});
    s.execute_prepared(command, &params, |s| install_loaded(s, metadata, refresh))
}
fn install_loaded(s: &mut Session, metadata: Value, refresh: Option<(&str, &str)>) -> Result<Value> {
    let (path, source) = checked_metadata(&metadata)?;
    let mut store = load(s.doc()?)?;
    let id = if let Some((id, expected)) = refresh {
        let old = model(&store, id)?;
        if old.source.sha256 != expected {
            return Err(err("stale_source: linked revision changed while loading"));
        }
        let revision = old.revision.checked_add(1).ok_or_else(|| err("model revision exhausted"))?;
        let id = id.to_string();
        let i = store.models.iter().position(|m| m.id == id).ok_or_else(|| err("model disappeared"))?;
        store.models[i] = Model { id: id.clone(), revision, path, source, metadata };
        id
    } else {
        if store.models.len() >= MAX_MODELS {
            return Err(err("linked-model count limit reached"));
        }
        let id = store.next_id.to_string();
        store.next_id = store.next_id.checked_add(1).ok_or_else(|| err("local model ID exhausted"))?;
        store.models.push(Model { id: id.clone(), revision: 1, path, source, metadata });
        id
    };
    let result = describe(s.doc()?, &store, model(&store, &id)?)?;
    save(s.doc_mut()?, &store)?;
    Ok(result)
}
fn list(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &[])?;
    let store = load(s.doc()?)?;
    let models = store.models.iter().map(|m| describe(s.doc()?, &store, m)).collect::<Result<Vec<_>>>()?;
    Ok(json!({"models":models}))
}
fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["id"])?;
    let store = load(s.doc()?)?;
    describe(s.doc()?, &store, model(&store, text(p, "id")?)?)
}
fn status(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["id"])?;
    let store = load(s.doc()?)?;
    let m = model(&store, text(p, "id")?)?;
    let status = (hooks()?.source_status)(&m.path, &m.source.sha256, m.source.bytes).map_err(err)?;
    let loaded: Source = serde_json::from_value(status.get("loaded").cloned().ok_or_else(|| err("status loaded fingerprint missing"))?)
        .map_err(|e| err(e.to_string()))?;
    let current: Source = serde_json::from_value(status.get("current").cloned().ok_or_else(|| err("status current fingerprint missing"))?)
        .map_err(|e| err(e.to_string()))?;
    if loaded != m.source
        || !digest(&current.sha256)
        || current.bytes > 8 << 20
        || status.get("changed").and_then(Value::as_bool) != Some(loaded != current)
    {
        return Err(err("inconsistent source status response"));
    }
    Ok(json!({"id":m.id,"revision":m.revision,"path":m.path,"loaded":loaded,"current":current,"changed":loaded != current,
        "model":describe(s.doc()?, &store, m)?}))
}
fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    keys(p, &["id"])?;
    let id = text(p, "id")?;
    let mut store = load(s.doc()?)?;
    model(&store, id)?;
    if !dependent_views(s.doc()?, id)?.is_empty() {
        return Err(err("remove dependent native views before removing the linked model"));
    }
    store.models.retain(|m| m.id != id);
    store.views.retain(|v| v.model_id != id);
    save(s.doc_mut()?, &store)?;
    Ok(json!({"id":id,"removed":true}))
}

struct Options {
    direction: [f64; 3],
    up: [f64; 3],
    at: Vec2,
    scale: f64,
    hidden: bool,
    accept: bool,
    space: Space,
}
struct SectionOverlay {
    entities: Vec<(Common, EntityKind)>,
    audit: Value,
    presentation: sections::Presentation,
}
fn vector(v: &Value) -> Result<[f64; 3]> {
    let a = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| err("expected [x,y,z]"))?;
    let n =
        |i: usize| a[i].as_f64().filter(|n| n.is_finite() && n.abs() <= 1e9).ok_or_else(|| err("finite vector coordinates within +/-1e9 required"));
    Ok([n(0)?, n(1)?, n(2)?])
}
fn validate_basis(basis: &Value) -> Result<()> {
    let read = |key| -> Result<Vec3> {
        let [x, y, z] = vector(basis.get(key).ok_or_else(|| err("saved view basis missing"))?)?;
        Ok(Vec3::new(x, y, z))
    };
    let x = read("x")?;
    let y = read("y")?;
    let d = read("direction")?;
    if (x.len() - 1.).abs() > 1e-8
        || (y.len() - 1.).abs() > 1e-8
        || (d.len() - 1.).abs() > 1e-8
        || x.dot(y).abs() > 1e-8
        || x.dot(d).abs() > 1e-8
        || y.dot(d).abs() > 1e-8
        || (x.cross(y) + d).len() > 1e-8
    {
        return Err(err("invalid saved model view basis"));
    }
    Ok(())
}
/// Canonical named views use the existing CADCraft camera convention. The worker
/// accepts sight rays, the negative of a camera's toward-eye vector; confusing
/// those conventions mirrors the front view and reverses top/bottom.
pub fn view_vectors(p: &Value) -> Result<([f64; 3], [f64; 3])> {
    keys(p, &["id", "layout", "at", "direction", "up", "scale", "hidden", "accept_incomplete"])?;
    let mut up = [0., 0., 1.];
    let direction = match p.get("direction") {
        None => [0., 1., 0.],
        Some(Value::String(name)) => match name.as_str() {
            "front" => [0., 1., 0.],
            "rear" => [0., -1., 0.],
            "left" => [1., 0., 0.],
            "right" => [-1., 0., 0.],
            "top" => {
                up = [0., 1., 0.];
                [0., 0., -1.]
            }
            "bottom" => {
                up = [0., -1., 0.];
                [0., 0., 1.]
            }
            "isometric" => [-1., 1., -1.],
            _ => return Err(err("unsupported view direction")),
        },
        Some(v) => vector(v)?,
    };
    if let Some(v) = p.get("up") {
        up = vector(v)?;
    }
    let d = Vec3::new(direction[0], direction[1], direction[2]);
    let u = Vec3::new(up[0], up[1], up[2]);
    if d.len() < 1e-12 || u.len() < 1e-12 || d.cross(u).len() <= 1e-10 * d.len() * u.len() {
        return Err(err("view direction/up must be nonzero and nonparallel"));
    }
    Ok((direction, up))
}

fn options(s: &Session, p: &Value) -> Result<Options> {
    let (direction, up) = view_vectors(p)?;
    options_for_basis(s, p, direction, up)
}
fn options_for_basis(s: &Session, p: &Value, direction: [f64; 3], up: [f64; 3]) -> Result<Options> {
    let scale = p
        .get("scale")
        .map_or(Some(1.), Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 1e-6 && *n <= 1e6)
        .ok_or_else(|| err("scale must be 1e-6..1e6 model mm per paper mm"))?;
    let at = if let Some(v) = p.get("at") {
        let a = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| err("at must be [x,y]"))?;
        let n = |i: usize| a[i].as_f64().filter(|n| n.is_finite() && n.abs() <= 1e9).ok_or_else(|| err("invalid insertion point"));
        Vec2::new(n(0)?, n(1)?)
    } else {
        Vec2::new(50., 100.)
    };
    let space = if p.get("layout").is_some() { Space::Paper(text(p, "layout")?.into()) } else { s.space().clone() };
    if let Space::Paper(name) = &space
        && s.doc()?.layout(name).is_none()
    {
        return Err(err("target layout missing"));
    }
    for layer in ["GOST-OUTLINE", "GOST-HIDDEN", "GOST-THIN"] {
        if s.doc()?.layer(layer).is_none() {
            return Err(err("GOST drawing layers required; run gost.setup first"));
        }
    }
    if s.doc()?.linetype("GOST-HIDDEN").is_none() {
        return Err(err("GOST hidden linetype required"));
    }
    Ok(Options { direction, up, at, scale, hidden: flag(p, "hidden", true)?, accept: flag(p, "accept_incomplete", false)?, space })
}
fn view(s: &mut Session, p: &Value) -> Result<Value> {
    let opts = options(s, p)?;
    let store = load(s.doc()?)?;
    let m = model(&store, text(p, "id")?)?;
    let projection = (hooks()?.view)(&m.path, &m.source.sha256, opts.direction, opts.up).map_err(err)?;
    install_view_options(s, &m.id, &m.source.sha256, p, projection, opts, None)
}
/// Complete background exact projection, never calling host I/O. The caller verifies document UID.
/// `params` are the original gost.model.view parameters, including the same local `id`.
pub fn apply_view(s: &mut Session, id: &str, expected_sha: &str, params: &Value, projection: Projection) -> Result<Value> {
    s.execute_prepared("gost.model.view", params, |s| install_view(s, id, expected_sha, params, projection))
}
fn install_view(s: &mut Session, id: &str, expected_sha: &str, p: &Value, projection: Projection) -> Result<Value> {
    let opts = options(s, p)?;
    install_view_options(s, id, expected_sha, p, projection, opts, None)
}
fn install_view_options(
    s: &mut Session,
    id: &str,
    expected_sha: &str,
    p: &Value,
    projection: Projection,
    opts: Options,
    mut section_overlay: Option<SectionOverlay>,
) -> Result<Value> {
    if text(p, "id")? != id {
        return Err(err("view completion model ID mismatch"));
    }
    let mut store = load(s.doc()?)?;
    let m = model(&store, id)?;
    if m.source.sha256 != expected_sha || projection.source_sha256 != expected_sha || projection.source_bytes != m.source.bytes {
        return Err(err("stale_source: exact view source does not match the linked revision"));
    }
    let q = projection.quality;
    // `covered` means already represented by a known visible/duplicate piece,
    // not an unresolved missing outline (upstream hlr::Piece contract). Keep the
    // conservative raw report, but assess the curves that can actually be drawn.
    let (mut uncertain, mut covered_uncertain) = (0_usize, 0_usize);
    for piece in projection.lines.iter().flat_map(|line| &line.pieces).filter(|p| p.visibility == Visibility::Uncertain) {
        uncertain += 1;
        covered_uncertain += usize::from(piece.covered);
    }
    let only_covered_uncertainty = uncertain > 0
        && uncertain == covered_uncertain
        && uncertain == q.uncertain_pieces
        && projection.visibility_contract == "raw_uncertain_and_covered_preserved_no_mesh_fallback";
    let drawable_complete = if q.analytic_complete { uncertain == 0 } else { only_covered_uncertainty };
    let incomplete = !drawable_complete
        || q.unsupported_faces > 0
        || uncertain != q.uncertain_pieces
        || uncertain != covered_uncertain
        || q.bounded_pieces > 0
        || q.sampled_pieces > 0
        || q.unsupported_trims > 0
        || q.import_issues > 0
        || q.open_bodies > 0
        || !projection.unsupported.is_empty()
        || projection.entities.iter().any(|e| (e.visibility == Visibility::Uncertain && !e.covered) || !matches!(e.accuracy, Accuracy::Analytic));
    if incomplete && !opts.accept {
        return Err(err("incomplete or uncertain exact projection; explicit accept_incomplete:true and manual engineering review required"));
    }
    let overlay_count = section_overlay.as_ref().map_or(0, |overlay| overlay.entities.len());
    if projection.entities.len().checked_add(overlay_count).is_none_or(|n| n > MAX_ENTITIES)
        || projection.lines.len() > MAX_ENTITIES
        || store.views.len() >= MAX_VIEWS
    {
        return Err(err("model-view count limit exceeded"));
    }
    let direction = Vec3::new(opts.direction[0], opts.direction[1], opts.direction[2]);
    let expected = direction * (1. / direction.len());
    let up = Vec3::new(opts.up[0], opts.up[1], opts.up[2]);
    let x = expected.cross(up);
    let x = x * (1. / x.len());
    let y = x.cross(expected);
    let vec = |v: [f64; 3]| Vec3::new(v[0], v[1], v[2]);
    if !crate::finite::is_finite(&projection.basis)
        || (vec(projection.basis.direction) - expected).len() > 1e-8
        || (vec(projection.basis.x) - x).len() > 1e-8
        || (vec(projection.basis.y) - y).len() > 1e-8
    {
        return Err(err("projection basis differs from requested sight direction/up"));
    }
    let presentation = section_overlay.as_ref().map(|overlay| overlay.presentation);
    let mut kinds = section_overlay.as_mut().map_or_else(Vec::new, |overlay| std::mem::take(&mut overlay.entities));
    for e in projection.entities {
        let line = projection.lines.get(e.line_index).ok_or_else(|| err("projection line provenance missing"))?;
        let piece = line.pieces.get(e.piece_index).ok_or_else(|| err("projection piece provenance missing"))?;
        if e.segment_index >= piece.segment_count
            || e.covered != piece.covered
            || e.visibility != piece.visibility
            || e.accuracy != piece.accuracy
            || e.t != piece.t
        {
            return Err(err("inconsistent projection provenance"));
        }
        if !matches!(&e.kind, EntityKind::Line(_) | EntityKind::Circle(_) | EntityKind::Arc(_) | EntityKind::Ellipse(_) | EntityKind::Spline(_)) {
            return Err(err("exact model view contains a non-curve entity"));
        }
        if presentation.is_some_and(|mode| !mode.draws_projection()) || e.covered || (e.visibility == Visibility::Hidden && !opts.hidden) {
            continue;
        }
        let uncertain = e.visibility == Visibility::Uncertain || !matches!(e.accuracy, Accuracy::Analytic);
        let mut common = Common {
            layer: if uncertain {
                "GOST-THIN"
            } else if e.visibility == Visibility::Hidden {
                "GOST-HIDDEN"
            } else {
                "GOST-OUTLINE"
            }
            .into(),
            linetype: if uncertain || e.visibility == Visibility::Hidden { "GOST-HIDDEN" } else { "Continuous" }.into(),
            ..Common::default()
        };
        if uncertain {
            common.color = cadcraft_color::Color::Index(1);
        }
        data::save(
            &mut common,
            PIECE,
            &json!({"model_id":id,"source_sha256":expected_sha,"line":e.line_index,"piece":e.piece_index,"segment":e.segment_index,
            "placement":line.placement,"source":line.source,"t":e.t,"visibility":e.visibility,"accuracy":e.accuracy,"covered":e.covered,
            "parameter_reversed":e.parameter_reversed,"source_arc":e.source_arc,"manual_review":uncertain}),
        )?;
        kinds.push((common, e.kind));
    }
    if kinds.is_empty() {
        return Err(err("projection has no requested uncovered native curves"));
    }
    let mut warnings = if incomplete {
        vec!["INCOMPLETE OR UNCERTAIN MODEL VIEW: manual engineering review required; red dashed curves are uncertain/approximated".into()]
    } else {
        Vec::new()
    };
    if covered_uncertain > 0 {
        warnings.push(format!("{covered_uncertain} raw uncertain pieces coincide with known visible pieces and were omitted under the upstream coverage contract; the raw quality report and provenance remain unchanged"));
    }
    if section_overlay.as_ref().is_some_and(|overlay| overlay.audit.get("notation").is_some()) {
        warnings.push("Source-bound cutting-plane notation uses the non-symmetric convention; verify symmetry and the axial hole/recess convention (GOST 2.305-2008 7.3-7.9). Exact section geometry is frozen; automatic 3D source rebuild is not implemented.".into());
    } else if presentation.is_some_and(|mode| !mode.draws_projection()) {
        warnings.push("Geometric cross-section: add cutting-plane marks or symmetry axes and check the axial hole/recess convention (GOST 2.305-2008 7.3-7.9); automatic source rebuild is not implemented.".into());
    }
    let mut provenance = json!({"lines":projection.lines,"unsupported":projection.unsupported,"prepare_reports":projection.prepare_reports,
        "drawing_stats":projection.drawing_stats,"source_mapping":projection.source_mapping,"visibility_contract":projection.visibility_contract});
    if let Some(overlay) = section_overlay {
        provenance["section"] = overlay.audit;
    }
    let saved = SavedView {
        handle: String::new(),
        model_id: id.into(),
        revision: m.revision,
        path: m.path.clone(),
        source: m.source.clone(),
        metadata: m.metadata.clone(),
        parameters: p.clone(),
        basis: json!(projection.basis),
        quality: q,
        provenance,
        warnings,
    };
    atomic(s, |s| {
        let d = s.doc_mut()?;
        let mut saved = saved;
        let mut common = Common { layer: "GOST-OUTLINE".into(), ..Common::default() };
        let h = d.add(&opts.space, Common { layer: "GOST-OUTLINE".into(), ..Common::default() }, EntityKind::Insert(Insert::default()))?;
        saved.handle = h.hex();
        data::save(&mut common, VIEW, &ViewLink { model_id: id.into(), view: saved.handle.clone(), revision: saved.revision })?;
        let name = format!("GOST-MODEL-VIEW-{}", h.hex());
        if d.block(&name).is_some() {
            return Err(err("model view block name collision"));
        }
        let mut block = Block::new(&name);
        block.units = 4;
        block.description = if incomplete {
            "INCOMPLETE exact-source model view; manual engineering review required"
        } else if let Some(presentation) = presentation {
            presentation.representation()
        } else {
            "Frozen editable exact-source model view (not associative topology)"
        }
        .into();
        for (common, kind) in kinds {
            block.entities.push(Entity { handle: d.new_handle(), common, kind });
        }
        let count = block.entities.len();
        d.blocks.insert(name.clone(), Arc::new(block));
        let insert =
            Insert { block: name, insert: opts.at.to3(0.), scale: Vec3::new(1. / opts.scale, 1. / opts.scale, 1. / opts.scale), ..Insert::default() };
        d.modify_entity(h, |e| {
            e.common = common;
            e.kind = EntityKind::Insert(insert);
        })?;
        let mut result = json!({"id":id,"handle":saved.handle,"revision":saved.revision,"source":saved.source,"basis":saved.basis,"quality":saved.quality,
            "warnings":saved.warnings,"stale":false,"entities":count,"representation":presentation.map_or("frozen-editable-exact-source-curves", sections::Presentation::representation)});
        if presentation.is_some() {
            result["section"] = saved.provenance["section"].clone();
        }
        store.views.push(saved);
        save(d, &store)?;
        Ok(result)
    })
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
