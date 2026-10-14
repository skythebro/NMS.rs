//! `NMS_MOD_DEFINITION_CONTAINER` tables from AMUMSS Lua scripts.

use serde_json::{Map, Number, Value};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;

pub const CONTAINER: &str = "NMS_MOD_DEFINITION_CONTAINER";

pub const CHANGE_ALIASES: &[(&str, &str)] = &[
    ("SKW", "SPECIAL_KEY_WORDS"),
    ("PKW", "PRECEDING_KEY_WORDS"),
    ("VCT", "VALUE_CHANGE_TABLE"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuaTableError(pub String);

impl fmt::Display for LuaTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LuaTableError {}

impl LuaTableError {
    fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

pub fn parse_mod_script(text: &str) -> Option<Value> {
    let chars: Vec<char> = text.chars().collect();
    let stripped = match strip_comments(&chars) {
        Ok(s) => s,
        Err(error) => {
            return if find_container_assign(&chars).is_some() {
                Some(named_only(&chars, &error.0))
            } else {
                None
            };
        }
    };
    let Some(value_at) = find_container_assign(&stripped) else {
        return None;
    };
    let env = collect_locals(&stripped);
    match parse_value(&stripped, value_at, &env) {
        Err(error) => Some(named_only(&stripped, &error.0)),
        Ok((value, _)) => match value {
            Value::Object(mut map) => {
                map.insert("_apply".into(), Value::Bool(true));
                Some(Value::Object(map))
            }
            _ => Some(named_only(
                &stripped,
                "NMS_MOD_DEFINITION_CONTAINER is not a table",
            )),
        },
    }
}

pub fn parse_mod_script_file(path: &Path) -> Option<Value> {
    match fs::read(path) {
        Ok(bytes) => match decode_utf8_sig(&bytes) {
            Ok(text) => parse_mod_script(&text),
            Err(error) => Some(named_only(&[], &error)),
        },
        Err(error) => Some(named_only(&[], &error.to_string())),
    }
}

fn decode_utf8_sig(bytes: &[u8]) -> Result<String, String> {
    let bytes = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    };
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok(text.to_owned()),
        Err(err) => {
            let pos = err.valid_up_to();
            let byte = bytes.get(pos).copied().unwrap_or(0);
            let why = if err.error_len().is_none() {
                "unexpected end of data"
            } else if pos == 0 || bytes.get(pos).is_some_and(|b| *b >= 0x80 && *b < 0xC0) {
                "invalid start byte"
            } else {
                "invalid continuation byte"
            };
            Err(format!(
                "'utf-8' codec can't decode byte {byte:#04x} in position {pos}: {why}"
            ))
        }
    }
}

fn named_only(text: &[char], reason: &str) -> Value {
    let targets: Vec<String> = targets_from_text(&text.iter().collect::<String>())
        .into_iter()
        .collect();
    let modifications = if targets.is_empty() {
        Value::Array(vec![])
    } else {
        Value::Array(vec![Value::Object(Map::from_iter([(
            "MBIN_CHANGE_TABLE".into(),
            Value::Array(vec![Value::Object(Map::from_iter([(
                "MBIN_FILE_SOURCE".into(),
                Value::Array(targets.into_iter().map(Value::String).collect()),
            )]))]),
        )]))])
    };
    let mut map = Map::new();
    map.insert("_apply".into(), Value::Bool(false));
    map.insert("_skip_reason".into(), Value::String(reason.to_owned()));
    map.insert("MODIFICATIONS".into(), modifications);
    Value::Object(map)
}

pub fn mbin_sources(entry: &Value) -> Vec<(String, String)> {
    let Some(source) = entry.get("MBIN_FILE_SOURCE") else {
        return Vec::new();
    };
    if let Some(text) = source.as_str() {
        return mbin_item(&Value::String(text.to_owned()));
    }
    if source.is_object() {
        let pairs = mbin_item(source);
        if !pairs.is_empty() {
            return pairs;
        }
        return identity_paths(source);
    }
    if source.is_array() {
        let mut pairs = Vec::new();
        if let Some(items) = source.as_array() {
            for item in items {
                let got = mbin_item(item);
                if got.is_empty() {
                    pairs.extend(identity_paths(item));
                } else {
                    pairs.extend(got);
                }
            }
        }
        return pairs;
    }
    identity_paths(source)
}

pub fn change_tables(container: &Value) -> Vec<Value> {
    let mut tables = Vec::new();
    for modification in rows(container.get("MODIFICATIONS").unwrap_or(&Value::Null)) {
        if !modification.is_object() {
            continue;
        }
        for row in rows(modification.get("MBIN_CHANGE_TABLE").unwrap_or(&Value::Null)) {
            if row.is_object() {
                tables.push(normalize_row(&row));
            }
        }
    }
    tables
}

fn normalize_row(row: &Value) -> Value {
    let Some(obj) = row.as_object() else {
        return row.clone();
    };
    let mut out = obj.clone();
    for (short, full) in CHANGE_ALIASES {
        if out.contains_key(*short) && !out.contains_key(*full) {
            if let Some(value) = out.get(*short).cloned() {
                out.insert((*full).to_owned(), value);
            }
        }
    }
    let exml = out.get("EXML_CHANGE_TABLE");
    let mxml = out.get("MXML_CHANGE_TABLE");
    let changes = match (exml, mxml) {
        (Some(value), _) if py_truthy(value) => Some(value.clone()),
        (_, Some(value)) if py_truthy(value) => Some(value.clone()),
        _ => None,
    };
    if let Some(changes) = changes {
        let key = if out.contains_key("EXML_CHANGE_TABLE") {
            "EXML_CHANGE_TABLE"
        } else {
            "MXML_CHANGE_TABLE"
        };
        let normalized: Vec<Value> = rows(&changes)
            .into_iter()
            .map(|item| {
                if item.is_object() {
                    normalize_row(&item)
                } else {
                    item
                }
            })
            .collect();
        out.insert(key.to_owned(), Value::Array(normalized));
    }
    Value::Object(out)
}

fn rows(value: &Value) -> Vec<Value> {
    if let Some(list) = value.as_array() {
        return list.clone();
    }
    if let Some(map) = value.as_object() {
        let mut keyed: Vec<(i64, Value)> = Vec::new();
        let mut ok = true;
        for (key, item) in map {
            match key.parse::<i64>() {
                Ok(n) => keyed.push((n, item.clone())),
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            keyed.sort_by_key(|(n, _)| *n);
            return keyed.into_iter().map(|(_, item)| item).collect();
        }
        return map.values().cloned().collect();
    }
    Vec::new()
}

pub fn script_targets(container: &Value) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for row in change_tables(container) {
        for (_source, target) in mbin_sources(&row) {
            found.insert(target);
        }
    }
    found
}

pub fn script_applyable(container: &Value) -> bool {
    container.get("_apply") != Some(&Value::Bool(false))
}

pub fn targets_from_text(text: &str) -> BTreeSet<String> {
    let chars: Vec<char> = text.chars().collect();
    let needle: Vec<char> = "MBIN_FILE_SOURCE".chars().collect();
    let mut found = BTreeSet::new();
    let mut i = 0;
    while i + needle.len() <= chars.len() {
        if chars[i..].starts_with(&needle) {
            let mut j = i + needle.len();
            while j < chars.len() && matches!(chars[j], '"' | '\'' | ']') {
                j += 1;
            }
            j = skip_space(&chars, j);
            if j < chars.len() && chars[j] == '=' {
                let start = j + 1;
                match parse_value(&chars, start, &Map::new()) {
                    Ok((value, _)) => {
                        let entry = Value::Object(Map::from_iter([(
                            "MBIN_FILE_SOURCE".into(),
                            value,
                        )]));
                        for (_extract, write) in mbin_sources(&entry) {
                            found.insert(write);
                        }
                    }
                    Err(_) => {
                        let end = (start + 800).min(chars.len());
                        let chunk: String = chars[start..end].iter().collect();
                        for hit in find_quoted_mbin(&chunk) {
                            found.insert(mbin_path_str(&hit));
                        }
                    }
                }
                i = start;
                continue;
            }
        }
        i += 1;
    }
    found
}

fn find_quoted_mbin(chunk: &str) -> Vec<String> {
    let chars: Vec<char> = chunk.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' || chars[i] == '\'' {
            let open = chars[i];
            i += 1;
            let begin = i;
            while i < chars.len() && chars[i] != '"' && chars[i] != '\'' {
                i += 1;
            }
            if i < chars.len() {
                let body: String = chars[begin..i].iter().collect();
                let close = chars[i];
                if (open == '"' || open == '\'')
                    && (close == '"' || close == '\'')
                    && body.to_ascii_uppercase().ends_with(".MBIN")
                    && !body.is_empty()
                {
                    out.push(body);
                }
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    out
}

fn is_mbin(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|text| text.to_ascii_uppercase().ends_with(".MBIN"))
}

fn mbin_item(item: &Value) -> Vec<(String, String)> {
    if let Some(text) = item.as_str() {
        if !is_mbin(item) {
            return Vec::new();
        }
        let path = mbin_path_str(text);
        return vec![(path.clone(), path)];
    }
    if let Some(map) = item.as_object() {
        let mut renamed = Vec::new();
        for (old, new) in map {
            if is_mbin(&Value::String(old.clone())) && is_mbin(new) {
                if let Some(new_text) = new.as_str() {
                    renamed.push((mbin_path_str(old), mbin_path_str(new_text)));
                }
            }
        }
        if !renamed.is_empty() {
            return renamed;
        }
        let mut pairs = Vec::new();
        for value in map.values() {
            pairs.extend(mbin_item(value));
        }
        return pairs;
    }
    if let Some(list) = item.as_array() {
        let paths: Vec<&str> = list.iter().filter_map(|part| {
            if is_mbin(part) {
                part.as_str()
            } else {
                None
            }
        }).collect();
        if paths.len() >= 2 {
            return vec![(mbin_path_str(paths[0]), mbin_path_str(paths[1]))];
        }
        if paths.len() == 1 {
            let path = mbin_path_str(paths[0]);
            return vec![(path.clone(), path)];
        }
        let mut pairs = Vec::new();
        for part in list {
            if !part.is_string() {
                pairs.extend(mbin_item(part));
            }
        }
        return pairs;
    }
    Vec::new()
}

fn mbin_paths_in(value: &Value) -> Vec<String> {
    if let Some(text) = value.as_str() {
        return if is_mbin(value) {
            vec![mbin_path_str(text)]
        } else {
            Vec::new()
        };
    }
    if let Some(map) = value.as_object() {
        let mut found = Vec::new();
        for (key, item) in map {
            found.extend(mbin_paths_in(&Value::String(key.clone())));
            found.extend(mbin_paths_in(item));
        }
        return found;
    }
    if let Some(list) = value.as_array() {
        let mut found = Vec::new();
        for item in list {
            found.extend(mbin_paths_in(item));
        }
        return found;
    }
    Vec::new()
}

fn identity_paths(value: &Value) -> Vec<(String, String)> {
    mbin_paths_in(value)
        .into_iter()
        .map(|path| (path.clone(), path))
        .collect()
}

fn mbin_path_str(value: &str) -> String {
    value.replace('\\', "/").trim_start_matches('/').to_uppercase()
}

fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(i) = number.as_i64() {
                i != 0
            } else if let Some(u) = number.as_u64() {
                u != 0
            } else if let Some(f) = number.as_f64() {
                f != 0.0
            } else {
                true
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn strip_comments(chars: &[char]) -> Result<Vec<char>, LuaTableError> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if starts_with(chars, i, &['-', '-', '[', '[']) {
            let Some(end) = find_pair(chars, i + 4, &[']', ']']) else {
                break;
            };
            i = end + 2;
            continue;
        }
        if starts_with(chars, i, &['-', '-']) {
            match find_char(chars, i, '\n') {
                Some(end) => i = end,
                None => i = chars.len(),
            }
            continue;
        }
        if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
            let (chunk, next) = read_string(chars, i)?;
            out.extend(chunk);
            i = next;
            continue;
        }
        if starts_with(chars, i, &['[', '[']) {
            let (chunk, next) = read_long_string(chars, i)?;
            out.extend(chunk);
            i = next;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    Ok(out)
}

fn collect_locals(chars: &[char]) -> Map<String, Value> {
    let container_at = find_substr(chars, CONTAINER).unwrap_or(chars.len());
    let mut env = Map::new();
    let local: Vec<char> = "local".chars().collect();
    let mut i = 0;
    while i + local.len() <= chars.len() {
        if chars[i..].starts_with(&local)
            && word_bound_before(chars, i)
            && ends_ident(chars, i + local.len())
        {
            if i > container_at {
                break;
            }
            let mut j = skip_space(chars, i + local.len());
            if let Some((name, name_end)) = read_ident(chars, j) {
                j = skip_space(chars, name_end);
                if j < chars.len() && chars[j] == '=' {
                    if let Ok((value, _)) = parse_value(chars, j + 1, &env) {
                        env.insert(name, value);
                    }
                    i = j + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    env
}

fn find_container_assign(chars: &[char]) -> Option<usize> {
    let name: Vec<char> = CONTAINER.chars().collect();
    let mut i = 0;
    while i + name.len() <= chars.len() {
        if chars[i..].starts_with(&name) && word_bound_before(chars, i) {
            let j = skip_space(chars, i + name.len());
            if j < chars.len() && chars[j] == '=' {
                return Some(j + 1);
            }
        }
        i += 1;
    }
    None
}

fn parse_value(
    chars: &[char],
    i: usize,
    env: &Map<String, Value>,
) -> Result<(Value, usize), LuaTableError> {
    let (mut value, mut i) = parse_atom(chars, i, env)?;
    i = skip_space(chars, i);
    while starts_with(chars, i, &['.', '.']) {
        let (right, next) = parse_atom(chars, i + 2, env)?;
        value = Value::String(format!("{}{}", py_str(&value), py_str(&right)));
        i = skip_space(chars, next);
    }
    Ok((value, i))
}

fn parse_atom(
    chars: &[char],
    i: usize,
    env: &Map<String, Value>,
) -> Result<(Value, usize), LuaTableError> {
    let i = skip_space(chars, i);
    if i >= chars.len() {
        return Err(LuaTableError::new("unexpected end of script"));
    }
    if chars[i] == '"' || chars[i] == '\'' {
        let (raw, end) = read_string(chars, i)?;
        let quote = raw.first().copied().unwrap_or('"');
        let body: String = if raw.len() >= 2 {
            raw[1..raw.len() - 1].iter().collect()
        } else {
            String::new()
        };
        return Ok((Value::String(unescape(&body, quote)), end));
    }
    if starts_with(chars, i, &['[', '[']) {
        let (raw, end) = read_long_string(chars, i)?;
        let body: String = if raw.len() >= 4 {
            raw[2..raw.len() - 2].iter().collect()
        } else {
            String::new()
        };
        return Ok((Value::String(body), end));
    }
    if chars[i] == '{' {
        return parse_table(chars, i, env);
    }
    if starts_with(chars, i, &['t', 'r', 'u', 'e']) && ends_ident(chars, i + 4) {
        return Ok((Value::Bool(true), i + 4));
    }
    if starts_with(chars, i, &['f', 'a', 'l', 's', 'e']) && ends_ident(chars, i + 5) {
        return Ok((Value::Bool(false), i + 5));
    }
    if starts_with(chars, i, &['n', 'i', 'l']) && ends_ident(chars, i + 3) {
        return Ok((Value::Null, i + 3));
    }
    if let Some((number, end)) = read_number(chars, i) {
        return Ok((number, end));
    }
    if let Some((ident, _)) = read_ident(chars, i) {
        return parse_ident_value(chars, i, &ident, env);
    }
    Err(LuaTableError::new(format!(
        "cannot parse value at {}",
        py_repr_slice(chars, i)
    )))
}

fn parse_ident_value(
    chars: &[char],
    i: usize,
    ident: &str,
    env: &Map<String, Value>,
) -> Result<(Value, usize), LuaTableError> {
    let mut j = skip_space(chars, i + ident.chars().count());
    if ident == "string" && starts_with(chars, j, &['.', 'f', 'o', 'r', 'm', 'a', 't']) {
        j = skip_space(chars, j + 7);
        let (args, end) = parse_call_args(chars, j, env)?;
        let Some(Value::String(fmt)) = args.first() else {
            return Err(LuaTableError::new("string.format needs a format string"));
        };
        match py_mod_format(fmt, &args[1..]) {
            Ok(text) => return Ok((Value::String(text), end)),
            Err(error) => {
                return Err(LuaTableError::new(format!("string.format failed: {error}")));
            }
        }
    }
    if j < chars.len() && chars[j] == '(' {
        return Err(LuaTableError::new(format!(
            "cannot parse value at {}",
            py_repr_slice(chars, i)
        )));
    }
    if let Some(value) = env.get(ident) {
        return Ok((value.clone(), i + ident.chars().count()));
    }
    Err(LuaTableError::new(format!(
        "cannot parse value at {}",
        py_repr_slice(chars, i)
    )))
}

fn parse_call_args(
    chars: &[char],
    i: usize,
    env: &Map<String, Value>,
) -> Result<(Vec<Value>, usize), LuaTableError> {
    let i = skip_space(chars, i);
    if i >= chars.len() || chars[i] != '(' {
        return Err(LuaTableError::new("expected ("));
    }
    let mut i = i + 1;
    let mut args = Vec::new();
    loop {
        i = skip_space(chars, i);
        if i >= chars.len() {
            return Err(LuaTableError::new("unclosed call"));
        }
        if chars[i] == ')' {
            return Ok((args, i + 1));
        }
        let (value, next) = parse_value(chars, i, env)?;
        args.push(value);
        i = skip_space(chars, next);
        if i < chars.len() && chars[i] == ',' {
            i += 1;
            continue;
        }
        if i < chars.len() && chars[i] == ')' {
            return Ok((args, i + 1));
        }
        return Err(LuaTableError::new(format!(
            "expected , or ) at {}",
            py_repr_slice(chars, i)
        )));
    }
}

fn parse_table(
    chars: &[char],
    i: usize,
    env: &Map<String, Value>,
) -> Result<(Value, usize), LuaTableError> {
    let i = skip_space(chars, i);
    if i >= chars.len() || chars[i] != '{' {
        return Err(LuaTableError::new("expected {"));
    }
    let mut i = i + 1;
    let mut array: Vec<Value> = Vec::new();
    let mut mapping = Map::new();
    let mut index: i64 = 1;
    loop {
        i = skip_space(chars, i);
        if i >= chars.len() {
            return Err(LuaTableError::new("unclosed table"));
        }
        if chars[i] == '}' {
            return Ok((table_result(array, mapping), i + 1));
        }
        let mut named = false;
        if chars[i] == '[' {
            let (key_val, next) = parse_value(chars, i + 1, env)?;
            i = skip_space(chars, next);
            if i >= chars.len() || chars[i] != ']' {
                return Err(LuaTableError::new("expected ] after key"));
            }
            i = skip_space(chars, i + 1);
            if i >= chars.len() || chars[i] != '=' {
                return Err(LuaTableError::new("expected = after [key]"));
            }
            let (value, next) = parse_value(chars, i + 1, env)?;
            mapping.insert(py_str(&key_val), value);
            i = next;
            named = true;
        } else if let Some((ident, ident_end)) = read_ident(chars, i) {
            let j = skip_space(chars, ident_end);
            if j < chars.len() && chars[j] == '=' {
                let (value, next) = parse_value(chars, j + 1, env)?;
                mapping.insert(ident, value);
                i = next;
                named = true;
            }
        }
        if !named {
            let (value, next) = parse_value(chars, i, env)?;
            array.push(value.clone());
            mapping.insert(index.to_string(), value);
            index += 1;
            i = next;
        }
        i = skip_space(chars, i);
        if i < chars.len() && chars[i] == ',' {
            i += 1;
            continue;
        }
        if i < chars.len() && chars[i] == '}' {
            return Ok((table_result(array, mapping), i + 1));
        }
        return Err(LuaTableError::new(format!(
            "expected , or }} at {}",
            py_repr_slice(chars, i)
        )));
    }
}

fn table_result(array: Vec<Value>, mapping: Map<String, Value>) -> Value {
    if mapping.keys().any(|key| !is_digit_key(key)) {
        Value::Object(mapping)
    } else {
        Value::Array(array)
    }
}

fn is_digit_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_digit())
}

fn skip_space(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

fn read_ident(chars: &[char], i: usize) -> Option<(String, usize)> {
    if i >= chars.len() {
        return None;
    }
    let first = chars[i];
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let mut j = i + 1;
    while j < chars.len() {
        let c = chars[j];
        if c.is_ascii_alphanumeric() || c == '_' {
            j += 1;
        } else {
            break;
        }
    }
    Some((chars[i..j].iter().collect(), j))
}

fn ends_ident(chars: &[char], i: usize) -> bool {
    i >= chars.len() || !(chars[i].is_alphanumeric() || chars[i] == '_')
}

fn word_bound_before(chars: &[char], i: usize) -> bool {
    i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_')
}

fn read_string(chars: &[char], i: usize) -> Result<(Vec<char>, usize), LuaTableError> {
    if i >= chars.len() {
        return Err(LuaTableError::new("unclosed string"));
    }
    let quote = chars[i];
    let mut j = i + 1;
    while j < chars.len() {
        if chars[j] == '\\' && j + 1 < chars.len() {
            j += 2;
            continue;
        }
        if chars[j] == quote {
            return Ok((chars[i..=j].to_vec(), j + 1));
        }
        j += 1;
    }
    Err(LuaTableError::new("unclosed string"))
}

fn read_long_string(chars: &[char], i: usize) -> Result<(Vec<char>, usize), LuaTableError> {
    let Some(end) = find_pair(chars, i + 2, &[']', ']']) else {
        return Err(LuaTableError::new("unclosed [[ string"));
    };
    Ok((chars[i..end + 2].to_vec(), end + 2))
}

fn unescape(body: &str, _quote: char) -> String {
    body.replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\\\", "\\")
        .replace("\\\"", "\"")
        .replace("\\'", "'")
}

fn read_number(chars: &[char], i: usize) -> Option<(Value, usize)> {
    let mut j = i;
    if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
        j += 1;
    }
    let start_digits = j;
    let mut saw_dot = false;
    let mut saw_exp = false;
    if j < chars.len() && chars[j] == '.' {
        saw_dot = true;
        j += 1;
        if j >= chars.len() || !chars[j].is_ascii_digit() {
            return None;
        }
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
    } else {
        if j >= chars.len() || !chars[j].is_ascii_digit() {
            return None;
        }
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if j < chars.len() && chars[j] == '.' {
            saw_dot = true;
            j += 1;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
        }
    }
    if j == start_digits && !saw_dot {
        return None;
    }
    if j < chars.len() && (chars[j] == 'e' || chars[j] == 'E') {
        let mut k = j + 1;
        if k < chars.len() && (chars[k] == '+' || chars[k] == '-') {
            k += 1;
        }
        if k < chars.len() && chars[k].is_ascii_digit() {
            saw_exp = true;
            j = k + 1;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
        }
    }
    if j == i {
        return None;
    }
    let token: String = chars[i..j].iter().collect();
    if token == "+" || token == "-" || token == "." || token == "+." || token == "-." {
        return None;
    }
    if saw_dot || saw_exp {
        let number: f64 = token.parse().ok()?;
        Some((Value::Number(Number::from_f64(number)?), j))
    } else {
        match token.parse::<i64>() {
            Ok(n) => Some((Value::Number(n.into()), j)),
            Err(_) => {
                let number: f64 = token.parse().ok()?;
                Some((Value::Number(Number::from_f64(number)?), j))
            }
        }
    }
}

fn starts_with(chars: &[char], i: usize, pat: &[char]) -> bool {
    i + pat.len() <= chars.len() && chars[i..i + pat.len()] == *pat
}

fn find_pair(chars: &[char], from: usize, pat: &[char]) -> Option<usize> {
    let mut i = from;
    while i + pat.len() <= chars.len() {
        if chars[i..].starts_with(pat) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_char(chars: &[char], from: usize, needle: char) -> Option<usize> {
    chars[from..].iter().position(|c| *c == needle).map(|p| from + p)
}

fn find_substr(chars: &[char], needle: &str) -> Option<usize> {
    let pat: Vec<char> = needle.chars().collect();
    let mut i = 0;
    while i + pat.len() <= chars.len() {
        if chars[i..].starts_with(&pat) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn py_repr_slice(chars: &[char], i: usize) -> String {
    let end = (i + 40).min(chars.len());
    let slice: String = if i >= chars.len() {
        String::new()
    } else {
        chars[i..end].iter().collect()
    };
    py_repr_str(&slice)
}

fn py_repr_str(s: &str) -> String {
    let has_single = s.contains('\'');
    let has_double = s.contains('"');
    let quote = if has_single && !has_double { '"' } else { '\'' };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' if quote == '\'' => out.push_str("\\'"),
            '"' if quote == '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 32 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(number) => {
            if number.is_i64() {
                number
                    .as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| number.to_string())
            } else if number.is_u64() {
                number
                    .as_u64()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| number.to_string())
            } else if let Some(f) = number.as_f64() {
                py_repr_float(f)
            } else {
                number.to_string()
            }
        }
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(py_repr_value).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", py_repr_str(k), py_repr_value(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn py_repr_value(value: &Value) -> String {
    match value {
        Value::String(text) => py_repr_str(text),
        _ => py_str(value),
    }
}

fn py_repr_float(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x == f64::INFINITY {
        return "inf".into();
    }
    if x == f64::NEG_INFINITY {
        return "-inf".into();
    }
    let mut s = format!("{x:?}");
    if let Some(idx) = s.find('e').or_else(|| s.find('E')) {
        let mant = s[..idx].to_owned();
        let rest = &s[idx + 1..];
        let (neg, num) = if let Some(stripped) = rest.strip_prefix('-') {
            (true, stripped)
        } else if let Some(stripped) = rest.strip_prefix('+') {
            (false, stripped)
        } else {
            (false, rest)
        };
        let n: i32 = num.parse().unwrap_or(0);
        s = format!("{mant}e{}{:02}", if neg { "-" } else { "+" }, n.unsigned_abs());
    } else if !s.contains('.') {
        s.push_str(".0");
    }
    s
}

fn py_mod_format(fmt: &str, args: &[Value]) -> Result<String, String> {
    let chars: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut arg_i = 0;
    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let spec_at = i;
        i += 1;
        if i < chars.len() && chars[i] == '%' {
            out.push('%');
            i += 1;
            continue;
        }
        if i >= chars.len() {
            return Err("incomplete format".into());
        }
        while i < chars.len() && matches!(chars[i], '#' | '0' | '-' | ' ' | '+') {
            i += 1;
        }
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let mut precision: Option<usize> = None;
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            let mut digits = String::new();
            while i < chars.len() && chars[i].is_ascii_digit() {
                digits.push(chars[i]);
                i += 1;
            }
            precision = Some(digits.parse().unwrap_or(0));
        }
        if i >= chars.len() {
            return Err("incomplete format".into());
        }
        let conv = chars[i];
        i += 1;
        match conv {
            's' | 'd' | 'i' | 'f' | 'x' => {}
            other => {
                return Err(format!(
                    "unsupported format character '{}' (0x{:x}) at index {}",
                    other, other as u32, spec_at + 1
                ));
            }
        }
        if arg_i >= args.len() {
            return Err("not enough arguments for format string".into());
        }
        let arg = &args[arg_i];
        arg_i += 1;
        match conv {
            's' => out.push_str(&py_str(arg)),
            'd' | 'i' => out.push_str(&format_int(arg, false)?),
            'x' => out.push_str(&format_int(arg, true)?),
            'f' => {
                let number = as_float(arg)?;
                let prec = precision.unwrap_or(6);
                out.push_str(&format!("{number:.prec$}"));
            }
            _ => unreachable!(),
        }
    }
    if arg_i != args.len() {
        return Err("not all arguments converted during string formatting".into());
    }
    Ok(out)
}

fn format_int(arg: &Value, hex: bool) -> Result<String, String> {
    let n = as_int(arg, hex)?;
    if hex {
        if n < 0 {
            Ok(format!("-{:x}", n.unsigned_abs()))
        } else {
            Ok(format!("{n:x}"))
        }
    } else {
        Ok(n.to_string())
    }
}

fn as_int(arg: &Value, hex: bool) -> Result<i64, String> {
    match arg {
        Value::Bool(flag) => Ok(if *flag { 1 } else { 0 }),
        Value::Number(number) => {
            if let Some(i) = number.as_i64() {
                Ok(i)
            } else if let Some(u) = number.as_u64() {
                Ok(i64::try_from(u).unwrap_or(i64::MAX))
            } else if let Some(f) = number.as_f64() {
                if hex {
                    return Err("%x format: an integer is required, not float".into());
                }
                Ok(f as i64)
            } else {
                Err("%d format: a real number is required, not str".into())
            }
        }
        Value::String(_) => {
            if hex {
                Err("%x format: an integer is required, not str".into())
            } else {
                Err("%d format: a real number is required, not str".into())
            }
        }
        other => Err(format!(
            "%d format: a real number is required, not {}",
            type_name(other)
        )),
    }
}

fn as_float(arg: &Value) -> Result<f64, String> {
    match arg {
        Value::Bool(flag) => Ok(if *flag { 1.0 } else { 0.0 }),
        Value::Number(number) => number
            .as_f64()
            .ok_or_else(|| "%f format: a real number is required, not str".into()),
        _ => Err(format!(
            "%f format: a real number is required, not {}",
            type_name(arg)
        )),
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(_) => "float",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    const FORMAT_AND_LOCALS: &str = r#"
local NMS_VERSION = "6.45.1"
local MOD_VERSION = "0"
local BUILDING_UPGRADE_TIME = 300

NMS_MOD_DEFINITION_CONTAINER = {
    ["MOD_FILENAME"] = string.format("Accelerated Settlements %s.%s", NMS_VERSION, MOD_VERSION),
    ["NMS_VERSION"] = NMS_VERSION,
    ["MODIFICATIONS"] = {
        {
            ["MBIN_CHANGE_TABLE"] = {
                {
                    ["MBIN_FILE_SOURCE"] = "GCSETTLEMENTGLOBALS.MBIN",
                    ["MXML_CHANGE_TABLE"] = {
                        {
                            ["VALUE_CHANGE_TABLE"] = {
                                {"BuildingUpgradeTimeInSeconds", BUILDING_UPGRADE_TIME},
                            }
                        },
                    }
                }
            }
        }
    }
}
"#;

    const BUILDS_TABLE: &str = r#"
function GetNewSpecial(ID)
    return ID
end

NEW_SPECIAL = {}
table.insert(NEW_SPECIAL, GetNewSpecial("X"))

NMS_MOD_DEFINITION_CONTAINER =
{
["MOD_FILENAME"] = "~Consumerism",
["MODIFICATIONS"] =
    {
        {
            ["MBIN_CHANGE_TABLE"] =
            {
                {
                    ["MBIN_FILE_SOURCE"] = { "METADATA\\REALITY\\TABLES\\PURCHASEABLESPECIALS.MBIN"},
                    ["MXML_CHANGE_TABLE"] = NEW_SPECIAL
                },
            }
        },
    }
}
"#;

    /// `NMS_GAME_DIR`\GAMEDATA\MODS when set, else the game folder above a checkout in
    /// `<game>\Modding\<repo>`. Tests that need a file there skip when it is missing.
    fn live_mods() -> PathBuf {
        match std::env::var_os("NMS_GAME_DIR") {
            Some(dir) => PathBuf::from(dir).join("GAMEDATA/MODS"),
            None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../../GAMEDATA/MODS"),
        }
    }

    #[test]
    fn format_and_locals_apply() {
        let formatted = parse_mod_script(FORMAT_AND_LOCALS).expect("seen as AMUMSS");
        assert!(script_applyable(&formatted), "{formatted:?}");
        assert_eq!(formatted.get("MOD_FILENAME"), Some(&json!("Accelerated Settlements 6.45.1.0")));
        assert_eq!(formatted.get("NMS_VERSION"), Some(&json!("6.45.1")));
        assert_eq!(
            script_targets(&formatted),
            BTreeSet::from(["GCSETTLEMENTGLOBALS.MBIN".to_string()])
        );
        let changes = &formatted["MODIFICATIONS"][0]["MBIN_CHANGE_TABLE"][0]["MXML_CHANGE_TABLE"][0]
            ["VALUE_CHANGE_TABLE"];
        assert_eq!(changes[0][1], json!(300));
    }

    #[test]
    fn function_built_scripts_name_files() {
        let built = parse_mod_script(BUILDS_TABLE).expect("seen as AMUMSS");
        assert!(!script_applyable(&built));
        assert_eq!(
            script_targets(&built),
            BTreeSet::from(["METADATA/REALITY/TABLES/PURCHASEABLESPECIALS.MBIN".to_string()])
        );
        let joined = targets_from_text(BUILDS_TABLE).into_iter().collect::<Vec<_>>().join(" ");
        assert!(joined.contains("PURCHASEABLESPECIALS.MBIN"));
    }

    #[test]
    fn mbin_remove_triple() {
        let remove_pairs = mbin_sources(&json!({
            "MBIN_FILE_SOURCE": [[
                r"MODELS\SPACE\ATLASSTATION\MODULARPARTS\INTERIOR\PATHORB\PATHORB\ENTITIES\ORBSTONE_1.ENTITY.MBIN",
                r"MODELS\SPACE\ATLASSTATION\MODULARPARTS\INTERIOR\PATHORB\PATHORB_DUMMY\ENTITIES\ORBSTONE_DUMMY.ENTITY.MBIN",
                "REMOVE"
            ]]
        }));
        assert_eq!(
            remove_pairs,
            vec![(
                "MODELS/SPACE/ATLASSTATION/MODULARPARTS/INTERIOR/PATHORB/PATHORB/ENTITIES/ORBSTONE_1.ENTITY.MBIN".into(),
                "MODELS/SPACE/ATLASSTATION/MODULARPARTS/INTERIOR/PATHORB/PATHORB_DUMMY/ENTITIES/ORBSTONE_DUMMY.ENTITY.MBIN".into(),
            )]
        );
    }

    #[test]
    fn mbin_two_files_stay_two() {
        let two = mbin_sources(&json!({"MBIN_FILE_SOURCE": ["UI/A.MBIN", "UI/B.MBIN"]}));
        assert_eq!(
            two,
            vec![
                ("UI/A.MBIN".into(), "UI/A.MBIN".into()),
                ("UI/B.MBIN".into(), "UI/B.MBIN".into()),
            ]
        );
    }

    #[test]
    fn mbin_path_plus_remove() {
        let drop = mbin_sources(&json!({"MBIN_FILE_SOURCE": [["UI/GONE.MBIN", "REMOVE"]]}));
        assert_eq!(drop, vec![("UI/GONE.MBIN".into(), "UI/GONE.MBIN".into())]);
    }

    #[test]
    fn mbin_rename_map() {
        let mapped = mbin_sources(&json!({"MBIN_FILE_SOURCE": {"UI/OLD.MBIN": "UI/NEW.MBIN"}}));
        assert_eq!(mapped, vec![("UI/OLD.MBIN".into(), "UI/NEW.MBIN".into())]);
    }

    #[test]
    fn mbin_unknown_source_empty() {
        assert_eq!(mbin_sources(&json!({"MBIN_FILE_SOURCE": 3})), Vec::<(String, String)>::new());
    }

    #[test]
    fn unreadable_container_named_only() {
        let junk = parse_mod_script("NMS_MOD_DEFINITION_CONTAINER = function() end");
        let junk = junk.expect("named-only");
        assert!(!script_applyable(&junk));
        assert!(script_targets(&junk).is_empty());
    }

    #[test]
    fn live_accelerated_settlements() {
        let path = live_mods()
            .join("Accelerated Settlements 6.45.1.0")
            .join("Accelerated Settlements.lua");
        if !path.is_file() {
            return;
        }
        let real = parse_mod_script_file(&path).expect("parse");
        assert!(script_applyable(&real), "{real:?}");
        assert!(script_targets(&real).contains("GCSETTLEMENTGLOBALS.MBIN"));
    }

    #[test]
    fn live_consumerism() {
        let path = live_mods().join("~Consumerism").join("Consumerism.lua");
        if !path.is_file() {
            return;
        }
        let real = parse_mod_script_file(&path).expect("parse");
        assert!(!script_applyable(&real));
        assert!(script_targets(&real).contains("METADATA/REALITY/TABLES/PURCHASEABLESPECIALS.MBIN"));
    }

    #[test]
    fn py_str_cases() {
        assert_eq!(py_str(&json!(1)), "1");
        assert_eq!(py_str(&json!(1.0)), "1.0");
        assert_eq!(py_str(&json!(0.5)), "0.5");
        assert_eq!(py_str(&Value::from(1e20)), "1e+20");
        assert_eq!(py_str(&Value::Bool(true)), "True");
        assert_eq!(py_str(&Value::Bool(false)), "False");
        assert_eq!(py_str(&Value::Null), "None");
    }

    #[test]
    fn format_conversions() {
        assert_eq!(
            py_mod_format("%s.%s", &[json!("6.45.1"), json!("0")]).unwrap(),
            "6.45.1.0"
        );
        assert_eq!(py_mod_format("%d", &[json!(1.9)]).unwrap(), "1");
        assert_eq!(py_mod_format("%i", &[json!(7)]).unwrap(), "7");
        assert_eq!(py_mod_format("%f", &[json!(1.5)]).unwrap(), "1.500000");
        assert_eq!(py_mod_format("%.2f", &[json!(1.5)]).unwrap(), "1.50");
        assert_eq!(py_mod_format("%x", &[json!(255)]).unwrap(), "ff");
        assert_eq!(py_mod_format("%%s", &[]).unwrap(), "%s");
        assert!(py_mod_format("%z", &[json!(1)]).is_err());
    }

    #[test]
    fn golden_lua_fixtures() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/lua");
        let mut pairs = 0;
        let entries = fs::read_dir(&dir).unwrap_or_else(|e| panic!("fixtures in {}: {e}", dir.display()));
        for entry in entries {
            let entry = entry.expect("dirent");
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("lua") {
                continue;
            }
            let json_path = path.with_extension("json");
            let lua_text = fs::read_to_string(&path).expect("lua");
            let golden: Value =
                serde_json::from_str(&fs::read_to_string(&json_path).expect("json")).expect("parse json");
            let parsed = parse_mod_script(&lua_text);
            assert_eq!(
                parsed.as_ref().unwrap_or(&Value::Null),
                golden.get("parse").unwrap_or(&Value::Null),
                "{}",
                path.display()
            );
            match golden.get("applyable") {
                Some(Value::Null) | None => {}
                Some(flag) => {
                    let parsed = parsed.as_ref().expect("applyable");
                    assert_eq!(Value::Bool(script_applyable(parsed)), *flag, "{}", path.display());
                }
            }
            match golden.get("targets") {
                Some(Value::Null) | None => {}
                Some(Value::Array(items)) => {
                    let parsed = parsed.as_ref().expect("targets");
                    let expect: BTreeSet<String> = items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect();
                    assert_eq!(script_targets(parsed), expect, "{}", path.display());
                }
                _ => panic!("bad targets in {}", path.display()),
            }
            match golden.get("mbin_sources") {
                Some(Value::Null) | None => {}
                Some(Value::Array(items)) => {
                    let parsed = parsed.as_ref().expect("mbin");
                    let mut got = Vec::new();
                    for row in change_tables(parsed) {
                        for (a, b) in mbin_sources(&row) {
                            got.push(json!([a, b]));
                        }
                    }
                    assert_eq!(Value::Array(got), Value::Array(items.clone()), "{}", path.display());
                }
                _ => panic!("bad mbin_sources in {}", path.display()),
            }
            match golden.get("text_targets") {
                Some(Value::Null) | None => {}
                Some(Value::Array(items)) => {
                    let expect: BTreeSet<String> = items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect();
                    assert_eq!(targets_from_text(&lua_text), expect, "{}", path.display());
                }
                _ => panic!("bad text_targets in {}", path.display()),
            }
            pairs += 1;
        }
        assert!(pairs > 0, "expected golden lua fixtures in {}", dir.display());
    }
}
