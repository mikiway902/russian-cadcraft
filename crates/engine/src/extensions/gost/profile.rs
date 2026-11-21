//! Native ESKD drafting preset and deliberately limited, read-only checks.
//!
//! Sources: GOST 2.303-68 (line families/width ratios), GOST 2.304-81 (lettering),
//! GOST 2.307-2011 (dimensions), https://files.stroyinf.ru/Data2/1/4293800/4293800223.pdf.
//! This is a preset, not certification: 0.7/0.25 mm strokes, 3.5 mm text/arrows,
//! 2 mm extension protrusion, 1 mm text gap and 7 mm baseline spacing are selected
//! working values, not claims that every GOST drawing must use these exact values.
//! Spacing from the part to the first dimension, readable arrow placement, adequate
//! dimensions and the suitability of lettering still require drawing-level review.

use cadcraft_color::Color;
use cadcraft_doc::{DimStyle, Drawing, EntityKind, Layer, Linetype, Lineweight, TextStyle};
use serde_json::{Value, json};

use super::standards::{self, Drafting};
use crate::cmd::{CommandSpec, bad};
use crate::{Result, Session};

const NAME: &str = "GOST";
const DEFAULT_FONT: &str = cadcraft_fonts::GOST_FONT;
// Practical annotation bounds, not a GOST restriction. Keep products with paper
// sizes finite and useful, and reject hostile/underflowing scales before any edit.
const MIN_SCALE: f64 = 1e-6;
const MAX_SCALE: f64 = 1e6;
const LAYERS: &[(&str, &str, bool)] = &[
    ("GOST-OUTLINE", "Continuous", true),
    ("GOST-THIN", "Continuous", false),
    ("GOST-HIDDEN", "GOST-HIDDEN", false),
    ("GOST-CENTER", "GOST-CENTER", false),
    ("GOST-HATCH", "Continuous", false),
    ("GOST-TEXT", "Continuous", false),
    ("GOST-DIM", "Continuous", false),
    ("GOST-FRAME", "Continuous", true),
    ("GOST-SECTION", "Continuous", true),
];
const LINETYPES: &[(&str, &str, &[f64])] = &[
    ("GOST-HIDDEN", "ESKD hidden: 4 mm dash, 2 mm gap", &[4.0, -2.0]),
    ("GOST-CENTER", "ESKD centre: 15/3/1/3 mm", &[15.0, -3.0, 1.0, -3.0]),
    ("GOST-PHANTOM", "ESKD thin double-chain: 15/3/1/3/1/3 mm", &[15.0, -3.0, 1.0, -3.0, 1.0, -3.0]),
];

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("gost.setup", "GOST Drawing Setup", install)
            .menu(&["Format", "GOST", "GOST Drawing Setup"])
            .params("{font?: installed font family/file, scale?: positive overall annotation scale (default 1)} → metric ESKD preset and warnings; never converts existing geometry"),
        CommandSpec::new("gost.check", "GOST Drawing Check", run_check)
            .menu(&["Format", "GOST", "GOST Drawing Check"])
            .params("{} → limited measurable preset findings and mandatory manual/unverified checks; not a certificate of compliance")
            .noundo(),
    ]
}

const MAX_FINDINGS: usize = 2000;

fn options<'a>(p: &'a Value, policy: &'a Drafting) -> Result<(&'a str, f64)> {
    if !p.is_object() {
        return Err(bad("gost.setup", "parameters must be an object"));
    }
    let font = match p.get("font") {
        None => &policy.font,
        Some(v) => v.as_str().ok_or_else(|| bad("gost.setup", "font must be a nonempty installed font family or file name"))?.trim(),
    };
    let scale = match p.get("scale") {
        None => policy.annotation_scale,
        Some(v) => v.as_f64().ok_or_else(|| bad("gost.setup", "scale must be a number, not a string or null"))?,
    };
    validate_options(font, scale)?;
    Ok((font, scale))
}

fn validate_options(font: &str, scale: f64) -> Result<()> {
    if font.trim().is_empty() || font.len() > 255 || font.chars().any(char::is_control) {
        return Err(bad("gost.setup", "font must contain 1..255 bytes and no control characters"));
    }
    if !scale.is_finite() || !(MIN_SCALE..=MAX_SCALE).contains(&scale) {
        return Err(bad("gost.setup", format!("scale must be finite and between {MIN_SCALE} and {MAX_SCALE}")));
    }
    Ok(())
}

