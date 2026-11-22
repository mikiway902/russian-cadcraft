//! Model metadata follows the actual clipboard graph, never a destination-local ID match.
use super::projections::projection_parent;
use super::{
    Arc, Common, Drawing, Entity, EntityKind, Handle, MAX_MODELS, MAX_VIEWS, PIECE, Result, Store, VIEW, Value, ViewLink, data, digest, entities,
    err, json, load, save, text,
};
use crate::extensions::{ClipboardHooks, CopyHooks, CopyPair};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_CLIPBOARD_MODELS: usize = 32;
const MAX_CLIPBOARD_VIEWS: usize = 128;
const MAX_CLIPBOARD_ENTITIES: usize = 50_000;
const SECTION_MARK: &str = super::section_notation::MARK;

pub(super) const HOOKS: ClipboardHooks = ClipboardHooks { prepare_blocks: None, capture: Some(capture), pasted: Some(pasted) };
pub(super) const COPY_HOOKS: CopyHooks = CopyHooks { tracks, apply: copied };

fn managed(e: &Entity) -> Result<bool> {
    Ok(data::load::<ViewLink>(&e.common, VIEW)?.is_some()
        || data::load::<Value>(&e.common, PIECE)?.is_some()
        || data::load::<super::dimensions::Link>(&e.common, super::dimensions::DIM)?.is_some()
        || data::load::<Value>(&e.common, SECTION_MARK)?.is_some())
}

fn references(e: &Entity) -> Option<&str> {
    match &e.kind {
        EntityKind::Insert(i) => Some(&i.block),
        EntityKind::Dimension(d) => d.block.as_deref(),
        EntityKind::MLeader(m) => m.block.as_ref().map(|b| b.block.as_str()),
        _ => None,
    }
}

fn tracks(d: &Drawing, handles: &[Handle]) -> bool {
    let mut blocks = Vec::new();
    let mut seen = BTreeSet::new();
    let mut walked = 0usize;
    for e in handles.iter().filter_map(|h| d.entity(*h)) {
        // Malformed metadata must reach the fallible copy callback, not bypass isolation.
        if managed(e).unwrap_or(true) {
            return true;
        }
        blocks.extend(references(e).into_iter().map(str::to_owned));
    }
    while let Some(name) = blocks.pop() {
        if !seen.insert(name.to_ascii_uppercase()) {
            continue;
        }
        let Some(block) = d.block(&name) else { continue };
        for e in block.entities.iter() {
            walked += 1;
            if walked > MAX_CLIPBOARD_ENTITIES || managed(e).unwrap_or(true) {
                return true;
            }
            blocks.extend(references(e).into_iter().map(str::to_owned));
        }
    }
    false
}

#[derive(Default)]
struct Needed {
    models: BTreeSet<String>,
    views: BTreeSet<String>,
    pieces: BTreeSet<(String, String)>,
    blocks: BTreeSet<String>,
    entities: usize,
}
impl Needed {
    fn note(&mut self, e: &Entity, block: Option<&str>) -> Result<()> {
        self.entities += 1;
        let mut linked = false;
        if let Some(link) = data::load::<ViewLink>(&e.common, VIEW)? {
            if !matches!(e.kind, EntityKind::Insert(_)) || link.view != e.handle.hex() || link.revision == 0 {
                return Err(err("inconsistent clipboard model view identity"));
            }
            if let EntityKind::Insert(insert) = &e.kind {
                self.blocks.insert(insert.block.clone());
            }
            self.models.insert(link.model_id);
            self.views.insert(link.view);
            linked = true;
        }
        if let Some(piece) = data::load::<Value>(&e.common, PIECE)? {
            let id = text(&piece, "model_id")?.to_owned();
            let sha = text(&piece, "source_sha256")?.to_owned();
            if !digest(&sha) {
                return Err(err("invalid clipboard model piece source"));
            }
            self.models.insert(id.clone());
            self.pieces.insert((id, sha));
            linked = true;
        }
        if let Some(link) = data::load::<super::dimensions::Link>(&e.common, super::dimensions::DIM)? {
            super::dimensions::validate(&link)?;
            if !matches!(e.kind, EntityKind::Dimension(_)) {
                return Err(err("exact dimension association is not on a native dimension"));
            }
            self.models.insert(link.model_id);
            self.views.insert(link.view);
            linked = true;
        }
        if data::load::<Value>(&e.common, SECTION_MARK)?.is_some() {
            linked = true;
        }
        if linked && let Some(block) = block {
            self.blocks.insert(block.to_owned());
        }
        Ok(())
    }
}

