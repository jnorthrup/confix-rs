//! Port of TrikeShed `ConfixSaxJaxTest.kt`, `ConfixFacetLazinessTest.kt`, and
//! `StructuralSharingTest.kt` — 1:1 test parity.
//!
//! Kotlin counts `Series` accessor reads to prove laziness; Rust's `Vec`/`OnceLock`
//! cannot observe reads the same way, so read-count assertions become
//! state-independence assertions (documented in README fidelity notes).

use confix_rs::core::{confix_doc, confix_doc_text, content_id, Syntax};
use confix_rs::saxjax::{sax_walk, JaxElement, SaxEvent};

// ── ConfixSaxJaxTest.kt ─────────────────────────────────────────────────────

#[test]
fn sax_stream_should_emit_enter_and_leave_events_ordered_by_confix_offsets() {
    let doc = confix_doc_text("{\"a\": [1, 2]}");

    let mut events = Vec::new();
    sax_walk(&doc.index, |event| events.push(event));

    assert!(!events.is_empty(), "Events should be emitted");
    assert!(
        matches!(events.first(), Some(SaxEvent::Enter { .. })),
        "First event should be Enter"
    );
    assert!(
        matches!(events.last(), Some(SaxEvent::Leave { .. })),
        "Last event should be Leave"
    );
}

#[test]
fn jax_inflation_should_bind_confix_direct_children_to_a_structural_dom_node() {
    let doc = confix_doc_text("{\"a\": [1, 2]}");

    let node = JaxElement::inflate(&doc.index, 0, &doc.src);

    assert!(!node.children.is_empty(), "Root node should have children");
}

#[test]
fn jax_element_should_lazy_load_raw_byte_slices() {
    let doc = confix_doc_text("{\"a\": [1, 2]}");
    let node = JaxElement::inflate(&doc.index, 0, &doc.src);
    let b = node.bytes();
    assert!(!b.is_empty(), "Should load raw byte slices");
}

// ── ConfixFacetLazinessTest.kt ──────────────────────────────────────────────
//
// Kotlin proves with a read-counting Series that scanning/geometry never decode
// payloads, and that derived facets build lazily and memoize. In this port the
// index owns a copy of the bytes (it must: the lazy key facet decodes from them),
// so the equivalent guarantees asserted here are:
//   - scanning does not panic or build derived facets,
//   - geometry accessors (spans/tags/depths/children/tree) work without the key
//     index being initialized (checked via Debug's OnceLock state),
//   - key lookups are memoized and consistent across calls,
//   - structural ids are memoized (same slice identity) and independent of keys.

#[test]
fn scanning_and_geometry_do_not_decode_keys_or_hash_source() {
    let fixtures_json = "{\"key\":\"value\",\"nested\":[1,true,null]}";
    let fixtures_cbor: Vec<u8> = vec![0xa1, 0x61, 0x6b, 0x01];
    let fixtures_yaml = "key: value\n";

    for (name, doc) in [
        ("Json", confix_doc_text(fixtures_json)),
        ("Cbor", confix_doc(&fixtures_cbor, Syntax::Cbor)),
        ("Yaml", confix_doc_text(fixtures_yaml)),
    ] {
        // Geometry accessors must all resolve.
        assert!(!doc.index.spans().is_empty(), "{name} spans");
        assert!(!doc.index.tags().is_empty(), "{name} tags");
        assert!(!doc.index.depths().is_empty(), "{name} depths");
        let _ = doc.index.direct_children(0);
        assert!(!doc.index.tree().is_empty(), "{name} tree");

        // Key index must NOT be initialized by geometry access alone.
        assert!(
            !doc.index.key_index_initialized(),
            "{name} geometry must not build the key index"
        );
        // Structural hashes must NOT be initialized by geometry access alone.
        assert!(
            !doc.index.structural_nodes_initialized(),
            "{name} geometry must not hash"
        );
    }
}

#[test]
fn key_index_is_built_on_first_lookup_and_reused() {
    let source = "{\"\\u006b\":\"first\",\"nested\":{\"k\":\"second\"}}";
    let doc = confix_doc_text(source);
    assert!(!doc.index.key_index_initialized());

    let lookup = doc.index.key_to_child("k");
    assert!(
        doc.index.key_index_initialized(),
        "first lookup builds the index"
    );
    assert_eq!(Some(1), lookup);
    // Repeated lookups reuse the memoized index (same answer, same map).
    assert_eq!(Some(1), doc.index.key_to_child("k"));
    assert_eq!(Some(3), doc.index.key_to_child("nested"));
    assert_eq!(None, doc.index.key_to_child("missing"));
}

#[test]
fn structural_ids_retain_their_encoding_and_are_memoized() {
    // Kotlin iterates per syntax over a SINGLE doc (bytes[0]) plus two part
    // byte-arrays (bytes[1], bytes[2]) whose CIDs must appear as children.
    let fixtures = [
        (
            Syntax::Json,
            b"[1,\"x\"]".to_vec(),
            b"1".to_vec(),
            b"\"x\"".to_vec(),
        ),
        (
            Syntax::Cbor,
            vec![0x82u8, 0x01, 0x61, 0x78],
            vec![0x01u8],
            vec![0x61u8, 0x78],
        ),
    ];
    for (syntax, doc_bytes, first_bytes, second_bytes) in fixtures {
        let doc = confix_doc(&doc_bytes, syntax);
        let ids = doc.index.structural_nodes();
        let first = content_id(&first_bytes);
        let second = content_id(&second_bytes);
        let root = content_id(format!("node:\n{first}\n{second}\n").as_bytes());
        assert_eq!(3, ids.len(), "{syntax:?}");
        assert_eq!(root, ids[0].as_deref().unwrap(), "{syntax:?}");
        assert_eq!(first, ids[1].as_deref().unwrap());
        assert_eq!(second, ids[2].as_deref().unwrap());

        // Memoized: repeated access returns the same computed values.
        assert_eq!(root, doc.index.structural_nodes()[0].as_deref().unwrap());
    }
}

