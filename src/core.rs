//! Core Confix scanner — Rust port of TrikeShed `parse/confix/Confix.kt` + `ConfixKit.kt`.
//!
//! The Kotlin original stores parse geometry in a faceted row (`FacetedRow<Any>` +
//! `ConfixIndexK` OpK keys). This port replaces that machinery with a plain
//! [`ConfixIndex`] struct: same projections (spans, tags, depths, direct children,
//! tree cursor, key index, structural nodes), same laziness semantics.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Type discriminant per token — mirrors Kotlin `IOMemento` (subset used by Confix).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IoMemento {
    IoBoolean,
    IoInt,
    IoLong,
    IoDouble,
    IoString,
    IoNothing,
    IoBytes,
    IoArray,
    IoObject,
}

/// Syntax dialects, mirroring Kotlin `Syntax`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Syntax {
    Json,
    Cbor,
    Yaml,
}

/// One parsed token: inclusive byte span plus type tag. This is Kotlin's `RowVec`
/// reduced to its four base columns (open/close/tag/kids).
#[derive(Clone, Debug, PartialEq)]
pub struct RowVec {
    pub open: usize,
    pub close: usize,
    pub tag: IoMemento,
    pub kids: Vec<RowVec>,
}

/// Flat parse geometry. Kotlin: `Syntax.FlatIndex` (spans/tags/depths/childOf).
#[derive(Clone, Debug)]
pub struct FlatIndex {
    /// Byte offset pairs per token, in source order (sorted by open).
    pub spans: Vec<(usize, usize)>,
    /// Type discriminant per token.
    pub tags: Vec<IoMemento>,
    /// Nesting depth per token.
    pub depths: Vec<usize>,
}

/// Cursor over the recursive token tree (Kotlin: the `TreeCursor` facet).
pub type TreeCursor = Vec<RowVec>;

/// Internal accumulator for scanner output (opens/closes/tags as parallel lists,
/// exactly like the Kotlin scanner's ChunkedMutableSeries triple).
#[derive(Default)]
struct Tokens {
    opens: Vec<usize>,
    closes: Vec<usize>,
    tags: Vec<IoMemento>,
}

impl Tokens {
    fn add(&mut self, open: usize, close: usize, tag: IoMemento) {
        self.opens.push(open);
        self.closes.push(close);
        self.tags.push(tag);
    }
}

/// Faceted index over a scanned document — replaces Kotlin's `ConfixIndex = FacetedRow<Any>`.
///
/// Derived facets (tree, key index, structural hashes) are lazy exactly as in Kotlin:
/// building them does not touch token payloads unless they must decode keys or hash spans.
/// The index captures the source bytes at `scan_index` time (the Kotlin lazy `keys`
/// facet likewise closes over `src`); this port stores a copy.
#[derive(Debug)]
pub struct ConfixIndex {
    syntax: Syntax,
    flat: FlatIndex,
    src: Vec<u8>,
    tree: OnceLock<TreeCursor>,
    key_to_child: OnceLock<HashMap<String, usize>>,
    structural_nodes: OnceLock<Vec<Option<String>>>,
}

/// Parsed document: index + source bytes. Kotlin: `ConfixDoc = Join<ConfixIndex, Series<Byte>>`.
#[derive(Debug)]
pub struct ConfixDoc {
    pub index: ConfixIndex,
    pub src: Vec<u8>,
}

/// Scan result pair (Kotlin `Join<Cursor, FlatIndex>`).
pub struct Scan0Result {
    pub tree: TreeCursor,
    pub flat: FlatIndex,
}

impl Syntax {
    /// `Syntax.recognize`: can this syntax begin with byte `first`?
    pub fn recognize(self, first: u8) -> bool {
        match self {
            Syntax::Json => matches!(first, b'{' | b'[' | b'"'),
            Syntax::Cbor => true,
            Syntax::Yaml => !matches!(first, b'{' | b'['),
        }
    }

    /// `Syntax.scan`: scan into the tree cursor only.
    pub fn scan(self, src: &[u8]) -> TreeCursor {
        match self {
            Syntax::Json => self.scan0(src).tree,
            Syntax::Cbor => self.scan_cbor0(src).tree,
            Syntax::Yaml => self.scan_yaml0(src).tree,
        }
    }

    /// `Syntax.dispatch`: route on the first byte through `recognize`.
    ///
    /// # Panics
    /// Panics on empty input and when no syntax recognizes the first byte
    /// (Kotlin `entries.first { it.recognize(source[0]) }` fails the same way).
    pub fn dispatch(bytes: &[u8]) -> TreeCursor {
        let first = bytes[0];
        let syntax = [Syntax::Json, Syntax::Cbor, Syntax::Yaml]
            .into_iter()
            .find(|s| s.recognize(first))
            .expect("no syntax recognizes input");
        syntax.scan(bytes)
    }

    /// `Syntax.decodeText`: strip a matched outer quote pair; pass through otherwise.
    /// Returns the adjusted (open, close) window (Kotlin returns a CharStr view).
    pub fn decode_text(src: &[u8], open: usize, close: usize) -> (usize, usize) {
        let first = src[open];
        let last = src[close];
        if first == b'"' && last == b'"' && close > open + 1 {
            (open + 1, close - 1)
        } else {
            (open, close)
        }
    }

