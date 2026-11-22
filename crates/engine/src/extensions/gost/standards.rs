//! Versioned normative identities and selected drafting policy. The evidence catalog
//! is a reference inventory, never a claim that its rules are all implemented.
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

use cadcraft_doc::{Drawing, Handle, RawTag, foreign::ForeignObject};
use serde_json::{Value, json};

use crate::{
    Result, Session,
    cmd::{CommandSpec, bad},
};

const RECORD: &str = "CADCRAFT_GOST_PROFILE";
mod profile;
use profile::{DEFAULT_ID, Selection, Standard};
pub(super) use profile::{Drafting, Profile};

const CATALOG_JSON: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gost/standards-catalog.json"));
include!(concat!(env!("OUT_DIR"), "/gost_profile.rs"));

pub(super) fn catalog() -> Result<&'static Value> {
    static DATA: LazyLock<std::result::Result<Value, serde_json::Error>> = LazyLock::new(|| serde_json::from_str(CATALOG_JSON));
    DATA.as_ref().map_err(|e| bad("gost.standards", format!("invalid bundled GOST catalog: {e}")))
}

/// The build script validates the external policy and emits this typed constructor.
/// Only the one-time owned allocations remain at runtime; invalid policy cannot build.
pub(super) fn default_profile() -> &'static Profile {
    static DATA: LazyLock<Profile> = LazyLock::new(bundled_profile);
    &DATA
}

fn lookup(id: &str) -> Result<&'static Profile> {
    let p = default_profile();
    if id != p.id {
        return Err(bad("gost.profile", "unknown or unimplemented profile; future editions require an explicitly implemented profile"));
    }
    Ok(p)
}

fn record(d: &Drawing) -> Result<Option<&ForeignObject>> {
    let mut entries = d.foreign.dictionary.iter().filter(|(key, _)| key.eq_ignore_ascii_case(RECORD));
    let Some((_, handle)) = entries.next() else { return Ok(None) };
    if entries.next().is_some() {
        return Err(bad("gost.profile", "duplicate profile dictionary entries; repair the document before changing its profile"));
    }
    let object = d
        .foreign
        .objects
        .iter()
        .find(|o| o.handle == *handle)
        .filter(|o| o.kind.eq_ignore_ascii_case("XRECORD"))
        .ok_or_else(|| bad("gost.profile", "profile dictionary entry must reference a native XRECORD"))?;
    Ok(Some(object))
}

fn selection(d: &Drawing) -> Result<Option<Selection>> {
    let Some(record) = record(d)? else { return Ok(None) };
    let mut text = String::new();
    for chunk in record.tags.iter().filter(|t| t.code == 1) {
        if chunk.value.len() > 4096 - text.len() {
            return Err(bad("gost.profile", "oversized profile XRECORD"));
        }
        text.push_str(&chunk.value);
    }
    if text.is_empty() {
        return Err(bad("gost.profile", "empty profile XRECORD"));
    }
    let s: Selection = serde_json::from_str(&text).map_err(|e| bad("gost.profile", format!("invalid profile XRECORD: {e}")))?;
    let p = lookup(&s.id)?;
    if s.schema_version != 1
        || s.version != p.version
        || s.catalog_version != p.catalog_version
        || !["cyrillic", "latin"].contains(&s.alphabet.as_str())
    {
        return Err(bad("gost.profile", "unsupported persisted profile version, catalog or alphabet; no automatic migration was performed"));
    }
    Ok(Some(s))
}

pub(super) fn selected(d: &Drawing) -> Result<Option<&'static Profile>> {
    selection(d)?.map(|s| lookup(&s.id)).transpose()
}

/// Legacy documents may use the working defaults without acquiring a new identity.
/// An unknown persisted identity is an error, never a default fallback.
pub(super) fn drafting(d: &Drawing) -> Result<&'static Drafting> {
    Ok(&selected(d)?.unwrap_or_else(default_profile).drafting)
}

fn tag(code: i32, value: impl Into<String>) -> RawTag {
    RawTag { code, value: value.into() }
}

fn persist(d: &mut Drawing, s: &Selection) -> Result<()> {
    let previous = selection(d)?;
    if previous.as_ref() == Some(s) {
        return Ok(());
    }
    let text = serde_json::to_string(s).map_err(|e| bad("gost.profile", e.to_string()))?;
    let existing = record(d)?.map(|o| o.handle);
    write_record(d, existing, text);
    Ok(())
}

