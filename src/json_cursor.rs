//! `json-cursor` feature — navigate parsed JSON documents via `serde_json::Value`.
//!
//! Mirrors the doc-level navigation family from ConfixKit (`getAt`/`value` with
//! `PathStep`s) but over serde_json's DOM. serde/serde_json are ONLY available
//! behind this flag and are pinned `=1` in Cargo.toml.

use crate::core::PathStep;
use serde_json::Value;

/// Focus a `serde_json::Value` at a path of keys/indices.
///
/// Returns `None` when any step fails to resolve (mirrors Kotlin `getAt`
/// returning null on a dead step). String steps require the current node to be
/// an object; integer steps require an array.
pub fn focus<'a>(root: &'a Value, path: &[PathStep]) -> Option<&'a Value> {
    let mut cur = root;
    for step in path {
        cur = match step {
            PathStep::Key(k) => cur.get(k)?,
            PathStep::Index(i) => cur.get(*i)?,
        };
    }
    Some(cur)
}

/// `focus` on a JSON string. Parses then navigates; parse errors are returned.
pub fn focus_str(json: &str, path: &[PathStep]) -> Result<Option<Value>, serde_json::Error> {
    let v: Value = serde_json::from_str(json)?;
    Ok(focus(&v, path).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_object_key_and_array_index() {
        let v: Value = serde_json::from_str(r#"{"a": [1, {"b": "deep"}]}"#).unwrap();
        let path = [
            PathStep::Key("a".into()),
            PathStep::Index(1),
            PathStep::Key("b".into()),
        ];
        assert_eq!(
            focus(&v, &path),
            Some(&serde_json::Value::String("deep".into()))
        );
        assert!(focus(&v, &[PathStep::Key("missing".into())]).is_none());
        assert!(focus(&v, &[PathStep::Index(0)]).is_none());
    }
}