    /// `Syntax.scan0` — the JSON scanner. Lenient by design: the Kotlin scanner never
    /// errors on malformed input; it indexes whatever structure it can see and closes
    /// unclosed containers at end-of-source.
    pub fn scan0(self, src: &[u8]) -> Scan0Result {
        // per-byte latin-1 projections, matching Kotlin's toInt().toChar()
        let chars: Vec<char> = src.iter().map(|&b| b as char).collect();
        let mut t = Tokens::default();
        struct Pending {
            open: usize,
            tag: IoMemento,
        }
        let mut stack: Vec<Pending> = Vec::new();
        let mut in_quote = false;
        let mut escaped = false;

        let n = src.len();
        let mut index = 0usize;
        while index < n {
            let ch = chars[index];
            if in_quote {
                // An escaped character never closes the string — `\"` is a quote INSIDE the text.
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_quote = false;
                    if let Some(p) = stack.pop() {
                        t.add(p.open, index, p.tag);
                    }
                }
            } else {
                match ch {
                    '{' => stack.push(Pending {
                        open: index,
                        tag: IoMemento::IoObject,
                    }),
                    '[' => stack.push(Pending {
                        open: index,
                        tag: IoMemento::IoArray,
                    }),
                    '}' | ']' => {
                        if let Some(p) = stack.pop() {
                            t.add(p.open, index, p.tag);
                        }
                    }
                    '"' => {
                        stack.push(Pending {
                            open: index,
                            tag: IoMemento::IoString,
                        });
                        in_quote = true;
                    }
                    't' if index + 3 < n
                        && chars[index + 1] == 'r'
                        && chars[index + 2] == 'u'
                        && chars[index + 3] == 'e' =>
                    {
                        t.add(index, index + 3, IoMemento::IoBoolean);
                        index += 3;
                    }
                    'f' if index + 4 < n
                        && chars[index + 1] == 'a'
                        && chars[index + 2] == 'l'
                        && chars[index + 3] == 's'
                        && chars[index + 4] == 'e' =>
                    {
                        t.add(index, index + 4, IoMemento::IoBoolean);
                        index += 4;
                    }
                    'n' if index + 3 < n
                        && chars[index + 1] == 'u'
                        && chars[index + 2] == 'l'
                        && chars[index + 3] == 'l' =>
                    {
                        t.add(index, index + 3, IoMemento::IoNothing);
                        index += 3;
                    }
                    '-' | '+' | '0'..='9' => {
                        let start = index;
                        while index < n {
                            let next = chars[index];
                            if !matches!(next, '0'..='9' | '.' | 'e' | 'E' | '+' | '-') {
                                break;
                            }
                            index += 1;
                        }
                        t.add(start, index - 1, IoMemento::IoDouble);
                        continue;
                    }
                    _ => {}
                }
            }
            index += 1;
        }
        // Unclosed containers close at end-of-source (Kotlin semantics).
        while let Some(p) = stack.pop() {
            t.add(p.open, n - 1, p.tag);
        }

