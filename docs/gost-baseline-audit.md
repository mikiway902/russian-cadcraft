# GOST baseline source audit

## Scope and evidence

Authorized comparison: original CADCraft base `11d66cc3cd6a3676aad79401c9d390f3ccbf3aa7` → baseline HEAD (stage-0 baseline identified by parent as `35cfa991f309661891adecf4b4ee44c72bc47cc1`). Diff inventory: **92 files, +13,020/−329 lines**, plus embedded font. No worktree diff, repository edits, builds, tests, linters, formatters, runtime repros or performance measurements were performed. Findings below are static control/data-flow proofs; repros are proposed regression cases, not claims of executed results. Existing tests were read, not run. This audit does not finish the external 15-stage CADCraftGOST roadmap or certify ESKD conformance.

## Changed original families

* Engine command registry and session lifecycle: `cmd.rs`, `lib.rs`, `settings.rs`, `session/{files,execute,interactive}.rs`, `cmd/file.rs`.
* Ordinary drafting interception: `cmd/layout.rs`, `cmd/draw2/centermark.rs`, `cmd/hatch.rs`; many existing command/unit/integration tests now use explicit raw/imperial fixtures instead of `Session::new()`.
* Added business implementation: `cmd/gost.rs` and `gost/{profile,sheet,book,annotations,axes,marks,requirements,specification,projection,norms,navigation,data}.rs` plus dedicated tests.
* Geometry: new public `geom::drafting3d` module, exported by `geom/src/lib.rs`.
* Fonts/exchange: `fonts/{lib,ttf}.rs`, new `assets/fonts/opengost/`; `io/src/dxf_write/blocks.rs` dimension block lineweights and `io/src/pdf.rs` product metadata.
* UI: `managers.rs`, seven `managers/gost*.rs` files, menu/dialog/closing/chrome/about/startup/update display strings, Russian catalogue additions.
* Hosts/packaging: desktop/web/CLI startup, arguments/window/product labels, Windows/macOS/Linux packaging and font-license copying. README, NOTICE, ATTRIBUTION, `docs/gost*.md`, example script.

## Feature inventory: implemented, not stubs

| Owner | Actual behavior and evidence |
|---|---|
| `profile.rs` | `gost.setup`: named GOST text/dimension styles, nine layers, three linetypes, millimetre headers, decimal comma, annotation scales. Populated non-mm geometry is refused; paper installation preserves model headers and existing GOST records. `gost.check` combines preset/sheet/dimension/link and bounded planar geometry checks; always exposes manual-review requirements. |
| `sheet.rs` / `book.rs` | A4–A0 graphical frames, forms 1/2a, editable native INSERT attributes, form-2 text-document geometry for specifications, fitted lettering, supplementary/revision fields, shared properties, sheet ordering/numbering and first-sheet promotion. Native paper-mm page setup and no arbitrary viewport. |
| `annotations.rs` / `axes.rs` | Persistent associative circle/arc axes; copied circle+axis translation remapping; source movement/radius changes/deletion. Static cutting-plane arrows and view designations. GOST hatch wraps native boundary tracing/islands/associativity and replaces pattern spacing. |
| `marks.rs` | Persistent editable roughness, weld and conventional 2D thread blocks; copy-on-write edits retain INSERT identity/transform and foreign XDATA. Tolerance wrapper validates input and delegates existing native TOLERANCE rather than drawing a counterfeit frame. |
| `requirements.rs` | Numbered text, actual-font wrapping, automatic or explicit multi-column placement, editable original/custom templates, overflow rejection, sheet/font reflow, DXF persistence. |
| `specification.rs` | Linked attribute-based part identities and manual rows; nine sections; native A4 paginated TABLE plus title-block geometry; copied-instance quantities and renumbered position leaders, missing-source `?`/zero quantities, copied pair translation remapping. Surplus generated pages removed without deleting unrelated user geometry. |
| `projection.rs` / `drafting3d.rs` | Native POLYFACE box/convex extrusion/explicit shell sources; orthographic/isometric/custom views, multi-body occlusion, true planar clipping/intersection hatching, persistent vertex-ID and edge/plane anchors, projected or true-3D aligned dimensions, same-transaction source rebuild and missing-source banners. This is genuine bounded faceted geometry, not a stub or B-Rep kernel. |
| `norms.rs` / `navigation.rs` | Native planar positive-length duplicate/overlap, dimension/extension crossings and paper-mm parallel spacing; incomplete-analysis warnings and live-handle navigation. |
| Metadata / automation | `data.rs` version-1 `CADCRAFTGOST` JSON in XDATA, 12 KB record ceiling and UTF-8-safe 240-byte DXF chunks. Registered `CommandSpec`s expose native command machinery to CLI/JSON/MCP, while UI remains a parameter-producing frontend. |

