//! Versioned, bounded application xdata. The visible geometry remains ordinary DXF entities.
use crate::{Result, cmd::bad};
use cadcraft_doc::{Common, RawTag};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

const APP: &str = "CADCRAFTGOST";
const MAX_BYTES: usize = 12_000;

fn section(common: &Common) -> Option<std::ops::Range<usize>> {
    let start = common.xdata.iter().position(|t| t.code == 1001 && t.value.eq_ignore_ascii_case(APP))?;
    let end = common.xdata.iter().enumerate().skip(start + 1).find(|(_, t)| t.code == 1001).map_or(common.xdata.len(), |(i, _)| i);
    Some(start..end)
}

pub(super) fn load<T: DeserializeOwned>(common: &Common, kind: &str) -> Result<Option<T>> {
    let Some(range) = section(common) else { return Ok(None) };
    let tags = &common.xdata[range];
    let size = tags
        .iter()
        .filter(|t| t.code == 1000)
        .try_fold(0usize, |n, t| n.checked_add(t.value.len()))
        .ok_or_else(|| bad("gost.data", "metadata too large"))?;
    if size > MAX_BYTES {
        return Err(bad("gost.data", "metadata exceeds 12000 bytes"));
    }
    let mut text = String::with_capacity(size);
    for t in tags.iter().filter(|t| t.code == 1000) {
        text.push_str(&t.value);
    }
    let value: Value = serde_json::from_str(&text).map_err(|e| bad("gost.data", format!("invalid metadata: {e}")))?;
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(bad("gost.data", "unsupported metadata version"));
    }
    if value.get("kind").and_then(Value::as_str) != Some(kind) {
        return Ok(None);
    }
    serde_json::from_value(value.get("data").cloned().ok_or_else(|| bad("gost.data", "metadata payload missing"))?)
        .map(Some)
        .map_err(|e| bad("gost.data", format!("invalid {kind} payload: {e}")))
}

pub(super) fn save<T: Serialize>(common: &mut Common, kind: &str, payload: &T) -> Result<()> {
    let text = serde_json::to_string(&serde_json::json!({"version":1,"kind":kind,"data":payload})).map_err(|e| bad("gost.data", e.to_string()))?;
    if text.len() > MAX_BYTES {
        return Err(bad("gost.data", "metadata exceeds 12000 bytes"));
    }
    // Encode before mutation; split on UTF-8 boundaries below DXF's 255-byte string limit.
    let mut tags = vec![RawTag { code: 1001, value: APP.into() }];
    let mut rest = text.as_str();
    while !rest.is_empty() {
        let mut end = rest.len().min(240);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let (chunk, tail) = rest.split_at(end);
        tags.push(RawTag { code: 1000, value: chunk.into() });
        rest = tail;
    }
    if let Some(range) = section(common) {
        common.xdata.drain(range);
    }
    common.xdata.extend(tags);
    Ok(())
}
