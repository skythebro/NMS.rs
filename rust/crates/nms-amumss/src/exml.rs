//! AMUMSS change tables applied to MBINCompiler EXML.

use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::Write;
use std::path::Path;
pub use xmltree::{Element, XMLNode};
use xmltree::EmitterConfig;

#[derive(Debug)]
pub struct ExmlChangeError(pub String);

impl fmt::Display for ExmlChangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExmlChangeError {}

/// An owned MBINCompiler MXML document.
pub struct Mxml {
    root: Element,
}

impl Mxml {
    pub fn read(path: impl AsRef<Path>) -> Result<Mxml, String> {
        let file = std::fs::File::open(path.as_ref()).map_err(|e| e.to_string())?;
        // The XML reader pulls one byte at a time; unbuffered that is one system call per byte
        // (seconds for a 1 MB page).
        let root = Element::parse(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
        Ok(Mxml { root })
    }

    pub fn parse(text: &str) -> Result<Mxml, String> {
        let root = Element::parse(text.as_bytes()).map_err(|e| e.to_string())?;
        Ok(Mxml { root })
    }

    pub fn root(&self) -> &Element {
        &self.root
    }

    pub fn root_mut(&mut self) -> &mut Element {
        &mut self.root
    }

    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let file = std::fs::File::create(path.as_ref()).map_err(|e| e.to_string())?;
        // The emitter writes many tiny pieces; buffer them.
        let mut out = std::io::BufWriter::new(file);
        self.write_to(&mut out)?;
        out.flush().map_err(|e| e.to_string())
    }

