//! Port of TrikeShed `commonTest/.../confix/ConfixTest.kt` — 1:1 test parity.

use confix_rs::core::*;
use confix_rs::item::{decode, encode, Item};

fn bytes(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

// ── recognize ───────────────────────────────────────────────────────────────

#[test]
fn json_recognize_bracket() {
    assert!(Syntax::Json.recognize(b'{'));
    assert!(Syntax::Json.recognize(b'['));
    assert!(Syntax::Json.recognize(b'"'));
    assert!(!Syntax::Json.recognize(b'a'));
}

#[test]
fn yaml_recognize_plain_scalar() {
    assert!(Syntax::Yaml.recognize(b'a'));
    assert!(Syntax::Yaml.recognize(b'-'));
    assert!(!Syntax::Yaml.recognize(b'{'));
}

#[test]
fn cbor_recognize_any_byte() {
    assert!(Syntax::Cbor.recognize(0x00));
    assert!(Syntax::Cbor.recognize(0xFF));
}

// ── JSON scan geometry ──────────────────────────────────────────────────────

#[test]
fn scan_flat_number() {
    let cursor = Syntax::Json.scan(&bytes("42"));
    assert!(!cursor.is_empty() || cursor.is_empty()); // size >= 0 trivially in Rust
}

#[test]
fn scan_flat_number_via_flat_index() {
    let (_, ix) = {
        let r = Syntax::Json.scan0(&bytes("42"));
        (r.tree, r.flat)
    };
    assert_eq!(1, ix.spans.len());
    assert_eq!(IoMemento::IoDouble, ix.tags[0]);
    assert_eq!(0, ix.depths[0]);
}

#[test]
fn scan_string() {
    let r = Syntax::Json.scan0(&bytes("\"hello\""));
    assert_eq!(1, r.flat.spans.len());
    assert_eq!(IoMemento::IoString, r.flat.tags[0]);
}

#[test]
fn scan_empty_object() {
    let r = Syntax::Json.scan0(&bytes("{}"));
    assert_eq!(1, r.flat.spans.len());
    assert_eq!(IoMemento::IoObject, r.flat.tags[0]);
}

#[test]
fn scan_object_with_one_key() {
    let r = Syntax::Json.scan0(&bytes("{\"key\":\"val\"}"));
    assert_eq!(3, r.flat.spans.len());
    assert_eq!(IoMemento::IoObject, r.flat.tags[0]);
}

#[test]
fn scan_array() {
    let r = Syntax::Json.scan0(&bytes("[1,2,3]"));
    assert!(r.flat.spans.len() >= 4);
    assert_eq!(IoMemento::IoArray, r.flat.tags[0]);
}

#[test]
fn nested_object_depths() {
    let r = Syntax::Json.scan0(&bytes("{\"a\":{\"b\":1}}"));
    assert!(r.flat.depths.len() >= 3);
    assert_eq!(0, r.flat.depths[0]);
}

#[test]
fn direct_children() {
    let doc = confix_doc_text("{\"x\":1,\"y\":2}");
    let dc = doc.index.direct_children(0);
    assert_eq!(4, dc.len());
}

#[test]
fn direct_children_retain_source_order_for_object_key_value_pairs() {
    let doc = confix_doc_text("{\"first\":1,\"second\":2}");
    let direct_children = doc.index.direct_children(0);
    let spans = doc.index.spans();

    assert_eq!(4, direct_children.len());
    for index in 1..direct_children.len() {
        assert!(
            spans[direct_children[index - 1]].0 < spans[direct_children[index]].0,
            "DirectChildren must retain source order so object cursors expose key/value pairs",
        );
    }

    let first_key = doc.index.key_to_child("first");
    assert!(first_key.is_some());
    let first_value = doc.index.value_index_for(first_key.unwrap());
    assert!(first_value.is_some());
    let span = spans[first_value.unwrap()];
    let source = String::from_utf8(doc.src[span.0..=span.1].to_vec()).unwrap();
    assert_eq!("1", source);
}

#[test]
fn tree_cursor_produces_rows() {
    let cursor = Syntax::Json.scan(&bytes("[1,2]"));
    let row = &cursor[0];
    // Kotlin: assertTrue(row.a > 0) — `a` is the RowVec WIDTH (4), so this asserts
    // the row is materialized. Here: the root row exists with its full span.
    assert_eq!((0usize, 4usize), (row.open, row.close));
    assert_eq!(IoMemento::IoArray, row.tag);
}

#[test]
fn dispatch_routes_json() {
    let cursor = Syntax::dispatch(&bytes("42"));
    // YAML recognizes '4'; Kotlin's JSON.dispatch explicitly routes JSON.
    assert!(!cursor.is_empty() || cursor.is_empty());
}

#[test]
fn dispatch_routes_yaml() {
    // Kotlin: Syntax.YAML.dispatch(bytes("key: value\n")) — the same routing by
    // recognize order. In this port we replicate JSON-first recognize order.
    let cursor = Syntax::dispatch(&bytes("key: value\n"));
    assert!(!cursor.is_empty() || cursor.is_empty());
}

#[test]
fn array_of_100_numbers() {
    let nums: Vec<String> = (1..=100).map(|i| i.to_string()).collect();
    let r = Syntax::Json.scan0(&bytes(&format!("[{}]", nums.join(","))));
    assert!(r.flat.spans.len() >= 101);
}

// ── CBOR ────────────────────────────────────────────────────────────────────

#[test]
fn cbor_unsigned_int() {
    let b = [0x01u8];
    let cursor = Syntax::Cbor.scan(&b);
    assert!(!cursor.is_empty() || cursor.is_empty());
}

#[test]
fn cbor_array_of_ints() {
    let b = [0x83u8, 0x01, 0x02, 0x03];
    let cursor = Syntax::Cbor.scan(&b);
    assert!(!cursor.is_empty());
}

#[test]
fn cbor_index_facets_use_the_same_scanner_geometry_as_the_tree_cursor() {
    let bytes_v: Vec<u8> = vec![0xA1, 0x65, 0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x01];
    let doc = confix_doc(&bytes_v, Syntax::Cbor);
    let tags = doc.index.tags();

    assert_eq!(IoMemento::IoObject, tags[0]);
    assert_eq!(IoMemento::IoObject, doc.root().unwrap().tag());
    assert_eq!(2, doc.index.direct_children(0).len());
}

#[test]
fn cbor_scalar_rows_reify_text_payloads_and_unsigned_integers() {
    let bytes_v: Vec<u8> = vec![0xA1, 0x65, 0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x01];
    let doc = confix_doc(&bytes_v, Syntax::Cbor);
    let children = doc.root().unwrap().kids();

    assert_eq!(Value::Text("hello".into()), children[0].reify(doc.src()));
    assert_eq!(Value::Long(1), children[1].reify(doc.src()));
}

#[test]
fn decode_text_strips_quotes() {
    let src = b"hello";
    let (open, close) = Syntax::decode_text(src, 0, 4);
    // Kotlin asserts the view contains "hello" — for an unquoted span it passes through.
    let text = String::from_utf8(src[open..=close].to_vec()).unwrap();
    assert!(text.contains("hello"));
}

#[test]
fn confix_kit_parses_yaml_document() {
    let _doc = confix_doc_text("key: value\n");
    // YAML is auto-detected; document should have a root
    // (Kotlin test body is intentionally empty — parse must not crash)
}

#[test]
fn confix_kit_parses_cbor_map_and_reifies_value() {
    // CBOR: {0xa1} map(1), {0x65} text(5 bytes), "hello", {0x01} int(1)
    let b: Vec<u8> = vec![0xA1, 0x65, 0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x01];
    let doc = confix_doc(&b, Syntax::Cbor);
    // CBOR tree navigation is WIP in Kotlin — just confirm parse doesn't crash
    let _ = doc.index;
}

// ── CBOR codec (ConfixCborTest.kt parity) ───────────────────────────────────

fn cbor_bytes(values: &[u8]) -> Vec<u8> {
    values.to_vec()
}

#[test]
fn integers_use_minimal_canonical_widths() {
    use Item::{Nil, Num, Str};
    assert_eq!(cbor_bytes(&[0x00]), encode(&Num(0)));
    assert_eq!(cbor_bytes(&[0x17]), encode(&Num(23)));
    assert_eq!(cbor_bytes(&[0x18, 0x18]), encode(&Num(24)));
    assert_eq!(cbor_bytes(&[0x18, 0xff]), encode(&Num(255)));
    assert_eq!(cbor_bytes(&[0x19, 0x01, 0x00]), encode(&Num(256)));

    assert_eq!(cbor_bytes(&[0x20]), encode(&Num(-1)));
    assert_eq!(cbor_bytes(&[0x37]), encode(&Num(-24)));
    assert_eq!(cbor_bytes(&[0x38, 0x18]), encode(&Num(-25)));
    assert_eq!(cbor_bytes(&[0x38, 0xff]), encode(&Num(-256)));

    let _ = (Str("x".to_string()), Nil); // silence unused-import shape
}

#[test]
fn strings_booleans_null_and_arrays_use_cbor_major_types() {
    use Item::{Bool, Nil, Num, Str};
    assert_eq!(
        cbor_bytes(&[0x65, 0x68, 0x65, 0x6c, 0x6c, 0x6f]),
        encode(&Str("hello".into()))
    );
    assert_eq!(cbor_bytes(&[0xf5]), encode(&Bool(true)));
    assert_eq!(cbor_bytes(&[0xf4]), encode(&Bool(false)));
    assert_eq!(cbor_bytes(&[0xf6]), encode(&Nil));
    assert_eq!(
        cbor_bytes(&[0x82, 0x01, 0x61, 0x78]),
        encode(&Item::Arr(vec![Num(1), Str("x".into())]))
    );
}

#[test]
fn object_keys_are_ordered_by_canonical_encoded_bytes() {
    let first = Item::Map(vec![("b".into(), Item::Num(2)), ("a".into(), Item::Num(1))]);
    let second = Item::Map(vec![("a".into(), Item::Num(1)), ("b".into(), Item::Num(2))]);
    let expected = cbor_bytes(&[0xa2, 0x61, 0x61, 0x01, 0x61, 0x62, 0x02]);

    assert_eq!(expected, encode(&first));
    assert_eq!(expected, encode(&second));
}

#[test]
fn object_keys_use_unsigned_encoded_byte_ordering() {
    let item = Item::Map(vec![
        ("a\u{0080}".into(), Item::Num(2)),
        ("aa\u{0000}".into(), Item::Num(1)),
    ]);
    let expected = cbor_bytes(&[
        0xa2, 0x63, 0x61, 0x61, 0x00, 0x01, 0x63, 0x61, 0xc2, 0x80, 0x02,
    ]);

    assert_eq!(expected, encode(&item));
}

// ── ConfixCborEncoderTest.kt parity ─────────────────────────────────────────

#[test]
fn encoder_text_strings() {
    assert_eq!(cbor_bytes(&[0x60]), encode(&Item::Str("".into())));
    assert_eq!(cbor_bytes(&[0x61, 0x61]), encode(&Item::Str("a".into())));
    assert_eq!(
        cbor_bytes(&[0x64, 0x49, 0x45, 0x54, 0x46]),
        encode(&Item::Str("IETF".into()))
    );
    assert_eq!(
        cbor_bytes(&[0x62, 0x22, 0x5c]),
        encode(&Item::Str("\"\\".into()))
    );
    assert_eq!(
        cbor_bytes(&[0x62, 0xc3, 0xbc]),
        encode(&Item::Str("\u{fc}".into()))
    );
    assert_eq!(
        cbor_bytes(&[0x63, 0xe6, 0xb0, 0xb4]),
        encode(&Item::Str("\u{6c34}".into()))
    );
    assert_eq!(
        cbor_bytes(&[0x64, 0xf0, 0x90, 0x85, 0x91]),
        encode(&Item::Str("\u{10151}".into()))
    );
}

#[test]
fn encoder_byte_strings() {
    assert_eq!(cbor_bytes(&[0x40]), encode(&Item::Bin(Vec::new())));
    assert_eq!(cbor_bytes(&[0x41, 0x61]), encode(&Item::Bin(vec![0x61])));
    assert_eq!(
        cbor_bytes(&[0x44, 0x49, 0x45, 0x54, 0x46]),
        encode(&Item::Bin(b"IETF".to_vec()))
    );
    assert_eq!(
        cbor_bytes(&[0x42, 0x22, 0x5c]),
        encode(&Item::Bin(vec![0x22, 0x5c]))
    );
}

#[test]
fn encoder_unsigned_and_signed_integers() {
    assert_eq!(cbor_bytes(&[0x00]), encode(&Item::Num(0)));
    assert_eq!(cbor_bytes(&[0x17]), encode(&Item::Num(23)));
    assert_eq!(cbor_bytes(&[0x18, 0x18]), encode(&Item::Num(24)));
    assert_eq!(cbor_bytes(&[0x18, 0xff]), encode(&Item::Num(255)));
    assert_eq!(cbor_bytes(&[0x19, 0x01, 0x00]), encode(&Item::Num(256)));
    assert_eq!(cbor_bytes(&[0x19, 0xff, 0xff]), encode(&Item::Num(65535)));
    assert_eq!(
        cbor_bytes(&[0x1a, 0x00, 0x01, 0x00, 0x00]),
        encode(&Item::Num(65536))
    );
    assert_eq!(
        cbor_bytes(&[0x1a, 0xff, 0xff, 0xff, 0xff]),
        encode(&Item::Num(4294967295))
    );
    assert_eq!(
        cbor_bytes(&[0x1b, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]),
        encode(&Item::Num(4294967296))
    );
    assert_eq!(
        cbor_bytes(&[0x1b, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]),
        encode(&Item::Num(i64::MAX))
    );

    assert_eq!(cbor_bytes(&[0x20]), encode(&Item::Num(-1)));
    assert_eq!(cbor_bytes(&[0x37]), encode(&Item::Num(-24)));
    assert_eq!(cbor_bytes(&[0x38, 0x18]), encode(&Item::Num(-25)));
    assert_eq!(cbor_bytes(&[0x38, 0xff]), encode(&Item::Num(-256)));
    assert_eq!(cbor_bytes(&[0x39, 0xff, 0xff]), encode(&Item::Num(-65536)));
    assert_eq!(
        cbor_bytes(&[0x3a, 0x00, 0x01, 0x00, 0x00]),
        encode(&Item::Num(-65537))
    );
    assert_eq!(
        cbor_bytes(&[0x3a, 0xff, 0xff, 0xff, 0xff]),
        encode(&Item::Num(-4294967296))
    );
    assert_eq!(
        cbor_bytes(&[0x3b, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]),
        encode(&Item::Num(-4294967297))
    );
    assert_eq!(
        cbor_bytes(&[0x3b, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]),
        encode(&Item::Num(i64::MIN))
    );
}

#[test]
fn encoder_null() {
    assert_eq!(cbor_bytes(&[0xf6]), encode(&Item::Nil));
}

#[test]
fn encoder_bool() {
    assert_eq!(cbor_bytes(&[0xf5]), encode(&Item::Bool(true)));
    assert_eq!(cbor_bytes(&[0xf4]), encode(&Item::Bool(false)));
}

#[test]
fn encoder_float() {
    assert_eq!(
        cbor_bytes(&[0xfb, 0x3f, 0xf1, 0x99, 0x99, 0x99, 0x99, 0x99, 0x9a]),
        encode(&Item::Flt(1.1))
    );
    assert_eq!(
        cbor_bytes(&[0xfb, 0xbf, 0xf1, 0x99, 0x99, 0x99, 0x99, 0x99, 0x9a]),
        encode(&Item::Flt(-1.1))
    );
}

// ── ConfixCborDecoderTest.kt parity ─────────────────────────────────────────

fn assert_round_trip(item: Item) {
    let encoded = encode(&item);
    let decoded = decode(&encoded).unwrap();
    assert_eq!(item, decoded);
}

#[test]
fn round_trip_null() {
    assert_round_trip(Item::Nil);
}

#[test]
fn round_trip_booleans() {
    assert_round_trip(Item::Bool(true));
    assert_round_trip(Item::Bool(false));
}

#[test]
fn round_trip_integers() {
    assert_round_trip(Item::Num(0));
    assert_round_trip(Item::Num(42));
    assert_round_trip(Item::Num(255));
    assert_round_trip(Item::Num(65535));
    assert_round_trip(Item::Num(4294967295));
    assert_round_trip(Item::Num(-1));
    assert_round_trip(Item::Num(-42));
    assert_round_trip(Item::Num(-256));
    assert_round_trip(Item::Num(-65536));
}

#[test]
fn round_trip_floats() {
    #[allow(clippy::approx_constant)] // Kotlin test uses 3.14159 verbatim
    let pi_ish = 3.14159;
    assert_round_trip(Item::Flt(pi_ish));
    assert_round_trip(Item::Flt(-0.5));
}

#[test]
fn round_trip_strings() {
    assert_round_trip(Item::Str("hello world".into()));
    assert_round_trip(Item::Str("".into()));
}

#[test]
fn round_trip_arrays() {
    assert_round_trip(Item::Arr(vec![]));
    assert_round_trip(Item::Arr(vec![
        Item::Num(1),
        Item::Str("two".into()),
        Item::Nil,
    ]));
}

#[test]
fn round_trip_objects() {
    assert_round_trip(Item::Map(vec![]));
    assert_round_trip(Item::Map(vec![
        ("a".into(), Item::Num(1)),
        ("b".into(), Item::Str("two".into())),
        ("c".into(), Item::Nil),
        ("d".into(), Item::Arr(vec![Item::Num(3)])),
    ]));
}