No `todo!`/`unimplemented!` implementation stub was identified in these GOST modules. Explicit `Ok(())` early exits in hooks are dependency/document-change guards, not evidence of fake functionality.

## Dependencies and reuse

The baseline diff adds **no Cargo manifest/lockfile dependency**. Engine implementation uses existing `cadcraft-doc`, `cadcraft-geom`, `cadcraft-render`, `cadcraft-fonts`, serde/serde_json and std; geometry adds only code on the existing serde/vector layer. UI uses existing egui/engine stack. No OpenCascade, proprietary assets, GPL or LGPL dependency is introduced by this diff. Existing NOTICE discloses unmodified MPL-2.0 `acadrust` via craft-dwg; this is pre-existing, not a new GOST dependency. OpenGost Type B is explicitly bundled under OFL-1.1 with license text and attribution (NOTICE/ATTRIBUTION/assets), not an extracted KOMPAS font.

Verified existing-algorithm reuse: native HATCH via `find_command("hatch").run`; native tolerance parser/command and rendering; native `dimlinear` for associative view dimensions; `table::make_table`; font metrics and native entity stores; existing `cadcraft_geom::{line_line,line_circle,intersect_segments}` for normcontrol. Do not replace these with second implementations. The new standalone faceted shell/HLR/section algorithms are a replacement seam under the new roadmap, **not proof of copied or duplicated SolidCraft algorithms**: SolidCraft source/license/API equivalence was not established by this baseline audit. Their current commands, IDs, stale-link behavior and DXF representation must remain available until an evidence-backed replacement passes equivalent acceptance.

Repeated local document-snapshot `atomic` wrappers are an ownership/transaction consolidation opportunity; do not mechanically rewrite them while extracting features.

## Concrete findings (patch-introduced)

### F1 — P2: Preserve generated symbol definitions across cross-document paste

Patch anchor: `crates/engine/src/cmd/gost/marks.rs:655-660` (also block creation at 644-648). Confidence 1.0.

`block_name` starts every document at the non-anonymous named block `GOST-MARK-0`. Existing clipboard `merge::block_renames` deliberately keeps the target definition on a named-block collision (`cmd/merge.rs:30-63`), and `clipboard::adopt` only remaps names present in that map (`clipboard.rs:306-324`). There is no symbol post-paste rebuild. Create a first roughness Ra 1.6 in document A and a first roughness Ra 6.3 (or weld) in document B; COPYCLIP the first from A and PASTECLIP in B. The pasted INSERT retains A's XDATA parameters but references B's pre-existing `GOST-MARK-0` geometry, so the drawing and subsequent PDF/DXF show the wrong engineering indication. Fix generated-private-block identity/collision handling through the existing anonymous/rename import mechanism or an explicit extension adoption hook; changing generic named-block replacement semantics would regress upstream behavior. Existing marks tests cover same-document COW and whole-document DXF reopen, not this boundary.

### F2 — P2: Detect removal of managed frames when synchronizing the book

Patch anchor: `crates/engine/src/cmd/gost/book.rs:387-400`. Confidence 0.99.

