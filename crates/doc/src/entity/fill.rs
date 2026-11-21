use super::common::Handle;
use super::{Color, Deserialize, HatchEdge, PolyVertex, Serialize, Vec2, one};

/// A closed hatch boundary: either polyline/bulges or exact directed edges, never both.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "HatchLoopInput")]
pub struct HatchLoop {
    #[serde(default)]
    pub vertices: Vec<PolyVertex>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<HatchEdge>,
    #[serde(default)]
    pub outer: bool,
    /// The objects the loop was made from (DXF group 97 count, 330 handles): an associative
    /// hatch re-fills the loop from them when they change.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Handle>,
}

#[derive(Deserialize)]
struct HatchLoopInput {
    #[serde(default)]
    vertices: Vec<PolyVertex>,
    #[serde(default)]
    edges: Vec<HatchEdge>,
    #[serde(default)]
    outer: bool,
    #[serde(default)]
    sources: Vec<Handle>,
}
impl TryFrom<HatchLoopInput> for HatchLoop {
    type Error = &'static str;
    fn try_from(input: HatchLoopInput) -> Result<Self, Self::Error> {
        if !input.vertices.is_empty() && !input.edges.is_empty() {
            return Err("hatch boundary cannot contain both polyline vertices and exact edges");
        }
        if input.edges.iter().any(|edge| !edge.is_valid()) {
            return Err("hatch boundary has an invalid exact edge");
        }
        Ok(Self { vertices: input.vertices, edges: input.edges, outer: input.outer, sources: input.sources })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hatch {
    pub pattern: String,
    pub solid: bool,
    pub loops: Vec<HatchLoop>,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub associative: bool,
    /// 0 normal (odd parity), 1 outer, 2 ignore.
    #[serde(default)]
    pub style: u8,
    #[serde(default)]
    pub elevation: f64,
    /// Gradient fill: name + second colour.
    #[serde(default)]
    pub gradient: Option<Gradient>,
    #[serde(default)]
    pub origin: Vec2,
    #[serde(default)]
    pub background: Option<Color>,
    /// The pattern's line families as a file defined them, for a pattern that isn't in the
    /// standard library (unit scale and angle, like the library's; empty otherwise).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pattern_lines: Vec<crate::library::PatternLine>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gradient {
    pub name: String,
    pub color1: Color,
    pub color2: Color,
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub centered: bool,
}
