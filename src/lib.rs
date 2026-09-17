#![forbid(unsafe_code)]
//! Confix — hermetic Rust port of TrikeShed's Confix parser.
//!
//! - [`core`]: JSON/CBOR/YAML span scanner, flat index, tree cursor, navigation,
//!   reification, structural content IDs (port of `parse/confix/Confix.kt` + `ConfixKit.kt`).
//! - [`item`]: the `Item` metamodel and canonical RFC 8949 CBOR codec
//!   (port of `collections/associative/Item.kt` + `Cbor.kt`).
//! - [`saxjax`]: SAX event stream + JAX DOM-style inflation
//!   (port of `parse/confix/ConfixSaxJax.kt`).
//! - [`typedef_oracle`]: typedef collection + IS-A lattice
//!   (port of `parse/confix/TypeDefOracle.kt` + `cursor/TypeSubsumption.kt`).
//!
//! No TrikeShed `Facets`/`Series⇔`/`Join` machinery — plain Rust structs/enums.
//! Default build: std only. `json-cursor` adds optional serde/serde_json 1.x.
#![doc = include_str!("../docs/navigation.md")]

pub mod core;
pub mod item;
pub mod saxjax;
pub mod typedef_oracle;

#[cfg(feature = "cbor-cursor")]
pub mod cbor_cursor;
#[cfg(feature = "json-cursor")]
pub mod json_cursor;

pub use core::{
    confix_doc, confix_doc_text, content_id, scan, ConfixDoc, ConfixIndex, FlatIndex, IoMemento,
    PathStep, RowVec, Scan0Result, Syntax, TreeCursor, Value,
};