fn populated(d: &Drawing) -> bool {
    !d.model.is_empty() || d.layouts.iter().any(|l| !l.entities.is_empty()) || d.blocks.values().any(|b| !b.entities.is_empty())
}

fn dimension_style(scale: f64, policy: &Drafting) -> DimStyle {
    DimStyle {
        name: NAME.into(),
        scale,
        arrow_size: policy.dimension_arrow_mm,
        ext_offset: 0.0,
        ext_extend: policy.extension_protrusion_mm,
        text_height: policy.dimension_text_mm,
        text_gap: policy.text_gap_mm,
        // Decimal places are an editable precision choice; trailing zeros are suppressed.
        decimals: 2,
        angular_decimals: 0,
        linear_factor: 1.0,
        text_above: 1,
        text_inside_horizontal: false,
        text_outside_horizontal: false,
        dim_line_color: Color::Index(7),
        ext_line_color: Color::Index(7),
        text_color: Color::Index(7),
        text_style: NAME.into(),
        center_mark: 0.0,
        zero_suppression: 8,
        baseline_spacing: policy.baseline_spacing_mm,
        decimal_separator: ",".into(),
        dim_linetype: "Continuous".into(),
        ext_linetype1: "Continuous".into(),
        ext_linetype2: "Continuous".into(),
        dim_lineweight: i16::from(policy.thin_width_mm100),
        ext_lineweight: i16::from(policy.thin_width_mm100),
        force_line_inside: true,
        ..DimStyle::default()
    }
}

/// Apply the metric model preset, never implicitly converting existing geometry.
pub(super) fn install(s: &mut Session, p: &Value) -> Result<Value> {
    apply(s, p, InstallMode::MetricDrawing)
}

/// Install native paper-mm drafting tables without changing ANY model header.
/// Omitted font/scale retain the existing GOST profile, so another sheet does not
/// silently replace a selected conforming font or model annotation scale.
pub(super) fn install_paper(s: &mut Session, p: &Value) -> Result<Value> {
    apply(s, p, InstallMode::PaperOnly)
}

#[derive(Clone, Copy)]
enum InstallMode {
    MetricDrawing,
    PaperOnly,
}

pub(super) fn configure(d: &mut Drawing, font: &str, scale: f64, model_preset: bool, policy: &Drafting) {
    for &(name, description, pattern) in LINETYPES {
        let expected = || Linetype::simple(name, description, pattern);
        if let Some(record) = d.linetypes.iter_mut().find(|r| r.name.eq_ignore_ascii_case(name)) {
            if model_preset {
                *record = expected();
            }
        } else {
            d.linetypes.push(expected());
        }
    }
    // Continuous is a predefined linetype, not a private GOST record: preserve it.
    if d.linetype("Continuous").is_none() {
        d.linetypes.push(Linetype::continuous());
    }
    for &(name, linetype, thick) in LAYERS {
        let width = if thick { policy.outline_width_mm100 } else { u16::from(policy.thin_width_mm100) };
        let expected = || Layer { name: name.into(), linetype: linetype.into(), lineweight: Lineweight::Mm100(width), ..Layer::default() };
        if let Some(record) = d.layer_mut(name) {
            if model_preset {
                *record = expected();
            }
        } else {
            d.layers.push(expected());
        }
    }
    let text = || TextStyle { name: NAME.into(), font: font.into(), ..TextStyle::default() };
    if let Some(record) = d.text_styles.iter_mut().find(|r| r.name.eq_ignore_ascii_case(NAME)) {
        if model_preset {
            *record = text();
        }
    } else {
        d.text_styles.push(text());
    }
    let dim = || dimension_style(scale, policy);
    if let Some(record) = d.dim_styles.iter_mut().find(|r| r.name.eq_ignore_ascii_case(NAME)) {
        if model_preset {
            *record = dim();
        }
    } else {
        d.dim_styles.push(dim());
    }
    if model_preset {
        d.header.set_i64("INSUNITS", 4);
        d.header.set_i64("MEASUREMENT", 1);
        d.header.set_i64("LUNITS", 2);
        d.header.set_i64("LUPREC", 2);
        d.header.set_i64("AUNITS", 0);
        d.header.set_i64("AUPREC", 0);
        d.header.set_str("TEXTSTYLE", NAME);
        d.header.set_str("DIMSTYLE", NAME);
        d.header.set_str("CLAYER", "GOST-OUTLINE");
        d.header.set_str("CELTYPE", "ByLayer");
        d.header.set_i64("CECOLOR", 256);
        d.header.set_i64("CELWEIGHT", -1);
        d.header.set_f64("CELTSCALE", 1.0);
        d.header.set_f64("TEXTSIZE", policy.dimension_text_mm * scale);
        d.header.set_f64("DIMSCALE", scale);
        d.header.set_str("HPLAYER", "GOST-HATCH");
        d.header.set_str("HPNAME", "ANSI31");
        // Metric ANSI31 is ISO-scaled: 0.125 × 25.4 = 3.175 mm.
        d.header.set_f64("HPSCALE", (3.0 / 3.175) * scale);
        d.header.set_f64("HPANG", 0.0);
        d.header.set_i64("HPASSOC", 1);
        d.header.set_str("CENTERLAYER", "GOST-CENTER");
        d.header.set_str("CENTERLTYPE", "GOST-CENTER");
        d.header.set_f64("CENTEREXE", 3.0 * scale);
        d.sync_dim_vars();
    }
}