    pub fn to_string(&self) -> String {
        let mut buf = Vec::new();
        if self.write_to(&mut buf).is_err() {
            return String::new();
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn write_to(&self, w: &mut impl Write) -> Result<(), String> {
        w.write_all(b"<?xml version='1.0' encoding='utf-8'?>\n")
            .map_err(|e| e.to_string())?;
        let cfg = EmitterConfig::new()
            .write_document_declaration(false)
            .perform_indent(true)
            .indent_string("\t");
        self.root
            .write_with_config(w, cfg)
            .map_err(|e| e.to_string())
    }
}

pub fn apply_change_table(doc: &mut Mxml, changes: &[Value]) -> Result<(), ExmlChangeError> {
    let mut saved: HashMap<String, Element> = HashMap::new();
    for change in changes {
        let Value::Object(map) = change else {
            return Err(ExmlChangeError(format!(
                "change entry is not a table: {change}"
            )));
        };
        apply_one(&mut doc.root, map, &mut saved)?;
    }
    Ok(())
}

#[derive(Clone)]
enum Loc {
    Main(Vec<usize>),
    Saved { key: String, path: Vec<usize> },
}

fn apply_one(
    root: &mut Element,
    change: &Map<String, Value>,
    saved: &mut HashMap<String, Element>,
) -> Result<(), ExmlChangeError> {
    let change = expand_aliases(change);

    let targets: Vec<Loc> = if py_truthy(change.get("SEC_EDIT")) {
        let edit_name = py_str(change.get("SEC_EDIT").unwrap_or(&Value::Null));
        if !saved.contains_key(&edit_name) {
            return Err(ExmlChangeError(format!(
                "SEC_EDIT {edit_name:?} has no saved section"
            )));
        }
        if py_truthy(change.get("SPECIAL_KEY_WORDS")) {
            let paths = {
                let scope = saved.get(&edit_name).ok_or_else(|| {
                    ExmlChangeError(format!("SEC_EDIT {edit_name:?} has no saved section"))
                })?;
                find_targets(scope, &change)?
            };
            paths
                .into_iter()
                .map(|path| Loc::Saved {
                    key: edit_name.clone(),
                    path,
                })
                .collect()
        } else {
            vec![Loc::Saved {
                key: edit_name,
                path: Vec::new(),
            }]
        }
    } else {
        let found = find_targets(root, &change)?;
        if found.is_empty() && needs_target(&change) {
            return Err(ExmlChangeError(format!(
                "no match for {:?}",
                change.get("SPECIAL_KEY_WORDS")
            )));
        }
        found.into_iter().map(Loc::Main).collect()
    };

    if py_truthy(change.get("SEC_SAVE_TO")) {
        let save_as = py_str(change.get("SEC_SAVE_TO").unwrap_or(&Value::Null));
        if targets.is_empty() {
            return Err(ExmlChangeError(format!(
                "SEC_SAVE_TO {save_as:?} found nothing"
            )));
        }
        let copied = match &targets[0] {
            Loc::Main(path) => {
                let widget = widget_entry(root, path);
                get(root, &widget)
                    .ok_or_else(|| ExmlChangeError("SEC_SAVE_TO target vanished".into()))?
                    .clone()
            }
            Loc::Saved { key, path } => get(
                saved
                    .get(key)
                    .ok_or_else(|| ExmlChangeError("SEC_SAVE_TO target vanished".into()))?,
                path,
            )
            .ok_or_else(|| ExmlChangeError("SEC_SAVE_TO target vanished".into()))?
            .clone(),
        };
        saved.insert(save_as, copied);
    }

    if py_truthy(change.get("VALUE_CHANGE_TABLE")) {
        for target in &targets {
            match target {
                Loc::Main(path) => {
                    let section = get_mut(root, path)
                        .ok_or_else(|| ExmlChangeError("value target vanished".into()))?;
                    apply_values(section, &change)?;
                }
                Loc::Saved { key, path } => {
                    let section = saved
                        .get_mut(key)
                        .and_then(|el| get_mut(el, path))
                        .ok_or_else(|| ExmlChangeError("value target vanished".into()))?;
                    apply_values(section, &change)?;
                }
            }
        }
    }

    if py_truthy(change.get("SEC_ADD_NAMED")) {
        let add_named = py_str(change.get("SEC_ADD_NAMED").unwrap_or(&Value::Null));
        if !saved.contains_key(&add_named) {
            return Err(ExmlChangeError(format!(
                "SEC_ADD_NAMED {add_named:?} has no saved section"
            )));
        }
        if targets.is_empty() {
            return Err(ExmlChangeError(
                "SEC_ADD_NAMED needs SPECIAL_KEY_WORDS for the insert point".into(),
            ));
        }
        let Loc::Main(path) = &targets[0] else {
            return Err(ExmlChangeError("target has no parent".into()));
        };
        let (parent_path, widget_i) = parent_and_widget(root, path)?;
        let copy = saved
            .get(&add_named)
            .ok_or_else(|| {
                ExmlChangeError(format!("SEC_ADD_NAMED {add_named:?} has no saved section"))
            })?
            .clone();
        let parent = get_mut(root, &parent_path)
            .ok_or_else(|| ExmlChangeError("target has no parent".into()))?;
        insert_after(parent, widget_i, copy)?;
    }

    if py_truthy(change.get("ADD")) && !py_truthy(change.get("SEC_ADD_NAMED")) {
        if targets.is_empty() {
            return Err(ExmlChangeError(
                "ADD needs SPECIAL_KEY_WORDS for the insert point".into(),
            ));
        }
        let raw = py_str(change.get("ADD").unwrap_or(&Value::Null));
        let parsed = Element::parse(raw.as_bytes()).map_err(|e| ExmlChangeError(e.to_string()))?;
        let Loc::Main(path) = &targets[0] else {
            return Err(ExmlChangeError("target has no parent".into()));
        };
        let (parent_path, widget_i) = parent_and_widget(root, path)?;
        let parent = get_mut(root, &parent_path)
            .ok_or_else(|| ExmlChangeError("target has no parent".into()))?;
        insert_after(parent, widget_i, parsed)?;
    }

    if is_remove_section(change.get("REMOVE")) {
        for target in &targets {
            if matches!(target, Loc::Saved { .. }) {
                return Err(ExmlChangeError("target has no parent".into()));
            }
        }
        let mut jobs: Vec<(Vec<usize>, usize)> = Vec::new();
        for target in &targets {
            if let Loc::Main(path) = target {
                jobs.push(parent_and_widget(root, path)?);
            }
        }
        jobs.sort_by(|a, b| b.cmp(a));
        for (parent_path, widget_i) in jobs {
            let parent = get_mut(root, &parent_path)
                .ok_or_else(|| ExmlChangeError("target has no parent".into()))?;
            remove_child(parent, widget_i)?;
        }
    }

    Ok(())
}

fn expand_aliases(change: &Map<String, Value>) -> Map<String, Value> {
    let mut out = change.clone();
    for (short, full) in [
        ("SKW", "SPECIAL_KEY_WORDS"),
        ("PKW", "PRECEDING_KEY_WORDS"),
        ("VCT", "VALUE_CHANGE_TABLE"),
    ] {
        if out.contains_key(short) && !out.contains_key(full) {
            if let Some(value) = out.get(short).cloned() {
                out.insert(full.to_string(), value);
            }
        }
    }
    out
}

fn needs_target(change: &Map<String, Value>) -> bool {
    [
        "VALUE_CHANGE_TABLE",
        "SEC_ADD_NAMED",
        "ADD",
        "REMOVE",
        "SEC_SAVE_TO",
    ]
    .iter()
    .any(|key| py_truthy(change.get(*key)))
}

fn name_list(value: Option<&Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    if !py_truthy(Some(value)) {
        return Vec::new();
    }
    match value {
        Value::String(s) => vec![s.clone()],
        Value::Array(items) => items.iter().map(py_str).collect(),
        Value::Object(map) if map.keys().all(|k| is_index(k)) => {
            let mut items: Vec<(i64, &Value)> = map
                .iter()
                .filter_map(|(k, v)| k.parse::<i64>().ok().map(|n| (n, v)))
                .collect();
            items.sort_by_key(|(n, _)| *n);
            items.into_iter().map(|(_, v)| py_str(v)).collect()
        }
        Value::Object(map) => map.values().map(py_str).collect(),
        other => vec![py_str(other)],
    }
}

fn preceding_scope(root: &Element, change: &Map<String, Value>) -> Option<Vec<usize>> {
    let names = name_list(change.get("PRECEDING_KEY_WORDS"));
    let mut current = Vec::new();
    for name in names {
        let el = get(root, &current)?;
        let mut found = None;
        for_each_el(el, &mut |node, rel| {
            if found.is_some() || rel.is_empty() {
                return;
            }
            if attr(node, "name") == Some(name.as_str()) {
                found = Some(rel.to_vec());
            }
        });
        current = join_path(&current, &found?);
    }
    Some(current)
}

fn find_targets(
    root: &Element,
    change: &Map<String, Value>,
) -> Result<Vec<Vec<usize>>, ExmlChangeError> {
    let Some(scope_path) = preceding_scope(root, change) else {
        return Ok(Vec::new());
    };
    let keys = pairs(change.get("SPECIAL_KEY_WORDS"))?;
    if keys.is_empty() {
        return Ok(vec![scope_path]);
    }
    let scope = get(root, &scope_path).ok_or_else(|| ExmlChangeError("scope vanished".into()))?;
    let (first_name, first_value) = &keys[0];
    let mut hits = Vec::new();
    for_each_el(scope, &mut |el, rel| {
        if attr(el, "name") == Some(first_name.as_str())
            && attr_or_empty(el, "value") == first_value
        {
            hits.push(rel.to_vec());
        }
    });
    let mut widgets = Vec::new();
    let mut seen: HashSet<Vec<usize>> = HashSet::new();
    for rel in hits {
        let abs = join_path(&scope_path, &rel);
        let widget = widget_entry(root, &abs);
        if seen.contains(&widget) {
            continue;
        }
        if keys.len() > 1 {
            let Some(widget_el) = get(root, &widget) else {
                continue;
            };
            if !section_has(widget_el, &keys[1..]) {
                continue;
            }
        }
        seen.insert(widget.clone());
        widgets.push(widget);
    }
    let replace = change
        .get("REPLACE_TYPE")
        .map(py_str)
        .unwrap_or_default()
        .to_uppercase();
    if replace == "ALL" {
        Ok(widgets)
    } else {
        Ok(widgets.into_iter().take(1).collect())
    }
}

fn section_has(section: &Element, keys: &[(String, String)]) -> bool {
    let mut remaining: HashSet<(String, String)> = keys.iter().cloned().collect();
    for_each_el(section, &mut |el, _| {
        let pair = (
            attr(el, "name").unwrap_or("").to_string(),
            attr_or_empty(el, "value").to_string(),
        );
        remaining.remove(&pair);
    });
    remaining.is_empty()
}

fn pairs(value: Option<&Value>) -> Result<Vec<(String, String)>, ExmlChangeError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if !py_truthy(Some(value)) {
        return Ok(Vec::new());
    }
    match value {
        Value::Object(map) if map.keys().all(|k| is_index(k)) => {
            Ok(chunk_pairs(ordered_index_values(map)))
        }
        Value::Object(map) => Ok(map.iter().map(|(k, v)| (k.clone(), py_str(v))).collect()),
        Value::Array(items) => Ok(chunk_pairs(items.iter().map(py_str).collect())),
        other => Err(ExmlChangeError(format!(
            "SPECIAL_KEY_WORDS must be pairs, not {other}"
        ))),
    }
}

fn ordered_index_values(map: &Map<String, Value>) -> Vec<String> {
    let mut items: Vec<(i64, &Value)> = map
        .iter()
        .filter_map(|(k, v)| k.parse::<i64>().ok().map(|n| (n, v)))
        .collect();
    items.sort_by_key(|(n, _)| *n);
    items.into_iter().map(|(_, v)| py_str(v)).collect()
}

fn chunk_pairs(strings: Vec<String>) -> Vec<(String, String)> {
    strings
        .chunks(2)
        .filter_map(|chunk| {
            if chunk.len() == 2 {
                Some((chunk[0].clone(), chunk[1].clone()))
            } else {
                None
            }
        })
        .collect()
}

fn is_index(key: &str) -> bool {
    key.parse::<i64>().is_ok()
}

fn widget_entry(root: &Element, node: &[usize]) -> Vec<usize> {
    let mut chain: Vec<Vec<usize>> = Vec::new();
    for i in 0..node.len() {
        chain.push(node[..i].to_vec());
    }
    chain.push(node.to_vec());
    let mut found = None;
    for window in chain.windows(2) {
        let Some(parent) = get(root, &window[0]) else {
            return node.to_vec();
        };
        let parent_value = attr_or_empty(parent, "value");
        let parent_name = attr(parent, "name").unwrap_or("");
        if (parent_name == "Children" || parent_name == "SpecialImages")
            && !parent_value.starts_with("GcNGui")
        {
            found = Some(window[1].clone());
        }
    }
    found.unwrap_or_else(|| node.to_vec())
}

fn parent_and_widget(
    root: &Element,
    node: &[usize],
) -> Result<(Vec<usize>, usize), ExmlChangeError> {
    let widget = widget_entry(root, node);
    let Some((last, parent)) = widget.split_last() else {
        return Err(ExmlChangeError("target has no parent".into()));
    };
    Ok((parent.to_vec(), *last))
}

fn apply_values(section: &mut Element, change: &Map<String, Value>) -> Result<(), ExmlChangeError> {
    let math = match change.get("MATH_OPERATION") {
        Some(v) if py_truthy(Some(v)) => py_str(v),
        _ => String::new(),
    };
    let replace_all = change
        .get("REPLACE_TYPE")
        .map(|v| py_str(v).to_uppercase())
        .unwrap_or_default()
        == "ALL";
    let keys: HashMap<String, String> = pairs(change.get("SPECIAL_KEY_WORDS"))?
        .into_iter()
        .collect();
    let Some(table) = change.get("VALUE_CHANGE_TABLE") else {
        return Ok(());
    };
    for pair in as_list(table)? {
        let (name, new) = value_pair(&pair)?;
        let key_filter = keys.get(&name).cloned();
        let mut hits = Vec::new();
        for_each_el(section, &mut |el, path| {
            if attr(el, "name") != Some(name.as_str()) {
                return;
            }
            if let Some(want) = &key_filter {
                if attr_or_empty(el, "value") != want.as_str() {
                    return;
                }
            }
            hits.push(path.to_vec());
        });
        if hits.is_empty() {
            return Err(ExmlChangeError(format!(
                "no {name:?} inside the matched section"
            )));
        }
        let apply_to: Vec<Vec<usize>> = if replace_all {
            hits
        } else {
            vec![hits[0].clone()]
        };
        for path in apply_to {
            let el = get_mut(section, &path)
                .ok_or_else(|| ExmlChangeError("value target vanished".into()))?;
            let old = attr_or_empty(el, "value").to_string();
            let value = math_op(&old, &new, &math)?;
            set_value(el, value);
        }
    }
    Ok(())
}

fn as_list(value: &Value) -> Result<Vec<Value>, ExmlChangeError> {
    match value {
        Value::Array(items) => Ok(items.clone()),
        Value::Object(map) if map.keys().all(|k| is_index(k)) => {
            let mut items: Vec<(i64, Value)> = map
                .iter()
                .filter_map(|(k, v)| k.parse::<i64>().ok().map(|n| (n, v.clone())))
                .collect();
            items.sort_by_key(|(n, _)| *n);
            Ok(items.into_iter().map(|(_, v)| v).collect())
        }
        other => Err(ExmlChangeError(format!("expected a list, not {other}"))),
    }
}

fn value_pair(pair: &Value) -> Result<(String, String), ExmlChangeError> {
    match pair {
        Value::Object(map) => {
            if map.contains_key("1") && map.contains_key("2") {
                return Ok((
                    py_str(map.get("1").unwrap_or(&Value::Null)),
                    py_str(map.get("2").unwrap_or(&Value::Null)),
                ));
            }
            if map.len() == 1 {
                if let Some((name, new)) = map.iter().next() {
                    return Ok((name.clone(), py_str(new)));
                }
            }
            if map.keys().all(|k| is_index(k)) {
                let ordered: Vec<String> = as_list(pair)?.into_iter().map(|v| py_str(&v)).collect();
                if ordered.len() >= 2 {
                    return Ok((ordered[0].clone(), ordered[1].clone()));
                }
            }
        }
        Value::Array(items) if items.len() >= 2 => {
            return Ok((py_str(&items[0]), py_str(&items[1])));
        }
        _ => {}
    }
    Err(ExmlChangeError(format!(
        "VALUE_CHANGE_TABLE entry must be {{name, value}}, not {pair}"
    )))
}

fn math_op(old: &str, new: &str, operation: &str) -> Result<String, ExmlChangeError> {
    if operation.is_empty() {
        return Ok(new.to_string());
    }
    let left: f64 = old.parse().map_err(|_| {
        ExmlChangeError(format!(
            "MATH_OPERATION {operation:?} needs numbers, not {old:?} and {new:?}"
        ))
    })?;
    let right: f64 = new.parse().map_err(|_| {
        ExmlChangeError(format!(
            "MATH_OPERATION {operation:?} needs numbers, not {old:?} and {new:?}"
        ))
    })?;
    let value = match operation {
        "+" => left + right,
        "-" => left - right,
        "*" => left * right,
        "/" => {
            if right == 0.0 {
                return Err(ExmlChangeError(
                    "MATH_OPERATION '/' needs a non-zero divisor".into(),
                ));
            }
            left / right
        }
        other => {
            return Err(ExmlChangeError(format!(
                "unsupported MATH_OPERATION {other:?}"
            )))
        }
    };
    Ok(format!("{value:.6}"))
}

fn py_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(n)) => n.as_f64().map(|x| x != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn is_remove_section(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(true)) => true,
        Some(Value::String(s)) if s == "SECTION" || s == "TRUE" => true,
        _ => false,
    }
}

