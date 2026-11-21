# SolidCraft: переиспользование и изолированный Bridge

**Выбран отдельный CADCraftGOST worker на публичных Rust-библиотеках SolidCraft, без его GUI.** Стоковый drawing snapshot аппроксимирует некруговые коники; собственный bridge вызывает публичный `solidcraft_geom::hlr` и сохраняет исходные rational Seg2.

Pinned SolidCraft revision: **`c5f47e501169c7f0f9e8191b2541e689e2f018e8`**, version `0.45.0-rc.4`.

## Проверенная реализация — 21.11.2026

- `tools/solidcraft-bridge`: отдельный workspace, общий JSON-RPC framing из
  `craft-jsonrpc`, точная геометрия из закреплённой ревизии SolidCraft/SolidCore.
- STEP/IGES и `.sc`: native Document migrations, timeline evaluation, выражения
  размещения, ограниченное решение joint-связей и применение world placement один раз.
  Поддержаны plain JSON и native container с проверкой authoritative embedded blobs.
  Preview/cache не заменяет повреждённый исходник; неизвестные ресурсы, сеточные
  native features и неподдерживаемые случаи отклоняются атомарно.
- Передаются определения/вхождения сборки, native IDs, сохранённая видимость,
  свойства и явные материалы/плотность. Отсутствующие значения остаются `null`.
  Эти IDs не гарантируют сопоставления рёбер и граней после перестроения.
- **12 тестов пройдено**: `cargo test --locked --manifest-path tools/solidcraft-bridge/Cargo.toml --all-targets`.
  Clippy `--all-targets -- -D warnings` прошёл.
- Настоящий host `cadcraft_io::model_worker::Client` загрузил native цилиндр
  радиуса 5 и высоты 20 мм: объём `1570.7963267948958` мм³, 10 рациональных
  конических сегментов наклонного вида, `analytic_complete:true`.
  Проверены сохранение сессии после remote method error и обнаружение изменения файла.
- Native conversion `model_projection::decode`: 3 теста rational weights/midpoint,
  signed arcs/near-full gaps и атомарного отказа на некорректные/чрезмерные данные.
- [Протокол, ограничения и известные пробелы](../tools/solidcraft-bridge/protocol.schema.json),
  [состав и лицензии выбранных зависимостей](../tools/solidcraft-bridge/dependency-licenses.json).
  Liberation Sans, встроенный upstream sketch, требует сохранения отдельного OFL notice.
- Это ещё не подключение к пользовательскому документу/GUI, не 3D-просмотр,
  не persistent topology matching и не жёсткая OS memory/filesystem sandbox.
  Этапы 4–6 не закрыты отдельным worker.


## 1. Sources and licensing

All SolidCraft paths below are relative to this immutable source root:

