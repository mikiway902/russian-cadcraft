//! Versioned profile selection and local normative evidence, using engine JSON commands only.
use crate::CadApp;
use crate::managers::{Action, set_temp, temp};
use egui::vec2;
use serde_json::{Value, json};
use std::sync::Arc;

const STATE_KEY: &str = "gost_standards_state";

#[derive(Clone)]
struct State {
    uid: u64,
    revision: u64,
    pass: u64,
    profile: Arc<Value>,
    warnings: Arc<[String]>,
    profiles: Arc<[Value]>,
    catalog: Arc<Value>,
    selected: String,
    pending: Option<String>,
    error: Option<String>,
}

fn malformed(command: &str, field: &str) -> String {
    format!("{}: {command} · {field}", crate::tl!("Malformed engine response"))
}

fn text<'a>(value: &'a Value, key: &str, command: &str) -> Result<&'a str, String> {
    value[key].as_str().filter(|s| !s.is_empty()).ok_or_else(|| malformed(command, key))
}

fn array<'a>(value: &'a Value, key: &str, command: &str) -> Result<&'a Vec<Value>, String> {
    value[key].as_array().ok_or_else(|| malformed(command, key))
}

fn validate_summary(value: &Value, allow_unselected: bool) -> Result<(), String> {
    for key in ["id", "name", "jurisdiction"] {
        text(value, key, "gost.profile")?;
    }
    let version = value["version"].as_u64().ok_or_else(|| malformed("gost.profile", "version"))?;
    if !(allow_unselected && version == 0 && value["as_of"].is_null()) {
        text(value, "as_of", "gost.profile")?;
    }
    if !allow_unselected && version == 0 {
        return Err(malformed("gost.profile", "version"));
    }
    Ok(())
}

fn load(app: &mut CadApp, st: &mut State) -> Result<(), String> {
    // Publish a snapshot only after every query has succeeded and its required fields are usable.
    let mut current = app.run("gost.profile", json!({"action":"get"}))?;
    let profile = &current["profile"];
    validate_summary(profile, true)?;
    if profile["version"].as_u64() != Some(0) && array(profile, "standards", "gost.profile")?.is_empty() {
        return Err(malformed("gost.profile", "standards"));
    }
    for standard in array(profile, "standards", "gost.profile")? {
        for key in ["id", "designation", "edition"] {
            text(standard, key, "gost.profile")?;
        }
    }
    let warnings = array(&current, "warnings", "gost.profile")?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or_else(|| malformed("gost.profile", "warnings")))
        .collect::<Result<Vec<_>, _>>()?;
    let mut listed = app.run("gost.profile", json!({"action":"list"}))?;
    let profiles = array(&listed, "profiles", "gost.profile")?;
    if profiles.is_empty() {
        return Err(malformed("gost.profile", "profiles"));
    }
    for candidate in profiles {
        validate_summary(candidate, false)?;
    }
    let catalog = app.run("gost.standards", json!({}))?;
    text(&catalog, "catalog_version", "gost.standards")?;
    let standards = array(&catalog, "standards", "gost.standards")?;
    if standards.is_empty() {
        return Err(malformed("gost.standards", "standards"));
    }
    for standard in standards {
        for key in ["id", "designation", "edition", "title_ru"] {
            text(standard, key, "gost.standards")?;
        }
        for key in ["sources", "rules", "manual_review", "amendments"] {
            array(standard, key, "gost.standards")?;
        }
        for key in ["rf", "as_of", "verification_status"] {
            text(&standard["status"], key, "gost.standards")?;
        }
        for source in array(standard, "sources", "gost.standards")? {
            for key in ["id", "url", "authority", "accessed_at", "evidence"] {
                text(source, key, "gost.standards")?;
            }
        }
        for rule in array(standard, "rules", "gost.standards")? {
            for key in ["id", "applies_to", "clause", "verification_status", "implementation_status"] {
                text(rule, key, "gost.standards")?;
            }
        }
        for review in array(standard, "manual_review", "gost.standards")? {
            for key in ["id", "requirement"] {
                text(review, key, "gost.standards")?;
            }
        }
        if !standard["rf_successor"].is_null() {
            for key in ["designation", "effective_from", "url"] {
                text(&standard["rf_successor"], key, "gost.standards")?;
            }
        }
    }
    // Family IDs stay stable even when the selected RF edition differs from the legacy card.
    for selected in profile["standards"].as_array().into_iter().flatten() {
        if !standards.iter().any(|s| s["id"] == selected["id"]) {
            return Err(malformed("gost.standards", "profile.standards.id"));
        }
    }
    st.selected = profiles.iter().find(|p| p["id"] == profile["id"]).unwrap_or(&profiles[0])["id"].as_str().unwrap_or_default().to_owned();
    // egui temp state is cloned each frame: share the validated immutable catalog
    // instead of deep-copying all standards, sources and rule assessments.
    st.profile = Arc::new(current["profile"].take());
    st.warnings = warnings.into();
    st.profiles = match listed["profiles"].take() {
        Value::Array(profiles) => profiles.into(),
        _ => return Err(malformed("gost.profile", "profiles")),
    };
    st.catalog = Arc::new(catalog);
    Ok(())
}