/// All fallible validation precedes mutation in either entry point.
fn apply(s: &mut Session, p: &Value, mode: InstallMode) -> Result<Value> {
    let policy = standards::drafting(s.doc()?)?;
    let (font, scale) = options(p, policy)?;
    let d = s.doc()?;
    let model_preset = matches!(mode, InstallMode::MetricDrawing);
    let inherited_font = if !model_preset && p.get("font").is_none() { d.text_style(NAME).map(|style| style.font.clone()) } else { None };
    let font = inherited_font.as_deref().unwrap_or(font);
    let scale = if !model_preset && p.get("scale").is_none() { d.dim_style(NAME).map_or(scale, |style| style.scale) } else { scale };
    if !model_preset {
        if p.get("font").is_some() && d.text_style(NAME).is_some_and(|style| style.font != font) {
            return Err(bad(
                "gost.sheet",
                "the requested font conflicts with the existing GOST text style; use gost.setup explicitly to change the shared profile",
            ));
        }
        if p.get("scale").is_some() && d.dim_style(NAME).is_some_and(|style| style.scale != scale) {
            return Err(bad(
                "gost.sheet",
                "the requested scale conflicts with the existing GOST dimension style; use gost.setup explicitly to change the shared profile",
            ));
        }
    }
    validate_options(font, scale)?;
    if model_preset && populated(d) && d.header.get("INSUNITS").and_then(cadcraft_doc::HVal::as_f64) != Some(4.0) {
        return Err(bad(
            "gost.setup",
            "existing model/layout/block geometry is not explicitly in millimetres (INSUNITS=4); convert or establish its true units first. GOST setup never rescales geometry or relabels unknown/non-mm geometry",
        ));
    }
    let d = s.doc_mut()?;
    if model_preset {
        standards::select_default(d)?;
    }
    configure(d, font, scale, model_preset, policy);
    // Do not reset global LTSCALE: that would silently change unrelated linework.
    let mut warnings = vec![font_warning(font)];
    if d.header.f64("LTSCALE", 1.0) != 1.0 {
        warnings.push(
            "LTSCALE is not 1: existing global linetype scaling was preserved; review printed GOST dash lengths or explicitly set LTSCALE to 1."
                .into(),
        );
    }
    Ok(json!({
        "profile": "GOST/ESKD", "units": if model_preset { "mm" } else { "paper_mm" }, "modelHeaderChanged": model_preset, "textStyle": NAME, "dimStyle": NAME,
        "font": font, "scale": scale, "geometryRescaled": false,
        "paperTextHeight": policy.dimension_text_mm, "paperArrowSize": policy.dimension_arrow_mm,
        "normativeProfile": standards::response(d)?,
        "warnings": warnings,
        "message": "Native ESKD preset installed; run gost.check and review the drawing. This is not a certificate of GOST compliance."
    }))
}

fn font_warning(font: &str) -> String {
    if font.eq_ignore_ascii_case(DEFAULT_FONT) {
        "Bundled OpenGost Type B (SIL OFL 1.1), designed for GOST 2.304-81. Verify printed document sizes and special symbols; this is not certification of the document.".into()
    } else {
        format!("Verify font `{font}` resolves without substitution and conforms to GOST 2.304. CADCraft Stroke is not a verified GOST font.")
    }
}

fn finding(findings: &mut Vec<Value>, severity: &str, code: &str, location: &str, message: &str, actual: Value, expected: Value) {
    findings.push(json!({ "severity": severity, "code": code, "location": location, "message": message, "actual": actual, "expected": expected }));
}