fn attr<'a>(el: &'a Element, key: &str) -> Option<&'a str> {
    el.attributes.get(key).map(String::as_str)
}

fn attr_or_empty<'a>(el: &'a Element, key: &str) -> &'a str {
    attr(el, key).unwrap_or("")
}

fn set_value(el: &mut Element, value: String) {
    if let Some(slot) = el.attributes.get_mut("value") {
        *slot = value;
    } else {
        el.attributes.insert("value".into(), value);
    }
}

fn for_each_el(el: &Element, visit: &mut impl FnMut(&Element, &[usize])) {
    fn rec(el: &Element, path: &mut Vec<usize>, visit: &mut impl FnMut(&Element, &[usize])) {
        visit(el, path);
        let mut i = 0;
        for child in &el.children {
            if let XMLNode::Element(child_el) = child {
                path.push(i);
                rec(child_el, path, visit);
                path.pop();
                i += 1;
            }
        }
    }
    rec(el, &mut Vec::new(), visit);
}

fn join_path(base: &[usize], rel: &[usize]) -> Vec<usize> {
    let mut out = base.to_vec();
    out.extend_from_slice(rel);
    out
}

fn child_el(el: &Element, index: usize) -> Option<&Element> {
    el.children
        .iter()
        .filter_map(|node| match node {
            XMLNode::Element(child) => Some(child),
            _ => None,
        })
        .nth(index)
}