**[SolidCraft pinned source](https://github.com/tangency-labs/solidcraft/tree/c5f47e501169c7f0f9e8191b2541e689e2f018e8)**

For a named file, use:

```text
https://github.com/tangency-labs/solidcraft/blob/c5f47e501169c7f0f9e8191b2541e689e2f018e8/<path>
```

Read root `AGENTS.md`, workspace manifest, `NOTICE`, `LICENSE-BRAND`, and SolidCore’s `AGENTS.md`, manifest and license.

| Material | Actual terms / consequence |
|---|---|
| SolidCraft geometry, document, engine, import/export code | Workspace declares **MIT OR Apache-2.0**. |
| Vendored SolidCore | Separate workspace, **Apache-2.0**; NVIDIA/SolidCore notices must remain. |
| `crates/ui-kit`, `crates/look`, `crates/theme`, branding/app icons | **Not MIT/Apache**: covered by `LICENSE-BRAND`; another application needs written permission. Exclude them. |
| NVIDIA USD BRep origins | SolidCore is a pure-Rust descendant of the Apache SMLib/SM_API components, not an OpenCascade dependency. Do not import the unrelated restricted components of the NVIDIA repository. |
| Third-party dependencies | Выбранный dependency closure и embedded-font notice зафиксированы в `tools/solidcraft-bridge/dependency-licenses.json`; изменение features/revision требует новой проверки. Это не лицензирование произвольной сборки всего upstream. |

Sources: [NOTICE](https://github.com/tangency-labs/solidcraft/blob/c5f47e501169c7f0f9e8191b2541e689e2f018e8/NOTICE), [brand exceptions](https://github.com/tangency-labs/solidcraft/blob/c5f47e501169c7f0f9e8191b2541e689e2f018e8/LICENSE-BRAND), [SolidCore manifest](https://github.com/tangency-labs/solidcraft/blob/c5f47e501169c7f0f9e8191b2541e689e2f018e8/solidcore/Cargo.toml).

Upstream contributor guidance asks for maintainer agreement on platform integrations. That is distinct from the MIT/Apache code license; do not represent this integration as upstream-endorsed.

## 2. Existing craft-libs helpers: reuse before inventing

The correct repository is **`storytold/craft-libs`**, not `tangency-labs/craft-libs` (the latter returned 404).

Observed main revision: **`56c3c833bb869a140271e2aceec2112322cacc58`**. Workspace version `0.1.0`, Rust `1.95`, edition 2024, MIT OR Apache-2.0.

Local `russian-cadcraft` searches did not locate a `craft-libs` directory; inspect the parent’s existing Cargo pins before adding another revision.

| Package/file | Real reusable API | Fit |
|---|---|---|
| [`craft-jsonrpc/src/lib.rs`](https://github.com/storytold/craft-libs/blob/56c3c833bb869a140271e2aceec2112322cacc58/crates/jsonrpc/src/lib.rs) | `Limits`, `read_line`, `parse_json`, `Request`, `RpcError`, `process`, `error_response` | Recommended framing/dispatch foundation for a worker. Explicit byte/batch limits; default serde nesting protection. Transport and application methods remain caller-owned. |
| `craft-jsonrpc/Cargo.toml` | Optional `async-lines` → Tokio `rt,sync,io-util,macros`; otherwise no Tokio requirement | Leave off for a simple blocking worker. |
| [`craft-control-protocol/src/lib.rs`](https://github.com/storytold/craft-libs/blob/56c3c833bb869a140271e2aceec2112322cacc58/crates/control-protocol/src/lib.rs) | `request_line`, `reply_result`, `read_bounded_line`, `serve_lines`, `valid_token` | Useful legacy `{id,method,params,token}` helper, **not JSON-RPC 2.0**. Its server authenticates **every** request. |
| [`craft-atomic/src/lib.rs`](https://github.com/storytold/craft-libs/blob/56c3c833bb869a140271e2aceec2112322cacc58/crates/atomic/src/lib.rs) | `atomic_write`, `atomic_write_with`, `WriteOptions`, `NewFilePermissions::Private` | Persist bridge/cache manifests without another atomic-save implementation. Apple-only `rustix` dependency; no features needed. |
| [`craft-ipc/src/lib.rs`](https://github.com/storytold/craft-libs/blob/56c3c833bb869a140271e2aceec2112322cacc58/crates/ipc/src/lib.rs) | Craft Link modules: discovery, auth, client/server, bulk, payload, refresh/watch; reexports `Discovery`, `Endpoint`, `Protocol`, `RuntimeDir` | Existing richer suite transport. Root API confirmed; individual wire/module contracts were not traced, so do not assert drop-in SolidCraft compatibility. |

**Important mismatch:** SolidCraft’s existing control protocol authenticates the first request and permits omitted tokens subsequently. `craft-control-protocol::serve_lines` requires a token on every request. Sharing envelope helpers does not establish identical server semantics.

## 3. Architecture and dependency boundaries

```text
SolidCraft Session / command registry
  ├─ solidcraft-doc: timeline, assemblies, names, drawing records
  ├─ solidcraft-io: .sc containers, imports/exports
  ├─ solidcraft-kernel: public Body + STEP/IGES + measurements
  │    └─ solidcraft-smlib: Rust wrapper, despite historical name
  │         └─ solidcore-ops / record / tess / solidcore
  └─ solidcraft-geom::hlr: exact BRep projection and visibility
```

SolidCore is vendored at `solidcore/`, a separate Cargo workspace. Its mirrored repository is not necessarily at the same revision as SolidCraft’s vendored tree: **pin the SolidCraft commit for both application and kernel crates**.

The old “SMLib” names and `measured_by:"smlib"` do **not** mean this revision links a C++ proprietary kernel. `NOTICE`, manifests and `solidcore/AGENTS.md` document the Rust cutover in SolidCraft 0.40.

### Integration alternatives

| Approach | What it really supports | Trade-off |
|---|---|---|
| Embed `solidcraft-engine` in CADCraftGOST | Existing commands, native loading, assembly/BOM, measurements, saved drawings | Large dependency/rebuild surface; host and model lifetimes coupled. |
| Invoke stock `solidcraft-cli run` | STEP/IGES/.sc loading, command scripts, measured JSON, projected line/arc JSON | Easiest demonstration; process startup per job unless MCP used; exact ellipse representation lost. |
| Persistent stock CLI MCP | Same commands in persistent headless session, schema discovery, screenshot, measurements | Real API, but MCP lifecycle/path policy overhead; same projection conversion limitations. |
| **Dedicated worker linking kernel + geom; engine when needed** | Raw HLR conics, explicit uncertainty/unsupported data, exact-surface measurements; engine adds native documents and metadata | **Recommended product boundary.** Reuses upstream implementation while isolating dependencies, heavy work and failure from CADCraft. |

This choice is an engineering recommendation, not a measured performance result.

## 4. Precise dependencies/features

### Small STEP/IGES → raw exact-view worker

Use git dependencies with the same `git` and `rev`, or a pinned source checkout with path dependencies:

```toml
solidcraft-kernel = {
  git = "https://github.com/tangency-labs/solidcraft",
  rev = "c5f47e501169c7f0f9e8191b2541e689e2f018e8"
}
solidcraft-geom = {
  git = "https://github.com/tangency-labs/solidcraft",
  rev = "c5f47e501169c7f0f9e8191b2541e689e2f018e8"
}
solidcore-record = {
  git = "https://github.com/tangency-labs/solidcraft",
  rev = "c5f47e501169c7f0f9e8191b2541e689e2f018e8"
}
serde_json = { version = "1", features = ["float_roundtrip"] }
```

No special kernel/geom feature is needed. Their manifests have no feature gate for STEP, IGES or HLR.

Relevant graph:

- `solidcraft-kernel` → record, ops, tess, geom, smlib, profile, `solidcore-step`, serde/json, thiserror, log, spade, web-time; native Rayon.
- `solidcraft-geom` → SolidCore, record, ops, tess, serde, thiserror.
- `solidcraft-smlib` is `publish=false`; source reuse, not an assumed crates.io package.
- `solidcore-step` is `publish=false`; dependencies are SolidCore, convert, ops, record.
- SolidCraft workspace requires **Rust 1.95**, edition 2024.
- Standalone SolidCore’s workspace says Rust 1.85, but `solidcore-record` explicitly requires **1.88**; do not promise the selected full stack builds on 1.85.

### Native `.sc`/assemblies/commands

Текущий worker использует `solidcraft-doc` и `solidcraft-format` с
`default-features = false`, а также публичный kernel Model. `solidcraft-engine`,
`solidcraft-io`, plugins и GUI не подключены. Это существенно уже альтернативы
полного stock command runtime, показанной ниже.

Для полного upstream command runtime возможна более тяжёлая альтернатива; она не используется текущим worker:

```toml
solidcraft-engine = {
  git = "https://github.com/tangency-labs/solidcraft",
  rev = "c5f47e501169c7f0f9e8191b2541e689e2f018e8",
  default-features = false
}
```

- Default engine feature is `plugins`; disable it to avoid the optional `craft-plugin-wasm` runtime.
- `profile`, `raytrace-cpu`, `bundled-render-library` are optional; no need for exact-view/measurement prototype.
- IO’s `trust-unknown-source` and `dxf-legacy-baseline-tests` are **test-only**; do not enable them in production.
- Engine/doc/render/viewport manifests examined do not directly introduce branded ui-kit/look/theme.
- Root egui/winit patches belong to the upstream workspace. A consuming workspace does not automatically inherit those patches; avoid pulling GUI crates.

`solidcraft-io` is not a tiny STEP-only library: its manifest includes cadmpeg, signatures, meshes, Gerber and paper exporters. The smaller kernel+geom path avoids that initial scope.

## 5. Smallest workable prototypes

### A. Stock CLI capability demonstration

Create a command script using the documented object form:

```json
{
  "commands": [
    {
      "command": "drawing.view.snapshot",
      "params": {
        "view": "front",
        "scale": "1:1",
        "visual_style": "visible_hidden",
        "chord_mm": 0.01,
        "max_error_mm": 0.01
      }
    },
    {"command": "inspect.measure", "params": {}}
  ]
}
```

Invoke:

```sh
solidcraft-cli run views.json --in part.step \
  --results --param-check strict --json-out result.json
```

`apps/solidcraft-cli/src/main.rs::cmd_run` returns a JSON **array of command results** with `--results`; without results it defaults to `inspect.measure`.

This is a real available path, but **not a full analytic-curve integration prototype**: `max_error_mm` permits proven bounded chords, and does not guarantee unambiguous visibility.

### B. Required exact-curve prototype

Actual APIs sufficient to implement it:

1. `solidcraft_kernel::start()`—same initialization used by CLI.
2. `solidcraft_kernel::step_import(&text) -> Result<StepImport>`.
   - Or `iges_import(&text) -> Result<StepImport>`.
3. For each nonmesh `ImportedBody.body`:
   - `Body::record(&self) -> &solidcore_record::brep::Brep`
   - `solidcraft_geom::hlr::prepare(record, &options)`.
4. Build `hlr::Placement { body, rotation:[Vec3;3], translation:Vec3 }`.
   - Using already placed imported bodies: identity placement.
   - Do **not** apply the STEP tree’s transform again.
   - For shared-definition optimization, use `Body::display_share()` and `DisplayShare::original()` with its returned matrix.
5. `hlr::View::new(direction, up)`.
6. `hlr::hidden_lines(&prepared, &placements, &view, &options, &cancel)`.
7. Serialize the returned `hlr::Drawing`; it derives `Serialize` and retains rational quadratic conics.
8. `solidcraft_kernel::measure(&body)` for exact-surface integrated properties.

Prefer borrowed `Body::record()` over `brep_record(&body)`, which clones the complete record.

**Acceptance policy:** preserve `unsupported`, `uncertain`, `covered`, per-piece accuracy and source indices. A strict exact-deliverable request must fail or explicitly report incomplete output when unsupported silhouettes or uncertain visibility remain; never silently relabel them exact.

Prototype should demonstrate an obliquely viewed cylinder: raw HLR preserves its ellipse as conic arcs, while stock snapshot chords it. That directly distinguishes the two integration choices.

## 6. Actual public CLI/MCP/control APIs

### CLI

`apps/solidcraft-cli/src/main.rs` implements:

- `run <script|design.sc|part.step> [--in ...] [--results] [--json-out FILE]`
- `eval <...>` / `inspect` alias: measurements JSON.
- `exec <command> [json-params]`: starts with an **empty design**.
- `commands`: `engine.commands` JSON.
- `snapshot --in part.sc --out shot.png --view iso ...`: rendered 3D preview.
- `mcp --headless --in part.sc --allow-path DIR --param-check strict`
- `mcp --connect 127.0.0.1:PORT [--token T]`

Use explicit `--headless`: default MCP mode can discover and connect to a running GUI.

### MCP

[Protocol documentation](https://github.com/tangency-labs/solidcraft/blob/c5f47e501169c7f0f9e8191b2541e689e2f018e8/docs/mcp.md):

- stdio JSON-RPC 2.0, one message per line.
- Protocol revisions `2025-06-18`, `2025-03-26`, `2024-11-05`.
- `version` → `{version,commit,mode}`.
- `describe_command {id}` → parameter docs and input/output JSON Schemas 2020-12.
- `execute {command,params?}`.
- `batch {commands:[{command,params}],rollback?...}`.
- `measure {body?|items?,summary?}`.
- `body_topology {body,faces?}`.
- `screenshot {view?,camera?,path?,width?,height?...}`.
- `open {path}`, `save`, `export`.
- Resources: `solidcraft://design`, `solidcraft://commands`.

Typical call:

```json
{
  "jsonrpc":"2.0",
  "id":2,
  "method":"tools/call",
  "params":{
    "name":"execute",
    "arguments":{
      "command":"drawing.view.snapshot",
      "params":{"view":"iso","visual_style":"visible_hidden"}
    }
  }
}
```

Use ordinary MCP initialization/tool discovery first. Engine failures are tool results with `isError:true`; malformed RPC/tool arguments use RPC errors.

Path restrictions matter: use existing explicit `--allow-path` directories, not `--allow-any-path` for routine integration.

### Existing GUI control channel

[Control protocol](https://github.com/tangency-labs/solidcraft/blob/c5f47e501169c7f0f9e8191b2541e689e2f018e8/docs/control-protocol.md):

```json
{"id":1,"token":"…","method":"engine.execute","params":{"command":"doc.open","params":{"path":"/absolute/part.step"}}}
```

Reply:

```json
{"id":1,"ok":true,"result":{}}
```

This is **not** JSON-RPC 2.0.

- Loopback; default port 27878 or next free.
- Discovery: `control/current.port`.
- Token: Linux `$XDG_DATA_HOME/solidcraft/control/PORT.token`.
- `control.capabilities` discovers supported boundary features.
- `control.subscribe` returns `{document_id,revision}` and enables:
  ```json
  {"event":"design.changed","params":{"document_id":"…","revision":4}}
  ```
- Runtime IDs/revisions are session-local, not persistent file identities.
- Request limit 16 MiB; 16 connections.
- Long-running replies may have `running:true`; use `control.pending`, not resending the operation.
- MCP does not expose this event subscription as an ordinary documented dedicated tool; use control for live events or poll `document.revision`.

## 7. STEP and IGES: implemented, with partial-import reporting

### STEP

Files:

- `crates/kernel/src/step_in/mod.rs`
- `solidcore/crates/solidcore-step/src/lib.rs`
- `solidcore/crates/solidcore-step/src/topology.rs`
- `crates/engine/src/cmd/file.rs`
- `crates/engine/src/cmd/file_import.rs`

Public kernel APIs:

```rust
step_import(text: &str) -> Result<StepImport>
step_import_shared(text: &str) -> Result<Arc<StepImport>>
```

Reader explicitly targets AP203/AP214/AP242 geometry.

`StepImport` contains:

```text
bodies, tree, warnings, report, schema, system, preprocessor, description
```

`ImportedBody` includes:

```text
source_item: u64
face_origin: Vec<u64>
name, body, color, path, instance
closed, file_faces, refused, hidden
```

`StepNode` includes name, column-major 4×4 parent transform, body indices, STEP item IDs, left-out solids and child nodes.

Limits: nesting 32, 10,000 imported body instances, 100,000 product/mapped-representation visits.

`ImportReport` distinguishes not-imported, refused, imported-as-surfaces, face-left-out, open-body, faces-not-shown, needs-repair, repaired and file issues. **A successful import is not proof every solid/face imported.**

`step_import_shared` may leave checks running; use settled report/current report semantics honestly.

### IGES

`crates/kernel/src/iges_in.rs`:

```rust
iges_to_step(text: &str) -> Result<(String, Vec<String>), String>
iges_import(text: &str) -> kernel::Result<StepImport>
```

IGES 5.3 solids type 186 and trimmed/bounded surfaces 144/143 are restated as STEP, then use the common reader. Units normalize to millimetres.

Actual limitations:

- Binary/compressed IGES refused.
- Unsupported entities become warnings.
- Incomplete faces/shells inherit STEP partial-import behavior.
- IGES-generated STEP entity identity is not evidence of stable identity across edits to the original IGES.

Engine `file.insert_step` accepts IGES too; `doc.open` handles STEP/IGES/.sc. Names, up-axis and flattening options exist; rigid matrix insertion rejects scale/shear/mirror.

## 8. Exact projection, HLR and schemas

Core: [`crates/geom/src/hlr/mod.rs`](https://github.com/tangency-labs/solidcraft/blob/c5f47e501169c7f0f9e8191b2541e689e2f018e8/crates/geom/src/hlr/mod.rs).

```rust
prepare(&Brep, &Options) -> Result<Prepared, HlrError>

hidden_lines(
    defs: &[Prepared],
    scene: &[Placement],
    view: &View,
    opts: &Options,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Drawing, HlrError>
```

Supported analytic surfaces: planes, cylinders, cones, spheres, tori; recognition also handles appropriate spline representations and rigid revolutions.

Raw HLR output:

```text
Drawing {
  lines: [
    {
      placement,
      source: {kind:"edge",edge,tangent}
           | {kind:"silhouette",face},
      pieces:[
        {t:[t0,t1],visibility,covered,geometry:[Seg2...],accuracy}
      ]
    }
  ],
  unsupported:[{placement,face,reason}],
  stats
}
```

`visibility`: `visible|hidden|uncertain`.

Accuracy:

```json
{"kind":"analytic"}
{"kind":"bounded","max_deviation_mm":0.01}
{"kind":"sampled","max_sample_deviation_mm":0.01}
```

**“Sampled with deviation zero” is not certified exact.**

`solidcore-record/src/profile.rs::Seg2`:

- `line {a,b}`
- `arc {center,radius,start,sweep}`
- `cubic {p0,p1,p2,p3}`
- `conic {a,apex,b,w}`: rational quadratic, endpoint weights 1.
- `bezier {n,p}`: higher degree.

### Critical stock-command limitation

`crates/engine/src/drawing_view_exact.rs::on_paper` converts:

- circular conics → exact `arc`;
- **other conics → bounded line chords**;
- other unsupported curve forms → sampled chords.

`schema/drawing_view.rs::PathSegResult` advertises only line/arc accordingly.

Also, `shown_as` converts uncertain HLR pieces into visible pieces while incrementing an uncertainty count.

Thus:

- `status.lines:"exact"` means no sampled pieces—**bounded chords count as exact**.
- `max_error_mm` rejects sampled/over-bound geometry, **not uncertain visibility**.
- Check `status.provenance`, `uncertain_pieces` and piece accuracy separately.
- The engine falls back to mesh projection for unsupported bodies and certain split edges. Raw HLR does not silently perform this engine fallback.

Other limitations:

- Freeform silhouettes disabled by default; opt-in contouring/seeded Newton is sampled and has documented misses.
- Oblique torus silhouettes sampled.
- Mirror/scale placements rejected by HLR.
- Unsupported trims and budgets explicitly reported/refused.
- Perspective not provided by this orthographic HLR API.

## 9. Drawing modules and sections

Drawing records are real, not merely README claims:

- `crates/doc/src/drawing.rs`: sheets, sources, views, dimensions, balloons, notes, symbols, sketches/fills, tables, revisions.
- `drawing/view.rs`: base/projected/pictorial/auxiliary/flat/section view records, scale/settings inheritance and alignment.
- `drawing/dimension.rs`: associative model references and resolution.
- `crates/engine/src/cmd/drawing_view.rs`: snapshot/base/projected/standard/auxiliary/flat/section commands.
- `drawing_view_hatch.rs`: section material regions and hatching.

Implemented commands include:

```text
drawing.view.snapshot
drawing.view.base
drawing.view.projected
drawing.view.standard
drawing.view.auxiliary
drawing.view.flat
drawing.view.section
drawing.view.inspect
drawing.view.edit
drawing.view.move
```

Sections:

- `drawing.view.section` accepts full/half/offset/aligned cutting-line records.
- **Only full sections currently draw their cut.** Others explicitly say not supported.
- `inspect.section` is a separate **display-only** cut, up to six planes—not an exact section-curve export API.
- `solidcraft_geom::section::SectionGeometry` is a DTO/structural validator, **not** a general body-plane intersection implementation.
- `solidcraft_kernel` exports actual `split_by_plane`, `half_space`, `face_section_along`, `axial_face_section`.
- Hatch extraction uses exact cut faces/history, then polygonalizes outlines: `FLATTEN_MM=0.05` paper mm; freeform outlines can be sampled. Refused/doubted hatches are recorded.
- Auxiliary views save the resulting direction, **not** an associative reference to the selected source edge/face.
- Projecting from pictorial or flat-pattern views is refused.

Do not infer local/detail/broken-out/removed-section support from the existence of general view records.

## 10. Native `.sc` loading

Files:

- `crates/io/src/design.rs`
- `crates/io/src/design/native_cache.rs`
- `crates/kernel/src/native.rs`

Public loader:

```rust
read_design_file(
    path: &str,
    notify: Option<impl FnOnce(DesignPreview)>
) -> Result<(Document, Option<SavedModel>)>
```

Handles container files and legacy/plain JSON through migrations.

- Container format: `solidcraft-container/1`.
- Optional exact evaluated model cache: `solidcraft-model-cache/10`.
- Cache admission is tied to document bytes, evaluation-source identity, architecture/OS and kernel identity.
- Cache absent/rejected → evaluate the timeline; do not assume `.sc` always contains reusable final BRep.
- `read_design_preview` returns **untrusted display-only meshes**, explicitly prohibited for measurement/picking/editing.
- `read_design_geometry` is publicly reexported, but remains a cache path—not a universal cross-version geometry reader.
- Native bodies use `encode_native_body` / `decode_native_body`; producer trust remains caller responsibility.

Simplest correct native workflow: `Session::default()`, `execute("doc.open", ...)`, then use `Session::world_state()` and bodies for raw HLR. This reuses lifecycle, migration and evaluation instead of hand-extracting container entries.

## 11. Stable references and identities

Three distinct identity levels must not be conflated:

1. **Result-local indices:** HLR placement/edge/face indices, BRep arrays.
2. **Native document identity:** occurrence IDs, feature IDs, persistent face/edge names.
3. **Imported-source provenance:** payload digest + STEP solid/face entity IDs.

Files/APIs:

- `crates/doc/src/persistent_name.rs`: `FaceName`, `EdgeName`, parsing/equality.
- `crates/doc/src/naming.rs`: lazy names; `face_name_exact`.
- `crates/doc/src/drawing/dimension.rs`:
  - `ModelEdge {body,origin_feature,name,path}`
  - `ModelFace {body,origin_feature,name,path}`
  - `ModelPoint` names shared incident edges or a circle centre.
  - `Document::edge_reference`, `face_reference`, `corner_reference`
  - `Document::resolve_dimensions`
- `crates/kernel/src/body.rs::FaceSource::Imported`:
  ```text
  item, face, payload_sha256:Option<String>, producer:Option<u64>
  ```

History-backed names follow feature roles and split adjacency. Geometry-derived names are marked unverified; source comments explicitly warn an upstream edit can fool them.

Dimension statuses include:

```text
Ok, Overridden, Ambiguous(candidates), Suppressed,
Failed(reason), Lost(reason,candidates), Unmeasurable(reason)
```

**Missing guarantee:** there is no demonstrated universal persistent matching of arbitrary externally regenerated STEP topology. STEP entity numbers are scoped to a payload; changing its digest invalidates that identity scope. Preserve native reference status or require explicit repair—never nearest-mesh rematching.

## 12. Assemblies, materials, designations

- `doc/src/assembly.rs`: definitions and occurrences; repeated parts share definitions.
- `Mat = [[f64;4];4]` is **column-major**.
- Occurrence IDs plus occurrence paths distinguish repeated placements.
- Imports preserve products/repeated instances where representable.
- Imports flatten portions exceeding component/depth limits or unsupported placement structure; `file_import.rs` limits new import components to 5,000 and depth to 32.
- Occurrence placements are rigid, nonmirrored.

Metadata:

`doc/src/metadata.rs::Metadata` preserves unknown namespaced values. Standard fields include:

```text
project, part_number, revision, author, organization,
description, license
```

Component-only:

```text
bom_type, material, bom_quantity, virtual
```

Commands:

```text
component.metadata
component.metadata.set
assembly.bom
material.list
material.assign
document.inspect
```

`assembly.bom` supports flat/top/all levels, part-number or definition grouping, quantities, static/physical material, optional masses and CSV/JSON export.

**Do not conflate material text with physical material:** component metadata `material` is a static BOM label and does not affect mass. Body physical material provides density.

**Missing evidence/capability:** inspected STEP import schema preserves names, geometry, colors and product structure, but exposes no imported engineering material/density/part-number field. Do not promise STEP automatically populates those native metadata fields.

## 13. Measurements and preview

`crates/kernel/src/measure.rs`:

```rust
measure(&Body) -> Result<BodyMeasure>
measure_mass(&Body) -> Result<MassMeasure>
```

BRep area/volume/centroid are integrated on exact surfaces; integration accuracy constant is 1e-8 relative. This is numerical surface integration, not symbolic exact arithmetic.

Results include volume, area, centroid, exact bounds, topology, `measured_by` and notes. Mesh bodies report `measured_by:"mesh"`; reject those for exact-model measurements.

Engine `inspect.measure`:

- volume mm³, area mm², centre of mass;
- density g/cm³, mass grams;
- item distances/angles/radii/diameters;
- closed-body-only volume/mass totals with open-body skip counts.

`model.faces` exposes exact/name information; selected-face measurement helpers can retain mesh-derived answers for unsupported surfaces. Do not assume every selection measurement is certified merely because the command is called Measure.

Preview options:

1. Stock CLI snapshot/MCP screenshot: rendered PNG, headless GPU or CPU fallback.
2. `Body::mesh(&MeshTolerance) -> Result<Mesh>` for interactive CADCraft display.
3. Native `DesignPreview` for provisional display only.
4. GLB export for another viewer: metres/Y-up; internal geometry remains mm/Z-up.

Mesh faces can be omitted when meshing fails; `Body` reports unmeshed-face information. A preview is not a completeness certificate.

## 14. Bridge protocol recommendation

**Proposed CADCraftGOST protocol—not an existing upstream API:**

Use `craft-jsonrpc` plus versioned bridge methods and capability discovery. Preserve raw upstream data semantics rather than adapting them to misleading booleans.

Minimum response concepts:

```text
protocol version
upstream version + commit
source content digest + import options
document/session revision, when applicable
units and coordinate convention
part definitions + occurrence paths/transforms
metadata + physical material separately
projection basis + scale
raw Seg2 + source references + parameter intervals
accuracy + visibility + unsupported/partial-import diagnostics
measurement provenance
display-only mesh payload reference
```

For live SolidCraft, use `control.subscribe`. For offline linked files, content digest/file-change tracking belongs to the bridge; runtime `document.revision` is not a persistent file fingerprint.

Do not advertise upstream CLI/MCP as implementing this proposed bridge contract already.

## 15. Actual missing upstream capabilities

1. Stock snapshot API does not preserve general analytic conics/ellipses.
2. “Exact” snapshot status does not guarantee analytic geometry or certain visibility.
3. General freeform certified HLR/silhouettes are incomplete.
4. Half/offset/aligned section **records exist**, but only full section drawing is implemented.
5. Display section analysis is not section geometry export.
6. `SectionGeometry` is not a producer/intersection algorithm.
7. Arbitrary external STEP regeneration has no proven stable-topology remapping guarantee.
8. Auxiliary view direction is not persistently bound to its original selected edge/face.
9. STEP engineering-material/density/designation transfer was not found in the real import result schema.
10. Native exact model cache is optional and producer/evaluation-specific.
11. Import/meshing success can be partial; warnings must reach users.
12. The current isolated prototype is not yet the full user-facing CADCraftGOST bridge: sections, preview and persistent topology matching remain open.

## Verification scope

The current checked implementation is summarized at the beginning of this document.
The following broader acceptance scenarios remain independent requirements:

- Small pinned worker dependency resolution, without GUI/brand crates.
- STEP oblique cylinder retaining `Seg2::Conic`.
- Matching front/iso projection against known analytic dimensions.
- Partial import and uncertain/freeform reporting.
- Closed/open body measurement behavior.
- `.sc` load with valid cache, absent cache and incompatible cache.
- Repeated-instance transforms and metadata/BOM.
- Native reference preservation and lost/ambiguous statuses.
- Full section versus explicitly unsupported section kinds.

The preceding static checklist is not a claim that every scenario has passed. See the dated implementation summary for the checks actually run.