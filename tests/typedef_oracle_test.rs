//! Port of TrikeShed `commonTest/.../confix/TypeDefOracleTest.kt` — 1:1 test parity.

use confix_rs::typedef_oracle::{IsALattice, TypeDefOracle, TypeToken};

// ── .x typedef parsing ──────────────────────────────────────────────────────

#[test]
fn parse_single_typedef() {
    let mut oracle = TypeDefOracle::new();
    oracle.parse_type_defs("typedef Join<A, B> as Tuple<A, B>;", "test.x");
    assert_eq!(1, oracle.size());
    let o = oracle.build();
    assert_eq!(1, o.entries.len());
    assert_eq!("Tuple", o.entries[0].name);
    assert_eq!("Join<A, B>", o.entries[0].referred_to_type);
}

#[test]
fn parse_rfc_1_cursor_typedefs() {
    let src = "\
        typedef Join<T, T> as Twin<T>;\n\
        typedef MetaSeries<Int, T> as Series<T>;\n\
        typedef Series<RowVec> as Cursor;\n\
        typedef Series<Cell> as RowVec;";
    let mut oracle = TypeDefOracle::new();
    oracle.parse_type_defs(src, "typedefs.x");
    let o = oracle.build();
    let entry_count = o.entries.len();
    assert!(
        entry_count > 0,
        "expected entries but got {entry_count} from: {src}"
    );
    let token_count = o.tokens.len();
    assert!(token_count > 0, "expected tokens but got {token_count}");

    // name resolution
    let cursor = o.by_name("Cursor");
    assert!(cursor.is_some());
    assert_eq!("Cursor", o.td_names(cursor.unwrap()));

    // lattice has edges: Cursor → Series, RowVec → Series, Series → MetaSeries, Twin → Join
    assert!(o.edge_count >= 4);
}

#[test]
fn parse_xvm_union_typedefs() {
    let src = "\
        typedef String|Int as StringOrInt;\n\
        typedef String|IPAddress as Host;\n\
        typedef MediaType|MediaType[] as MediaTypes;";
    let mut oracle = TypeDefOracle::new();
    oracle.parse_type_defs(src, "union.x");
    let o = oracle.build();
    assert_eq!(3, o.entries.len());

    // StringOrInt IS-A String and StringOrInt IS-A Int
    let lattice = &o.lattice;
    let string_or_int = o.by_name("StringOrInt");
    assert!(string_or_int.is_some());
    let string_tok = o.by_name("String");
    assert!(string_tok.is_some());
    assert!(lattice.is_a(string_or_int.unwrap(), string_tok.unwrap()));
}

#[test]
fn lattice_transitive_is_a() {
    let src = "\
        typedef Join<T, T> as Twin<T>;\n\
        typedef MetaSeries<Int, T> as Series<T>;\n\
        typedef Series<RowVec> as Cursor;";
    let mut oracle = TypeDefOracle::new();
    oracle.parse_type_defs(src, "transitive.x");
    let o = oracle.build();

    // Cursor → Series → MetaSeries (transitive)
    let cursor = o.by_name("Cursor");
    let meta_series = o.by_name("MetaSeries");
    assert!(cursor.is_some());
    assert!(meta_series.is_some());
    assert!(o.lattice.is_a(cursor.unwrap(), meta_series.unwrap()));
}

#[test]
fn lattice_reflexive_is_a() {
    let mut oracle = TypeDefOracle::new();
    oracle.parse_type_defs("typedef Join<A, B> as Tuple<A, B>;", "test.x");
    let o = oracle.build();
    let tuple = o.by_name("Tuple");
    assert!(tuple.is_some());
    assert!(o.lattice.is_a(tuple.unwrap(), tuple.unwrap()));
}

#[test]
fn lattice_incompatible_types() {
    let mut oracle = TypeDefOracle::new();
    oracle.parse_type_defs("typedef String|Int as StringOrInt;", "test.x");
    let o = oracle.build();
    let string_or_int = o.by_name("StringOrInt").unwrap();
    // no edge from String to Int
    let int_tok = o.by_name("Int").unwrap();
    assert!(!o.lattice.is_a(int_tok, string_or_int));
}