fn write_record(d: &mut Drawing, existing: Option<Handle>, text: String) {
    let handle = existing.unwrap_or_else(|| d.new_handle());
    // Foreign::targets is the native writer's handle-to-role map. Registering the
    // NOD role makes the new owner chain roundtrip just like imported XRECORDs.
    let owner = d.foreign.targets.iter().find(|(_, role)| role.0 == "NOD" && role.1.is_empty()).map(|(h, _)| *h);
    let owner = owner.unwrap_or_else(|| d.new_handle());
    let tags = vec![tag(5, handle.hex()), tag(330, owner.hex()), tag(100, "AcDbXrecord"), tag(280, "1"), tag(1, text)];
    let foreign = Arc::make_mut(&mut d.foreign);
    foreign.targets.insert(owner, ("NOD".into(), String::new()));
    if let Some(object) = foreign.objects.iter_mut().find(|o| o.handle == handle) {
        object.tags = tags;
    } else {
        foreign.objects.push(ForeignObject { handle, kind: "XRECORD".into(), tags });
        foreign.dictionary.push((RECORD.into(), handle));
    }
}

/// A fresh drawing cannot contain a malformed imported profile. Its identity JSON
/// is serialized and checked by the build script, not by an infallible runtime factory.
pub(super) fn initial_drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    write_record(&mut d, None, DEFAULT_SELECTION_JSON.to_owned());
    d
}

pub(super) fn select_default(d: &mut Drawing) -> Result<()> {
    if selection(d)?.is_none() {
        write_record(d, None, DEFAULT_SELECTION_JSON.to_owned());
    }
    Ok(())
}

pub(super) fn response(d: &Drawing) -> Result<Value> {
    match selection(d)? {
        Some(s) => {
            let p = lookup(&s.id)?;
            let mut profile = serde_json::to_value(p).map_err(|e| bad("gost.profile", e.to_string()))?;
            profile["drafting"]["designation_alphabet"] = json!(s.alphabet);
            Ok(json!({"profile":profile,"warnings":p.limits}))
        }
        None => Ok(
            json!({"profile":{"id":"legacy-unversioned","name":"Unversioned drawing — no normative profile selected","version":0,"jurisdiction":"unverified","as_of":null,"standards":[]},
            "warnings":["No persisted normative identity. Opening or inspecting an old document never assigns a new edition. Select a supported profile explicitly; existing geometry is not migrated."]}),
        ),
    }
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("gost.standards", "GOST Standards Catalog", |_, p| {
            if !p.is_object() || p.as_object().is_some_and(|o| !o.is_empty()) { return Err(bad("gost.standards", "expected {}")) }
            Ok(catalog()?.clone())
        }).noundo().params("{} → versioned standard records, evidence sources, rule assessments and manual-review limits; read-only"),
        CommandSpec::new("gost.profile", "GOST Normative Profile", command)
            .menu(&["Format","GOST","GOST Normative Profile"])
            .params("{action?:get|list|set,id?:profile ID,alphabet?:cyrillic|latin (explicit set only)} → persisted versioned normative identity; existing geometry and measurements are never migrated"),
    ]
}

fn command(s: &mut Session, p: &Value) -> Result<Value> {
    let object = p.as_object().ok_or_else(|| bad("gost.profile", "parameters must be an object"))?;
    let action = p.get("action").map_or(Ok("get"), |v| v.as_str().ok_or_else(|| bad("gost.profile", "action must be a string")))?;
    match action {
        "get" if object.keys().all(|k| k == "action") => response(s.doc()?),
        "list" if object.keys().all(|k| k == "action") => {
            let p = default_profile();
            Ok(json!({"profiles":[{"id":p.id,"name":p.name,"version":p.version,"jurisdiction":p.jurisdiction,"as_of":p.as_of}]}))
        }
        "set" if object.keys().all(|k| matches!(k.as_str(), "action" | "id" | "alphabet")) => {
            let p = lookup(p.get("id").and_then(Value::as_str).ok_or_else(|| bad("gost.profile", "profile ID required"))?)?;
            // Check the old identity before mutation; unsupported stored versions
            // require an intentional migration implementation, not replacement.
            let old = selection(s.doc()?)?;
            let alphabet = match p_alphabet(object.get("alphabet"))? {
                Some(a) => a.to_owned(),
                None => old.map_or_else(|| p.drafting.designation_alphabet.clone(), |s| s.alphabet),
            };
            persist(
                s.doc_mut()?,
                &Selection { schema_version: 1, id: p.id.clone(), version: p.version, catalog_version: p.catalog_version.clone(), alphabet },
            )?;
            let mut result = response(s.doc()?)?;
            result["warnings"].as_array_mut().ok_or_else(|| bad("gost.profile", "profile response is missing its warnings array"))?.push(json!("Profile/alphabet selection is explicit and undoable. Existing geometry, lettering and measurements were not changed; review legacy annotations for mixed alphabets before issuing the document."));
            Ok(result)
        }
        _ => Err(bad("gost.profile", "expected get, list, or set with a supported ID; unknown parameters are rejected")),
    }
}

