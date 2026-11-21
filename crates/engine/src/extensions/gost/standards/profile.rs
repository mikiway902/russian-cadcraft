//! Plain profile data shared by the build-time validator and runtime consumers.
use serde::{Deserialize, Serialize};

pub const DEFAULT_ID: &str = "rf-2026-v1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Standard {
    pub id: String,
    pub designation: String,
    pub edition: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drafting {
    pub font: String,
    pub annotation_scale: f64,
    pub outline_width_mm100: u16,
    /// Fits both unsigned layer and signed dimension lineweights without a fallible cast.
    pub thin_width_mm100: u8,
    pub dimension_text_mm: f64,
    pub dimension_arrow_mm: f64,
    pub extension_protrusion_mm: f64,
    pub text_gap_mm: f64,
    pub baseline_spacing_mm: f64,
    pub designation_height_factor: f64,
    pub designation_alphabet: String,
    pub thread_minimum_separation_mm: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: u32,
    pub jurisdiction: String,
    pub as_of: String,
    pub catalog_version: String,
    pub standards: Vec<Standard>,
    pub drafting: Drafting,
    pub consumed_rules: Vec<String>,
    pub limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub schema_version: u32,
    pub id: String,
    pub version: u32,
    pub catalog_version: String,
    pub alphabet: String,
}