        finish(Tokens {
            opens: t.opens,
            closes: t.closes,
            tags: t.tags,
        })
    }

    /// `Syntax.scanCbor0` — CBOR span scanner.
    ///
    /// # Panics
    /// On CBOR additional-info values outside 0..=31 (`error("cbor ai …")` in Kotlin).
    pub fn scan_cbor0(self, src: &[u8]) -> Scan0Result {
        let mut t = Tokens::default();

        fn read_length(src: &[u8], position: usize, ai: u8) -> (i64, usize) {
            match ai {
                0..=23 => (i64::from(ai), position),
                24 => (i64::from(src[position]), position + 1),
                25 => (
                    i64::from(src[position]) << 8 | i64::from(src[position + 1]),
                    position + 2,
                ),
                26 => (
                    i64::from(src[position]) << 24
                        | i64::from(src[position + 1]) << 16
                        | i64::from(src[position + 2]) << 8
                        | i64::from(src[position + 3]),
                    position + 4,
                ),
                27 => {
                    let mut value: i64 = 0;
                    for offset in 0..8 {
                        value = (value << 8) | i64::from(src[position + offset]);
                    }
                    (value, position + 8)
                }
                31 => (-1, position),
                _ => panic!("cbor ai {}", ai),
            }
        }

        fn parse_item(src: &[u8], position: usize, t: &mut Tokens) -> usize {
            let open = position;
            let initial = src[position] as usize;
            let major = initial >> 5;
            let ai = (initial & 0x1F) as u8;
            match major {
                0 | 1 => {
                    let (_, next) = read_length(src, position + 1, ai);
                    t.add(open, next - 1, IoMemento::IoLong);
                    next
                }
                2 => {
                    let (length, next) = read_length(src, position + 1, ai);
                    if length < 0 {
                        next
                    } else {
                        t.add(open, next + length as usize - 1, IoMemento::IoBytes);
                        next + length as usize
                    }
                }
                3 => {
                    let (length, next) = read_length(src, position + 1, ai);
                    if length < 0 {
                        next
                    } else {
                        t.add(open, next + length as usize - 1, IoMemento::IoString);
                        next + length as usize
                    }
                }
                4 | 5 => {
                    let (length, next) = read_length(src, position + 1, ai);
                    let mut cursor = next;
                    if length < 0 {
                        while cursor < src.len() && src[cursor] != 0xFF {
                            cursor = parse_item(src, cursor, t);
                            if major == 5 {
                                cursor = parse_item(src, cursor, t);
                            }
                        }
                    } else {
                        let count = if major == 5 {
                            length as usize * 2
                        } else {
                            length as usize
                        };
                        for _ in 0..count {
                            cursor = parse_item(src, cursor, t);
                        }
                    }
                    if cursor < src.len() && length < 0 {
                        cursor += 1;
                    }
                    t.add(
                        open,
                        cursor - 1,
                        if major == 4 {
                            IoMemento::IoArray
                        } else {
                            IoMemento::IoObject
                        },
                    );
                    cursor
                }
                6 => {
                    let (_, next) = read_length(src, position + 1, ai);
                    parse_item(src, next, t)
                }
                7 => {
                    let tag = match ai {
                        20 | 21 => IoMemento::IoBoolean,
                        22 | 23 => IoMemento::IoNothing,
                        25..=27 => IoMemento::IoDouble,
                        _ => IoMemento::IoNothing,
                    };
                    let size = match ai {
                        25 => 2usize,
                        26 => 4,
                        27 => 8,
                        24 => 1,
                        _ => 0,
                    };
                    t.add(open, open + size, tag);
                    position + 1 + size
                }
                _ => {
                    t.add(open, open, IoMemento::IoNothing);
                    position + 1
                }
            }
        }

        let mut position = 0usize;
        while position < src.len() {
            position = parse_item(src, position, &mut t);
        }
        finish(t)
    }

    /// `Syntax.scanYaml0` — YAML scanner.
    ///
    /// Input whose first non-whitespace char is `{`/`[` falls through to the JSON
    /// scanner (Kotlin does the same). Otherwise a line-oriented recursive walk
    /// (Kotlin walks `YamlParser`'s AST and maps line spans to byte offsets; both
    /// emit key/value/container tokens with byte spans).
    pub fn scan_yaml0(self, src: &[u8]) -> Scan0Result {
        let mut first_non_ws = None;
        for (i, &b) in src.iter().enumerate() {
            if !is_kt_whitespace(b as char) {
                first_non_ws = Some(i);
                break;
            }
        }
        if let Some(i) = first_non_ws {
            let c = src[i] as char;
            if c == '{' || c == '[' {
                return self.scan0(src);
            }
        }

        // Kotlin: CharArray(src.a){...}.concatToString() — a latin-1 style projection
        // of raw bytes to chars, NOT a UTF-8 decode.
        let text: String = src.iter().map(|&b| b as char).collect();
        let lines = split_lines(&text);
        let line_offsets = line_offsets(&lines, text.len());
        let mut t = Tokens::default();

        /// Meaningful (non-blank, non-comment) line info: (line index, indent, trim_start).
        fn meaningful<'a>(lines: &[&'a str], from: usize) -> Option<(usize, usize, &'a str)> {
            lines[from..].iter().enumerate().find_map(|(k, line)| {
                let trimmed = line.trim_start();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    None
                } else {
                    Some((k + from, line.len() - trimmed.len(), trimmed))
                }
            })
        }

        /// Parse a block starting at line `li` with indent `indent`; emits the
        /// container token first, then children, mirroring DirectYamlParser.
        fn parse_block(
            lines: &[&str],
            line_offsets: &[usize],
            t: &mut Tokens,
            li: usize,
            indent: usize,
            text_len: usize,
        ) -> usize /* next unconsumed line */ {
            let Some((_, _, first)) = meaningful(lines, li) else {
                return li;
            };
            if first.starts_with('-') {
                parse_sequence(lines, line_offsets, t, li, indent, text_len)
            } else if find_colon(first).is_some() {
                parse_mapping(lines, line_offsets, t, li, indent, text_len)
            } else {
                // plain scalar block
                let (start, end) = scalar_span(lines, line_offsets, li, indent, text_len);
                t.add(start, end, classify_scalar(first.trim_end()));
                li + 1
            }
        }

        fn scalar_span(
            lines: &[&str],
            line_offsets: &[usize],
            li: usize,
            indent: usize,
            text_len: usize,
        ) -> (usize, usize) {
            // first meaningful line from li: value text after indent, trailing ws trimmed
            let (_, col, trimmed) = meaningful(lines, li).expect("caller checked");
            let start = line_offsets[li] + col;
            let _ = indent;
            let _ = text_len;
            let end = (start + trimmed.trim_end().len()).saturating_sub(1); // inclusive close
            (start.min(end), end)
        }

        fn parse_mapping(
            lines: &[&str],
            line_offsets: &[usize],
            t: &mut Tokens,
            li: usize,
            indent: usize,
            text_len: usize,
        ) -> usize {
            let mut i = li;
            // container open: from this line's first char to document end (patched at close)
            let open_tok = t.opens.len();
            t.add(
                line_offsets[i],
                text_len.saturating_sub(1),
                IoMemento::IoObject,
            );
            while let Some((lineno, col, trimmed)) = meaningful(lines, i) {
                if col < indent {
                    break;
                }
                if col > indent {
                    // Kotlin's parseMapping skips deeper lines into the void
                    i = lineno + 1;
                    continue;
                }
                let Some(colon) = find_colon(trimmed) else {
                    break;
                };
                i = lineno + 1;

                // key token: raw key text, quote-inclusive when quoted
                let key_raw = trimmed[..colon].trim();
                let key_start = line_offsets[lineno] + col;
                let key_end = key_start + key_raw.len().saturating_sub(1);
                t.add(key_start, key_end, IoMemento::IoString);

                let val_raw = trimmed[colon + 1..].trim();
                if val_raw.is_empty() {
                    // nested block under this key, else null
                    match meaningful(lines, i) {
                        Some((nl, ncol, _)) if ncol > col => {
                            let nested = t.opens.len();
                            let kind = if meaningful(lines, nl)
                                .map(|(_, _, tt)| tt.starts_with('-'))
                                .unwrap_or(false)
                            {
                                IoMemento::IoArray
                            } else {
                                IoMemento::IoObject
                            };
                            t.add(line_offsets[nl], text_len.saturating_sub(1), kind);
                            let next = parse_block(lines, line_offsets, t, nl, ncol, text_len);
                            t.closes[nested] =
                                last_consumed_end(lines, line_offsets, next, text_len);
                            i = next;
                        }
                        _ => {
                            t.add(key_start, key_start, IoMemento::IoNothing);
                        }
                    }
                } else {
                    let vs = line_offsets[lineno] + col + (trimmed.len() - val_raw.len());
                    let ve = line_offsets[lineno] + trimmed.trim_end().len().saturating_sub(1);
                    emit_scalar(t, vs, ve, val_raw);
                }
            }
            t.closes[open_tok] = last_consumed_end(lines, line_offsets, i, text_len);
            i
        }

        fn parse_sequence(
            lines: &[&str],
            line_offsets: &[usize],
            t: &mut Tokens,
            li: usize,
            indent: usize,
            text_len: usize,
        ) -> usize {
            let mut i = li;
            let open_tok = t.opens.len();
            t.add(
                line_offsets[i],
                text_len.saturating_sub(1),
                IoMemento::IoArray,
            );
            while let Some((lineno, col, trimmed)) = meaningful(lines, i) {
                if col < indent || !trimmed.starts_with('-') {
                    break;
                }
                i = lineno + 1;
                let rest = trimmed[1..].trim();
                if rest.is_empty() {
                    match meaningful(lines, i) {
                        Some((nl, ncol, _)) if ncol > col => {
                            let nested = t.opens.len();
                            let kind = if meaningful(lines, nl)
                                .map(|(_, _, tt)| tt.starts_with('-'))
                                .unwrap_or(false)
                            {
                                IoMemento::IoArray
                            } else {
                                IoMemento::IoObject
                            };
                            t.add(line_offsets[nl], text_len.saturating_sub(1), kind);
                            let next = parse_block(lines, line_offsets, t, nl, ncol, text_len);
                            t.closes[nested] =
                                last_consumed_end(lines, line_offsets, next, text_len);
                            i = next;
                        }
                        _ => {
                            let off = line_offsets[lineno];
                            t.add(off, off, IoMemento::IoNothing);
                        }
                    }
                } else if find_colon(rest).is_some() {
                    // "- key: value" — an inline mapping item; treat as nested mapping
                    let item_indent = col + (trimmed.len() - trimmed[1..].trim_start().len());
                    let nested = t.opens.len();
                    t.add(
                        line_offsets[lineno],
                        text_len.saturating_sub(1),
                        IoMemento::IoObject,
                    );
                    let next = parse_mapping(lines, line_offsets, t, lineno, item_indent, text_len);
                    t.closes[nested] = last_consumed_end(lines, line_offsets, next, text_len);
                    i = next;
                } else {
                    let item_start = line_offsets[lineno]
                        + col
                        + (trimmed.len() - trimmed[1..].trim_start().len());
                    let ve = line_offsets[lineno] + trimmed.trim_end().len().saturating_sub(1);
                    emit_scalar(t, item_start, ve, rest);
                }
            }
            t.closes[open_tok] = last_consumed_end(lines, line_offsets, i, text_len);
            i
        }

        if let Some((li, col, _)) = meaningful(&lines, 0) {
            parse_block(&lines, &line_offsets, &mut t, li, col, text.len());
        }

        finish(t)
    }

    /// `Syntax.scanIndex` — scan + payload bounds check + faceted index.
    ///
    /// # Panics
    /// When a produced span exceeds source bounds (Kotlin `require`): this is how
    /// truncated CBOR payloads fail before derived facets are requested.
    pub fn scan_index(self, src: &[u8]) -> ConfixIndex {
        let mut flat = match self {
            Syntax::Cbor => self.scan_cbor0(src).flat,
            Syntax::Yaml => self.scan_yaml0(src).flat,
            _ => self.scan0(src).flat,
        };
        // Payload bounds must be checked even when neither derived facet is requested.
        for (i, &(a, b)) in flat.spans.iter().enumerate() {
            // Kotlin: require(span.b < span.a || (span.a >= 0 && span.b < src.size))
            if !(b < a || b < src.len()) {
                panic!("Token {} exceeds source bounds: {}..{}", i, a, b);
            }
        }
        flat.spans.shrink_to_fit();
        ConfixIndex {
            syntax: self,
            flat,
            src: src.to_vec(),
            tree: OnceLock::new(),
            key_to_child: OnceLock::new(),
            structural_nodes: OnceLock::new(),
        }
    }
}