The hook detects changed layout count/page/order and scans only surviving current frame INSERTs. Removing a frame inside a surviving tab is therefore invisible. Create two automatic sheets, then ERASE only the first frame (ordinary erase path `cmd/modify/transform.rs:89-103`, which removes it without deleting the layout). No other surviving frame changed, so `relevant` remains false and the remaining managed frame still says sheet 2 of 2/form 2a instead of becoming sheet 1 of 1/form 1. Explicit `gost.book.sync` recomputes it correctly, establishing the missing trigger. Compare prior/current owned-frame membership, not just layout membership. Existing book tests cover deleting a *layout*, not deleting its frame.

### F3 — P2: Validate modifications to existing specification page INSERTs

Patch anchor: `crates/engine/src/cmd/gost/specification.rs:1092-1118`. Confidence 0.99.

The post-command routing notices new authoritative owners (to reject copies), part changes and position changes, but ignores changes/deletion to an existing `GOST-SPEC-*` INSERT. A normal MOVE of a specification page succeeds through native transform handling (`cmd/modify/transform.rs:61-87,112-116`); the hook returns early when no part/position changed. The fixed A4 paper grid is now displaced, although `rebuild` explicitly requires origin/unit-scale/zero-rotation at lines 655-665. The next linked-part edit then invokes rebuild and fails, rolling back that otherwise valid part edit until the page is repaired. Likewise erasing a continuation page is accepted and leaves persistent page handles unresolved until the next rebuild. Route existing page ownership/transform/membership changes to an immediate validation/rebuild or atomic refusal. Current tests cover regeneration/shrinking, not ordinary page transforms/deletion.

## Architectural and hostile-input risks (not additional speculative bugs)

* **No modular opt-out today.** Registry always chains GOST specs (`cmd.rs:205`), `Session::new()` always creates metric/GOST (`lib.rs:173`), *all* `new_drawing(true)` calls choose GOST (`session/files.rs:7`), native NEW ignores saved MEASUREINIT unless explicit units/template, and layout/centermark dispatch infer GOST from header string equality. These defaults are deliberate and documented, not accidental regressions, but removing command registration alone makes ordinary commands fail with “generator unavailable”. The metric raw CADCraft path must become available independently of product preset selection.
* **Post-command transaction authority is global.** `interactive.rs:494-505` calls ordered book→axes→requirements→specification→projection passes after existing groups/constraints/assoc; `execute.rs:60-75` and interactive finish roll back document+selection on hook errors. Keep same-document UID/snapshot guards and a single undo owner. Snapshot rollback does not universally restore all session UI state (`space`, views, pending window), so extension edits should stage fallible work before changing session state rather than assuming a full-session transaction.
* **Malformed/future XDATA can poison unrelated edits.** `requirements.rs:625-628` calls `data::load` on every paper entity after each successful mutation; `data.rs:31-36` parses/version-checks before kind filtering. Even without any requirements, a paper entity with `CADCRAFTGOST` version 2 or malformed JSON makes an unrelated LINE fail and roll back. Explicit GOST record failure is defensible; unconditional unrelated-edit veto is a real isolation risk for opt-out/import/future migration. Preserve visible geometry and expose unsupported metadata diagnostics rather than silently reinterpreting it.
* **Budgets are mostly local, not a measured global latency bound.** XDATA 12 KB; axes 256 per create/1024 managed, 20k top-level entities and segment ceilings; requirements 64 items/8k text/2048 lines/256 records/16 columns; specifications 8 owners/128 rows/256 sources/256 leaders/16 pages/20k top-level entities and blocks; views 128, dimensions 1024, bodies per view 16, mesh 512 vertices/1024 faces and 4m geometric-work budget; normcontrol 4096 entities/8192 primitives/100k pairs/64 spaces/256 geometry findings. Repeated per-record full scans/mesh validation/font metrics execute synchronously, so these bounds do not establish acceptable UI time or memory for the product roadmap.
* **Profile checks are not uniformly work bounded.** `profile.rs:543-614` iterates model/layout/block definitions until *findings* reaches 2000, not until a scanned-object budget; a huge benign non-dimension block population still gets visited. `check_sheets` similarly scans imported layouts/blocks. `norms.rs` has stronger explicit work ceilings. Performance claims require parent profiling; no timing is supplied here.
* **Clipboard/import handle rebasing is not versioned extension-aware.** Persistent references are inside JSON 1000 tags rather than native 1005 handles; `clipboard::adopt` does not update them. Axes and positions implement special same-command translation heuristics, not a universal cross-document remap. Imported/copied structured owners, references, private block names and missing/ambiguous links need a single adoption contract. Never silently bind old IDs to coincidentally equal target handles.
* **Generated geometry edits are not generally authoritative.** Symbol edits rebuild from metadata; projection source changes replace private cached blocks; specifications replace their owned block. Ordinary DXF entities remain editable in other programs, but these changes are not a bidirectional parametric edit protocol. Preserve user annotations separately from generated output in a replacement API.