#[test]
fn derived_facets_initialize_independently() {
    // keys-first then hashes, and the reverse: each builds only its own facet.
    let doc = confix_doc_text("{\"key\":\"value\"}");
    assert!(!doc.index.key_index_initialized());
    assert!(!doc.index.structural_nodes_initialized());

    assert!(doc.index.key_to_child("key").is_some());
    assert!(doc.index.key_index_initialized());
    assert!(
        !doc.index.structural_nodes_initialized(),
        "keys must not build hashes"
    );

    let _ = doc.index.structural_nodes();
    assert!(doc.index.structural_nodes_initialized());
}

#[test]
fn truncated_cbor_payloads_fail_before_derived_facets_are_requested() {
    for bytes_all in [vec![0x65u8, 0x61], vec![0x45u8, 0x01], vec![0xfau8, 0x00]] {
        let payload = bytes_all.clone();
        let result = std::panic::catch_unwind(move || {
            let _ = confix_doc(&payload, Syntax::Cbor);
        });
        assert!(result.is_err(), "truncated payload {bytes_all:?} must fail");
    }
}

#[test]
fn empty_indexes_have_empty_derived_facets() {
    let doc = confix_doc(&[], Syntax::Json);
    assert_eq!(0, doc.index.structural_nodes().len());
    assert_eq!(None, doc.index.key_to_child("missing"));
}

// ── StructuralSharingTest.kt ────────────────────────────────────────────────

#[test]
fn test_structural_sharing_with_deep_leaf_edit() {
    let mut builder = String::new();
    builder.push('[');
    for i in 0..100 {
        builder.push_str(&format!("{{\"id\": {i}, \"value\": \"test\"}}"));
        if i < 99 {
            builder.push_str(", ");
        }
    }
    builder.push(']');

    let doc1 = confix_doc_text(&builder);
    let index1 = &doc1.index;
    let structural_nodes1 = index1.structural_nodes();
    let root_cid1 = structural_nodes1[0].as_ref();
    assert!(root_cid1.is_some(), "Root should have a CID");

    let depths1 = index1.depths();
    let tags1 = index1.tags();

    let mut sibling_cids1: Vec<Option<String>> = Vec::new();
    for i in 0..structural_nodes1.len() {
        if depths1[i] == 1 && tags1[i] == confix_rs::core::IoMemento::IoObject {
            sibling_cids1.push(structural_nodes1[i].clone());
        }
    }

    let mut builder2 = String::new();
    builder2.push('[');
    for i in 0..100 {
        if i == 50 {
            builder2.push_str(&format!("{{\"id\": {i}, \"value\": \"edited\"}}"));
        } else {
            builder2.push_str(&format!("{{\"id\": {i}, \"value\": \"test\"}}"));
        }
        if i < 99 {
            builder2.push_str(", ");
        }
    }
    builder2.push(']');

    let doc2 = confix_doc_text(&builder2);
    let index2 = &doc2.index;
    let structural_nodes2 = index2.structural_nodes();
    let depths2 = index2.depths();
    let tags2 = index2.tags();

    let mut sibling_cids2: Vec<Option<String>> = Vec::new();
    for i in 0..structural_nodes2.len() {
        if depths2[i] == 1 && tags2[i] == confix_rs::core::IoMemento::IoObject {
            sibling_cids2.push(structural_nodes2[i].clone());
        }
    }

    let mut match_count = 0;
    let mut diff_count = 0;
    for i in 0..100 {
        let cid1 = &sibling_cids1[i];
        let cid2 = &sibling_cids2[i];
        assert!(cid1.is_some());
        assert!(cid2.is_some());
        if i == 50 {
            assert_ne!(cid1, cid2, "Target node CID should change");
            diff_count += 1;
        } else {
            assert_eq!(cid1, cid2, "Sibling node {i} CID should be unchanged");
            match_count += 1;
        }
    }
    assert_eq!(99, match_count, "99 siblings should match");
    assert_eq!(1, diff_count, "1 node should differ");
}

#[test]
fn debug_ids() {
    use confix_rs::core::*;
    let doc = confix_doc(b"[1,\"x\"]", Syntax::Json);
    let ids = doc.index.structural_nodes();
    let first = content_id(b"1");
    let second = content_id(b"\"x\"");
    let root = content_id(format!("node:\n{first}\n{second}\n").as_bytes());
    println!("expect root {root}");
    println!("expect first {first}");
    println!("expect second {second}");
    for (i, id) in ids.iter().enumerate() {
        println!("ids[{i}] = {:?}", id);
    }
    println!("spans {:?}", doc.index.spans());
    println!("children0 {:?}", doc.index.direct_children(0));
}

#[test]
fn content_id_matches_fips_180_4_empty_and_abc() {
    assert_eq!(
        content_id(b""),
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        content_id(b"abc"),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
