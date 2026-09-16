//! `cbor-cursor` feature — navigate canonical CBOR documents (std-only).
//!
//! Focus on the decoded `Item` tree: path of keys/indices over the RFC 8949
//! canonical representation from [`crate::item`] (map keys sorted, minimal
//! widths, shortest float encodings).
//!
//! Note: the Item tree is already navigable directly (`Item::map_get`,
//! `Item::Arr`); this module provides the shared `focus(path)` shape so JSON,
//! CBOR and Confix-span navigation have the same API surface.

use crate::core::PathStep;
use crate::item::{decode, Item};

/// Focus a decoded CBOR `Item` at a path of keys/indices.
pub fn focus<'a>(root: &'a Item, path: &[PathStep]) -> Option<&'a Item> {
    let mut cur = root;
    for step in path {
        cur = match step {
            PathStep::Key(k) => cur.map_get(k)?,
            PathStep::Index(i) => match cur {
                Item::Arr(items) => items.get(*i)?,
                _ => return None,
            },
        };
    }
    Some(cur)
}

/// `focus` on canonical CBOR bytes. Parses then navigates.
pub fn focus_bytes(cbor: &[u8], path: &[PathStep]) -> Result<Option<Item>, String> {
    let item = decode(cbor)?;
    Ok(focus(&item, path).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{encode, item_array_of, item_map_of};

    #[test]
    fn focus_canonical_cbor_tree() {
        let item = item_map_of(vec![(
            "a",
            item_array_of(vec![
                Item::Num(1),
                item_map_of(vec![("b", Item::Str("deep".into()))]),
            ]),
        )]);
        let bytes = encode(&item);
        let path = [
            PathStep::Key("a".into()),
            PathStep::Index(1),
            PathStep::Key("b".into()),
        ];
        let focused = focus_bytes(&bytes, &path).unwrap();
        assert_eq!(focused, Some(Item::Str("deep".into())));
    }
}
