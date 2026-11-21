//! Compile-time extension interface. Business logic never lives in the native command registry.
//!
//! Hosts explicitly choose extension defaults. Document callbacks run inside the command's
//! existing guard and Undo transaction; a callback error rolls back the entire command.

use std::sync::Arc;

use cadcraft_doc::{Drawing, Entity, Handle, Space};
use serde_json::Value;

use crate::{CommandSpec, Result, Session};

#[cfg(feature = "gost")]
mod data;
#[cfg(feature = "gost")]
mod drawing;
#[cfg(feature = "gost")]
mod gost;
#[cfg(feature = "gost")]
pub mod models;
#[cfg(feature = "gost")]
mod norms;
#[cfg(feature = "gost")]
mod specification;

/// Original command identity and snapshot, borrowed without copying the document.
#[derive(Clone, Copy)]
pub struct DocumentChange<'a> {
    pub before: Option<&'a Arc<Drawing>>,
    pub uid: Option<u64>,
    pub command: &'a str,
}

/// A native command may offer an override after resolving its own names/parameters.
#[derive(Clone, Copy)]
pub struct CommandContext<'a> {
    pub command: &'a str,
    pub params: &'a Value,
    pub resolved_name: Option<&'a str>,
}

pub type ChangeHook = fn(&mut Session, DocumentChange<'_>) -> Result<()>;
pub type CommandHook = fn(&mut Session, CommandContext<'_>) -> Option<Result<Value>>;
pub type CopyPair = (Handle, Handle);

pub struct CopyHooks {
    /// Avoid collecting identity pairs for ordinary native objects.
    pub tracks: fn(&Drawing, &[Handle]) -> bool,
    pub apply: fn(&mut Drawing, &[CopyPair]) -> Result<()>,
}

pub struct ClipboardHooks {
    /// Normalize private definitions, without touching the source drawing.
    pub prepare_blocks: Option<fn(&mut Drawing)>,
    /// Capture bounded extension metadata for the selected objects and their definitions.
    pub capture: Option<fn(&Drawing, &[Entity], &mut Drawing) -> Result<()>>,
    /// Remap metadata after adopting every top-level and nested entity, inside the command guard.
    pub pasted: Option<fn(&mut Drawing, &Drawing, &[CopyPair], &std::collections::BTreeMap<String, String>) -> Result<()>>,
}

pub struct DrawingPreset {
    pub create: fn() -> Drawing,
    pub first_layout: &'static str,
}

/// Immutable registration; no dynamic ABI, global mutable subscriptions or second command API.
pub struct Extension {
    pub id: &'static str,
    pub version: u32,
    pub commands: fn() -> Vec<CommandSpec>,
    pub drawing_preset: Option<DrawingPreset>,
    pub document_changed: ChangeHook,
    pub command_override: Option<CommandHook>,
    pub copy: Option<CopyHooks>,
    pub clipboard: Option<ClipboardHooks>,
}

/// Deterministic dependency order: drafting properties, specification, then drawing links.
pub fn installed() -> &'static [Extension] {
    #[cfg(feature = "gost")]
    {
        &[gost::EXTENSION, specification::EXTENSION, drawing::EXTENSION, models::EXTENSION]
    }
    #[cfg(not(feature = "gost"))]
    {
        &[]
    }
}

pub(crate) fn append_commands(commands: &mut Vec<CommandSpec>) {
    for extension in installed() {
        commands.extend((extension.commands)());
    }
}

fn preset() -> Option<&'static DrawingPreset> {
    installed().iter().find_map(|extension| extension.drawing_preset.as_ref())
}

/// Product Start tab. Native `Session` constructors and opened files are never converted.
pub fn session() -> Session {
    let mut session = Session::empty();
    session.extension_defaults = preset().is_some();
    session.settings.measureinit = session.extension_defaults;
    session
}

/// Product CLI/MCP session, initially in model space with a clean save/Undo baseline.
pub fn new_session() -> Session {
    let mut session = session();
    let metric = session.settings.measureinit;
    new_drawing(&mut session, metric);
    session
}

pub fn new_drawing(session: &mut Session, metric: bool) -> usize {
    if session.extension_defaults
        && metric
        && let Some(preset) = preset()
    {
        return session.new_document((preset.create)());
    }
    session.new_drawing(metric)
}

pub(crate) fn activate_new_drawing(session: &mut Session, metric: bool) -> Result<()> {
    if session.extension_defaults
        && metric
        && let Some(preset) = preset()
    {
        session.state_mut()?.space = Space::Paper(preset.first_layout.into());
        session.zoom_extents()?;
        let state = session.state_mut()?;
        state.saved = state.doc.clone();
    }
    Ok(())
}

pub(crate) fn after_command(session: &mut Session, event: DocumentChange<'_>) -> Result<()> {
    for extension in installed() {
        (extension.document_changed)(session, event)?;
    }
    // A subscriber may remove an empty generated tab in the same transaction.
    if let Some(before) = event.before
        && let Ok(state) = session.state()
        && before.layouts.len() > state.doc.layouts.len()
    {
        let state = session.state_mut()?;
        if state.space.layout().is_some_and(|name| state.doc.layout(name).is_none()) {
            state.space = Space::Model;
            state.mspace = None;
            state.selection.clear();
        }
        state.views.retain(|(space, _)| space.layout().is_none_or(|name| state.doc.layout(name).is_some()));
    }
    Ok(())
}

pub(crate) fn override_command(session: &mut Session, context: CommandContext<'_>) -> Option<Result<Value>> {
    for extension in installed() {
        if let Some(run) = extension.command_override
            && let Some(result) = run(session, context)
        {
            return Some(result);
        }
    }
    None
}

pub(crate) fn owns_copy_links(drawing: &Drawing, handles: &[Handle]) -> bool {
    installed().iter().filter_map(|extension| extension.copy.as_ref()).any(|hooks| (hooks.tracks)(drawing, handles))
}

pub(crate) fn copied_entities(drawing: &mut Drawing, pairs: &[CopyPair]) -> Result<()> {
    for hooks in installed().iter().filter_map(|extension| extension.copy.as_ref()) {
        (hooks.apply)(drawing, pairs)?;
    }
    Ok(())
}

pub(crate) fn capture_clipboard(source: &Drawing, entities: &[Entity], definitions: &mut Drawing) -> Result<()> {
    for hooks in installed().iter().filter_map(|extension| extension.clipboard.as_ref()) {
        if let Some(capture) = hooks.capture {
            capture(source, entities, definitions)?;
        }
        if let Some(prepare) = hooks.prepare_blocks {
            prepare(definitions);
        }
    }
    Ok(())
}

pub(crate) fn pasted_clipboard(
    drawing: &mut Drawing,
    definitions: &Drawing,
    pairs: &[CopyPair],
    renames: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    for run in installed().iter().filter_map(|extension| extension.clipboard.as_ref()).filter_map(|hooks| hooks.pasted) {
        run(drawing, definitions, pairs, renames)?;
    }
    Ok(())
}