fn refresh(app: &mut CadApp, st: &mut State) {
    let pending = st.pending.take();
    st.error = load(app, st).err();
    if st.error.is_none() && pending.as_ref().is_some_and(|id| st.profile["id"].as_str() != Some(id.as_str())) {
        st.error = Some(crate::tl!("Profile change was not confirmed. See the command line.").to_string());
    }
}

fn detail(ui: &mut egui::Ui, label: &str, value: &Value) {
    if !value.is_null() {
        let value = value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string());
        ui.label(format!("{label}: {value}"));
    }
}

fn summary(ui: &mut egui::Ui, profile: &Value) {
    detail(ui, crate::tl!("Profile identifier"), &profile["id"]);
    detail(ui, crate::tl!("Profile version"), &profile["version"]);
    detail(ui, crate::tl!("Jurisdiction"), &profile["jurisdiction"]);
    if profile["as_of"].is_null() {
        ui.label(crate::tl!("No versioned profile is selected. Choose one explicitly before normative review."));
    } else {
        detail(ui, crate::tl!("Applicability date"), &profile["as_of"]);
    }
}

fn verification(ui: &mut egui::Ui, value: &Value) {
    match value.as_str() {
        Some("primary_metadata_verified") => {
            ui.label(crate::tl!("Official metadata verified; authoritative full text is not verified."));
        }
        Some("secondary_text_verified_primary_text_unavailable") => {
            ui.label(crate::tl!("Secondary text only; comparison with an authorized authoritative edition is required."));
        }
        Some(other) => {
            ui.label(format!("{}: {other}", crate::tl!("Verification status")));
        }
        None => {
            ui.colored_label(ui.visuals().error_fg_color, crate::tl!("Verification status unavailable"));
        }
    }
}

fn link(ui: &mut egui::Ui, label: &str, url: &Value) {
    if let Some(url) = url.as_str().filter(|url| url.starts_with("https://") || url.starts_with("http://")) {
        ui.hyperlink_to(label, url);
    } else {
        ui.colored_label(ui.visuals().error_fg_color, crate::tl!("Source link unavailable"));
    }
}

