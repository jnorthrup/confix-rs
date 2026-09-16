# confix-rs

Standalone, hermetic Rust port of TrikeShed's **Confix** parser
(`borg.trikeshed.parse.confix`), a byte-span document index for JSON, canonical
CBOR, and YAML.

## Grammar (what Confix indexes)

Confix is **not** a validating parser. It scans a byte source into *tokens* —
inclusive byte spans `(open, close)` with a type tag — and assembles them into a
span tree. The scanner is lenient by design; malformed input never errors
(except CBOR payload truncation, which fails the bounds check):

- **JSON scanner** (`Syntax::scan0`)
  - `{` / `}` → `IoObject` container spans; `[` / `]` → `IoArray`.
  - `"…"` → `IoString` spans (opening quote through closing quote, inclusive).
    An escaped character never closes the string: `\"` is a quote *inside* the
    text.
  - `true` / `false` → `IoBoolean`; `null` → `IoNothing`; runs of
    `[0-9+-.eE]` → `IoDouble`.
  - Anything else is whitespace/ignored.
  - Unclosed containers close at end-of-source.
- **CBOR scanner** (`Syntax::scan_cbor0`) — RFC 8949 items mapped to tags:
  major 0/1 → `IoLong`, 2 → `IoBytes`, 3 → `IoString`, 4 → `IoArray`,
  5 → `IoObject`, 7/20-21 → `IoBoolean`, 7/22-23 → `IoNothing`,
  7/25-27 → `IoDouble`. Definite and indefinite lengths.
- **YAML scanner** (`Syntax::scan_yaml0`) — documents whose first non-blank char
  is `{`/`[` fall through to the JSON scanner; line-oriented YAML emits key
  `IoString` spans, scalar values (`IoString`/`IoDouble`/`IoBoolean`/
  `IoNothing`), and `IoObject`/`IoArray` container spans built by an
  indentation-aware recursive walk.
- **Tree**: a token's children are tokens strictly inside its span at
  depth+1; object children are ordered `(key, value)` pairs in source order.

## Layout

| Kotlin source (`parse/confix/` unless noted) | Rust module |
|---|---|
| `Confix.kt` (`Syntax`, `scan0`, `scanCbor0`, `scanYaml0`, `buildTree`, `FlatIndex`, `scanIndex`) | `src/core.rs` |
| `ConfixKit.kt` (`confixDoc`, navigation, `RowVec.reify`, `decodeTextSpan`, CBOR value decoders) | `src/core.rs` |
| `ConfixIndexK.kt` (facet keys) | `src/core.rs` inherent methods on `ConfixIndex` |
| `ConfixSaxJax.kt` (`SaxEvent`, `saxWalk`, `JaxElement`) | `src/saxjax.rs` |
| `ConfixElement.kt` | dropped (see fidelity notes) |
| `ConfixSerialFormat.kt` (Item JSON rendering) | `src/item.rs` (`to_json_string`) |
| `collections/associative/Item.kt` + `Cbor.kt` | `src/item.rs` |
| `TypeDefOracle.kt` + `cursor/TypeSubsumption.kt` | `src/typedef_oracle.rs` |
| `RowVecBuilder.kt` | dropped (TrikeShed cursor/Join machinery, not parser) |
| `parse/yaml/Yaml.kt` (scanner AST source) | folded into `scan_yaml0`'s line walk |

Test files (`tests/`):

| Kotlin test | Rust test file |
|---|---|
| `commonTest/.../confix/ConfixTest.kt` | `tests/confix_test.rs` |
| `commonTest/.../parse/confix/ConfixCborTest.kt` | `tests/confix_test.rs` |
| `commonTest/.../parse/confix/ConfixCborEncoderTest.kt` | `tests/confix_test.rs` |
| `commonTest/.../parse/confix/ConfixCborDecoderTest.kt` | `tests/confix_test.rs` |
| `commonTest/.../parse/confix/ConfixSaxJaxTest.kt` | `tests/confix_geometry_test.rs` |
| `commonTest/.../parse/confix/ConfixFacetLazinessTest.kt` | `tests/confix_geometry_test.rs` |
| `commonTest/.../confix/StructuralSharingTest.kt` | `tests/confix_geometry_test.rs` |
| `commonTest/.../confix/TypeDefOracleTest.kt` | `tests/typedef_oracle_test.rs` |

## Kotlin → Rust API mapping