fn check_limits(store: &Store) -> Result<()> {
    if store.models.len() > MAX_CLIPBOARD_MODELS || store.views.len() > MAX_CLIPBOARD_VIEWS {
        return Err(err("model clipboard metadata count limit exceeded"));
    }
    Ok(())
}

/// Only selected/reachable links are captured. In particular, a whole source Store is not copied.
fn capture(source: &Drawing, ents: &[Entity], defs: &mut Drawing) -> Result<()> {
    let mut needed = Needed::default();
    for e in ents {
        needed.note(e, None)?;
    }
    for (name, block) in &defs.blocks {
        for e in block.entities.iter() {
            needed.note(e, Some(name))?;
        }
    }
    if needed.entities > MAX_CLIPBOARD_ENTITIES {
        return Err(err("model clipboard entity limit exceeded"));
    }
    if needed.models.is_empty() {
        isolate_definitions(defs, &needed.blocks)?;
        return Ok(());
    }
    let source_store = load(source)?;
    let models: Vec<_> = source_store.models.iter().filter(|m| needed.models.contains(&m.id)).cloned().collect();
    if models.len() != needed.models.len() {
        return Err(err("clipboard model link has no source model record"));
    }
    // A detached stale curve still needs the old source snapshot, not only the current model.
    for (id, sha) in &needed.pieces {
        if !models.iter().any(|m| &m.id == id && &m.source.sha256 == sha) {
            let mut found = false;
            for view in source_store.views.iter().filter(|v| &v.model_id == id && &v.source.sha256 == sha) {
                needed.views.insert(view.handle.clone());
                found = true;
            }
            if !found {
                return Err(err("clipboard model piece has no exact source snapshot"));
            }
        }
    }
    let views: Vec<_> = source_store.views.iter().filter(|v| needed.views.contains(&v.handle)).cloned().collect();
    if views.len() != needed.views.len() {
        return Err(err("clipboard model view has no saved source record"));
    }
    let store = Store { version: 1, next_id: source_store.next_id, models, views };
    check_limits(&store)?;
    // Check each entity link against the saved revision, including links inside nested blocks.
    let mut handles = BTreeSet::new();
    for e in ents.iter().chain(defs.blocks.values().flat_map(|b| b.entities.iter()).map(Arc::as_ref)) {
        if !handles.insert(e.handle) {
            return Err(err("duplicate clipboard source entity handle"));
        }
        if let Some(link) = data::load::<ViewLink>(&e.common, VIEW)? {
            let saved = store.views.iter().find(|v| v.handle == link.view).ok_or_else(|| err("missing clipboard saved view"))?;
            if saved.model_id != link.model_id || saved.revision != link.revision {
                return Err(err("clipboard view revision differs from its saved record"));
            }
            let EntityKind::Insert(insert) = &e.kind else { return Err(err("clipboard view is not an INSERT")) };
            if defs.block(&insert.block).is_none() {
                return Err(err("clipboard model view block is missing"));
            }
        }
    }
    isolate_definitions(defs, &needed.blocks)?;
    // The parent's capped writer/reader enforce MAX_BYTES for both clipboard and destination.
    save(defs, &store)
}