/// End offset (inclusive) of the last consumed line before `next_li`, or doc end.
fn last_consumed_end(
    lines: &[&str],
    line_offsets: &[usize],
    next_li: usize,
    text_len: usize,
) -> usize {
    if next_li == 0 {
        return 0;
    }
    let li = next_li - 1;
    line_offsets
        .get(li)
        .map(|&off| off + lines[li].trim_end().len().saturating_sub(1))
        .unwrap_or(text_len.saturating_sub(1))
}

/// Emit a scalar token with the scanner's string/number/bool/null classification.
/// String scalars include their surrounding quotes (Kotlin vStartAdj/vEndAdj).
fn emit_scalar(t: &mut Tokens, start: usize, end: usize, raw: &str) {
    let trimmed = raw.trim();
    let tag = if trimmed == "true" || trimmed == "false" {
        IoMemento::IoBoolean
    } else if trimmed == "null" || trimmed == "~" {
        IoMemento::IoNothing
    } else if trimmed.parse::<f64>().is_ok() {
        IoMemento::IoDouble
    } else {
        IoMemento::IoString
    };
    let (open, close) = if tag == IoMemento::IoString {
        // widen to include quotes when present
        if raw.starts_with('"') {
            (start.saturating_sub(1), end + 1)
        } else {
            (start, end)
        }
    } else {
        (start, end)
    };
    t.add(open, close, tag);
}

