//! Validate the approved policy snapshot before compiling an infallible default.
//! This checks the consumed data contract, not full normative conformance.
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    path::PathBuf,
};

use chrono::{Datelike, NaiveDate};
use serde::Deserialize;
use serde_json::Value;

#[path = "src/extensions/gost/standards/profile.rs"]
mod profile;
use profile::{DEFAULT_ID, Profile};

type BuildResult<T> = Result<T, Box<dyn Error>>;

#[derive(Deserialize)]
struct Catalog {
    schema_version: u32,
    catalog_version: String,
    researched_at: String,
    standards: Vec<Evidence>,
}

#[derive(Deserialize)]
struct Evidence {
    id: String,
    designation: String,
    edition: String,
    effective_from: String,
    default_rf_designation: String,
    status: Status,
    rf_successor: Option<Successor>,
    rules: Vec<Rule>,
}

#[derive(Deserialize)]
struct Status {
    as_of: String,
    rf_applicable_from: String,
    rf_applicable_until: Option<String>,
}

#[derive(Deserialize)]
struct Successor {
    designation: String,
    effective_from: String,
}

#[derive(Deserialize)]
struct Rule {
    id: String,
    applies_to: String,
    parameters: Value,
}

fn invalid(message: impl Into<String>) -> Box<dyn Error> {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()).into()
}

fn main() -> BuildResult<()> {
    if std::env::var_os("CARGO_FEATURE_GOST").is_none() {
        return Ok(());
    }
    generate().map_err(|e| invalid(format!("bundled GOST policy validation failed: {e}")))
}

fn generate() -> BuildResult<()> {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let profile_path = root.join("../../data/gost/profiles/rf-2026-v1.json");
    let catalog_path = root.join("../../data/gost/standards-catalog.json");
    println!("cargo:rerun-if-changed={}", profile_path.display());
    println!("cargo:rerun-if-changed={}", catalog_path.display());
    println!("cargo:rerun-if-changed=src/extensions/gost/standards/profile.rs");
    println!("cargo:rerun-if-changed=build.rs");
    let profile: Profile = serde_json::from_slice(&fs::read(&profile_path)?).map_err(|e| invalid(format!("{}: {e}", profile_path.display())))?;
    // Parse the complete value as well as the consumed typed subset: fields not
    // used by validation must still be representable by the runtime catalog API.
    let catalog_value: Value = serde_json::from_slice(&fs::read(&catalog_path)?).map_err(|e| invalid(format!("{}: {e}", catalog_path.display())))?;
    let catalog: Catalog = serde_json::from_value(catalog_value).map_err(|e| invalid(format!("{}: {e}", catalog_path.display())))?;
    validate(&profile, &catalog)?;
    let output = PathBuf::from(std::env::var("OUT_DIR")?).join("gost_profile.rs");
    let selection = profile::Selection {
        schema_version: 1,
        id: profile.id.clone(),
        version: profile.version,
        catalog_version: profile.catalog_version.clone(),
        alphabet: profile.drafting.designation_alphabet.clone(),
    };
    let selection_json = serde_json::to_string(&selection)?;
    if selection_json.len() > 4096 {
        return Err(invalid("default profile identity exceeds its XRECORD reader limit"));
    }
    fs::write(output, format!("{}\nconst DEFAULT_SELECTION_JSON: &str = {selection_json:?};\n", constructor(&profile)))?;
    Ok(())
}

// Lexicographical comparison is valid only after checking complete ISO calendar dates.
fn date(value: &str) -> BuildResult<()> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || !bytes.iter().enumerate().all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return Err(invalid(format!("invalid ISO calendar date {value:?}")));
    }
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d")?;
    if parsed.year() <= 0 {
        return Err(invalid(format!("invalid ISO calendar date {value:?}")));
    }
    Ok(())
}