/// Isolate managed definitions, every enclosing wrapper, and their annotation dependencies.
/// Anonymous-block collision handling then clones them using the ordinary native merge path.
fn isolate_definitions(defs: &mut Drawing, managed_blocks: &BTreeSet<String>) -> Result<()> {
    let names: BTreeMap<_, _> = defs.blocks.keys().map(|n| (n.to_ascii_uppercase(), n.clone())).collect();
    if names.len() != defs.blocks.len() {
        return Err(err("ambiguous clipboard block names"));
    }
    let mut parents: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, block) in &defs.blocks {
        for target in block.entities.iter().flat_map(|e| references(e)) {
            if let Some(target) = names.get(&target.to_ascii_uppercase()) {
                parents.entry(target.clone()).or_default().push(name.clone());
                children.entry(name.clone()).or_default().push(target.clone());
            }
        }
    }
    let mut isolated: BTreeSet<_> = managed_blocks.iter().filter_map(|n| names.get(&n.to_ascii_uppercase())).cloned().collect();
    let mut queue: VecDeque<_> = isolated.iter().cloned().collect();
    while let Some(name) = queue.pop_front() {
        for parent in parents.get(&name).into_iter().flatten() {
            if isolated.insert(parent.clone()) {
                queue.push_back(parent.clone());
            }
        }
    }
    queue.extend(isolated.iter().cloned());
    while let Some(name) = queue.pop_front() {
        for child in children.get(&name).into_iter().flatten() {
            if isolated.insert(child.clone()) {
                queue.push_back(child.clone());
            }
        }
    }
    for name in isolated {
        if let Some(block) = defs.blocks.get_mut(&name) {
            Arc::make_mut(block).anonymous = true;
        }
    }
    Ok(())
}

fn pasted(d: &mut Drawing, defs: &Drawing, pairs: &[CopyPair], renames: &BTreeMap<String, String>) -> Result<()> {
    install(d, defs, pairs, renames, false)
}

fn fresh_handle(d: &mut Drawing) -> Result<Handle> {
    if d.handseed == u64::MAX {
        return Err(err("model clipboard handle space exhausted"));
    }
    Ok(d.new_handle())
}