fn standard(ui: &mut egui::Ui, record: &Value, selected: Option<&Value>, as_of: &str) {
    if let Some(selected) = selected {
        detail(ui, crate::tl!("Selected document edition"), &selected["designation"]);
        detail(ui, crate::tl!("Edition year"), &selected["edition"]);
    } else {
        ui.label(crate::tl!("Not selected in this document profile"));
    }
    detail(ui, crate::tl!("Catalog card edition"), &record["designation"]);
    detail(ui, crate::tl!("Edition year"), &record["edition"]);
    detail(ui, crate::tl!("RF card status"), &record["status"]["rf"]);
    detail(ui, crate::tl!("Card status date"), &record["status"]["as_of"]);
    detail(ui, crate::tl!("Applicable from"), &record["status"]["rf_applicable_from"]);
    detail(ui, crate::tl!("Applicable until (exclusive)"), &record["status"]["rf_applicable_until"]);
    verification(ui, &record["status"]["verification_status"]);
    let successor = &record["rf_successor"];
    if !successor.is_null() {
        ui.group(|ui| {
            detail(ui, crate::tl!("RF successor edition"), &successor["designation"]);
            detail(ui, crate::tl!("Effective from"), &successor["effective_from"]);
            if successor["effective_from"].as_str().is_some_and(|date| date > as_of) {
                ui.label(crate::tl!("Future edition at the profile or catalog reference date; not the current default."));
                if successor["early_application"].as_bool() == Some(true) {
                    ui.label(crate::tl!(
                        "Early application is permitted only with explicit validated profile selection; legacy rules do not transfer automatically."
                    ));
                }
            }
            detail(ui, crate::tl!("Successor publication verification"), &successor["publication_verification"]);
            link(ui, crate::tl!("Official successor card"), &successor["url"]);
        });
    }
    ui.collapsing(crate::tl!("Cited normative sources"), |ui| {
        for source in record["sources"].as_array().into_iter().flatten() {
            ui.group(|ui| {
                let label = source["id"].as_str().unwrap_or(crate::tl!("Source"));
                link(ui, label, &source["url"]);
                detail(ui, crate::tl!("Source authority"), &source["authority"]);
                detail(ui, crate::tl!("Accessed on"), &source["accessed_at"]);
                detail(ui, crate::tl!("Source evidence"), &source["evidence"]);
            });
        }
    });
    ui.collapsing(crate::tl!("Amendments and corrections"), |ui| {
        for amendment in record["amendments"].as_array().into_iter().flatten() {
            ui.group(|ui| {
                link(ui, amendment["publication"].as_str().unwrap_or(crate::tl!("Source")), &amendment["url"]);
                detail(ui, crate::tl!("Effective from"), &amendment["effective_from"]);
                verification(ui, &amendment["verification_status"]);
                detail(ui, crate::tl!("Authoritative amendment text comparison"), &amendment["text_verification"]);
            });
        }
    });
    ui.collapsing(crate::tl!("Scoped rules and implementation assessments"), |ui| {
        ui.label(crate::tl!("Catalog presence is not implementation or a conformity certificate. Numerical rules remain secondary-source evidence."));
        for (index, rule) in record["rules"].as_array().into_iter().flatten().enumerate() {
            ui.push_id(index, |ui| {
                ui.collapsing(rule["id"].as_str().unwrap_or(crate::tl!("Rule")), |ui| {
                    detail(ui, crate::tl!("Applies to edition"), &rule["applies_to"]);
                    detail(ui, crate::tl!("Clause"), &rule["clause"]);
                    verification(ui, &rule["verification_status"]);
                    detail(ui, crate::tl!("Implementation assessment"), &rule["implementation_status"]);
                    detail(ui, crate::tl!("Rule source identifiers"), &rule["source_ids"]);
                    detail(ui, crate::tl!("Rule parameters"), &rule["parameters"]);
                });
            });
        }
    });
    ui.collapsing(crate::tl!("Required manual review"), |ui| {
        for review in record["manual_review"].as_array().into_iter().flatten() {
            detail(ui, review["id"].as_str().unwrap_or(crate::tl!("Manual review")), &review["requirement"]);
        }
        detail(ui, crate::tl!("Source discrepancies"), &record["source_discrepancies"]);
    });
}

pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, _: &str, open: &mut bool) -> Action {
    let title = crate::tl!("GOST Standards Profile");
    let Ok(doc) = app.session.state() else {
        ctx.data_mut(|d| d.remove::<State>(egui::Id::unique(STATE_KEY)));
        egui::Window::new(title).open(open).show(ctx, |ui| {
            ui.colored_label(ui.visuals().error_fg_color, crate::tl!("Open a drawing first."));
        });
        return None;
    };
    let (uid, revision) = (doc.uid, doc.revision);
    let pass = ctx.cumulative_pass_nr();
    let mut st = temp::<State>(ctx, STATE_KEY).filter(|st| st.uid == uid && st.pass.saturating_add(1) >= pass).unwrap_or_else(|| {
        let mut st = State {
            uid,
            revision,
            pass,
            profile: Arc::new(Value::Null),
            warnings: Arc::from([]),
            profiles: Arc::from([]),
            catalog: Arc::new(Value::Null),
            selected: String::new(),
            pending: None,
            error: None,
        };
        refresh(app, &mut st);
        st
    });
    if st.revision != revision || st.pending.is_some() {
        refresh(app, &mut st);
    }
    st.pass = pass;
    st.revision = revision;
    let mut action = None;
    let mut close = false;
    let mut reload = false;
    egui::Window::new(title).id(egui::Id::unique("gostprofile")).open(open).default_size(vec2(760.0, 650.0)).resizable(true).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.75).show(ui, |ui| {
            ui.label(crate::tl!("Profile selection does not certify compliance or rescale existing geometry."));
            ui.label(crate::tl!(
                "Metadata verification is not verification of authoritative clauses. Review all applicable editions and amendments."
            ));
            if let Some(error) = &st.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            if ui.button(crate::tl!("Reload profile and catalog")).clicked() {
                reload = true;
            }
            if st.error.is_none() {
                ui.heading(crate::tl!("Current document profile"));
                ui.label(st.profile["name"].as_str().unwrap_or_default());
                summary(ui, &st.profile);
                for warning in st.warnings.iter() {
                    ui.label(format!("{}: {warning}", crate::tl!("Manual review")));
                }
                ui.separator();
                let label = ui.label(crate::tl!("Available validated profiles"));
                let selected_name = st
                    .profiles
                    .iter()
                    .find(|p| p["id"].as_str() == Some(st.selected.as_str()))
                    .and_then(|p| p["name"].as_str())
                    .unwrap_or(st.selected.as_str());
                craft_ui::ComboBox::from_id_salt("gost_profile_choice")
                    .selected_text(selected_name)
                    .show_ui(ui, |ui| {
                        for profile in st.profiles.iter() {
                            ui.selectable_value(
                                &mut st.selected,
                                profile["id"].as_str().unwrap_or_default().to_owned(),
                                profile["name"].as_str().unwrap_or_default(),
                            );
                        }
                    })
                    .response
                    .labelled_by(label.id);
                if let Some(candidate) = st.profiles.iter().find(|p| p["id"].as_str() == Some(st.selected.as_str())) {
                    summary(ui, candidate);
                    if ui
                        .add_enabled(
                            st.profile["id"].as_str() != Some(st.selected.as_str()),
                            egui::Button::new(crate::tl!("Apply selected standards profile")),
                        )
                        .clicked()
                    {
                        st.pending = Some(st.selected.clone());
                        action = Some(("gost.profile", json!({"action":"set","id":st.selected})));
                    }
                }
                ui.separator();
                detail(ui, crate::tl!("Local catalog version"), &st.catalog["catalog_version"]);
                let as_of = st.profile["as_of"].as_str();
                for record in st.catalog["standards"].as_array().into_iter().flatten() {
                    ui.push_id(record["id"].as_str().unwrap_or_default(), |ui| {
                        let selected = st.profile["standards"].as_array().and_then(|a| a.iter().find(|s| s["id"] == record["id"]));
                        let designation =
                            selected.and_then(|s| s["designation"].as_str()).unwrap_or_else(|| record["designation"].as_str().unwrap_or_default());
                        ui.collapsing(format!("{} — {}", designation, record["title_ru"].as_str().unwrap_or_default()), |ui| {
                            standard(ui, record, selected, as_of.unwrap_or_else(|| record["status"]["as_of"].as_str().unwrap_or_default()));
                        });
                    });
                }
            }
            if ui.button(crate::tl!("Close")).clicked() {
                close = true;
            }
        });
    });
    if reload {
        refresh(app, &mut st);
    }
    if close {
        *open = false;
    }
    if *open {
        set_temp(ctx, STATE_KEY, st);
    } else {
        ctx.data_mut(|d| d.remove::<State>(egui::Id::unique(STATE_KEY)));
    }
    action
}
