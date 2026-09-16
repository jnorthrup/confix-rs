//! `Item` — the shared metamodel for JSON / YAML / CBOR, and the canonical
//! RFC 8949 CBOR codec. Port of TrikeShed `collections/associative/Item.kt` + `Cbor.kt`.
//!
//! Canonical rules: map keys sorted by their encoded byte sequences before encoding;
//! minimal-width integer heads (0..23 inline, then 8/16/32/64-bit).

/// Shared metamodel node.
#[derive(Clone, Debug)]
pub enum Item {
    /// CBOR text string (major 3) / JSON string.
    Str(String),
    /// CBOR byte string (major 2).
    Bin(Vec<u8>),
    /// Signed integer (major 0/1).
    Num(i64),
    /// Float, always encoded as f64 (major 7, additional 27).
    Flt(f64),
    /// true/false (0xF5/0xF4).
    Bool(bool),
    /// null (0xF6).
    Nil,
    /// Ordered key/value list. Encoding sorts entries by encoded key bytes;
    /// equality/hashing are order-independent (Kotlin Item.Map semantics).
    Map(Vec<(String, Item)>),
    /// Array.
    Arr(Vec<Item>),
    /// CBOR tag (major 6).
    Tag(u64, Box<Item>),
}

impl PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Item::Str(a), Item::Str(b)) => a == b,
            (Item::Bin(a), Item::Bin(b)) => a == b,
            (Item::Num(a), Item::Num(b)) => a == b,
            (Item::Flt(a), Item::Flt(b)) => a == b,
            (Item::Bool(a), Item::Bool(b)) => a == b,
            (Item::Nil, Item::Nil) => true,
            (Item::Arr(a), Item::Arr(b)) => a == b,
            (Item::Tag(ta, ia), Item::Tag(tb, ib)) => ta == tb && ia == ib,
            (Item::Map(a), Item::Map(b)) => {
                if a.len() != b.len() {
                    return false;
                }
                // Kotlin Item.Map.equals: every (k,v) in self matches by key lookup.
                a.iter()
                    .all(|(k, v)| b.iter().any(|(k2, v2)| k2 == k && v2 == v))
            }
            _ => false,
        }
    }
}