fn check_dimension_style(st: &DimStyle, location: &str, findings: &mut Vec<Value>, expected_factor: f64, policy: &Drafting) {
    // Exact-value findings below are deviations from THIS preset, not a claim that
    // alternative valid GOST sizes or precision choices violate the standard.
    macro_rules! preset {
        ($field:ident, $value:expr) => {
            if st.$field != $value {
                finding(findings, "warning", concat!("dimension.preset.", stringify!($field)), location,
                    "Differs from the ESKD preset; review the effective style/overrides. Reapply gost.setup for the named style, or clear unwanted per-dimension overrides.",
                    json!(st.$field), json!($value));
            }
        };
    }
    if !st.scale.is_finite() || !(MIN_SCALE..=MAX_SCALE).contains(&st.scale) {
        finding(
            findings,
            "error",
            "dimension.scale",
            location,
            "Use a finite positive overall annotation scale.",
            json!(st.scale),
            json!({"min": MIN_SCALE, "max": MAX_SCALE}),
        );
    }
    preset!(text_height, policy.dimension_text_mm);
    preset!(arrow_size, policy.dimension_arrow_mm);
    preset!(ext_extend, policy.extension_protrusion_mm);
    preset!(ext_offset, 0.0);
    preset!(text_gap, policy.text_gap_mm);
    preset!(text_above, 1);
    preset!(text_inside_horizontal, false);
    preset!(text_outside_horizontal, false);
    preset!(dim_lineweight, i16::from(policy.thin_width_mm100));
    preset!(ext_lineweight, i16::from(policy.thin_width_mm100));
    preset!(dim_line_color, Color::Index(7));
    preset!(ext_line_color, Color::Index(7));
    preset!(text_color, Color::Index(7));
    preset!(linear_factor, expected_factor);
    preset!(linear_unit, 2);
    preset!(angular_unit, 0);
    preset!(tick_size, 0.0);
    preset!(fixed_ext, false);
    preset!(suppress_ext1, false);
    preset!(suppress_ext2, false);
    preset!(suppress_dim1, false);
    preset!(suppress_dim2, false);
    preset!(annotative, false);
    for (field, actual, expected) in [
        ("decimalSeparator", st.decimal_separator.as_str(), ","),
        ("textStyle", st.text_style.as_str(), NAME),
        ("dimLinetype", st.dim_linetype.as_str(), "Continuous"),
        ("extLinetype1", st.ext_linetype1.as_str(), "Continuous"),
        ("extLinetype2", st.ext_linetype2.as_str(), "Continuous"),
        ("arrowBlock", st.arrow_block.as_str(), ""),
        ("arrowBlock1", st.arrow_block1.as_str(), ""),
        ("arrowBlock2", st.arrow_block2.as_str(), ""),
    ] {
        if !actual.eq_ignore_ascii_case(expected) {
            finding(
                findings,
                "warning",
                &format!("dimension.preset.{field}"),
                location,
                "Review this effective dimension setting against the ESKD preset.",
                json!(actual),
                json!(expected),
            );
        }
    }
    if !st.baseline_spacing.is_finite() || st.baseline_spacing < 7.0 {
        finding(
            findings,
            "warning",
            "dimension.baselineSpacing",
            location,
            "Use at least 7 mm paper baseline spacing; actual dimension-line separation still needs manual review.",
            json!(st.baseline_spacing),
            json!({"minimumPaperMm": 7}),
        );
    }
    if !st.post.is_empty() {
        finding(
            findings,
            "warning",
            "dimension.post",
            location,
            "Review the dimension prefix/suffix: linear dimensions in mm normally omit the unit symbol.",
            json!(st.post),
            json!(""),
        );
    }
}

fn check_text_style(d: &Drawing, findings: &mut Vec<Value>) {
    if let Some(st) = d.text_style(NAME) {
        if st.height != 0.0
            || st.width_factor != 1.0
            || st.oblique != 0.0
            || st.backwards
            || st.upside_down
            || st.vertical
            || st.annotative
            || !st.big_font.is_empty()
        {
            finding(
                findings,
                "warning",
                "text.style",
                "textStyle.GOST",
                "Use variable-height, upright, unmirrored, non-annotative GOST text with width factor 1; reapply gost.setup with the intended font.",
                json!(st),
                json!({"height": 0, "widthFactor": 1, "oblique": 0, "backwards": false, "upsideDown": false, "vertical": false, "annotative": false, "bigFont": ""}),
            );
        }
        if st.font.trim().is_empty() {
            finding(
                findings,
                "error",
                "text.font.empty",
                "textStyle.GOST.font",
                "Supply a nonempty installed font name with gost.setup.",
                json!(st.font),
                json!("installed GOST 2.304 font"),
            );
        }
    }
}