fn p_alphabet(value: Option<&Value>) -> Result<Option<&str>> {
    match value {
        None => Ok(None),
        Some(Value::String(s)) if ["cyrillic", "latin"].contains(&s.as_str()) => Ok(Some(s)),
        _ => Err(bad("gost.profile", "alphabet must be cyrillic or latin; Latin is an explicit when-necessary choice")),
    }
}

fn letter_policy(d: &Drawing, cmd: &str) -> Result<(bool, &'static [Value])> {
    let latin = selection(d)?.is_some_and(|s| s.alphabet == "latin");
    let rule = catalog()?["standards"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|s| s["rules"].as_array().into_iter().flatten())
        .find(|r| r["id"] == "gost-2-316.designation-alphabets")
        .ok_or_else(|| bad(cmd, "bundled catalog is missing the designation alphabet rule"))?;
    let key = if latin { "latin_uppercase_excluded" } else { "russian_uppercase_excluded" };
    let excluded = rule["parameters"][key]
        .as_array()
        .filter(|values| values.iter().all(|v| v.as_str().is_some_and(|s| s.chars().count() == 1)))
        .ok_or_else(|| bad(cmd, "bundled designation alphabet exclusions must be an array of single letters"))?;
    Ok((latin, excluded))
}

pub(in crate::extensions) fn validate_letter(d: &Drawing, label: &str, cmd: &str) -> Result<()> {
    let (latin, excluded) = letter_policy(d, cmd)?;
    let valid = designation_prefix(label).is_some_and(|((c, _), rest)| {
        rest.is_empty()
            && (if latin { c.is_ascii_uppercase() } else { ('А'..='Я').contains(&c) })
            && !excluded.iter().any(|v| v.as_str().is_some_and(|v| v.starts_with(c)))
    });
    if !valid {
        return Err(bad(
            cmd,
            if latin {
                "use one Latin capital excluding I, O, V, X, optionally followed by a decimal index; the document uses the explicitly selected Latin alphabet"
            } else {
                "use one Russian capital excluding Ё, З, Й, О, Х, Ч, Ъ, Ы, Ь, optionally followed by a decimal index; select the Latin document alphabet explicitly when necessary"
            },
        ));
    }
    Ok(())
}

fn designation_prefix(text: &str) -> Option<((char, Option<u32>), &str)> {
    let mut chars = text.chars();
    let first = chars.next()?;
    let rest = chars.as_str();
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 10 {
        return None;
    }
    let (digits, rest) = rest.split_at(digits);
    let index = if digits.is_empty() { None } else { Some(digits.parse().ok()?) };
    Some(((first, index), rest))
}

fn designation_initial(text: &str) -> Option<(char, Option<u32>)> {
    let (first, rest) = designation_prefix(text.trim())?;
    let rest = rest.trim_start();
    if rest.is_empty() || (rest.starts_with('(') && rest.ends_with(')')) {
        return Some(first);
    }
    let rest = rest.strip_prefix(['—', '–', '-'])?.trim_start();
    let (second, rest) = designation_prefix(rest)?;
    let rest = rest.trim_start();
    (first == second && (rest.is_empty() || (rest.starts_with('(') && rest.ends_with(')')))).then_some(first)
}

