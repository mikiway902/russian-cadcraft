//! Explicit ESKD presets and drawing tools. This is not a conformity certificate.

mod annotations;
mod profile;
mod sheet;

use super::CommandSpec;

pub(super) fn specs() -> Vec<CommandSpec> {
    profile::specs().into_iter().chain(sheet::specs()).chain(annotations::specs()).collect()
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