fn validate(p: &Profile, catalog: &Catalog) -> BuildResult<()> {
    if p.schema_version != 1 || p.version != 1 || p.id != DEFAULT_ID || p.jurisdiction != "RF" {
        return Err(invalid("profile must have schema/version 1, ID rf-2026-v1 and jurisdiction RF; another edition needs explicit implementation"));
    }
    if catalog.schema_version != 1 || catalog.catalog_version.is_empty() || p.catalog_version != catalog.catalog_version {
        return Err(invalid("profile catalog_version must match a schema-1 bundled catalog"));
    }
    date(&p.as_of)?;
    date(&catalog.researched_at)?;
    if p.as_of != catalog.researched_at {
        return Err(invalid("profile as_of must match the catalog researched_at snapshot; future editions require an explicit profile"));
    }
    let mut entries = BTreeMap::new();
    let mut rules = BTreeMap::new();
    for entry in &catalog.standards {
        if entry.id.is_empty() || entries.insert(entry.id.as_str(), entry).is_some() {
            return Err(invalid(format!("empty or duplicate catalog standard ID {:?}", entry.id)));
        }
        date(&entry.effective_from)?;
        date(&entry.status.as_of)?;
        date(&entry.status.rf_applicable_from)?;
        if entry.status.as_of != p.as_of {
            return Err(invalid(format!("{} status as_of differs from the profile snapshot", entry.id)));
        }
        if let Some(until) = &entry.status.rf_applicable_until {
            date(until)?;
            if until <= &entry.status.rf_applicable_from {
                return Err(invalid(format!("{} has an empty RF applicability interval", entry.id)));
            }
        }
        if let Some(successor) = &entry.rf_successor {
            date(&successor.effective_from)?;
        }
        for rule in &entry.rules {
            if rule.id.is_empty() || rule.applies_to.is_empty() || rules.insert(rule.id.as_str(), (entry.id.as_str(), rule)).is_some() {
                return Err(invalid(format!("empty or duplicate rule ID, or missing applies_to: {:?}", rule.id)));
            }
        }
    }
    if entries.is_empty() {
        return Err(invalid("catalog standards must not be empty"));
    }
    let mut selected = BTreeMap::new();
    for standard in &p.standards {
        if selected.insert(standard.id.as_str(), standard).is_some() {
            return Err(invalid(format!("duplicate profile standard {}", standard.id)));
        }
        let entry = entries.get(standard.id.as_str()).ok_or_else(|| invalid(format!("profile standard {} is missing from catalog", standard.id)))?;
        if standard.designation != entry.default_rf_designation {
            return Err(invalid(format!("{} must select the catalog's effective RF designation {}", standard.id, entry.default_rf_designation)));
        }
        let edition = if standard.designation == entry.designation {
            if entry.effective_from > p.as_of
                || entry.status.rf_applicable_from > p.as_of
                || entry.status.rf_applicable_until.as_ref().is_some_and(|until| p.as_of >= *until)
            {
                return Err(invalid(format!("{} is outside its RF applicability interval at {}", standard.designation, p.as_of)));
            }
            entry.edition.as_str()
        } else {
            let successor = entry
                .rf_successor
                .as_ref()
                .filter(|s| s.designation == standard.designation)
                .ok_or_else(|| invalid(format!("{} is not the recorded RF successor", standard.designation)))?;
            if successor.effective_from > p.as_of {
                return Err(invalid(format!("{} is not effective at {}; future editions need another profile", standard.designation, p.as_of)));
            }
            successor.designation.rsplit('-').next().ok_or_else(|| invalid(format!("{} has no successor edition", standard.id)))?
        };
        if standard.edition != edition || edition.is_empty() {
            return Err(invalid(format!("{} edition must be {edition:?}", standard.id)));
        }
    }
    if selected.len() != entries.len() {
        return Err(invalid("profile must select every catalog standard exactly once"));
    }
    let mut consumed = BTreeSet::new();
    for id in &p.consumed_rules {
        if !consumed.insert(id.as_str()) {
            return Err(invalid(format!("duplicate consumed rule {id}")));
        }
        let (family, rule) = rules.get(id.as_str()).ok_or_else(|| invalid(format!("consumed rule {id} is missing from catalog")))?;
        if !selected.get(family).is_some_and(|s| s.designation == rule.applies_to) {
            return Err(invalid(format!("consumed rule {id} does not apply to its family's selected edition")));
        }
    }
    let alphabet_id = "gost-2-316.designation-alphabets";
    let (_, alphabet) = rules.get(alphabet_id).ok_or_else(|| invalid("missing designation alphabet rule"))?;
    if !consumed.contains(alphabet_id) {
        return Err(invalid("designation alphabet rule must be declared as consumed"));
    }
    for (key, latin) in [("latin_uppercase_excluded", true), ("russian_uppercase_excluded", false)] {
        let exclusions = alphabet.parameters[key].as_array().ok_or_else(|| invalid(format!("alphabet {key} must be an array")))?;
        let mut letters = BTreeSet::new();
        for value in exclusions {
            let letter = value
                .as_str()
                .filter(|s| s.chars().count() == 1)
                .ok_or_else(|| invalid(format!("alphabet {key} exclusions must be single letters")))?;
            if !letters.insert(letter) || !letter.chars().all(|c| if latin { c.is_ascii_uppercase() } else { ('А'..='Я').contains(&c) || c == 'Ё' })
            {
                return Err(invalid(format!("alphabet {key} contains a duplicate or invalid capital {letter:?}")));
            }
        }
    }
    if !["cyrillic", "latin"].contains(&p.drafting.designation_alphabet.as_str()) {
        return Err(invalid("drafting designation_alphabet must be cyrillic or latin"));
    }
    Ok(())
}

