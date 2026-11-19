//! Explicit ESKD presets and drawing tools. This is not a conformity certificate.

mod annotations;
mod axes;
mod book;
mod data;
mod marks;
mod navigation;
mod norms;
mod profile;
mod projection;
mod requirements;
mod sheet;
mod specification;

use super::CommandSpec;

pub(super) fn specs() -> Vec<CommandSpec> {
    profile::specs()
        .into_iter()
        .chain(sheet::specs())
        .chain(book::specs())
        .chain(annotations::specs())
        .chain(marks::specs())
        .chain(navigation::specs())
        .chain(requirements::specs())
        .chain(specification::specs())
        .chain(projection::specs())
        .collect()
}

pub(crate) fn after_command(
    s: &mut crate::Session,
    before: Option<&std::sync::Arc<cadcraft_doc::Drawing>>,
    uid: Option<u64>,
    command: &str,
) -> crate::Result<()> {
    book::after_command(s, before, uid)?;
    axes::after_command(s, before, uid)?;
    requirements::after_command(s, before, uid)?;
    specification::after_command(s, before, uid)?;
    projection::after_command(s, before, uid, command)?;
    // Automatic repagination can remove an empty generated tab, just like LAYOUT Delete.
    if let Some(before) = before
        && let Ok(state) = s.state()
        && before.layouts.len() > state.doc.layouts.len()
    {
        let state = s.state_mut()?;
        let doc = &state.doc;
        if state.space.layout().is_some_and(|name| doc.layout(name).is_none()) {
            state.space = cadcraft_doc::Space::Model;
            state.mspace = None;
            state.selection.clear();
        }
        state.views.retain(|(space, _)| space.layout().is_none_or(|name| doc.layout(name).is_some()));
    }
    Ok(())
}

pub(crate) fn owns_copy_links(d: &cadcraft_doc::Drawing, handles: &[cadcraft_doc::Handle]) -> bool {
    handles.iter().filter_map(|h| d.entity(*h)).any(|e| match &e.kind {
        cadcraft_doc::EntityKind::Insert(i) => i.block.starts_with("GOST-PROJECTION-"),
        cadcraft_doc::EntityKind::Dimension(_) => e.common.xdata.iter().any(|t| t.code == 1001 && t.value.eq_ignore_ascii_case("CADCRAFTGOST")),
        _ => false,
    })
}

/// COPY supplies exact identity pairs; association consumers never infer them from coordinates.
pub(crate) fn copied_entities(d: &mut cadcraft_doc::Drawing, pairs: &[(cadcraft_doc::Handle, cadcraft_doc::Handle)]) -> crate::Result<()> {
    projection::copied_entities(d, pairs)
}

pub(crate) fn prepare_clipboard_blocks(d: &mut cadcraft_doc::Drawing) {
    marks::prepare_clipboard_blocks(d);
}

pub(crate) fn new_drawing() -> cadcraft_doc::Drawing {
    let mut d = cadcraft_doc::Drawing::new_metric();
    // Do not leave blank, non-ESKD paper tabs in the initial drawing or PDF book.
    d.layouts.clear();
    profile::configure(&mut d, cadcraft_fonts::GOST_FONT, 1.0, true);
    // Native table-cell defaults still reference Standard; keep that face ESKD too.
    for style in &mut d.text_styles {
        style.font = cadcraft_fonts::GOST_FONT.into();
    }
    for style in &mut d.mleader_styles {
        style.text_style = "GOST".into();
        style.text_height = 3.5;
        style.arrow_size = 3.5;
        style.landing_gap = 1.0;
        style.dogleg = 7.0;
    }
    for style in &mut d.table_styles {
        style.text_height = 3.5;
    }
    sheet::initial(&mut d);
    d
}
