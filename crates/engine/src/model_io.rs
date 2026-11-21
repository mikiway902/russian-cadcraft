//! Host-installed model services. The engine never opens files or starts a worker.
use std::sync::OnceLock;

use cadcraft_doc::{model_preview::Preview, model_projection::Projection};
use serde_json::Value;

pub struct ModelHooks {
    pub load: fn(&str) -> std::result::Result<Value, String>,
    pub view: fn(&str, &str, [f64; 3], [f64; 3]) -> std::result::Result<Projection, String>,
    pub preview: fn(&str, &str, u8) -> std::result::Result<Preview, String>,
    pub source_status: fn(&str, &str, u64) -> std::result::Result<Value, String>,
    pub references: fn(&str, &str) -> std::result::Result<Value, String>,
    pub resolve_point: fn(&str, &str, &str, f64) -> std::result::Result<Value, String>,
}

static HOOKS: OnceLock<ModelHooks> = OnceLock::new();

pub fn set_hooks(hooks: ModelHooks) {
    let _ = HOOKS.set(hooks);
}

pub fn hooks() -> Option<&'static ModelHooks> {
    HOOKS.get()
}