fn run_check(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let policy = standards::drafting(d)?;
    let normative = standards::check_metadata(d)?;
    let mut findings = Vec::new();
    let mut missing_records = Vec::new();
    for (table, missing) in [("textStyle", d.text_style(NAME).is_none()), ("dimStyle", d.dim_style(NAME).is_none())] {
        if missing {
            missing_records.push(json!({"table": table, "name": NAME}));
        }
    }
    for &(name, _, _) in LAYERS {
        if d.layer(name).is_none() {
            missing_records.push(json!({"table": "layer", "name": name}));
        }
    }
    for &(name, _, _) in LINETYPES {
        if d.linetype(name).is_none() {
            missing_records.push(json!({"table": "linetype", "name": name}));
        }
    }
    let profile_present = missing_records.is_empty();
    if !profile_present {
        finding(
            &mut findings,
            "error",
            "profile.missing",
            "drawing",
            "The listed native GOST records are absent. Run gost.setup (after resolving non-mm geometry), then check again.",
            json!({"missingRecords": missing_records}),
            json!({"missingRecords": []}),
        );
    }
    if d.header.get("INSUNITS").and_then(cadcraft_doc::HVal::as_f64) != Some(4.0) {
        finding(
            &mut findings,
            "error",
            "units.not_mm",
            "header.INSUNITS",
            "Establish/convert the actual geometry units to mm before applying the preset; changing INSUNITS alone does not convert geometry.",
            json!(d.header.get("INSUNITS")),
            json!(4),
        );
    }
    if d.header.i64("MEASUREMENT", -1) != 1 {
        finding(
            &mut findings,
            "warning",
            "units.measurement",
            "header.MEASUREMENT",
            "Select metric measurement conventions with gost.setup.",
            json!(d.header.get("MEASUREMENT")),
            json!(1),
        );
    }
    if d.header.f64("LTSCALE", 1.0) != 1.0 {
        finding(
            &mut findings,
            "warning",
            "linetype.globalScale",
            "header.LTSCALE",
            "Global scaling changes preset dash lengths. Review printed lengths before explicitly changing LTSCALE; setup preserves unrelated linework.",
            json!(d.header.get("LTSCALE")),
            json!(1),
        );
    }
    check_text_style(d, &mut findings);
    if let Some(st) = d.dim_style(NAME) {
        check_dimension_style(st, "dimStyle.GOST", &mut findings, 1.0, policy);
    }
    for &(name, _, pattern) in LINETYPES {
        if let Some(lt) = d.linetype(name)
            && (lt.pattern.len() != pattern.len()
                || lt.pattern.iter().zip(pattern).any(|(dash, length)| dash.length != *length || dash.text.is_some() || dash.shape.is_some()))
        {
            finding(
                &mut findings,
                "warning",
                "linetype.pattern",
                name,
                "The named GOST dash pattern differs from the preset; reapply gost.setup or review the intentional alternative.",
                json!(lt.pattern),
                json!(pattern),
            );
        }
    }
    if let Some(lt) = d.linetype("Continuous") {
        if !lt.pattern.is_empty() {
            finding(
                &mut findings,
                "error",
                "linetype.continuous",
                "Continuous",
                "Continuous has a dash pattern: repair this shared definition before using GOST solid outlines/dimension lines. Setup deliberately does not overwrite non-GOST definitions.",
                json!(lt.pattern),
                json!([]),
            );
        }
    } else {
        finding(
            &mut findings,
            "error",
            "linetype.continuous.missing",
            "Continuous",
            "Load the Continuous linetype or reapply gost.setup.",
            Value::Null,
            json!([]),
        );
    }
    for &(name, linetype, thick) in LAYERS {
        let width = if thick { policy.outline_width_mm100 } else { u16::from(policy.thin_width_mm100) };
        if let Some(layer) = d.layer(name)
            && (layer.color != Color::Index(7)
                || layer.linetype != linetype
                || layer.lineweight != Lineweight::Mm100(width)
                || !layer.plot
                || !layer.on
                || layer.frozen
                || layer.transparency != 0
                || !layer.plot_style.eq_ignore_ascii_case("Normal"))
        {
            finding(
                &mut findings,
                "warning",
                "layer.preset",
                name,
                "Review printable black GOST layer properties; reapply gost.setup to restore the preset. Plot-style tables and viewport overrides require manual review.",
                json!(layer),
                json!({"colorAci": 7, "linetype": linetype, "lineweightMm": f64::from(width) / 100.0, "plot": true, "on": true, "frozen": false, "transparency": 0, "plotStyle": "Normal"}),
            );
        }
    }
    if d.header.str("DIMSTYLE", "Standard").eq_ignore_ascii_case(NAME) {
        let overrides = d.dim_overrides();
        if !overrides.is_empty() {
            finding(
                &mut findings,
                "warning",
                "dimension.headerOverrides",
                "header",
                "Current DIM variable overrides alter newly created dimensions; making GOST current or rerunning gost.setup clears them.",
                json!(overrides),
                json!({}),
            );
        }
    }
    let (sheets, mut truncated) = check_sheets(d, &mut findings);
    findings.extend(crate::extensions::drawing::link_findings(d));
    let mut dimensions = 0usize;
    let entities = d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())).chain(d.blocks.values().flat_map(|b| b.entities.iter()));
    // Finding output is bounded independently of the number of imported entities.
    for entity in entities {
        if findings.len() >= MAX_FINDINGS {
            truncated = true;
            break;
        }
        let EntityKind::Dimension(dim) = &entity.kind else { continue };
        dimensions += 1;
        let location = format!("dimension:{}", entity.handle.hex());
        if !dim.style.eq_ignore_ascii_case(NAME) {
            finding(
                &mut findings,
                "warning",
                "dimension.otherStyle",
                &location,
                "This dimension does not use the GOST style. Review it explicitly; setup does not migrate existing dimensions.",
                json!(dim.style),
                json!(NAME),
            );
        }
        let projection_factor =
            crate::extensions::drawing::dimension_factor(d, entity).or_else(|| crate::extensions::models::dimension_factor(d, entity));
        if let Some(base) = d.dim_style(&dim.style) {
            let effective;
            let style = if dim.overrides.is_empty() {
                base
            } else {
                effective = base.with_overrides(&dim.overrides);
                &effective
            };
            check_dimension_style(style, &location, &mut findings, projection_factor.unwrap_or(1.0), policy);
        } else {
            finding(
                &mut findings,
                "error",
                "dimension.missingStyle",
                &location,
                "The dimension references a missing style; assign a loaded style.",
                json!(dim.style),
                json!(NAME),
            );
        }
        if !dim.text.is_empty() && !dim.text.contains("<>") {
            finding(
                &mut findings,
                "warning",
                "dimension.measurementReplaced",
                &location,
                "Dimension text replaces or suppresses the measured value. Remove the override or retain a <> measurement placeholder unless the replacement has been deliberately verified.",
                json!(dim.text),
                json!("measured value or <> placeholder"),
            );
        }
        if dim.assoc.is_empty() && projection_factor.is_none() {
            finding(
                &mut findings,
                "warning",
                "dimension.disassociated",
                &location,
                "No native geometry associations are recorded. Verify the definition points against the part; this is a maintenance risk, not by itself a GOST violation.",
                json!(0),
                json!("verified geometry associations or manual measurement"),
            );
        } else if dim.assoc.iter().any(|a| d.entity(a.handle).is_none() && !d.blocks.values().any(|block| block.entities.contains(a.handle))) {
            finding(
                &mut findings,
                "warning",
                "dimension.associationTarget",
                &location,
                "An association target cannot be resolved in the drawing; inspect and reassociate the dimension.",
                json!(dim.assoc),
                json!("resolvable association targets"),
            );
        }
    }
    let (geometry, geometry_incomplete) = crate::extensions::norms::inspect(d);
    let room = MAX_FINDINGS.saturating_sub(findings.len());
    truncated |= geometry_incomplete || geometry.len() > room;
    findings.extend(geometry.into_iter().take(room));
    let errors = findings.iter().filter(|f| f.get("severity").and_then(Value::as_str) == Some("error")).count();
    let warnings = findings.iter().filter(|f| f.get("severity").and_then(Value::as_str) == Some("warning")).count();
    let font = d.text_style(NAME).map(|st| st.font.as_str()).unwrap_or(DEFAULT_FONT);
    Ok(json!({
        "profilePresent": profile_present,
        "normativeProfile": normative,
        "status": "manual_review_required",
        "automaticChecksPassed": profile_present && errors == 0 && warnings == 0 && !truncated,
        "errors": errors, "warnings": warnings, "findings": findings,
        "dimensionsInspected": dimensions, "gostSheetsInspected": sheets, "findingsTruncated": truncated,
        "scope": "Native style tables, units, dimension settings, native GOST sheets and bounded planar overlap/intersection/spacing checks; not semantic compliance or certification.",
        "manualChecks": [
            {"code": "font.conformance", "status": "unverified", "message": font_warning(font)},
            {"code": "dimensions.sufficiency", "status": "manual", "message": "Verify sufficient/nonredundant dimensions, tolerances, fits, datum choices, actual measured geometry and dimension placement (including first-line clearance and actual >=7 mm parallel-line spacing). Review any justified deviations from preset sizes."},
            {"code": "views.3d", "status": "unverified", "message": "Verify projections, sections, cuts, view labels and consistency with the current 3D model. This check neither generates nor proves associative 3D drawing views."},
            {"code": "electronic.signoff", "status": "unverified", "message": "Verify approvals, electronic signatures and document-control workflow; drawn names/date fields are not electronic signoff."},
            {"code": "sheet.print", "status": "manual", "message": "Verify GOST R 2.104-2023 title-block fields and sheet/form selection, GOST R 2.109-2023 document requirements, paper scale, printed widths/dash lengths, font substitution, plot-style/viewport overrides and all non-dimension annotation geometry."}
        ],
        "message": "A clean automatic result only matches the supported preset checks; it is NOT a finding of full GOST compliance."
    }))
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;