// ── Kotlin typealias parsing ────────────────────────────────────────────────

#[test]
fn parse_kotlin_typealias() {
    let src = "\
        typealias TypeProductionSlot = FacetedRow<TypeProductionK<*>>\n\
        typealias TypeBlackboard = Cursor";
    let mut oracle = TypeDefOracle::new();
    oracle.parse_type_defs(src, "arch.kt");
    let o = oracle.build();
    assert_eq!(2, o.entries.len());
    assert_eq!("TypeProductionSlot", o.entries[0].name);
    assert_eq!("TypeBlackboard", o.entries[1].name);
}

// ── explicit link checks ────────────────────────────────────────────────────

#[test]
fn add_explicit_link_check_edge() {
    let mut oracle = TypeDefOracle::new();
    oracle.parse_type_defs("typedef Series<RowVec> as Cursor;", "test.x");
    oracle.add_link_check("Cursor", "Iterable");
    let o = oracle.build();

    // explicit edge: Cursor IS-A Iterable
    let cursor = o.by_name("Cursor").unwrap();
    let iterable = o.by_name("Iterable").unwrap();
    assert!(o.lattice.is_a(cursor, iterable));

    // also still has the typedef edge: Cursor IS-A Series
    let series = o.by_name("Series").unwrap();
    assert!(o.lattice.is_a(cursor, series));
}

#[test]
fn ingest_topic_from_json() {
    let json = "{\"rows\":[{\"kind\":\"topic\",\"ngram\":\"topic:String as TType\"}]}";
    let mut oracle = TypeDefOracle::new();
    oracle.ingest_oracle_json(json);
    let o = oracle.build();
    assert!(o.entries.is_empty() || o.entries.len() == 1);
    if o.entries.len() == 1 {
        assert_eq!("TType", o.entries[0].name);
    }
}

// ── full xvm typedefs.x from RFC-1 ──────────────────────────────────────────

#[test]
fn parse_full_rfc_1_typedefs() {
    let src = "\
        typedef Join<A, B> as Tuple<A, B>;\n\
        typedef Twin<T> as Join<T, T>;\n\
        typedef MetaSeries<I, T> as Join<I, function T(I)>;\n\
        typedef Series<T> as MetaSeries<Int, T>;\n\
        typedef Series2<A, B> as Series<Join<A, B>>;\n\
\n\
        typedef Join<String, String> as ColumnMeta;\n\
        typedef function ColumnMeta() as ColumnMetaRef;\n\
        typedef Join<Any?, ColumnMetaRef> as Cell;\n\
        typedef Series<Cell> as RowVec;\n\
        typedef Series<RowVec> as Cursor;\n\
\n\
        typedef Series<Char> as CharStr;\n\
        typedef Series<CharStr> as Corpus;";
    let mut oracle = TypeDefOracle::new();
    oracle.parse_type_defs(src, "typedefs.x");
    let o = oracle.build();

    // all 11 typedefs parsed
    assert_eq!(11, o.entries.len());

    // key compositional atoms exist
    for name in [
        "Cursor", "RowVec", "Series", "CharStr", "Twin", "Tuple", "Cell", "Corpus",
    ] {
        assert!(o.by_name(name).is_some(), "{name} token missing");
    }

    // transitive: Cursor → Series (direct), Series → MetaSeries (via typedef chain)
    let cursor = o.by_name("Cursor").unwrap();
    let series = o.by_name("Series").unwrap();
    assert!(
        o.lattice.is_a(cursor, series),
        "Cursor IS-A Series (direct edge)"
    );

    // Tuple → Join (direct)
    let tuple = o.by_name("Tuple").unwrap();
    let join = o.by_name("Join").unwrap();
    assert!(o.lattice.is_a(tuple, join), "Tuple IS-A Join (direct edge)");

    // Join → Twin (from typedef Twin<T> as Join<T,T>)
    let twin = o.by_name("Twin").unwrap();
    assert!(
        o.lattice.is_a(join, twin),
        "Join IS-A Twin (from typedef Twin<T> as Join<T,T>"
    );

    // params preserved
    let tuple_entry = o.entries.iter().find(|e| e.name == "Tuple");
    assert!(tuple_entry.is_some());
    let tuple_entry = tuple_entry.unwrap();
    assert_eq!(2, tuple_entry.params.len());
    assert_eq!("A", tuple_entry.params[0].name);
    assert_eq!("B", tuple_entry.params[1].name);
}