## Explicit documented limitations, not defects

`docs/gost.md` correctly limits static `gost.section`/`gost.view` to notation, symbolic threads to 2D, roughness/welds to drafting rather than manufacturing verification, fields to text rather than digital signatures, and checks to incomplete measurable coverage rather than certification. It discloses conditional/special frame fields not generated, no automatic construction/electrical standards, no curved B-Rep/NURBS/ACIS/booleans/helical kernel, no stepped/broken sections or automatic assembly hatching exclusions, no automatic optimal view selection, explicit faceting controlled by author, limited material hatch conventions and font/print/manual review. These must not be recast as bugs or claimed completed roadmap items.

## Export and stale-reference acceptance gaps

Read tests establish *test intent* for native whole-document DXF roundtrips of symbols, book fields, requirements templates, specification/position ownership, axes and projection anchors; PDF sheet tests only assert a PDF signature and geometry/page bounds. No executed compatibility result is claimed. External viewers cannot execute CADCRAFTGOST hooks; PDF is a static snapshot; DWG may preserve visible geometry without guaranteeing extension JSON links. Actual external DWG/DXF interoperability, Cyrillic/special glyphs, exact PDF sizes/lineweights, roundtrip edits, clipboard collisions, cross-document reference adoption, orphan-owner cleanup and data-loss warnings need explicit acceptance cases. `io/dxf_write/blocks.rs` correctly adds native dimension/ext lineweights to anonymous dimension geometry; retain it as a general upstream fix rather than entangling it with extension enablement.

## Extension API ownership boundaries

1. **Core owns** document/entity/handle stores, native drafting/render/font/I/O algorithms, selection, command execution, UID/document switching, dirty state and one undo/redo transaction. GOST must not nest `Session::execute` for delegated primitives; keep direct native-run composition or typed core services.
2. **Registration surface owns** compile-time command descriptors, menu contributions, dialog identifiers/factories and optional lifecycle contributions. A disabled extension must contribute zero commands/menus/hooks/default changes while still opening/rendering preserved ordinary entities and opaque XDATA.
3. **GOST core/standards owns** profile definitions, normative version/limits, sheet/mark/requirements semantics and metadata codecs. Product startup chooses a preset; base metric-vs-imperial drawing construction must not imply a standards extension.
4. **Normcontrol owns** immutable report generation with severity/completeness/manual requirements and bounded work. UI owns display/navigation requests only; live core handles validate navigation. Specifications own linked identities, pages and position relationships independently of sheet-book numbering.
5. **Drawing extension owns** persistent view/dimension anchor semantics and generated 2D cache; geometry backend is replaceable (existing polyhedral implementation first, licensed SolidCraft reuse only after direct source/API/license evidence). Bridge owns model revision/assembly/material/topology identities, not GOST stamps or UI. Avoid engine→extension→engine crate cycles by exposing host/document services at the boundary.
6. **Events must be transactional and typed**: same-document before/after plus changed/added/removed handles, layout/style/block changes and copy/import old→new handle/name maps. Separate prepare/validate from apply; failures restore the original transaction. Route every produced event at its consumer; a generic “command finished” string and private-name prefixes are insufficient as a stable external plugin contract.
7. **Persistence/adoption owns migration**: namespace/version/kind, resource budgets, unknown-record preservation, diagnostics, handle rebasing and generated-block identities. Do not make each feature rediscover clipboard imports from geometry.