/// Recognize our native sheet blocks, not arbitrary user layouts. A complete
/// title block is still a document-level judgement; these checks only expose
/// missing/empty primary fields and damaged native geometry/plot configuration.
fn check_sheets(d: &Drawing, findings: &mut Vec<Value>) -> (usize, bool) {
    const PREFIX: &str = "GOST-SHEET-";
    let mut sheets = 0usize;
    for layout in &d.layouts {
        if findings.len() >= MAX_FINDINGS {
            return (sheets, true);
        }
        let expected_name = format!("{PREFIX}{}", layout.name);
        let native_insert = |kind: &EntityKind| matches!(kind, EntityKind::Insert(insert) if insert.block.starts_with(PREFIX));
        if d.block(&expected_name).is_none() && !layout.entities.iter().any(|entity| native_insert(&entity.kind)) {
            continue;
        }
        sheets += 1;
        let page = &layout.page;
        let location = format!("layout:{}", layout.name);
        if !page.paper_mm || page.scale_to_fit || page.scale != 1.0 || !page.plot_area.eq_ignore_ascii_case("layout") || !page.lineweights {
            finding(
                findings,
                "error",
                "sheet.plot",
                &location,
                "Native paper-mm sheets require explicit mm paper units, layout plotting at 1:1, no fit-to-paper, and enabled lineweights; restore this page setup before printing.",
                json!({"paperMm": page.paper_mm, "scaleToFit": page.scale_to_fit, "scale": page.scale, "plotArea": page.plot_area, "lineweights": page.lineweights}),
                json!({"paperMm": true, "scaleToFit": false, "scale": 1, "plotArea": "layout", "lineweights": true}),
            );
        }
        if page.center || page.margins_mm != [0.0; 4] {
            finding(
                findings,
                "warning",
                "sheet.plotOrigin",
                &location,
                "The native frame uses the paper origin with zero digital margins and no plot centering. Review printer clipping/origin and ensure the 20/5 mm frame margins are not shifted.",
                json!({"center": page.center, "marginsMm": page.margins_mm}),
                json!({"center": false, "marginsMm": [0, 0, 0, 0]}),
            );
        }
        let (w, h) = if page.landscape { (page.height_mm, page.width_mm) } else { (page.width_mm, page.height_mm) };
        let mut inserts = 0usize;
        for entity in layout.entities.iter() {
            if findings.len() >= MAX_FINDINGS {
                return (sheets, true);
            }
            let EntityKind::Insert(insert) = &entity.kind else { continue };
            if !insert.block.starts_with(PREFIX) {
                continue;
            }
            inserts += 1;
            if insert.insert != cadcraft_geom::Vec3::ZERO
                || insert.scale != cadcraft_geom::Vec3::new(1.0, 1.0, 1.0)
                || insert.rotation != 0.0
                || insert.rows != 1
                || insert.cols != 1
                || insert.clip.is_some()
            {
                finding(
                    findings,
                    "error",
                    "sheet.insertTransform",
                    &location,
                    "The native sheet block must remain unscaled/unrotated at paper origin, without arrays or clipping; restore its placement or verify the entire transformed sheet.",
                    json!({"insert": insert.insert, "scale": insert.scale, "rotation": insert.rotation, "rows": insert.rows, "cols": insert.cols, "clipped": insert.clip.is_some()}),
                    json!({"insert": [0, 0, 0], "scale": [1, 1, 1], "rotation": 0, "rows": 1, "cols": 1, "clipped": false}),
                );
            }
            let Some(block) = d.block(&insert.block) else {
                finding(
                    findings,
                    "error",
                    "sheet.blockMissing",
                    &location,
                    "The sheet insert references a missing block; restore the native block definition.",
                    json!(insert.block),
                    json!("existing GOST sheet block"),
                );
                continue;
            };
            if block.units != 4 {
                finding(
                    findings,
                    "error",
                    "sheet.blockUnits",
                    &location,
                    "Native sheet blocks are in millimetres, independent of model units. Verify geometry before restoring block units to mm.",
                    json!(block.units),
                    json!(4),
                );
            }
            let corners = [(20.0, 5.0), (w - 5.0, 5.0), (w - 5.0, h - 5.0), (20.0, h - 5.0)];
            let near = |point: &cadcraft_geom::Vec3, expected: (f64, f64)| {
                (point.x - expected.0).abs() < 1e-6 && (point.y - expected.1).abs() < 1e-6 && point.z.abs() < 1e-6
            };
            let frame_intact = corners.iter().zip(corners.iter().cycle().skip(1)).all(|(&a, &b)| {
                block.entities.iter().any(|edge| {
                    edge.common.layer.eq_ignore_ascii_case("GOST-FRAME")
                        && matches!(&edge.kind, EntityKind::Line(line) if (near(&line.a, a) && near(&line.b, b)) || (near(&line.a, b) && near(&line.b, a)))
                })
            });
            if !frame_intact {
                finding(
                    findings,
                    "warning",
                    "sheet.frame",
                    &location,
                    "Native frame edges do not match the current paper's 20 mm left and 5 mm other margins. Review changed paper size, block geometry or an intentional non-native frame representation.",
                    json!({"block": insert.block, "paperWidthMm": w, "paperHeightMm": h}),
                    json!({"leftMm": 20, "otherMm": 5, "nativeEdges": 4}),
                );
            }
            let first_form = block.description.starts_with("GOST R 2.104-2023 form 1,");
            for tag in ["DESIGNATION", "SHEET", "DESIGNATION_TOP", "FORMAT"]
                .into_iter()
                .chain(["TITLE", "SCALE", "SHEETS", "ORG"].into_iter().filter(|_| first_form))
            {
                let attribute = insert.attribs.iter().find(|attribute| attribute.tag.eq_ignore_ascii_case(tag));
                let definition = block
                    .entities
                    .iter()
                    .any(|entity| matches!(&entity.kind, EntityKind::AttDef(attribute) if attribute.tag.eq_ignore_ascii_case(tag)));
                if attribute.is_none() || !definition {
                    finding(
                        findings,
                        "error",
                        "sheet.attributeMissing",
                        &location,
                        "A primary field is missing from the native block/insert. Restore its attribute definition and instance; visible loose text is not an editable native field.",
                        json!({"tag": tag, "definitionPresent": definition, "instancePresent": attribute.is_some()}),
                        json!({"definitionPresent": true, "instancePresent": true}),
                    );
                } else if let Some(attribute) = attribute
                    && attribute.text.value.trim().is_empty()
                {
                    finding(
                        findings,
                        "warning",
                        "sheet.attributeEmpty",
                        &location,
                        "Complete or explicitly justify this unfilled primary title-block field for the document's type and approval workflow.",
                        json!({"tag": tag, "value": attribute.text.value}),
                        json!("completed or reviewed field"),
                    );
                }
            }
        }
        if inserts != 1 {
            finding(
                findings,
                "warning",
                "sheet.insertCount",
                &location,
                "A native GOST sheet should have one frame/title-block insert. Restore a deleted insert or remove unintended duplicates after reviewing the drawing.",
                json!(inserts),
                json!(1),
            );
        }
    }
    (sheets, false)
}