fn child_el_mut(el: &mut Element, index: usize) -> Option<&mut Element> {
    el.children
        .iter_mut()
        .filter_map(|node| match node {
            XMLNode::Element(child) => Some(child),
            _ => None,
        })
        .nth(index)
}

fn get<'a>(root: &'a Element, path: &[usize]) -> Option<&'a Element> {
    let mut current = root;
    for &index in path {
        current = child_el(current, index)?;
    }
    Some(current)
}

fn get_mut<'a>(root: &'a mut Element, path: &[usize]) -> Option<&'a mut Element> {
    let mut current = root;
    for &index in path {
        current = child_el_mut(current, index)?;
    }
    Some(current)
}

fn insert_after(
    parent: &mut Element,
    widget_index: usize,
    new: Element,
) -> Result<(), ExmlChangeError> {
    let mut seen = 0;
    let mut at = None;
    for (i, node) in parent.children.iter().enumerate() {
        if matches!(node, XMLNode::Element(_)) {
            if seen == widget_index {
                at = Some(i + 1);
                break;
            }
            seen += 1;
        }
    }
    let at = at.ok_or_else(|| ExmlChangeError("target has no parent".into()))?;
    parent.children.insert(at, XMLNode::Element(new));
    Ok(())
}

fn remove_child(parent: &mut Element, widget_index: usize) -> Result<(), ExmlChangeError> {
    let mut seen = 0;
    let mut at = None;
    for (i, node) in parent.children.iter().enumerate() {
        if matches!(node, XMLNode::Element(_)) {
            if seen == widget_index {
                at = Some(i);
                break;
            }
            seen += 1;
        }
    }
    let at = at.ok_or_else(|| ExmlChangeError("target has no parent".into()))?;
    parent.children.remove(at);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn crate_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn repo_root() -> PathBuf {
        crate_dir().join("../../..")
    }

    fn data_dir() -> PathBuf {
        crate_dir().join("tests/data/exml")
    }

    fn load_changes(path: &Path) -> Vec<Value> {
        let text =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let value: Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        match value {
            Value::Array(items) => items,
            other => panic!("{}: expected array, got {other}", path.display()),
        }
    }

    fn element_eq(left: &Element, right: &Element) -> Result<(), String> {
        if left.name != right.name {
            return Err(format!("tag {} != {}", left.name, right.name));
        }
        let left_attrs: Vec<(&String, &String)> = left.attributes.iter().collect();
        let right_attrs: Vec<(&String, &String)> = right.attributes.iter().collect();
        if left_attrs != right_attrs {
            return Err(format!("attrs {left_attrs:?} != {right_attrs:?}"));
        }
        let left_els: Vec<&Element> = left
            .children
            .iter()
            .filter_map(|node| match node {
                XMLNode::Element(el) => Some(el),
                _ => None,
            })
            .collect();
        let right_els: Vec<&Element> = right
            .children
            .iter()
            .filter_map(|node| match node {
                XMLNode::Element(el) => Some(el),
                _ => None,
            })
            .collect();
        if left_els.len() != right_els.len() {
            return Err(format!(
                "child count {} != {}",
                left_els.len(),
                right_els.len()
            ));
        }
        for (a, b) in left_els.iter().zip(right_els) {
            element_eq(a, b)?;
        }
        Ok(())
    }

    fn ids(root: &Element) -> HashSet<String> {
        let mut found = HashSet::new();
        for_each_el(root, &mut |el, _| {
            if attr(el, "name") == Some("ID") {
                if let Some(value) = attr(el, "value") {
                    if !value.is_empty() {
                        found.insert(value.to_string());
                    }
                }
            }
        });
        found
    }

    fn pos(root: &Element, ident: &str, field: &str) -> Option<String> {
        let mut id_path = None;
        for_each_el(root, &mut |el, path| {
            if id_path.is_none()
                && attr(el, "name") == Some("ID")
                && attr(el, "value") == Some(ident)
            {
                id_path = Some(path.to_vec());
            }
        });
        let id_path = id_path?;
        if id_path.is_empty() {
            return None;
        }
        let parent_path = &id_path[..id_path.len() - 1];
        let element = get(root, parent_path)?;
        let layout = element.children.iter().find_map(|node| match node {
            XMLNode::Element(el) if attr(el, "name") == Some("Layout") => Some(el),
            _ => None,
        })?;
        let field_el = layout.children.iter().find_map(|node| match node {
            XMLNode::Element(el) if attr(el, "name") == Some(field) => Some(el),
            _ => None,
        })?;
        attr(field_el, "value").map(str::to_string)
    }

    #[test]
    fn small_cases_match_python() {
        for name in ["value_change", "math", "add", "remove"] {
            let folder = data_dir().join("cases").join(name);
            let mut got =
                Mxml::read(folder.join("input.mxml")).unwrap_or_else(|e| panic!("{name}: {e}"));
            let changes = load_changes(&folder.join("changes.json"));
            apply_change_table(&mut got, &changes).unwrap_or_else(|e| panic!("{name}: {e}"));
            let expected =
                Mxml::read(folder.join("expected.mxml")).unwrap_or_else(|e| panic!("{name}: {e}"));
            element_eq(&got.root, &expected.root).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn sidebar_matches_python() {
        let vanilla =
            repo_root().join("build/vanilla_sidebar/ui/components/pageselectbarside.MXML");
        let golden = repo_root().join("build/amumss_golden/pageselectbarside.MXML");
        if !vanilla.is_file() || !golden.is_file() {
            eprintln!("skip sidebar: build/ vanilla or golden MXML is missing");
            return;
        }
        let mut doc = Mxml::read(&vanilla).expect("vanilla");
        let sis = load_changes(&data_dir().join("SmallerInventorySlots.json"));
        let qta = load_changes(&data_dir().join("QuickDeposit.json"));
        apply_change_table(&mut doc, &sis).expect("SIS");
        apply_change_table(&mut doc, &qta).expect("QuickDeposit");
        let expected = Mxml::read(&golden).expect("golden");
        element_eq(&doc.root, &expected.root).expect("sidebar tree");

        let rust_bytes = doc.to_string();
        let py_bytes = std::fs::read_to_string(&golden).expect("read golden");
        if rust_bytes.as_bytes() == py_bytes.as_bytes() {
            eprintln!("sidebar write is byte-identical to Python");
        } else {
            eprintln!(
                "sidebar write is not byte-identical to Python (Rust {} bytes, Python {} bytes); trees match",
                rust_bytes.len(),
                py_bytes.len()
            );
        }

        let found = ids(&doc.root);
        for needed in [
            "FILTER6",
            "FILTER8",
            "FILTER17",
            "DEPOSIT",
            "LOCKMODE",
            "SORTBYTYPE",
            "FILTER1",
        ] {
            assert!(found.contains(needed), "missing {needed} after merge");
        }
        assert_eq!(
            pos(&doc.root, "DEPOSIT", "Position Y").as_deref(),
            Some("68.000000")
        );
        assert_eq!(
            pos(&doc.root, "LOCKMODE", "Position X").as_deref(),
            Some("32.631579")
        );
        assert_eq!(
            pos(&doc.root, "LOCKMODE", "Position Y").as_deref(),
            Some("68.000000")
        );
        assert_eq!(
            pos(&doc.root, "SORTBYTYPE", "Position Y").as_deref(),
            Some("76.000000")
        );
        assert_eq!(
            pos(&doc.root, "TECH_EXPAND", "Position Y").as_deref(),
            Some("350.000000")
        );
        assert_eq!(
            pos(&doc.root, "ITEM_EXPAND", "Position Y").as_deref(),
            Some("420.000000")
        );
        assert_eq!(
            pos(&doc.root, "FILTER6", "Position X").as_deref(),
            Some("0.000000")
        );
        assert_eq!(
            pos(&doc.root, "FILTER1", "Position X").as_deref(),
            Some("124.000000")
        );
        eprintln!("ok: FILTER6-17 and DEPOSIT are on the merged side bar");
    }
}
