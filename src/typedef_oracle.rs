//! Typedef oracle — typedef collection + IS-A lattice.
//! Port of TrikeShed `parse/confix/TypeDefOracle.kt` + `cursor/TypeSubsumption.kt`.

use std::collections::{HashMap, HashSet, VecDeque};

/// `TypeToken` — a pool index identifying a type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TypeToken(pub usize);

/// `IsAEdge` — directed IS-A edge (sub → sup).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IsAEdge {
    pub sub: TypeToken,
    pub sup: TypeToken,
}

/// `TypeDefParam` — a type parameter on a typedef.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeDefParam {
    pub name: String,
    pub bound: Option<String>,
}

/// `TypeDefEntry` — a single typedef declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeDefEntry {
    pub name: String,
    pub referred_to_type: String,
    pub params: Vec<TypeDefParam>,
    pub source: String,
}

/// `IsALattice` — query algebra over IS-A edges.
#[derive(Clone, Debug, Default)]
pub struct IsALattice {
    pub edges: Vec<IsAEdge>,
}

impl IsALattice {
    pub fn new(edges: Vec<IsAEdge>) -> Self {
        IsALattice { edges }
    }

    /// Direct supertypes of `token` — single hop.
    pub fn direct_supers(&self, token: TypeToken) -> Vec<TypeToken> {
        self.edges
            .iter()
            .filter(|e| e.sub == token)
            .map(|e| e.sup)
            .collect()
    }

    /// Direct subtypes of `token` — single hop.
    pub fn direct_subs(&self, token: TypeToken) -> Vec<TypeToken> {
        self.edges
            .iter()
            .filter(|e| e.sup == token)
            .map(|e| e.sub)
            .collect()
    }

    /// Transitive supertype chain of `token`, BFS order (shallowest first),
    /// seed excluded (Kotlin `supertypes`).
    pub fn supertypes(&self, token: TypeToken, max_depth: usize) -> Vec<TypeToken> {
        let mut visited: HashSet<TypeToken> = HashSet::new();
        visited.insert(token);
        let mut order: Vec<TypeToken> = Vec::new();
        let mut frontier: VecDeque<TypeToken> = VecDeque::new();
        frontier.push_back(token);
        let mut depth = 0usize;
        while !frontier.is_empty() && depth < max_depth {
            let mut next = VecDeque::new();
            while let Some(cur) = frontier.pop_front() {
                for e in &self.edges {
                    if e.sub == cur && visited.insert(e.sup) {
                        next.push_back(e.sup);
                        order.push(e.sup);
                    }
                }
            }
            frontier = next;
            depth += 1;
        }
        order
    }

    /// Is `sub` a subtype of `sup` (transitively)? Reflexive.
    pub fn is_a(&self, sub: TypeToken, sup: TypeToken) -> bool {
        if sub == sup {
            return true;
        }
        // BFS reachability (Kotlin uses a memoized closure index; same relation).
        let mut visited: HashSet<TypeToken> = HashSet::new();
        visited.insert(sub);
        let mut frontier: VecDeque<TypeToken> = VecDeque::new();
        frontier.push_back(sub);
        while let Some(cur) = frontier.pop_front() {
            for e in &self.edges {
                if e.sub == cur {
                    if e.sup == sup {
                        return true;
                    }
                    if visited.insert(e.sup) {
                        frontier.push_back(e.sup);
                    }
                }
            }
        }
        false
    }
}

/// `TypeDefOracle` — collects typedef declarations and builds the lattice.
#[derive(Default)]
pub struct TypeDefOracle {
    entries: Vec<TypeDefEntry>,
    name_to_idx: HashMap<String, usize>,
    idx_to_name: Vec<String>,
    edges: Vec<IsAEdge>,
}

/// Built oracle row (Kotlin `TypeDefOracleRow` faceted row → plain struct).
#[derive(Clone, Debug)]
pub struct TypeDefOracleRow {
    pub entries: Vec<TypeDefEntry>,
    pub tokens: Vec<TypeToken>,
    pub names_by_token: Vec<String>,
    pub name_to_token: HashMap<String, TypeToken>,
    pub lattice: IsALattice,
    pub edge_count: usize,
}

impl TypeDefOracleRow {
    /// `o.byName(name)`
    pub fn by_name(&self, name: &str) -> Option<TypeToken> {
        self.name_to_token.get(name).copied()
    }

    /// `o.tdNames(token)`
    pub fn td_names(&self, token: TypeToken) -> String {
        self.names_by_token
            .get(token.0)
            .cloned()
            .unwrap_or_else(|| format!("TypeToken({})", token.0))
    }
}