| Kotlin | Rust |
|---|---|
| `Syntax` | `core::Syntax` (`Json`, `Cbor`, `Yaml`) |
| `Syntax.recognize(first)` | `Syntax::recognize(self, u8) -> bool` |
| `Syntax.scan(src)` | `Syntax::scan(self, &[u8]) -> Vec<RowVec>` |
| `Syntax.scan0(src): Join<Cursor, FlatIndex>` | `Syntax::scan0(self, &[u8]) -> Scan0Result` |
| `Syntax.scanCbor0(src)` | `Syntax::scan_cbor0(self, &[u8]) -> Scan0Result` |
| `Syntax.scanYaml0(src)` | `Syntax::scan_yaml0(self, &[u8]) -> Scan0Result` |
| `Syntax.scanIndex(src): ConfixIndex` | `Syntax::scan_index(self, &[u8]) -> ConfixIndex` |
| `Syntax.dispatch(bytes)` | `Syntax::dispatch(&[u8]) -> Vec<RowVec>` |
| `Syntax.decodeText(src, open, close)` | `Syntax::decode_text(&[u8], usize, usize) -> (usize, usize)` |
| `ConfixIndex = FacetedRow<Any>` | `core::ConfixIndex` (struct) |
| `index.facet(ConfixIndexK.Spans)` | `ConfixIndex::spans()` |
| `index.facet(ConfixIndexK.Tags)` | `ConfixIndex::tags()` |
| `index.facet(ConfixIndexK.Depths)` | `ConfixIndex::depths()` |
| `index.facet(ConfixIndexK.DirectChildren)(i)` | `ConfixIndex::direct_children(i)` |
| `index.facet(ConfixIndexK.TreeCursor)` | `ConfixIndex::tree()` |
| `index.facet(ConfixIndexK.KeyToChild)(k)` | `ConfixIndex::key_to_child(k)` |
| `index.facet(ConfixIndexK.StructuralNodes)` | `ConfixIndex::structural_nodes()` |
| `ConfixIndex.valueIndexFor(i)` / `.resolve(key)` / `.resolve(i, n)` | `value_index_for` / `resolve_key` / `resolve_index` |
| `ConfixDoc = Join<ConfixIndex, Series<Byte>>` | `core::ConfixDoc` (`{ index, src }`) |
| `confixDoc(bytes, syntax)` / `confixDoc(text)` | `confix_doc(&[u8], Syntax)` / `confix_doc_text(&str)` |
| `scan(bytes, syntax)` / `scan(text)` | `core::scan(&[u8], Syntax) -> ConfixIndex` |
| `doc.roots` / `doc.root` | `ConfixDoc::roots()` / `ConfixDoc::root()` |
| `doc.getAt(*path)` / `doc.scalar(*path)` / `doc.value(*path)` | `ConfixDoc::get_at(&[PathStep])` / `scalar` / `value` |
| `doc.reify(tokenIdx)` | `ConfixDoc::reify_token(usize)` |
| `RowVec` (4 base columns) | `core::RowVec { open, close, tag, kids }` |
| `RowVec.step(key, src)` / `.step(idx)` | `RowVec::step_key` / `step_index` |
| `RowVec.getAt(*path, src)` | `RowVec::get_at(&[PathStep], &[u8])` |
| `RowVec.reify(src): Any?` | `RowVec::reify(&[u8]) -> Value` |
| `IOMemento` (subset) | `core::IoMemento` |
| `ContentId.of(bytes)` | `core::content_id(&[u8]) -> String` (`sha256:…`) |
| `Item` / `Cbor.encode` / `Cbor.decode` | `item::{Item, encode, decode}` |
| `Item.toJsonString()` | `item::to_json_string` |
| `SaxEvent` / `saxWalk` | `saxjax::{SaxEvent, sax_walk}` |
| `JaxElement.inflate` / `.bytes()` | `saxjax::JaxElement::inflate` / `.bytes()` |
| `TypeDefOracle` / `.build()` / `TypeDefOracleRow` | `typedef_oracle::{TypeDefOracle, TypeDefOracleRow}` |
| `IsALattice` (`.isA`, `.directSupers`, `.supertypes`) | `typedef_oracle::IsALattice` |
| `TypeToken` / `IsAEdge` | `typedef_oracle::{TypeToken, IsAEdge}` |

Path steps: Kotlin passes `Any?` path segments (`String`/`Int`); the Rust port
uses `core::PathStep::{Key(String), Index(usize)}`.

## Intentionally dropped (Facets machinery)

- `Facets`/`FacetedRow<Any>`/`OpK<R>` — replaced by plain structs. The seven
  `ConfixIndexK` facets became accessor methods; laziness uses `OnceLock`.
- `Series<T>` / `Join<A, B>` / `Twin<Int>` / `α` / `▶` — replaced by `Vec`,
  tuples, and iterators.
- `Cursor` as a lazy function-backed series — the tree cursor is a materialized
  `Vec<RowVec>` (built lazily, memoized).
- `ChunkedMutableSeries` — the scanners accumulate into plain `Vec`s.
- `ConfixElement` (`ConfixNull`/`ConfixPrimitive`/`ConfixArray`/`ConfixObject`) —
  a materialized DOM built only by `DescriptorFragments`/tests outside the
  parser core; the `Item` tree plus `RowVec` reification cover the same need.
- `RowVecBuilder` / widen nodes / `ColumnMeta↻` — blackboard/cursor plumbing,
  not parser behavior.
