# CONFIX token navigation: String or Int

The fundamental navigation path is an ordered sequence of **Either<String, Int>**
steps: select by string key, or select by integer position, then continue from the
selected token. Index first; navigate; reify only what is needed.

This is the small "mini-jq" mechanism, not a jq expression language. No textual
path parser, pipes, predicates, recursive descent, or wildcard expansion is
implied. For example, `["facts", 0, "location"]` denotes three typed steps;
`"facts[0].location"` passed as one string is just one literal key.

## Representations and source lineage

Canonical source checkout: `/Users/jim/work/TrikeShed` (not the linux-native fork).
The inspected revision was `f34c9c3b3`; references below are source inspection,
not a claim that the Kotlin suite was run.

- `parse/json/Json.kt:25–38`: `JsonPathElement = Either<String, Int>`,
  `JsonPath = Series<JsonPathElement>`, and `List<*>.toJsonPath`.
  Other element types are rejected.
- `parse/json/Json.kt:334–455`: `JsonParser.jsPath` walks these steps;
  `reifyResult` chooses a value versus a selected source segment.
- `parse/confix/ConfixKit.kt:68–106,225–301`: `getAt`, `docAt`, `value`,
  `navigate`, `keyOf`, `idxOf`, and `toJsPath`. The Confix `JsPathElement` stores
  the discriminator using `Join<String, Int>` and sentinels, rather than the
  legacy JSON parser's literal `Either` type. The conceptual key-or-index choice
  is the same; these encodings are not identical APIs.
- Rust `src/core.rs`: `PathStep::Key(String)` / `PathStep::Index(usize)`,
  `ConfixDoc::get_at`, `scalar`, `value`, and `RowVec::get_at`.

All Kotlin paths above are relative to
`src/commonMain/kotlin/borg/trikeshed/`. The source README already sketches this
at its Confix navigation section and legacy JSON indexer section; this document
makes the primitive and its limits explicit for the Rust port.

## Rust: select a token, then reify

```rust
use confix_rs::{confix_doc_text, PathStep, Value};

let doc = confix_doc_text(
    r#"{"facts":[{"name":"depot","location":"North"}]}"#,
);
let path = [
    PathStep::Key("facts".into()),
    PathStep::Index(0),
    PathStep::Key("location".into()),
];
let token = doc.get_at(&path).expect("selected token");
assert_eq!(&doc.src()[token.open..=token.close], b"\"North\"");
assert_eq!(doc.scalar(&path), Some(Value::Text("North".into())));
assert!(doc.get_at(&[PathStep::Key("facts[0].location".into())]).is_none());
assert!(doc.get_at(&[]).is_some()); // empty path selects the existing root
```

`get_at` returns the indexed row/span. `scalar` and `value` reify the selected
row. In this Rust span API a container reifies to `Value::Kids(count)`, not a
fully materialized JSON object. A missing step returns `None`; a present null
reifies to `Some(Value::Null)`.

## Do not conflate the indexing rules

The key-or-index primitive does not make all navigators interchangeable:

| Navigator | Integer step | String step |
|---|---|---|
| Legacy Kotlin `JsonParser.jsPath` | Array element, or nth object **value** in source order | Object key |
| Kotlin Confix `RowVec.step(Int)` and Rust span `step_index` | Raw child-row position; object children alternate key/value rows | Scan the object's key/value child pairs |
| Rust `json_cursor::focus` / `cbor_cursor::focus` | Array element only; wrong node type returns `None` | Object/map key |

Rust `usize` does not represent negative steps. The Confix span navigator is a
low-level index API, not a strict JSON-path validator: it does not enforce the
same node-type restrictions as the DOM cursor helpers. Its key lookup compares
source bytes between presumed quotes; empty/escaped keys and CBOR key spans
must not be advertised as fully equivalent to decoded-key lookup. The legacy
JSON selector's `Unit` miss sentinel is also distinct from Confix's nullable
result and Rust's `Option`.

The feature-gated JSON/CBOR cursor modules accept the same Rust `PathStep` shape,
but work on parsed value trees. They are alternatives with different cost and
semantics, not proof that the raw token path contract was fully ported.

## WONTFIX: path stability across deterministic-CBOR canonicalization

**Disposition: WONTFIX — accepted one-time mutation, outside CONFIX's
responsibility.** Deterministic CBOR sorts map entries by encoded keys. That
transformation can change ordinal/token paths and byte spans relative to the
original representation. CONFIX indexes the source it receives; it does not
preserve pre-mutation coordinates, remap old paths, or restore insertion order.

Index the final representation and bind token paths to that indexed source.
If a caller changes representation after indexing, rebuilding or translating
its references belongs to that caller. No path-stability compatibility layer
or canonicalizer change is planned in CONFIX for this behavior.

This disposition concerns paths invalidated by the transformation, not crashes
or incorrect navigation within an unchanged indexed source.

## Role in LOOM

A reusable fact bundle and an explicit token path provide source plus selection.
Several paths can select different facets of the same bundle for different
questions without assigning a new identity to the unchanged source bundle.
Projection identity and model-prefill identity are separate from source identity.

LOOM's optional Rust `loom-context` composer now depends on `confix-rs`. It
indexes JSON with CONFIX and navigates its row spans, with a JSON adapter for
decoded keys and array-only integer steps. It does not use the raw `get_at` key
shortcut described above. It replaces only an explicitly selected text-message
slot in a native request. This is **not** automatic server-side stored-bundle
resolution or a new provider protocol. No custom path syntax is mandatory on
the provider wire, and ordinary requests are never silently rewritten.
See [LOOM's projection contract](../../loom/docs/facet-projection.md).
Its message-prefix cache remains a separate storage mechanism, not proof of
model-state reuse.