## Parent verification handoff

No checks were run by this read-only worker. Parent owns full baseline CI/performance and later fixes. Add focused cases for F1–F3, unsupported/future XDATA isolation, cross-document owner/reference adoption and extension-disabled raw metric/new/layout/centermark behavior before claiming modular parity. Keep existing GOST unit/integration behavior as acceptance while replacing the obsolete custom polyhedral backend. Prior 7-phase `docs/gost-roadmap.md`/README completion language refers only to historical scope, not completion of `CADCraftGOST_ROADMAP.md`.

## Geometry/projection deep-audit findings

The following were independently traced through the kernel, engine consumers, existing tests, and (where applicable) native polyface render/export consumers. All are static predictions, not executed repros.

### F4 — P2: Deduplicate on-plane mesh edges when stitching section contours

Patch anchor: `crates/geom/src/drafting3d.rs:373-388`. Confidence 1.0.

For cube min `[0,0,0]`, max `[10,10,10]`, request `gost.section.view` with plane normal `[1,1,0]`, offset `10`. The plane makes a regular rectangular cross-section through two existing vertical edges, without coinciding with any face. Each incident face adds the same on-plane edge to `segments`; the path builder sees more than one continuation and returns `section branches at a nonregular point`. Canonicalize/deduplicate actual section segments rather than rejecting this valid transverse cut. Existing section tests only cut through edge interiors.

### F5 — P2: Stitch intersections by mesh-edge identity rather than fixed absolute proximity

Patch anchor: `crates/geom/src/drafting3d.rs:360-364` (consumer matching 380-393). Confidence 0.97.

A supported cube `[0,0,0]..[1e9,1e9,1e9]` cut at normal `[0,0,1]`, offset `333333333.3333333`, computes a shared-edge intersection independently from opposite traversal directions. Forward/reverse interpolation differs by approximately 5.96e-8, exceeding the fixed 1e-8 stitching threshold, so a closed square is reported as `section contour is not closed`. Cache the canonical intersection keyed by the undirected mesh edge (or use a genuinely scale-aware validated construction), retaining published coordinate limits.

### F6 — P2: Scale interval tolerances to physical edge length in hidden-line removal

Patch anchor: `crates/geom/src/drafting3d.rs:450-452`; analogous sites 528-529 and 663-674. Confidence 0.99.

Top-project back box `[0,0,0]..[1e9,10,10]` together with front box `[500,-5,20]..[501,15,30]`. The front body occludes a full one-unit interval on the long back edge, but its normalized parameter width is 1e-9. `triangle_interval` discards it because `hi-lo <= 1e-8`, so the line remains visibly continuous through the occluding body. Fragment emission and coincident-line subtraction repeat this dimensionless cutoff. Compare physical lengths, not the same absolute epsilon in both model and parameter domains. This violates actual occlusion behavior within declared bounds, not the deliberate faceted-model limitation.

### F7 — P2: Remap copied associative dimensions to copied views

Patch anchor: `crates/engine/src/cmd/gost/projection.rs:947-952`. Confidence 1.0.

Create a top view and a `gost.projection.dimension` attached to it; invoke native `copy` on both handles with delta `[100,0]`. Native `modify/transform.rs:61-75,116-128` clones `Common` unchanged, hence dimension XDATA still refers to the old view handle. The copied view is recognized as new and gets a private block, but copied dimension regeneration resolves the original view and overwrites copied definition points/offset at `projection.rs:654-696`. The successful COPY leaves the dimension beside, and associated with, the original view rather than the new view. Route an explicit copy handle map through the dimension-reference consumer; do not special-case command text. Existing projection tests do not cover COPY.