/// Choose a free designation from the selected document alphabet, then indexed
/// alphabets (А1, Б1, …, А2, …), per ГОСТ Р 2.316-2023 §5.9. Scan only at creation.
/// Each encountered index uses one letter bitset, not one allocation per label.
pub(in crate::extensions) fn next_letter(d: &Drawing, cmd: &str) -> Result<String> {
    let (latin, excluded) = letter_policy(d, cmd)?;
    let (first, last) = if latin { ('A', 'Z') } else { ('А', 'Я') };
    let allowed = (first..=last)
        .filter(|letter| !excluded.iter().any(|value| value.as_str().is_some_and(|value| value.starts_with(*letter))))
        .fold(0_u32, |mask, letter| mask | (1_u32 << (u32::from(letter) - u32::from(first))));
    if allowed == 0 {
        return Err(bad(cmd, "selected designation alphabet has no permitted letters"));
    }
    let mut unindexed = 0_u32;
    let mut used = BTreeMap::<u32, u32>::new();
    let mut visited = 0_usize;
    let mut bytes = 0_usize;
    for entity in d
        .model
        .iter()
        .chain(d.layouts.iter().flat_map(|layout| layout.entities.iter()))
        .chain(d.blocks.values().flat_map(|block| block.entities.iter()))
    {
        visited += 1;
        if visited > 100_000 {
            return Err(bad(cmd, "automatic designation scan exceeds 100000 entities; specify a letter explicitly"));
        }
        let text = match &entity.kind {
            cadcraft_doc::EntityKind::Text(text) => text.value.as_str(),
            cadcraft_doc::EntityKind::MText(text) => text.contents.as_str(),
            _ => continue,
        };
        bytes = bytes
            .checked_add(text.len())
            .filter(|n| *n <= 8 * 1024 * 1024)
            .ok_or_else(|| bad(cmd, "automatic designation text scan exceeds 8 MiB; specify a letter explicitly"))?;
        let decoded;
        let displayed = if text.contains(['\\', '{', '}', '%']) {
            decoded = cadcraft_fonts::plain_mtext(text);
            decoded.as_str()
        } else {
            text
        };
        for line in displayed.lines() {
            if let Some((letter, index)) = designation_initial(line)
                && (first..=last).contains(&letter)
            {
                let bit = 1_u32 << (u32::from(letter) - u32::from(first));
                if bit & allowed != 0 {
                    match index {
                        None => unindexed |= bit,
                        Some(index) => *used.entry(index).or_default() |= bit,
                    }
                }
            }
        }
    }
    // Try the unindexed alphabet first, then len+1 indexed groups: at least one
    // indexed group is absent from a map of len groups (pigeonhole principle).
    // The 8 MiB text budget bounds the number of groups well below u32::MAX.
    for index in 0..=used.len() + 1 {
        let index = (index != 0).then_some(index as u32);
        let occupied = index.map_or(unindexed, |index| used.get(&index).copied().unwrap_or(0));
        let available = allowed & !occupied;
        if available != 0 {
            let letter =
                char::from_u32(u32::from(first) + available.trailing_zeros()).ok_or_else(|| bad(cmd, "invalid designation alphabet codepoint"))?;
            let mut buffer = [0_u8; 4];
            let letter = letter.encode_utf8(&mut buffer);
            return Ok(match index {
                None => letter.to_owned(),
                Some(index) => format!("{letter}{index}"),
            });
        }
    }
    Err(bad(cmd, "automatic designation allocation exceeded its bounded alphabet groups"))
}

/// Paper lettering follows the actual GOST dimension-number height, including
/// user changes; model overall scale is applied by the caller exactly once.
pub(in crate::extensions) fn designation_height(d: &Drawing) -> Result<f64> {
    let policy = drafting(d)?;
    let number = d.dim_style("GOST").map_or(policy.dimension_text_mm, |s| s.text_height);
    let height = number * policy.designation_height_factor;
    if !height.is_finite() || !(0.1..=100.0).contains(&height) {
        return Err(bad("gost.profile", "designation height is not usable (0.1..100 paper mm); repair the GOST dimension text height"));
    }
    Ok(height)
}

pub(super) fn check_metadata(d: &Drawing) -> Result<Value> {
    let selected = response(d)?;
    let catalog = catalog()?;
    let profile = selected.get("profile").cloned().unwrap_or(Value::Null);
    let applicable: Vec<_> = catalog["standards"].as_array().into_iter().flatten().map(|entry| {
        let designation = profile["standards"].as_array().into_iter().flatten().find(|s| s["id"] == entry["id"]).map(|s| &s["designation"]);
        json!({"id":entry["id"],"selected_designation":designation,"sources":entry["sources"],"amendments":entry["amendments"],"manual_review":entry["manual_review"],
            "rule_assessments":entry["rules"],"rule_applicability":"Only rules whose applies_to equals selected_designation concern that selected edition; no cross-edition transfer."})
    }).collect();
    Ok(
        json!({"profile":profile,"warnings":selected["warnings"],"catalog_version":catalog["catalog_version"],"standards":applicable,"access_limitations":catalog["access_limitations"],
        "implementation_scope":"Only explicitly consumed drafting rules and existing limited checks are implemented. Catalog implementation_status remains an independent, unmodified assessment; authoritative full-text/amendment review remains required."}),
    )
}
