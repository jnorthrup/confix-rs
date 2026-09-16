//! SAX event stream + JAX DOM-style inflation over a Confix index.
//! Port of TrikeShed `parse/confix/ConfixSaxJax.kt`.

use crate::core::{ConfixIndex, IoMemento};

/// `SaxEvent`: Enter/Leave with tag and offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SaxEvent {
    Enter { tag: IoMemento, offset: usize },
    Leave { tag: IoMemento, offset: usize },
}

/// `ConfixIndex.saxWalk`: Enter/Leave per token, ordered by confix offsets.
pub fn sax_walk(index: &ConfixIndex, mut action: impl FnMut(SaxEvent)) {
    let spans: Vec<(usize, usize)> = index.spans().to_vec();
    let tags: Vec<IoMemento> = index.tags().to_vec();
    for (twin, tag) in spans.iter().zip(tags.iter()) {
        action(SaxEvent::Enter {
            tag: *tag,
            offset: twin.0,
        });
        action(SaxEvent::Leave {
            tag: *tag,
            offset: twin.1,
        });
    }
}

/// `JaxElement`: structural DOM node bound to DirectChildren.
#[derive(Clone, Debug)]
pub struct JaxElement {
    pub tag: IoMemento,
    pub start_index: usize,
    pub end_index: usize,
    pub children: Vec<JaxElement>,
    backing_bytes: Option<Vec<u8>>,
}

impl JaxElement {
    /// `JaxElement.bytes()`: raw byte slice of the root span (empty until backed).
    pub fn bytes(&self) -> &[u8] {
        self.backing_bytes.as_deref().unwrap_or(&[])
    }

    /// `JaxElement.inflate`: bind the index's DirectChildren at `parent_token_idx`
    /// to a structural tree, backing the root with its raw byte slice.
    pub fn inflate(index: &ConfixIndex, parent_token_idx: usize, src: &[u8]) -> JaxElement {
        if index.spans().is_empty() {
            return JaxElement {
                tag: IoMemento::IoObject,
                start_index: 0,
                end_index: 0,
                children: Vec::new(),
                backing_bytes: None,
            };
        }

        let root_tag = index.tags()[parent_token_idx];
        let (root_start, root_end) = index.spans()[parent_token_idx];
        let children = index
            .direct_children(parent_token_idx)
            .into_iter()
            .map(|child_idx| JaxElement::inflate(index, child_idx, src))
            .collect();

        let len = root_end.saturating_sub(root_start) + 1;
        let backing = if root_end < src.len() && len > 0 {
            Some(src[root_start..=root_end].to_vec())
        } else {
            None
        };

        JaxElement {
            tag: root_tag,
            start_index: root_start,
            end_index: root_end,
            children,
            backing_bytes: backing,
        }
    }
}
