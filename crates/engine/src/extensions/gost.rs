//! Explicit ESKD presets and drawing tools. This is not a conformity certificate.

mod annotations;
mod axes;
pub(super) mod book;
mod marks;
mod navigation;
mod profile;
mod requirements;
pub(super) mod sheet;
pub(super) mod standards;

use super::{CommandContext, DrawingPreset, Extension, data};
use crate::cmd::CommandSpec;

pub(super) const EXTENSION: Extension = Extension {
    id: "gost",
    version: 1,
    commands: specs,
    drawing_preset: Some(DrawingPreset { create: new_drawing, first_layout: "GOST1" }),
    document_changed: |session, event| after_command(session, event.before, event.uid),
    command_override: Some(override_command),
    copy: None,
    clipboard: Some(super::ClipboardHooks { prepare_blocks: Some(prepare_clipboard_blocks), capture: None, pasted: None }),
};

pub(super) fn specs() -> Vec<CommandSpec> {
    profile::specs()
        .into_iter()
        .chain(standards::specs())
        .chain(sheet::specs())
        .chain(book::specs())
        .chain(annotations::specs())
        .chain(marks::specs())
        .chain(navigation::specs())
        .chain(requirements::specs())
        .collect()
}

fn after_command(s: &mut crate::Session, before: Option<&std::sync::Arc<cadcraft_doc::Drawing>>, uid: Option<u64>) -> crate::Result<()> {
    book::after_command(s, before, uid)?;
    axes::after_command(s, before, uid)?;
    requirements::after_command(s, before, uid)?;
    Ok(())
}

fn prepare_clipboard_blocks(d: &mut cadcraft_doc::Drawing) {
    marks::prepare_clipboard_blocks(d);
}

fn override_command(session: &mut crate::Session, context: CommandContext<'_>) -> Option<crate::Result<serde_json::Value>> {
    let Ok(drawing) = session.doc() else { return None };
    match context.command {
        "layout.new" if context.params.get("viewport").is_none() && drawing.header.str("TEXTSTYLE", "") == "GOST" => {
            let Some(name) = context.resolved_name else {
                return Some(Err(crate::cmd::bad("layout.new", "resolved layout name required")));
            };
            Some(sheet::run(session, &serde_json::json!({"layout": name})).map(|sheet| serde_json::json!({"name":name,"viewport":null,"gost":sheet})))
        }
        "centermark" if drawing.header.str("CENTERLAYER", "") == "GOST-CENTER" => Some(annotations::center(session, context.params)),
        _ => None,
    }
}

pub(crate) fn new_drawing() -> cadcraft_doc::Drawing {
    let mut d = standards::initial_drawing();
    // Do not leave blank, non-ESKD paper tabs in the initial drawing or PDF book.
    d.layouts.clear();
    let policy = &standards::default_profile().drafting;
    profile::configure(&mut d, &policy.font, policy.annotation_scale, true, policy);
    // Native table-cell defaults still reference Standard; keep that face ESKD too.
    for style in &mut d.text_styles {
        style.font.clone_from(&policy.font);
    }
    for style in &mut d.mleader_styles {
        style.text_style = "GOST".into();
        style.text_height = policy.dimension_text_mm;
        style.arrow_size = policy.dimension_arrow_mm;
        style.landing_gap = 1.0;
        style.dogleg = policy.baseline_spacing_mm;
    }
    for style in &mut d.table_styles {
        style.text_height = policy.dimension_text_mm;
    }
    sheet::initial(&mut d);
    d
}