impl TypeDefOracle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of collected entries.
    pub fn size(&self) -> usize {
        self.entries.len()
    }

    /// `parseTypeDefs`: scan `.x` / Kotlin source for typedef declarations.
    ///
    /// Handles both forms:
    /// - `typedef <referred> as <Name>;` / `typedef Join<A, B> as Tuple<A, B>;`
    /// - `typealias Name = Referred` (Kotlin, reversed order)
    pub fn parse_type_defs(&mut self, text: &str, source: &str) {
        for line in text.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("typedef ") {
                if let Some((referred, name)) = split_typedef(rest) {
                    // Kotlin parity: the typedef regex's optional `<([^>]+)>` after the
                    // name cannot span a nested '>' — e.g.
                    // `typedef Series2<A, B> as Series<Join<A, B>>;` does NOT match in
                    // Kotlin (the second '>' breaks the group), so it must not match here.
                    if !name_params_nested(name) {
                        let params_str = type_params_of(name);
                        self.add_entry(
                            &clean_name(name),
                            referred.trim(),
                            params_str.as_deref(),
                            source,
                        );
                    }
                }
            } else if let Some(rest) = t.strip_prefix("typealias ") {
                // typealias Name<T> = Referred
                if let Some((name, referred)) = rest.split_once('=') {
                    let name = name.trim();
                    let params_str = type_params_of(name);
                    self.add_entry(
                        &clean_name(name),
                        referred.trim(),
                        params_str.as_deref(),
                        source,
                    );
                }
            }
        }
    }

    /// `parseCBORTypeDefs`: walk a CBOR-scanned ConfixDoc for {name, referredTo} rows.
    pub fn parse_cbor_type_defs(&mut self, doc: &crate::core::ConfixDoc) {
        for row in doc.roots() {
            if row.tag != crate::core::IoMemento::IoObject {
                continue;
            }
            let kids = row.kids();
            let mut name: Option<String> = None;
            let mut referred: Option<String> = None;
            let mut params: Option<String> = None;
            let mut k = 0usize;
            while k + 1 < kids.len() {
                let key_row = &kids[k];
                let val_row = &kids[k + 1];
                if key_row.tag == crate::core::IoMemento::IoString {
                    if let crate::core::Value::Text(key_text) = key_row.reify(doc.src()) {
                        match key_text.as_str() {
                            "name" => name = reify_to_string(val_row.reify(doc.src())),
                            "referredTo" => referred = reify_to_string(val_row.reify(doc.src())),
                            "params" => params = reify_to_string(val_row.reify(doc.src())),
                            _ => {}
                        }
                    }
                }
                k += 2;
            }
            if let (Some(n), Some(r)) = (name, referred) {
                self.add_entry(&n, &r, params.as_deref(), "cbor");
            }
        }
    }

    /// `ingestOracleJson`: `{"rows":[{"kind":…,"ngram":"a -> b"}]}` link ingestion.
    pub fn ingest_oracle_json(&mut self, json_text: &str) {
        let doc = crate::core::confix_doc_text(json_text);
        // rows: root object member "rows" → array
        let rows_token = doc.index.resolve_key("rows");
        let rows = rows_token.and_then(|ri| {
            doc.index.tree().get(ri).map(|arr| {
                arr.kids().iter().filter_map(|obj| {
                    let kind =
                        obj.step_key("kind", doc.src())
                            .and_then(|r| match r.reify(doc.src()) {
                                crate::core::Value::Text(t) => Some(t),
                                _ => None,
                            })?;
                    let ngram = obj.step_key("ngram", doc.src()).and_then(|r| {
                        match r.reify(doc.src()) {
                            crate::core::Value::Text(t) => Some(t),
                            _ => None,
                        }
                    })?;
                    Some((kind, ngram))
                })
            })
        });
        let Some(rows) = rows else { return };

        for (kind, ngram) in rows {
            match kind.as_str() {
                "ngram" | "factory_step" => {
                    let parts: Vec<&str> = ngram.split(" -> ").map(str::trim).collect();
                    for pair in parts.windows(2) {
                        if !pair[0].is_empty() && !pair[1].is_empty() {
                            self.add_link_check(pair[0], pair[1]);
                        }
                    }
                }
                "isA_edge" => {
                    let parts: Vec<&str> = ngram.split(" -> ").map(str::trim).collect();
                    if parts.len() >= 2 {
                        self.add_link_check(parts[0], parts[1]);
                    }
                }
                "topic" => {
                    // topic:<name> as <TType>
                    if let Some((name, ttype)) = parse_topic(&ngram) {
                        self.add_entry(&ttype, &name, None, "lda-topic");
                    }
                }
                _ => {}
            }
        }
    }

    /// `addLinkCheck`.
    pub fn add_link_check(&mut self, sub: &str, sup: &str) {
        let sub_token = self.ensure_token(sub);
        let sup_token = self.ensure_token(sup);
        self.edges.push(IsAEdge {
            sub: sub_token,
            sup: sup_token,
        });
    }

    /// `build`: freeze into the read-only row + lattice.
    pub fn build(&mut self) -> TypeDefOracleRow {
        let snapshot = self.entries.clone();
        for entry in &snapshot {
            let name_token = self.ensure_token(&entry.name);
            for referred in extract_base_names(&entry.referred_to_type) {
                let referred_token = self.ensure_token(&referred);
                if name_token != referred_token {
                    self.edges.push(IsAEdge {
                        sub: name_token,
                        sup: referred_token,
                    });
                }
            }
        }

        let entries = self.entries.clone();
        let idx_to_name = self.idx_to_name.clone();
        let name_to_idx = self.name_to_idx.clone();
        let edges = self.edges.clone();
        let tokens: Vec<TypeToken> = (0..idx_to_name.len()).map(TypeToken).collect();
        TypeDefOracleRow {
            edge_count: edges.len(),
            lattice: IsALattice::new(edges),
            tokens,
            names_by_token: idx_to_name,
            name_to_token: name_to_idx
                .into_iter()
                .map(|(k, v)| (k, TypeToken(v)))
                .collect(),
            entries,
        }
    }

    fn add_entry(&mut self, name: &str, referred_to: &str, params_str: Option<&str>, source: &str) {
        let params = parse_params(params_str);
        self.entries.push(TypeDefEntry {
            name: name.to_string(),
            referred_to_type: referred_to.to_string(),
            params,
            source: source.to_string(),
        });
        self.ensure_token(name);
    }

    fn ensure_token(&mut self, name: &str) -> TypeToken {
        let next = self.idx_to_name.len();
        let idx = *self.name_to_idx.entry(name.to_string()).or_insert(next);
        if idx == next {
            self.idx_to_name.push(name.to_string());
        }
        TypeToken(idx)
    }
}