### F8 — P2: Reject a non-finite public frame before projection

Patch anchor: `crates/geom/src/drafting3d.rs:554-559`. Confidence 1.0.

`Frame` exposes writable public `right`, `up`, `toward_eye` fields (18-22). A Rust caller can construct a valid frame then set `right.x = NaN`. `project` reconstructs/checks only the other vectors and tests `(checked.right-frame.right).len() > EPS`; NaN comparisons are false, so it passes and can return `Ok` strokes containing NaN. Validate finiteness of every provided frame component or use the checked reconstructed frame. Scope is the public Rust geometry API: the current JSON-command consumer uses `Frame::new`, so this is not evidence of a JSON input exploit.

### F9 — P2: Hide triangulation-only cap diagonals in native polyfaces

Patch anchor: `crates/engine/src/cmd/gost/projection.rs:187-204`. Confidence 1.0.

Create `gost.solid` using convex profile `[[0,0],[10,0],[12,5],[5,10],[0,5]]`, z0 `0`, z1 `10`. Pentagonal caps are automatically triangulated, with every outgoing face index positive. Existing `doc/src/entity/curves.rs:81-112` interprets negative indices as invisible outgoing edges and positive indices as visible, and `io/src/dxf_write/entity.rs:39-85` preserves these signs. Thus model/DXF wire geometry draws internal fan diagonals as GOST outlines even though generated projections intentionally suppress coplanar triangulation edges. Encode only synthetic diagonals as hidden using existing native support; retain the real shell edges.

Geometry tests read: five kernel tests (cube projection/section area, partial multi-body occlusion, visible precedence, open shell, intersecting shells); six engine tests (model edit/undo/DXF, section anchors, stable IDs, invalid anchors/transforms, isometric measurement/offset, override preservation/unresolved relinking). None covers F4–F9. An existing even-odd point-in-polygon predicate exists at `geom/src/curve.rs:655-673`, and hatch parity already exists in rendering; this is evidence for reusing a planar predicate, not proof of a reusable upstream 3D kernel.

## UI consumer/producer audit

All seven GOST manager modules and manager routing were inspected. Every identifier produced in `managers.rs:37-54` has an explicit renderer in 76-87. Only null-parameter menu/typed calls open dialogs; JSON objects fall through to engine execution. GOST dialogs call `app.run` themselves and return `None` intentionally: this is not a discarded command. Circle axes/static cuts/static designations/hatch use engine interactive factories. There is no dangling unconditional GOST dialog route.

### F10 — P2: Refresh stable vertex IDs after creating a body in the dialog

Patch anchor: `crates/ui-egui/src/managers/gost_projection.rs:235-242`. Confidence 1.0.

The dialog sends IDs only when its optional text field is nonempty (225-233), then saves the newly created handle without fetching/persisting its generated IDs. Create an extrusion or explicit mesh with IDs blank, change z1/a coordinate, and Apply again: the second request contains the handle but no IDs and the engine correctly rejects it (`engine/.../projection.rs:156-160`). Only explicit Load populates IDs (87-103). Populate the generated identity state on successful creation while preserving the engine requirement; ordinary create-then-edit should not require an unexplained manual reload.

### F11 — P2: Disable unsupported specification reassociation in the position editor

Patch anchor: `crates/ui-egui/src/managers/gost_position.rs:94-102` (enabled input 50). Confidence 1.0.

Load a position, change its enabled “Specification handle” to another valid owner, then Apply. The edit request silently omits `specification`; `gost.position.edit` deliberately retains the old owner (`engine/.../specification.rs:962-980`). Apply succeeds but the displayed edit never takes effect. Disable/show the owner read-only for existing positions unless reassociation is intentionally implemented end to end; do not silently ignore a live input.

### F12 — P2: Reject or disable creation-only layout changes for existing views

Patch anchor: `crates/engine/src/cmd/gost/projection.rs:471-475`; UI producer `ui-egui/src/managers/gost_projection.rs:349,398-403`. Confidence 1.0.