fn classify_scalar(_raw: &str) -> IoMemento {
    IoMemento::IoString
}

fn is_kt_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r')
}

fn find_colon(s: &str) -> Option<usize> {
    // Mirrors DirectYamlParser.findColon: colon outside quotes, followed by ws/EOL.
    let mut inside_quote = false;
    let mut quote_char = '\0';
    for (i, c) in s.char_indices() {
        if inside_quote {
            if c == quote_char {
                inside_quote = false;
            }
        } else if c == '"' || c == '\'' {
            inside_quote = true;
            quote_char = c;
        } else if c == ':' {
            let next = s[i + 1..].chars().next();
            if next.is_none() || next.is_some_and(char::is_whitespace) {
                return Some(i);
            }
        }
    }
    None
}

fn split_lines(text: &str) -> Vec<&str> {
    // Kotlin String.lines(): split on '\n', keeping a trailing empty segment.
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            out.push(&text[start..i]);
            start = i + 1;
        }
    }
    out.push(&text[start..]);
    out
}

fn line_offsets(lines: &[&str], total: usize) -> Vec<usize> {
    let mut offs = Vec::with_capacity(lines.len() + 1);
    let mut offset = 0usize;
    for line in lines {
        offs.push(offset);
        offset += line.len() + 1;
    }
    offs.push(total);
    offs
}

// ── buildTree / flat geometry ───────────────────────────────────────────────

fn build_flat(t: Tokens) -> FlatIndex {
    let total = t.opens.len();
    let mut source_order: Vec<usize> = (0..total).collect();
    source_order.sort_by_key(|&i| t.opens[i]);
    let spans: Vec<(usize, usize)> = source_order
        .iter()
        .map(|&i| (t.opens[i], t.closes[i]))
        .collect();
    let tags: Vec<IoMemento> = source_order.iter().map(|&i| t.tags[i]).collect();
    let depths: Vec<usize> = spans
        .iter()
        .enumerate()
        .map(|(index, span)| {
            (0..total)
                .filter(|&other| {
                    other != index && spans[other].0 < span.0 && spans[other].1 >= span.1
                })
                .count()
        })
        .collect();
    FlatIndex {
        spans,
        tags,
        depths,
    }
}

/// Kotlin `buildTree`: assemble the recursive RowVec tree plus flat geometry.
fn finish(t: Tokens) -> Scan0Result {
    let flat = build_flat(Tokens {
        opens: t.opens,
        closes: t.closes,
        tags: t.tags,
    });
    let total = flat.spans.len();
    if total == 0 {
        return Scan0Result {
            tree: Vec::new(),
            flat,
        };
    }
    // Parent assignment via a stack over source-ordered spans (equivalent to the
    // Kotlin depth-count childOf filter, O(n) instead of O(n²)).
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); total];
    let mut roots: Vec<usize> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for i in 0..total {
        while let Some(&top) = stack.last() {
            let (ta, tb) = flat.spans[top];
            let (a, b) = flat.spans[i];
            if ta < a && b <= tb {
                break;
            }
            stack.pop();
        }
        match stack.last() {
            Some(&parent) => children[parent].push(i),
            None => roots.push(i),
        }
        stack.push(i);
    }

    fn row(i: usize, children: &[Vec<usize>], flat: &FlatIndex) -> RowVec {
        let (a, b) = flat.spans[i];
        RowVec {
            open: a,
            close: b,
            tag: flat.tags[i],
            kids: children[i]
                .iter()
                .map(|&c| row(c, children, flat))
                .collect(),
        }
    }

    let tree: Vec<RowVec> = roots.iter().map(|&r| row(r, &children, &flat)).collect();
    Scan0Result { tree, flat }
}

impl ConfixIndex {
    /// The syntax this index was scanned with.
    pub fn syntax(&self) -> Syntax {
        self.syntax
    }

    /// Source bytes captured at scan time.
    pub fn src(&self) -> &[u8] {
        &self.src
    }

    /// `Spans` facet.
    pub fn spans(&self) -> &[(usize, usize)] {
        &self.flat.spans
    }

    /// `Tags` facet.
    pub fn tags(&self) -> &[IoMemento] {
        &self.flat.tags
    }

    /// `Depths` facet.
    pub fn depths(&self) -> &[usize] {
        &self.flat.depths
    }

    /// `DirectChildren` facet: source-ordered child token indices of `parent`
    /// (Kotlin `childOf`: strictly inside the parent span at depth+1).
    pub fn direct_children(&self, parent: usize) -> Vec<usize> {
        let total = self.flat.spans.len();
        if parent >= total {
            return Vec::new();
        }
        let parent_span = self.flat.spans[parent];
        let child_depth = self.flat.depths[parent] + 1;
        (0..total)
            .filter(|&candidate| {
                candidate != parent
                    && self.flat.spans[candidate].0 > parent_span.0
                    && self.flat.spans[candidate].1 <= parent_span.1
                    && self.flat.depths[candidate] == child_depth
            })
            .collect()
    }