fn string(value: &str) -> String {
    format!("{value:?}.to_owned()")
}

fn strings(values: &[String]) -> String {
    format!("vec![{}]", values.iter().map(|v| string(v)).collect::<Vec<_>>().join(","))
}

fn constructor(p: &Profile) -> String {
    let standards = p
        .standards
        .iter()
        .map(|s| format!("Standard {{ id: {}, designation: {}, edition: {} }}", string(&s.id), string(&s.designation), string(&s.edition)))
        .collect::<Vec<_>>()
        .join(",");
    let d = &p.drafting;
    let drafting = format!(
        concat!(
            "Drafting {{ font: {}, annotation_scale: {:?}, outline_width_mm100: {}, thin_width_mm100: {}, ",
            "dimension_text_mm: {:?}, dimension_arrow_mm: {:?}, extension_protrusion_mm: {:?}, text_gap_mm: {:?}, ",
            "baseline_spacing_mm: {:?}, designation_height_factor: {:?}, designation_alphabet: {}, thread_minimum_separation_mm: {:?} }}"
        ),
        string(&d.font),
        d.annotation_scale,
        d.outline_width_mm100,
        d.thin_width_mm100,
        d.dimension_text_mm,
        d.dimension_arrow_mm,
        d.extension_protrusion_mm,
        d.text_gap_mm,
        d.baseline_spacing_mm,
        d.designation_height_factor,
        string(&d.designation_alphabet),
        d.thread_minimum_separation_mm
    );
    format!(
        concat!(
            "// Generated from the validated external GOST profile; do not edit.\n",
            "fn bundled_profile() -> Profile {{ Profile {{ schema_version: {}, id: DEFAULT_ID.to_owned(), name: {}, version: {}, ",
            "jurisdiction: {}, as_of: {}, catalog_version: {}, standards: vec![{}], drafting: {}, consumed_rules: {}, limits: {} }} }}\n"
        ),
        p.schema_version,
        string(&p.name),
        p.version,
        string(&p.jurisdiction),
        string(&p.as_of),
        string(&p.catalog_version),
        standards,
        drafting,
        strings(&p.consumed_rules),
        strings(&p.limits)
    )
}