impl Item {
    pub fn map_get(&self, key: &str) -> Option<&Item> {
        match self {
            Item::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn map_keys(&self) -> Vec<&str> {
        match self {
            Item::Map(entries) => entries.iter().map(|(k, _)| k.as_str()).collect(),
            _ => Vec::new(),
        }
    }

    pub fn size(&self) -> usize {
        match self {
            Item::Map(entries) => entries.len(),
            Item::Arr(items) => items.len(),
            _ => 0,
        }
    }
}

/// `itemMapOf(vararg pairs)`
pub fn item_map_of(pairs: Vec<(&str, Item)>) -> Item {
    Item::Map(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// `itemArrayOf(vararg items)`
pub fn item_array_of(items: Vec<Item>) -> Item {
    Item::Arr(items)
}

// ── Encoder ─────────────────────────────────────────────────────────────────

/// Canonical encode: Item → bytes (RFC 8949 canonical profile: sorted map keys,
/// minimal widths, definite lengths, float64).
pub fn encode(item: &Item) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_into(item, &mut buf);
    buf
}

fn encode_head(buf: &mut Vec<u8>, major: u8, value: u64) {
    let mt = major << 5;
    #[allow(clippy::match_overlapping_arm)] // tiered widths are the point
    match value {
        0..=23 => buf.push(mt | value as u8),
        0..=0xFF => {
            buf.push(mt | 24);
            buf.push(value as u8);
        }
        0..=0xFFFF => {
            buf.push(mt | 25);
            buf.extend_from_slice(&(value as u16).to_be_bytes());
        }
        0..=0xFFFF_FFFF => {
            buf.push(mt | 26);
            buf.extend_from_slice(&(value as u32).to_be_bytes());
        }
        _ => {
            buf.push(mt | 27);
            buf.extend_from_slice(&value.to_be_bytes());
        }
    }
}

fn encode_map_key(key: &str) -> Vec<u8> {
    encode(&Item::Str(key.to_string()))
}

fn encode_into(item: &Item, buf: &mut Vec<u8>) {
    match item {
        Item::Num(v) => {
            if *v >= 0 {
                encode_head(buf, 0, *v as u64);
            } else {
                encode_head(buf, 1, (*v as i128).unsigned_abs().wrapping_sub(1) as u64);
            }
        }
        Item::Str(s) => {
            let bytes = s.as_bytes();
            encode_head(buf, 3, bytes.len() as u64);
            buf.extend_from_slice(bytes);
        }
        Item::Bin(b) => {
            encode_head(buf, 2, b.len() as u64);
            buf.extend_from_slice(b);
        }
        Item::Arr(items) => {
            encode_head(buf, 4, items.len() as u64);
            for i in items {
                encode_into(i, buf);
            }
        }
        Item::Map(entries) => {
            let mut sorted: Vec<(Vec<u8>, &Item)> = entries
                .iter()
                .map(|(k, v)| (encode_map_key(k), v))
                .collect();
            sorted.sort_by(|a, b| compare_unsigned(&a.0, &b.0));
            encode_head(buf, 5, sorted.len() as u64);
            for (key_bytes, value) in &sorted {
                buf.extend_from_slice(key_bytes);
                encode_into(value, buf);
            }
        }
        Item::Bool(v) => buf.push(if *v { 0xF5 } else { 0xF4 }),
        Item::Nil => buf.push(0xF6),
        Item::Flt(v) => {
            buf.push(0xFB);
            buf.extend_from_slice(&v.to_bits().to_be_bytes());
        }
        Item::Tag(tag, inner) => {
            encode_head(buf, 6, *tag);
            encode_into(inner, buf);
        }
    }
}

fn compare_unsigned(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let limit = a.len().min(b.len());
    for i in 0..limit {
        match a[i].cmp(&b[i]) {
            std::cmp::Ordering::Equal => continue,
            o => return o,
        }
    }
    a.len().cmp(&b.len())
}

// ── Decoder ─────────────────────────────────────────────────────────────────

/// Decode bytes → Item. Indefinite lengths are accepted and converted to definite.
pub fn decode(bytes: &[u8]) -> Result<Item, String> {
    let mut pos = 0usize;
    let item = read_item(bytes, &mut pos)?;
    Ok(item)
}

fn read_item(data: &[u8], pos: &mut usize) -> Result<Item, String> {
    let head = u8_at(data, pos)?;
    let major = head >> 5;
    let additional = head & 0x1F;

    if additional == 31 {
        return match major {
            2 => read_indef_bytes(data, pos),
            3 => read_indef_text(data, pos),
            4 => read_indef_array(data, pos),
            5 => read_indef_map(data, pos),
            _ => Err(format!(
                "Indefinite length not supported for major type {major}"
            )),
        };
    }

    let argument = read_argument(data, pos, additional)?;

    match major {
        0 => Ok(Item::Num(
            i64::try_from(argument).map_err(|_| "u64 too large")?,
        )),
        1 => Ok(Item::Num(
            -(i64::try_from(argument).map_err(|_| "u64 too large")? + 1),
        )),
        2 => {
            let n = usize::try_from(argument).map_err(|_| "length too large")?;
            Ok(Item::Bin(read_bytes(data, pos, n)?))
        }
        3 => {
            let n = usize::try_from(argument).map_err(|_| "length too large")?;
            let raw = read_bytes(data, pos, n)?;
            String::from_utf8(raw)
                .map(Item::Str)
                .map_err(|e| format!("invalid utf8: {e}"))
        }
        4 => {
            let n = usize::try_from(argument).map_err(|_| "length too large")?;
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(read_item(data, pos)?);
            }
            Ok(Item::Arr(items))
        }
        5 => {
            let n = usize::try_from(argument).map_err(|_| "length too large")?;
            let mut entries = Vec::with_capacity(n);
            for _ in 0..n {
                let key_item = read_item(data, pos)?;
                let key = match key_item {
                    Item::Str(s) => s,
                    other => format!("{other:?}"),
                };
                let value = read_item(data, pos)?;
                entries.push((key, value));
            }
            Ok(Item::Map(entries))
        }
        6 => {
            let inner = read_item(data, pos)?;
            Ok(Item::Tag(argument, Box::new(inner)))
        }
        7 => read_simple(additional, argument),
        _ => Err(format!("Invalid CBOR major type: {major}")),
    }
}

fn read_simple(additional: u8, argument: u64) -> Result<Item, String> {
    match additional {
        20 => Ok(Item::Bool(false)),
        21 => Ok(Item::Bool(true)),
        22 | 23 => Ok(Item::Nil),
        25 => Ok(Item::Flt(read_f16(argument))),
        26 => Ok(Item::Flt(f64::from(f32::from_bits(argument as u32)))),
        27 => Ok(Item::Flt(f64::from_bits(argument))),
        _ => Err(format!("Unknown CBOR simple value: {additional}")),
    }
}

fn read_f16(bits: u64) -> f64 {
    let bits = bits as u16;
    let sign = f64::from((bits >> 15) & 1);
    let exp = ((bits >> 10) & 0x1F) as i32;
    let frac = f64::from(bits & 0x3FF);
    match exp {
        0 => (if sign != 0.0 { -1.0 } else { 1.0 }) * 2f64.powi(-14) * (frac / 1024.0),
        31 => {
            if frac == 0.0 {
                if sign != 0.0 {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                }
            } else {
                f64::NAN
            }
        }
        _ => (if sign != 0.0 { -1.0 } else { 1.0 }) * 2f64.powi(exp - 15) * (1.0 + frac / 1024.0),
    }
}

fn read_argument(data: &[u8], pos: &mut usize, additional: u8) -> Result<u64, String> {
    Ok(match additional {
        0..=23 => additional as u64,
        24 => u64::from(u8_at(data, pos)?),
        25 => {
            let hi = u16::from(u8_at(data, pos)?) << 8 | u16::from(u8_at(data, pos)?);
            u64::from(hi)
        }
        26 => {
            let mut v: u32 = 0;
            for _ in 0..4 {
                v = v << 8 | u32::from(u8_at(data, pos)?);
            }
            u64::from(v)
        }
        27 => {
            let mut v: u64 = 0;
            for _ in 0..8 {
                v = v << 8 | u64::from(u8_at(data, pos)?);
            }
            v
        }
        other => return Err(format!("Invalid CBOR additional info: {other}")),
    })
}

fn u8_at(data: &[u8], pos: &mut usize) -> Result<u8, String> {
    let b = *data.get(*pos).ok_or("unexpected end of input")?;
    *pos += 1;
    Ok(b)
}

fn read_bytes(data: &[u8], pos: &mut usize, n: usize) -> Result<Vec<u8>, String> {
    if *pos + n > data.len() {
        return Err("unexpected end of input".into());
    }
    let out = data[*pos..*pos + n].to_vec();
    *pos += n;
    Ok(out)
}

fn read_indef_bytes(data: &[u8], pos: &mut usize) -> Result<Item, String> {
    let mut chunks = Vec::new();
    loop {
        if u8_at(data, pos)? == 0xFF {
            break;
        }
        *pos -= 1;
        match read_item(data, pos)? {
            Item::Bin(b) => chunks.extend_from_slice(&b),
            _ => return Err("indefinite byte string chunk must be a byte string".into()),
        }
    }
    Ok(Item::Bin(chunks))
}

fn read_indef_text(data: &[u8], pos: &mut usize) -> Result<Item, String> {
    let mut out = String::new();
    loop {
        if u8_at(data, pos)? == 0xFF {
            break;
        }
        *pos -= 1;
        match read_item(data, pos)? {
            Item::Str(s) => out.push_str(&s),
            _ => return Err("indefinite text chunk must be text".into()),
        }
    }
    Ok(Item::Str(out))
}

fn read_indef_array(data: &[u8], pos: &mut usize) -> Result<Item, String> {
    let mut items = Vec::new();
    loop {
        if u8_at(data, pos)? == 0xFF {
            break;
        }
        *pos -= 1;
        items.push(read_item(data, pos)?);
    }
    Ok(Item::Arr(items))
}

fn read_indef_map(data: &[u8], pos: &mut usize) -> Result<Item, String> {
    let mut entries = Vec::new();
    loop {
        if u8_at(data, pos)? == 0xFF {
            break;
        }
        *pos -= 1;
        let key = match read_item(data, pos)? {
            Item::Str(s) => s,
            other => format!("{other:?}"),
        };
        let value = read_item(data, pos)?;
        entries.push((key, value));
    }
    Ok(Item::Map(entries))
}

// ── JSON rendering (ConfixSerialFormat renderJson) ──────────────────────────

/// `Item.toJsonString()` — JSON rendering per ConfixSerialFormat.
pub fn to_json_string(item: &Item) -> String {
    let mut sb = String::new();
    render_json(item, &mut sb);
    sb
}

fn render_json(item: &Item, sb: &mut String) {
    match item {
        Item::Nil => sb.push_str("null"),
        Item::Str(value) => {
            sb.push('"');
            for c in value.chars() {
                match c {
                    '"' => sb.push_str("\\\""),
                    '\\' => sb.push_str("\\\\"),
                    '\n' => sb.push_str("\\n"),
                    '\r' => sb.push_str("\\r"),
                    '\t' => sb.push_str("\\t"),
                    _ => sb.push(c),
                }
            }
            sb.push('"');
        }
        Item::Num(value) => sb.push_str(&value.to_string()),
        Item::Flt(value) => sb.push_str(&value.to_string()),
        Item::Bool(value) => sb.push_str(&value.to_string()),
        Item::Bin(value) => {
            sb.push('"');
            for b in value {
                sb.push(char::from_digit(u32::from(b >> 4), 16).unwrap());
                sb.push(char::from_digit(u32::from(b & 0xF), 16).unwrap());
            }
            sb.push('"');
        }
        Item::Arr(items) => {
            sb.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    sb.push(',');
                }
                render_json(item, sb);
            }
            sb.push(']');
        }
        Item::Map(entries) => {
            sb.push('{');
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    sb.push(',');
                }
                sb.push('"');
                sb.push_str(k);
                sb.push_str("\":");
                render_json(v, sb);
            }
            sb.push('}');
        }
        Item::Tag(_, inner) => render_json(inner, sb),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_smoke() {
        let item = item_map_of(vec![
            ("a", Item::Num(1)),
            ("b", Item::Str("two".into())),
            ("c", Item::Nil),
            ("d", item_array_of(vec![Item::Num(3)])),
        ]);
        let enc = encode(&item);
        let dec = decode(&enc).unwrap();
        assert_eq!(item, dec);
    }

    #[test]
    fn keys_sorted_by_encoded_bytes() {
        let first = item_map_of(vec![("b", Item::Num(2)), ("a", Item::Num(1))]);
        let second = item_map_of(vec![("a", Item::Num(1)), ("b", Item::Num(2))]);
        let expected: Vec<u8> = vec![0xa2, 0x61, 0x61, 0x01, 0x61, 0x62, 0x02];
        assert_eq!(encode(&first), expected);
        assert_eq!(encode(&second), expected);
    }
}