    /// `TreeCursor` facet (lazy; built on first access).
    pub fn tree(&self) -> &[RowVec] {
        self.tree.get_or_init(|| {
            finish(Tokens {
                opens: self.flat.spans.iter().map(|s| s.0).collect(),
                closes: self.flat.spans.iter().map(|s| s.1).collect(),
                tags: self.flat.tags.to_vec(),
            })
            .tree
        })
    }

    /// Introspection: has the lazy key index been built? (Test parity with Kotlin's
    /// read-counting laziness probes.)
    pub fn key_index_initialized(&self) -> bool {
        self.key_to_child.get().is_some()
    }

    /// Introspection: have the structural hashes been built?
    pub fn structural_nodes_initialized(&self) -> bool {
        self.structural_nodes.get().is_some()
    }

    /// `KeyToChild` facet (lazy): first token index whose decoded string equals `key`.
    pub fn key_to_child(&self, key: &str) -> Option<usize> {
        self.key_to_child
            .get_or_init(|| self.build_key_index())
            .get(key)
            .copied()
    }

    fn build_key_index(&self) -> HashMap<String, usize> {
        let mut keys = HashMap::new();
        for (index, tag) in self.flat.tags.iter().enumerate() {
            if *tag != IoMemento::IoString {
                continue;
            }
            let span = self.flat.spans[index];
            let key = if self.syntax == Syntax::Cbor {
                match decode_cbor_text(&self.src, span.0) {
                    Some(k) => k,
                    None => continue,
                }
            } else {
                if span.1 == 0 {
                    continue;
                }
                let open = span.0 + 1;
                let close = span.1 - 1;
                if close < open {
                    continue;
                }
                decode_text_span(&self.src, open, close)
            };
            keys.entry(key).or_insert(index);
        }
        keys
    }

    /// `StructuralNodes` facet (lazy): content-addressed IDs, bottom-up.
    pub fn structural_nodes(&self) -> &[Option<String>] {
        self.structural_nodes
            .get_or_init(|| self.build_structural_nodes())
    }

    fn build_structural_nodes(&self) -> Vec<Option<String>> {
        let total = self.flat.spans.len();
        let mut cids: Vec<Option<String>> = vec![None; total];
        for i in (0..total).rev() {
            let children = self.direct_children(i);
            if self.flat.tags[i] == IoMemento::IoObject
                || self.flat.tags[i] == IoMemento::IoArray
                || !children.is_empty()
            {
                let mut hashed = String::from("node:\n");
                for child in children {
                    hashed.push_str(cids[child].as_deref().unwrap_or(""));
                    hashed.push('\n');
                }
                cids[i] = Some(content_id(hashed.as_bytes()));
            } else {
                let span = self.flat.spans[i];
                let length = span.1.saturating_sub(span.0) + 1;
                let bytes: Vec<u8> = (0..length)
                    .filter_map(|o| self.src.get(span.0 + o).copied())
                    .collect();
                cids[i] = Some(content_id(&bytes));
            }
        }
        cids
    }

    /// `ConfixIndex.valueIndexFor(keyTokenIdx)`: next token at the same depth.
    pub fn value_index_for(&self, key_token_idx: usize) -> Option<usize> {
        let depths = &self.flat.depths;
        let d = *depths.get(key_token_idx)?;
        (key_token_idx + 1..depths.len()).find(|&i| depths[i] == d)
    }

    /// `ConfixIndex.resolve(key)`: key → value token index.
    pub fn resolve_key(&self, key: &str) -> Option<usize> {
        self.key_to_child(key).and_then(|k| self.value_index_for(k))
    }

    /// `ConfixIndex.resolve(parent, arrayIdx)`.
    pub fn resolve_index(&self, parent_token_idx: usize, array_idx: usize) -> Option<usize> {
        self.direct_children(parent_token_idx)
            .get(array_idx)
            .copied()
    }

    /// `ConfixDoc.reify(tokenIdx)`.
    pub fn reify_token(&self, token_idx: usize) -> Option<Value> {
        self.tree().get(token_idx).map(|row| row.reify(&self.src))
    }
}

// ── ConfixDoc ───────────────────────────────────────────────────────────────

/// `confixDoc(bytes, syntax)` — Kotlin entry point.
pub fn confix_doc(bytes: &[u8], syntax: Syntax) -> ConfixDoc {
    ConfixDoc {
        index: syntax.scan_index(bytes),
        src: bytes.to_vec(),
    }
}

/// `confixDoc(text)` — auto-detect JSON vs YAML from the first non-blank char.
pub fn confix_doc_text(text: &str) -> ConfixDoc {
    let bytes = text.as_bytes();
    let syntax = match text.trim_start().chars().next() {
        Some('{') | Some('[') | Some('"') => Syntax::Json,
        _ => Syntax::Yaml,
    };
    confix_doc(bytes, syntax)
}

/// `scan(bytes, syntax)` — index-only entry. Kotlin `scan(bytes, syntax) = confixDoc(bytes, syntax).index`.
pub fn scan(bytes: &[u8], syntax: Syntax) -> ConfixIndex {
    syntax.scan_index(bytes)
}

impl ConfixDoc {
    /// `ConfixDoc.src`
    pub fn src(&self) -> &[u8] {
        &self.src
    }

    /// `ConfixDoc.roots`
    pub fn roots(&self) -> &[RowVec] {
        self.index.tree()
    }

    /// `ConfixDoc.root`
    pub fn root(&self) -> Option<&RowVec> {
        self.roots().first()
    }