fn install(d: &mut Drawing, defs: &Drawing, pairs: &[CopyPair], renames: &BTreeMap<String, String>, same_document: bool) -> Result<()> {
    let source = load(defs)?;
    check_limits(&source)?;
    if pairs.len() > MAX_CLIPBOARD_ENTITIES {
        return Err(err("model clipboard entity limit exceeded"));
    }
    let mut forward = BTreeMap::new();
    let mut reverse = BTreeMap::new();
    for (old, new) in pairs {
        if forward.insert(*old, *new).is_some() || reverse.insert(*new, *old).is_some() {
            return Err(err("ambiguous clipboard entity handle mapping"));
        }
    }
    if source.models.is_empty() {
        // Hand-built or malformed clipboards cannot fall through to a colliding destination Store.
        for e in entities(d).filter(|e| reverse.contains_key(&e.handle)) {
            if managed(e)? && data::load::<Value>(&e.common, SECTION_MARK)?.is_none() {
                return Err(err("clipboard model association has no transferred metadata"));
            }
        }
        return remap_section_marks(d, &reverse, &BTreeMap::new(), &BTreeMap::new());
    }
    for (name, block) in &defs.blocks {
        if block.anonymous && !renames.contains_key(&name.to_ascii_uppercase()) {
            return Err(err("private clipboard block could not be isolated"));
        }
    }
    let mut destination = load(d)?;
    let mut ids = BTreeMap::new();
    for model in &source.models {
        let id = if same_document {
            let original = super::model(&destination, &model.id)?;
            if original.revision != model.revision || original.metadata != model.metadata {
                return Err(err("same-document model changed during copy"));
            }
            model.id.clone()
        } else {
            if destination.models.len() >= MAX_MODELS {
                return Err(err("destination linked-model count limit exceeded"));
            }
            let id = destination.next_id.to_string();
            destination.next_id = destination.next_id.checked_add(1).ok_or_else(|| err("local model identity space exhausted"))?;
            let mut copied = model.clone();
            copied.id = id.clone();
            destination.models.push(copied);
            id
        };
        ids.insert(model.id.clone(), id);
    }
    let mut view_handles = BTreeMap::new();
    let first_copied_view = destination.views.len();
    for view in &source.views {
        if destination.views.len() >= MAX_VIEWS {
            return Err(err("destination saved-view count limit exceeded"));
        }
        let old = Handle::parse_hex(&view.handle).ok_or_else(|| err("invalid clipboard view handle"))?;
        let new = match forward.get(&old) {
            Some(h) => *h,
            None => fresh_handle(d)?,
        };
        let mut copied = view.clone();
        copied.handle = new.hex();
        copied.model_id = ids.get(&view.model_id).ok_or_else(|| err("clipboard view model is missing"))?.clone();
        copied.parameters["id"] = json!(copied.model_id);
        view_handles.insert(view.handle.clone(), copied.handle.clone());
        destination.views.push(copied);
    }
    let mut seen_views = BTreeSet::new();
    let mut remap = |e: &Entity| -> Result<Option<Common>> {
        let Some(old) = reverse.get(&e.handle) else { return Ok(None) };
        if let Some(mut link) = data::load::<ViewLink>(&e.common, VIEW)? {
            let saved = source.views.iter().find(|v| v.handle == link.view).ok_or_else(|| err("clipboard view metadata missing"))?;
            if !matches!(e.kind, EntityKind::Insert(_))
                || link.view != old.hex()
                || saved.model_id != link.model_id
                || saved.revision != link.revision
            {
                return Err(err("clipboard view identity does not match the adopted entity"));
            }
            seen_views.insert(link.view.clone());
            link.model_id = ids.get(&link.model_id).ok_or_else(|| err("clipboard view model mapping missing"))?.clone();
            link.view = view_handles.get(&link.view).ok_or_else(|| err("clipboard view handle mapping missing"))?.clone();
            let mut common = e.common.clone();
            data::save(&mut common, VIEW, &link)?;
            return Ok(Some(common));
        }
        if let Some(mut link) = data::load::<super::dimensions::Link>(&e.common, super::dimensions::DIM)? {
            super::dimensions::validate(&link)?;
            let saved = source.views.iter().find(|v| v.handle == link.view).ok_or_else(|| err("clipboard exact dimension saved view missing"))?;
            if saved.model_id != link.model_id || saved.source.sha256 != link.source_sha256 {
                return Err(err("clipboard exact dimension source snapshot mismatch"));
            }
            link.model_id = ids.get(&link.model_id).ok_or_else(|| err("clipboard dimension model mapping missing"))?.clone();
            // Missing views get reserved fresh handles, never destination-local or original IDs.
            link.view = view_handles.get(&link.view).ok_or_else(|| err("clipboard dimension view mapping missing"))?.clone();
            let mut common = e.common.clone();
            data::save(&mut common, super::dimensions::DIM, &link)?;
            return Ok(Some(common));
        }
        if let Some(mut piece) = data::load::<Value>(&e.common, PIECE)? {
            let id = text(&piece, "model_id")?;
            let new = ids.get(id).ok_or_else(|| err("clipboard curve model mapping missing"))?;
            let sha = text(&piece, "source_sha256")?;
            if !source.models.iter().any(|m| m.id == id && m.source.sha256 == sha)
                && !source.views.iter().any(|v| v.model_id == id && v.source.sha256 == sha)
            {
                return Err(err("clipboard curve source snapshot missing"));
            }
            piece["model_id"] = json!(new);
            let mut common = e.common.clone();
            data::save(&mut common, PIECE, &piece)?;
            return Ok(Some(common));
        }
        Ok(None)
    };
    // EntityStore::modify is also used for nested block entities: Drawing::modify_entity only
    // addresses model/layout space. PASTEBLOCK's freshly created wrapper is included here too.
    let mut updates = BTreeMap::new();
    let mut seen_entities = BTreeSet::new();
    for e in entities(d).filter(|e| reverse.contains_key(&e.handle)) {
        if !seen_entities.insert(e.handle) {
            return Err(err("adopted clipboard handle collides with another entity"));
        }
        if let Some(common) = remap(e)? {
            updates.insert(e.handle, common);
        }
    }
    if seen_entities.len() != reverse.len()
        || source.views.iter().any(|v| Handle::parse_hex(&v.handle).is_some_and(|h| forward.contains_key(&h)) && !seen_views.contains(&v.handle))
    {
        return Err(err("clipboard entity or saved view mapping is incomplete"));
    }
    // Reserved handles for snapshot-only records are not transferred INSERTs. Resolve only
    // after every adopted entity has been checked, so selection order cannot break chains.
    let active_views: BTreeMap<_, _> = source
        .views
        .iter()
        .filter(|v| seen_views.contains(&v.handle))
        .map(|v| {
            Handle::parse_hex(&v.handle).map(|handle| (handle, v.handle.as_str())).ok_or_else(|| err("invalid transferred projection view handle"))
        })
        .collect::<Result<_>>()?;
    for (original, view) in source.views.iter().zip(&mut destination.views[first_copied_view..]) {
        if let Some(parent) = projection_parent(view)? {
            let new_parent =
                seen_views.contains(&original.handle).then(|| active_views.get(&parent).and_then(|old| view_handles.get(*old))).flatten();
            let link = &mut view.provenance["projection_link"];
            if let Some(parent) = new_parent {
                link["parent"] = json!(parent);
            } else {
                link["parent"] = Value::Null;
                link["status"] = json!("missing_parent");
            }
        }
        if let Some(notation) = view.provenance.get_mut("section").and_then(|section| section.get_mut("notation")) {
            let new_source = (seen_views.contains(&original.handle) && notation["status"] == "linked")
                .then(|| {
                    notation["source_view"]
                        .as_str()
                        .and_then(Handle::parse_hex)
                        .and_then(|source| active_views.get(&source))
                        .and_then(|old| view_handles.get(*old))
                })
                .flatten();
            notation["source_view"] = new_source.map_or(Value::Null, |source| json!(source));
            notation["status"] = json!(if new_source.is_some() { "linked" } else { "missing_source" });
            view.parameters["source_view"] = notation["source_view"].clone();
        }
    }
    for space in std::iter::once(&mut d.model).chain(d.layouts.iter_mut().map(|l| &mut l.entities)) {
        apply_updates(space, &mut updates);
    }
    for block in d.blocks.values_mut() {
        if block.entities.iter().any(|e| updates.contains_key(&e.handle)) {
            apply_updates(&mut Arc::make_mut(block).entities, &mut updates);
        }
        let mut nested = Vec::new();
        for e in block.entities.iter().filter(|e| reverse.contains_key(&e.handle)) {
            if let Some((common, dimension)) = super::dimensions::nested_copy(e)? {
                nested.push((e.handle, common, dimension));
            }
        }
        if !nested.is_empty() {
            let block = Arc::make_mut(block);
            for (h, common, dimension) in nested {
                block.entities.modify(h, |e| {
                    e.common = common;
                    e.kind = EntityKind::Dimension(dimension);
                });
            }
        }
    }
    if !updates.is_empty() {
        return Err(err("adopted model clipboard entity disappeared"));
    }
    let mut source_blocks: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for e in entities(d).filter(|e| reverse.contains_key(&e.handle)) {
        if data::load::<ViewLink>(&e.common, VIEW)?.is_some()
            && let EntityKind::Insert(insert) = &e.kind
        {
            source_blocks.entry(insert.block.to_ascii_uppercase()).or_default().insert(e.handle.hex());
        }
    }
    let linked_sections = destination.views[first_copied_view..]
        .iter()
        .filter_map(|view| {
            let notation = &view.provenance["section"]["notation"];
            if notation["status"] != "linked" {
                return None;
            }
            let original = reverse.get(&Handle::parse_hex(&view.handle)?)?;
            let source = notation["source_view"].as_str()?;
            Some((*original, (view.handle.clone(), source.to_owned())))
        })
        .collect();
    remap_section_marks(d, &reverse, &source_blocks, &linked_sections)?;
    save(d, &destination)
}