fn reify_to_string(v: crate::core::Value) -> Option<String> {
    match v {
        crate::core::Value::Text(t) => Some(t),
        crate::core::Value::Long(l) => Some(l.to_string()),
        crate::core::Value::Double(d) => Some(d.to_string()),
        crate::core::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Split `Referred as Name;` → (referred, name).
fn split_typedef(rest: &str) -> Option<(&str, &str)> {
    let rest = rest.trim_end_matches(';').trim();
    // find top-level " as "
    let mut depth = 0usize;
    let bytes = rest.as_bytes();
    for i in 0..rest.len() {
        match bytes[i] {
            b'<' => depth += 1,
            b'>' => depth = depth.saturating_sub(1),
            _ => {
                if depth == 0 && rest[i..].starts_with(" as ") {
                    return Some((&rest[..i], &rest[i + 4..]));
                }
            }
        }
    }
    None
}

/// True when the name's `<...>` params section closes early or leaves trailing
/// junk — the Kotlin regex `<([^>]+)>` + `\s*;` rejects nested closing angles
/// (`Series<Join<A, B>>`) and unterminated ones alike.
fn name_params_nested(name: &str) -> bool {
    let Some(start) = name.find('<') else {
        return false;
    };
    let rest = &name[start + 1..];
    match rest.find('>') {
        None => true, // unterminated '<' — no regex match possible in Kotlin either
        Some(close) => {
            let after = rest[close + 1..].trim();
            !after.is_empty()
        }
    }
}

/// Extract `<A, B>` param string from a name like `Tuple<A, B>`.
fn type_params_of(name: &str) -> Option<String> {
    let start = name.find('<')?;
    let end = name.rfind('>')?;
    Some(name[start + 1..end].to_string())
}

/// Strip generics from a name: `Tuple<A, B>` → `Tuple`.
fn clean_name(name: &str) -> String {
    match name.find('<') {
        Some(i) => name[..i].trim().to_string(),
        None => name.trim().to_string(),
    }
}

/// `topic:(\w+)\s+as\s+(\w+)` → (name, ttype)
fn parse_topic(ngram: &str) -> Option<(String, String)> {
    let rest = ngram.strip_prefix("topic:")?;
    let (name, ttype) = rest.split_once(" as ")?;
    let name = name.trim();
    let ttype = ttype.trim();
    if name.is_empty() || ttype.is_empty() {
        return None;
    }
    Some((name.to_string(), ttype.to_string()))
}

fn parse_params(params_str: Option<&str>) -> Vec<TypeDefParam> {
    let Some(s) = params_str else {
        return Vec::new();
    };
    if s.trim().is_empty() {
        return Vec::new();
    }
    s.split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once(':') {
            Some((n, b)) => TypeDefParam {
                name: n.trim().to_string(),
                bound: Some(b.trim().to_string()),
            },
            None => TypeDefParam {
                name: p.to_string(),
                bound: None,
            },
        })
        .collect()
}

/// `extractBaseNames`: strip `<…>` and `(…)`, split on `|,` and whitespace, keep
/// parts starting with a letter.
fn extract_base_names(type_expr: &str) -> Vec<String> {
    let mut cleaned = String::with_capacity(type_expr.len());
    let mut depth = 0usize;
    for c in type_expr.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => cleaned.push(c),
            _ => {}
        }
    }
    cleaned
        .split(|c: char| c == '|' || c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty() && s.chars().next().is_some_and(char::is_alphabetic))
        .map(str::trim)
        .map(str::to_string)
        .collect()
}