    /// `ConfixDoc.getAt(*path)`
    pub fn get_at(&self, path: &[PathStep]) -> Option<&RowVec> {
        self.root().and_then(|r| r.get_at(path, &self.src))
    }

    /// `ConfixDoc.scalar(*path)` — navigate and reify.
    pub fn scalar(&self, path: &[PathStep]) -> Option<Value> {
        self.get_at(path).map(|r| r.reify(&self.src))
    }

    /// `ConfixDoc.value(*path)` — alias of scalar (Kotlin `value` via docAt).
    pub fn value(&self, path: &[PathStep]) -> Option<Value> {
        self.scalar(path)
    }

    /// `ConfixDoc.reify(tokenIdx)`
    pub fn reify_token(&self, token_idx: usize) -> Option<Value> {
        self.index.reify_token(token_idx)
    }
}

/// A path step: key or index (Kotlin passes `Any` path vars).
#[derive(Clone, Debug, PartialEq)]
pub enum PathStep {
    Key(String),
    Index(usize),
}

/// Reified scalar value (Kotlin reify returns `Any?`).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Long(i64),
    Double(f64),
    Text(String),
    Bytes(Vec<u8>),
    /// Containers reify to their kids Cursor in Kotlin; we expose the child count.
    Kids(usize),
}

impl RowVec {
    /// Child rows (Kotlin `RowVec.kids`).
    pub fn kids(&self) -> &[RowVec] {
        &self.kids
    }

    /// Token type tag (Kotlin `RowVec.tag`).
    pub fn tag(&self) -> IoMemento {
        self.tag
    }

    /// `RowVec.step(key, src)` — object lookup over flat (key, value) kid pairs.
    pub fn step_key<'a>(&'a self, key: &str, src: &'a [u8]) -> Option<&'a RowVec> {
        let ch = &self.kids;
        let mut i = 0usize;
        while i + 1 < ch.len() {
            let k = &ch[i];
            let v = &ch[i + 1];
            if k.tag == IoMemento::IoString && k.close >= 1 {
                let k_open = k.open + 1;
                let k_close = k.close - 1;
                if k_close >= k_open {
                    let k_len = k_close - k_open + 1;
                    if k_len == key.len() {
                        let mut matched = true;
                        for (d, kb) in key.as_bytes().iter().enumerate() {
                            if src.get(k_open + d).copied().unwrap_or(0) != *kb {
                                matched = false;
                                break;
                            }
                        }
                        if matched {
                            return Some(v);
                        }
                    }
                }
            }
            i += 2;
        }
        None
    }

    /// `RowVec.step(idx)` — array indexing.
    pub fn step_index(&self, array_idx: usize) -> Option<&RowVec> {
        self.kids.get(array_idx)
    }

    /// `RowVec.getAt(*path, src)`
    pub fn get_at<'a>(&'a self, path: &[PathStep], src: &'a [u8]) -> Option<&'a RowVec> {
        let mut cur: Option<&RowVec> = Some(self);
        for step in path {
            cur = match step {
                PathStep::Key(k) => cur.and_then(|c| c.step_key(k, src)),
                PathStep::Index(i) => cur.and_then(|c| c.step_index(*i)),
            };
            cur?;
        }
        cur
    }

    /// `RowVec.reify(src)` — decode the token the row spans.
    pub fn reify(&self, src: &[u8]) -> Value {
        match self.tag {
            IoMemento::IoNothing => Value::Null,
            IoMemento::IoBoolean => {
                let b = src.get(self.open).copied().unwrap_or(0);
                match u16::from(b) {
                    0xF4 => Value::Bool(false),
                    0xF5 => Value::Bool(true),
                    _ => Value::Bool(b as char == 't'),
                }
            }
            IoMemento::IoDouble => match decode_cbor_float(src, self.open) {
                Some(f) => Value::Double(f),
                None => match span_str(src, self.open, self.close).parse::<f64>() {
                    Ok(f) => Value::Double(f),
                    Err(_) => Value::Null,
                },
            },
            IoMemento::IoInt => Value::Long(span_long(src, self.open, self.close)),
            IoMemento::IoLong => Value::Long(
                decode_cbor_long(src, self.open)
                    .unwrap_or_else(|| span_long(src, self.open, self.close)),
            ),
            IoMemento::IoString => {
                Value::Text(decode_cbor_text(src, self.open).unwrap_or_else(|| {
                    decode_text_span(src, self.open + 1, self.close.saturating_sub(1))
                }))
            }
            IoMemento::IoBytes => {
                Value::Bytes(decode_cbor_bytes(src, self.open).unwrap_or_else(|| {
                    let end = self.close.min(src.len().saturating_sub(1));
                    src[self.open..=end.min(self.open)].to_vec()
                }))
            }
            IoMemento::IoObject | IoMemento::IoArray => Value::Kids(self.kids.len()),
        }
    }
}

// ── decoders (ConfixKit.kt internals) ───────────────────────────────────────

struct CborHead {
    major: u8,
    value: u64,
    payload_open: usize,
}