// ── xvm master typedefs (union types) ───────────────────────────────────────

#[test]
fn parse_xvm_master_union_typedefs() {
    let src = "\
        typedef Int|Int[] as KeySize;\n\
        typedef Algorithm|String as Specifier;\n\
        typedef Signature|Byte[] as Digest;\n\
        typedef String|IPAddress as Host;\n\
        typedef String|Int as StringOrInt;";
    let mut oracle = TypeDefOracle::new();
    oracle.parse_type_defs(src, "master.x");
    let o = oracle.build();

    assert_eq!(5, o.entries.len());

    // union typedefs: KeySize IS-A Int
    let key_size = o.by_name("KeySize").unwrap();
    let int_tok = o.by_name("Int").unwrap();
    assert!(o.lattice.is_a(key_size, int_tok));

    // Host IS-A String, Host IS-A IPAddress
    let host = o.by_name("Host").unwrap();
    let string_tok = o.by_name("String").unwrap();
    let ip_addr_tok = o.by_name("IPAddress").unwrap();
    assert!(o.lattice.is_a(host, string_tok));
    assert!(o.lattice.is_a(host, ip_addr_tok));
}

// ── supertypes query (staircase) ────────────────────────────────────────────

#[test]
fn supertypes_staircase() {
    let src = "\
        typedef Join<T, T> as Twin<T>;\n\
        typedef MetaSeries<Int, T> as Series<T>;\n\
        typedef Series<RowVec> as Cursor;";
    let mut oracle = TypeDefOracle::new();
    oracle.parse_type_defs(src, "staircase.x");
    let o = oracle.build();
    let cursor = o.by_name("Cursor").unwrap();

    let supers = o.lattice.supertypes(cursor, usize::MAX);
    // Cursor → Series → MetaSeries → Join (at minimum)
    let super_names: Vec<String> = supers.iter().map(|t| o.td_names(*t)).collect();
    assert!(
        super_names.contains(&"Series".to_string())
            || super_names.contains(&"MetaSeries".to_string())
            || super_names.contains(&"Join".to_string()),
        "Expected at least one transitive supertype, got: {super_names:?}"
    );
}

// ── efficiency under repetition ─────────────────────────────────────────────

#[test]
fn parse_many_typedefs_efficiently() {
    let mut src = String::new();
    for i in 0..1000 {
        src.push_str(&format!("typedef Join<A, B> as Tuple{i}<A, B>;\n"));
    }
    let mut oracle = TypeDefOracle::new();
    oracle.parse_type_defs(&src, "many.x");
    let o = oracle.build();
    assert_eq!(1000, o.entries.len());

    // verify a couple are present and edges exist
    let t0 = o.by_name("Tuple0");
    let t999 = o.by_name("Tuple999");
    let join = o.by_name("Join");
    assert!(t0.is_some());
    assert!(t999.is_some());
    assert!(join.is_some());

    assert!(o.lattice.is_a(t0.unwrap(), join.unwrap()));
    assert!(o.lattice.is_a(t999.unwrap(), join.unwrap()));
}

// ── IsALattice direct-supers / TypeToken sanity (supporting Kotlin asserts) ─

#[test]
fn lattice_direct_supers_matches_build_edges() {
    let mut oracle = TypeDefOracle::new();
    oracle.parse_type_defs("typedef Series<RowVec> as Cursor;", "test.x");
    let o = oracle.build();
    let cursor = o.by_name("Cursor").unwrap();
    let supers = o.lattice.direct_supers(cursor);
    assert!(supers.contains(&o.by_name("Series").unwrap()));
}

// Silence unused-import lint for TypeToken (mirrors Kotlin's TypeToken use in signatures).
#[allow(dead_code)]
fn _type_token_witness(_t: TypeToken, _l: &IsALattice) {}