- kotlinx-serialization `ConfixFormat` encoder/decoder (`ConfixSerialFormat.kt`) —
  Rust has no kotlinx runtime; `Item` + `encode`/`decode` + `to_json_string`
  carry the data path. The boundary tests (`ConfixSerializationBoundaryTest`,
  `ConfixCborBoundaryTest`) are filesystem-scans of the Kotlin repo layout and
  have no Rust equivalent; their *intent* (no forbidden serialization deps in
  the default build) is enforced by the feature-flag structure here.
- `ConfixAllocationProbe` (JVM `ThreadMXBean` allocation benchmark) — JVM-only.
- `ConfixOracleServiceBenchmarkTest` (timing benchmark) — not a behavioral test.

## Feature flags

- **default**: std only. No crates. No unsafe. No I/O.
- **`json-cursor`**: adds `serde =1` + `serde_json =1` (exact-pinned,
  `default-features = false`, `std` feature on) and the `json_cursor` module —
  `focus(&serde_json::Value, &[PathStep])` navigation.
- **`cbor-cursor`**: std-only (no crates); adds `cbor_cursor::focus` over the
  canonical-CBOR `Item` tree (canonical rules: map keys sorted by encoded
  bytes, minimal-width integer heads, float64).

## Fidelity notes (deviations, all deliberate)

1. **Laziness proofs.** Kotlin's `ConfixFacetLazinessTest` counts `Series`
   element reads through a lambda-backed series. Rust slices cannot observe
   reads, so the port asserts the same guarantees via `OnceLock` state
   (`key_index_initialized` / `structural_nodes_initialized`): geometry access
   never builds derived facets; each derived facet initializes independently;
   computed values are memoized.
2. **Index owns a source copy.** Kotlin's lazy `KeyToChild` facet closes over
   the caller's `Series<Byte>`; the Rust index stores a copy of the bytes at
   `scan_index` time so the lazy facet stays self-contained. Behavior is
   identical for immutable sources.
3. **`RowVec` has no width column.** Kotlin `RowVec = Join<Int, …>` where `.a`
   is the column count (always 4). `tree_cursor_produces_rows` asserted
   `row.a > 0`, i.e. the row is materialized; the port asserts the row exists
   with its expected span/tag instead.
4. **YAML scanner.** Kotlin `scanYaml0` walks `YamlParser`'s line-based AST and
   re-maps line spans to byte offsets; the port performs an equivalent
   line-based recursive walk emitting byte spans directly (same fall-through to
   the JSON scanner, same quote-adjustment for string spans, same
   scalar classification). YAML support in Kotlin is itself explicitly
   WIP ("YAML parsing not yet fully implemented" per `ConfixTest`), so parity
   is at the Kotlin behavior level, not byte-exact span level.
5. **TypeDefOracle parsing.** Kotlin uses multiline regexes; the port uses a
   line splitter with the same grammar, including the regex quirk where
   `typedef Series2<A, B> as Series<Join<A, B>>;` does **not** match (nested
   `>` breaks `<([^>]+)>`), reproduced by `name_params_nested`. Kotlin
   `typealias` entries are extracted without a trailing `;` requirement.
   `TypeDefOracle.parseCBORTypeDefs` reads `name`/`referredTo`/`params` keys —
   note the Kotlin key decode has an off-by-one (spans `open..close`
   exclusive of the close byte); the port uses `reify` and decodes the full
   key text, matching the ingest path's intent.
6. **`ingestOracleJson` topic entries.** Kotlin's `topicPattern` is
   `^topic:(\w+)\s+as\s+(\w+)`; the port's `parse_topic` splits on the first
   ` as ` after `topic:` — equivalent for the tested inputs.
7. **CBOR decode bounds.** Kotlin's decoder throws `IndexOutOfBounds` on
   truncated input via raw array indexing; the port returns `Err(String)`.
   Error *messages* are Kotlin-internal and not part of the tested contract;
   the tested contract is "truncated CBOR fails at `scan_index` before derived
   facets" (panic in both, via the same `require`-style bounds check).
8. **Char semantics.** Kotlin maps bytes to chars via `toInt().toChar()`
   (latin-1 projection) for scanning, but UTF-8-decodes in `decodeTextSpan`.
   Both are reproduced exactly.
9. **`Syntax.dispatch` order.** `entries.first { it.recognize(...) }` iterates
   JSON → CBOR → YAML; CBOR recognizes every byte, so non-`{[ "` first bytes
   route to CBOR and never reach YAML in `dispatch` (only explicit
   `Syntax.YAML.scan` reaches the YAML scanner with such input). Reproduced.
10. **Numbers reify.** `IoDouble` reify falls back to text parse; on failure
    Kotlin returns `null` (`toDoubleOrNull`), the port `Value::Null`.

## Hermeticity

- Default build: `std` only; no dependencies; `#![forbid(unsafe_code)]`; no
  filesystem or network access in library code.
- `json-cursor` deps are pinned with `=1` (exact major-and-minor per semver
  semantics of `=1`): serde `=1`, serde_json `=1`, only reachable behind the
  feature flag.

## Verify

```sh
cargo build --no-default-features   # std-only core
cargo build --all-features          # + json-cursor + cbor-cursor
cargo test                          # full suite
cargo clippy --all-targets -- -D warnings
```