fn cbor_head(src: &[u8], open: usize) -> Option<CborHead> {
    if open >= src.len() {
        return None;
    }
    let initial = src[open] as usize;
    let major = (initial >> 5) as u8;
    let additional = initial & 0x1F;
    let mut cursor = open + 1;
    let value: u64 = match additional {
        0..=23 => additional as u64,
        24 => {
            let v = u64::from(*src.get(cursor)?);
            cursor += 1;
            v
        }
        25 => {
            let v = u64::from(*src.get(cursor)?) << 8 | u64::from(*src.get(cursor + 1)?);
            cursor += 2;
            v
        }
        26 => {
            let mut result: u64 = 0;
            for k in 0..4 {
                result = result << 8 | u64::from(*src.get(cursor + k)?);
            }
            cursor += 4;
            result
        }
        27 => {
            let mut result: u64 = 0;
            for k in 0..8 {
                result = result << 8 | u64::from(*src.get(cursor + k)?);
            }
            cursor += 8;
            result
        }
        _ => return None,
    };
    Some(CborHead {
        major,
        value,
        payload_open: cursor,
    })
}

pub(crate) fn decode_cbor_text(src: &[u8], open: usize) -> Option<String> {
    let head = cbor_head(src, open)?;
    if head.major != 3 {
        return None;
    }
    let len = usize::try_from(head.value).ok()?;
    let mut out = String::with_capacity(len);
    for offset in 0..len {
        out.push(*src.get(head.payload_open + offset)? as char);
    }
    Some(out)
}

fn decode_cbor_bytes(src: &[u8], open: usize) -> Option<Vec<u8>> {
    let head = cbor_head(src, open)?;
    if head.major != 2 {
        return None;
    }
    let len = usize::try_from(head.value).ok()?;
    let mut out = Vec::with_capacity(len);
    for offset in 0..len {
        out.push(*src.get(head.payload_open + offset)?);
    }
    Some(out)
}

fn decode_cbor_long(src: &[u8], open: usize) -> Option<i64> {
    let head = cbor_head(src, open)?;
    match head.major {
        0 => i64::try_from(head.value).ok(),
        1 => i64::try_from(head.value).ok().map(|v| -1 - v),
        _ => None,
    }
}

fn decode_cbor_float(src: &[u8], open: usize) -> Option<f64> {
    let initial = *src.get(open)? as usize;
    if initial >> 5 != 7 {
        return None;
    }
    match initial & 0x1F {
        26 => {
            let mut bits: u32 = 0;
            for offset in 0..4 {
                bits = bits << 8 | u32::from(*src.get(open + 1 + offset)?);
            }
            Some(f64::from(f32::from_bits(bits)))
        }
        27 => {
            let mut bits: u64 = 0;
            for offset in 0..8 {
                bits = bits << 8 | u64::from(*src.get(open + 1 + offset)?);
            }
            Some(f64::from_bits(bits))
        }
        _ => None,
    }
}

/// The text between a JSON/YAML string's quotes with escapes resolved.
pub(crate) fn decode_text_span(src: &[u8], open: usize, close: usize) -> String {
    if close < open || open >= src.len() {
        return String::new();
    }
    let end = close.min(src.len() - 1);
    let raw: Vec<u8> = src[open..=end].to_vec();
    // Kotlin decodes these bytes as UTF-8 (decodeToString).
    let raw_str = String::from_utf8_lossy(&raw).into_owned();
    if !raw_str.contains('\\') {
        return raw_str;
    }
    let chars: Vec<char> = raw_str.chars().collect();
    let mut out = String::with_capacity(raw_str.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c != '\\' || i + 1 >= chars.len() {
            out.push(c);
            i += 1;
            continue;
        }
        match chars[i + 1] {
            '"' | '\\' | '/' => {
                out.push(chars[i + 1]);
                i += 2;
            }
            'n' => {
                out.push('\n');
                i += 2;
            }
            't' => {
                out.push('\t');
                i += 2;
            }
            'r' => {
                out.push('\r');
                i += 2;
            }
            'b' => {
                out.push('\u{0008}');
                i += 2;
            }
            'f' => {
                out.push('\u{000C}');
                i += 2;
            }
            'u' => {
                let hex = if i + 5 < chars.len() {
                    u32::from_str_radix(&chars[i + 2..i + 6].iter().collect::<String>(), 16).ok()
                } else {
                    None
                };
                match hex {
                    Some(h) => {
                        out.push(char::from_u32(h).unwrap_or('\u{FFFD}'));
                        i += 6;
                    }
                    None => {
                        out.push('\\');
                        i += 1;
                    }
                }
            }
            _ => {
                out.push('\\');
                i += 1;
            }
        }
    }
    out
}

fn span_str(src: &[u8], open: usize, close: usize) -> String {
    if close < open || open >= src.len() {
        return String::new();
    }
    let end = close.min(src.len() - 1);
    src[open..=end].iter().map(|&b| b as char).collect()
}

fn span_long(src: &[u8], open: usize, close: usize) -> i64 {
    let mut v: i64 = 0;
    let mut neg = false;
    let mut i = open;
    if i <= close && src.get(i).copied().unwrap_or(b'-') == b'-' {
        neg = true;
        i += 1;
    }
    while i <= close {
        let b = src.get(i).copied().unwrap_or(b'0');
        v = v
            .wrapping_mul(10)
            .wrapping_add(i64::from(b.wrapping_sub(b'0')));
        i += 1;
    }
    if neg {
        -v
    } else {
        v
    }
}

// ── ContentId (SHA-256) — port of TrikeShed ContentId ───────────────────────

/// `ContentId.of`: `"sha256:" + 64 lowercase hex chars`.
pub fn content_id(bytes: &[u8]) -> String {
    let digest = sha256(bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(71);
    out.push_str("sha256:");
    for b in digest {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xF) as usize] as char);
    }
    out
}

/// Pure Rust SHA-256 (FIPS 180-4) — mirrors TrikeShed's Sha256Pure.
fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}