Load an associative view, edit its enabled Layout name to another existing paper tab, and Apply. The UI sends the value, but the engine's existing-handle branch always resolves the current space and ignores `layout`, returning success without moving the view. Make creation-only state read-only or reject conflicting input, or implement a real cross-layout move including dimensions/associations. The specification UI already demonstrates disabling its creation-only layout field (`gost_specification.rs:112-115`).

### F13 — P2: Refresh cached editors when the selected owner or document revision changes

Patch anchor: `crates/ui-egui/src/managers/gost_specification.rs:64-85`; same pattern `gost_requirements.rs:57-84`, `gost_book.rs:103-122`. Confidence 0.99.

Editor caches are keyed only by dialog name and document UID and survive Close. Open/edit specification A, close, select specification B, invoke Edit GOST Specification: initialization does not run, so the dialog still targets A and Apply edits A. Requirements has the same stale selected-target behavior. Likewise open book properties, close, change common fields via JSON/attedit, reopen and Apply: the old field map is submitted and can overwrite newer data. Refresh on dialog-open/selected-owner/revision transitions while preserving intentional in-dialog drafts (or make stale state/conflicts explicit). `gost_marks.rs:72-75` already keys symbol editing by selected handle, showing an existing convention. Explicit Load/Reload is a workaround, not automatic state synchronization.

### F14 — P2: Derive automatic-sheet controls from managed sheets rather than any frame

Patch anchor: `crates/ui-egui/src/managers/gost.rs:29-35`, input filtering 151-168. Confidence 1.0.

Set the only GOST1 sheet independent with `gost.sheet.set {"layout":"GOST1","automatic":false}`, then open sheet creation for the first automatic book sheet. UI `continuation` is based on existence of any GOST frame and is disabled in automatic mode, so TITLE/MATERIAL/ORG and other first-form fields are hidden/filtered. The engine correctly chooses form 1 from `managed_count == 0` (`sheet.rs:77-82`), yielding a first-form sheet whose fields the UI incorrectly prevented supplying. Use the engine's effective form/managed-count information in automatic mode rather than the unrelated independent continuation flag.

No GOST-specific UI regression cases were found by the UI audit; generic null-vs-JSON dispatch tests do not exercise these request/cache transitions. Parent should test UI-generated JSON and lifecycle state directly, not only engine command handlers.

## Additional export caveats (not patch findings)

* Existing `io/src/svg.rs:32-55,73-85` uses a single viewport-derived stroke width rather than each display primitive's lineweight, so generic SVG loses GOST thick/thin hierarchy. The SVG implementation is unchanged in the authorized diff: this is **pre-existing**, not a new review finding.
* Generic file-image export chooses Model; PLOT/EXPORTPDF with an explicit layout is the correct sheet path. This is an existing API contract, not proof that paper-sheet export is broken.
* Drawing OpenGost embedding (`fonts/lib.rs:39-43`, `ttf.rs:301-323`) is independent of optional CRAFT_FONTS_DIR and independent of egui chrome's font chain. PDF has permitted-font subsetting/ToUnicode plus outline fallback; DXF STYLE stores a font reference, not the TTF. Existing portable-font substitution does not include OpenGost. “Offline in CADCraftGOST” therefore must not be advertised as “font embedded in DXF/DWG”.

## Verdict

**Overall correctness: incorrect. Confidence: 0.98.** Fourteen concrete source-level defects are documented above, alongside separately labeled architectural risks and pre-existing export caveats. Registration and most native command delegation are complete rather than stubs, but copy/import identity, valid section geometry, generated-page validation and UI request/state handling need fixes before claiming reliable modular parity. No runtime validation was performed; full baseline CI and targeted regression/performance runs remain the parent's responsibility.

A cached check report itself is a snapshot until the explicit Run GOST Check button is used; lack of automatic refresh alone was not elevated to a finding. The editor-cache finding above instead concerns submitting stale values to an authoritative mutating command or selecting a different owner while an edit invocation retains the old target.