fn apply_updates(store: &mut cadcraft_doc::EntityStore, updates: &mut BTreeMap<Handle, Common>) {
    let handles: Vec<_> = store.iter().filter(|e| updates.contains_key(&e.handle)).map(|e| e.handle).collect();
    for h in handles {
        if let Some(common) = updates.remove(&h) {
            store.modify(h, |e| e.common = common);
        }
    }
}

/// Only adopted native entities may acquire new associations. Orphan marks in copied
/// source blocks disappear; detached marks remain ordinary editable static geometry.
fn remap_section_marks(
    d: &mut Drawing,
    reverse: &BTreeMap<Handle, Handle>,
    source_blocks: &BTreeMap<String, BTreeSet<String>>,
    sections: &BTreeMap<Handle, (String, String)>,
) -> Result<()> {
    let remap = |store: &mut cadcraft_doc::EntityStore, sources: Option<&BTreeSet<String>>| -> Result<()> {
        let mut updates = BTreeMap::new();
        let mut removed = Vec::new();
        for e in store.iter().filter(|e| reverse.contains_key(&e.handle)) {
            let Some(mut mark) = data::load::<Value>(&e.common, SECTION_MARK)? else { continue };
            let target = mark["section"].as_str().and_then(Handle::parse_hex).and_then(|old| sections.get(&old));
            let target = target.filter(|(_, source)| sources.is_none_or(|sources| sources.contains(source)));
            if sources.is_some() && target.is_none() {
                removed.push(e.handle);
                continue;
            }
            let mut common = e.common.clone();
            if let Some((section, _)) = target {
                mark["section"] = json!(section);
                data::save(&mut common, SECTION_MARK, &mark)?;
            } else {
                super::section_notation::detach_mark(&mut common);
            }
            updates.insert(e.handle, common);
        }
        apply_updates(store, &mut updates);
        for handle in removed {
            store.remove(handle);
        }
        Ok(())
    };
    for store in std::iter::once(&mut d.model).chain(d.layouts.iter_mut().map(|layout| &mut layout.entities)) {
        remap(store, None)?;
    }
    for (name, block) in &mut d.blocks {
        if block.entities.iter().any(|e| reverse.contains_key(&e.handle)) {
            remap(&mut Arc::make_mut(block).entities, source_blocks.get(&name.to_ascii_uppercase()))?;
        }
    }
    Ok(())
}

fn copied(d: &mut Drawing, pairs: &[CopyPair]) -> Result<()> {
    let handles: Vec<_> = pairs.iter().map(|(old, _)| *old).collect();
    if !tracks(d, &handles) {
        return Ok(());
    }
    let originals: Vec<_> = handles.iter().filter_map(|old| d.entity(*old).map(|e| (**e).clone())).collect();
    let defs = crate::cmd::clipboard::defs_for(d, &originals)?;
    let mut mapped = Vec::new();
    let renames = crate::cmd::clipboard::merge(d, &defs, &mut mapped);
    for (_, new) in pairs {
        d.modify_entity(*new, |e| crate::cmd::clipboard::rename_references(e, &renames))?;
    }
    mapped.extend_from_slice(pairs);
    install(d, &defs, &mapped, &renames, true)
}

#[cfg(test)]
#[path = "clipboard_tests.rs"]
mod tests;
