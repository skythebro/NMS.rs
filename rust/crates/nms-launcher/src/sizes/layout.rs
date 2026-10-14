//! Port of `layout.py`: rewrite POPUP_INVENTORY and grow transfer/build popups.

use nms_amumss::exml::{Element, Mxml, XMLNode};
use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use super::grid::{py_g, Grid};
use super::{transform, Job};
use crate::tools::Tools;

const PRIVATE: &str = "UI/BIGGERITEMGRIDS";
const VANILLA_MSG: &str =
    "Grid is 4 by 2 in the vanilla window: layout mod removed, the game uses its own.";

pub fn build(
    grid: &Grid,
    mod_dir: &Path,
    pcbanks: &Path,
    tools: &Tools,
    work: &Path,
) -> Result<String, String> {
    if grid.vanilla() {
        if mod_dir.exists() {
            fs::remove_dir_all(mod_dir).map_err(|e| e.to_string())?;
        }
        return Ok(VANILLA_MSG.to_string());
    }
    for tool in [&tools.hgpaktool, &tools.mbincompiler] {
        if !tool.is_file() {
            return Err(format!("{} is missing.", tool.display()));
        }
    }
    let pak = pcbanks.join("NMSARC.Precache.pak");
    let (extra_w, extra_h) = grid.extra_size();
    let copies: Rc<RefCell<Vec<(String, String, Element)>>> = Rc::new(RefCell::new(Vec::new()));

    let mut jobs = vec![Job {
        source: "UI/POPUP_INVENTORY.MBIN".into(),
        target: "UI/POPUP_INVENTORY.MBIN".into(),
        apply: {
            let grid = *grid;
            Box::new(move |doc: &mut Mxml| rewrite(doc.root_mut(), &grid))
        },
    }];
    if extra_w != 0.0 || extra_h != 0.0 {
        for name in ["POPUP_TRANSFER", "POPUP_BUILD"] {
            let copies = copies.clone();
            jobs.push(Job {
                source: format!("UI/{name}.MBIN"),
                target: format!("UI/{name}.MBIN"),
                apply: Box::new(move |doc: &mut Mxml| {
                    let found = grow_popup(doc.root_mut(), name, extra_w, extra_h)?;
                    copies.borrow_mut().extend(found);
                    Ok(())
                }),
            });
        }
    }

    if work.exists() {
        fs::remove_dir_all(work).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let mut built = transform(&work.join("popups"), jobs, &pak, tools)?;
    let taken = copies.borrow_mut().drain(..).collect::<Vec<_>>();
    if !taken.is_empty() {
        let copy_jobs: Vec<Job> = taken
            .into_iter()
            .map(|(target, source, layout)| Job {
                source,
                target,
                apply: Box::new(move |doc: &mut Mxml| set_layout(doc.root_mut(), &layout)),
            })
            .collect();
        let more = transform(&work.join("copies"), copy_jobs, &pak, tools)?;
        built.extend(more);
    }

    let ui = mod_dir.join("UI");
    if ui.exists() {
        fs::remove_dir_all(&ui).map_err(|e| e.to_string())?;
    }
    for (target, path) in &built {
        let out = mod_dir.join(target);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(path, &out).map_err(|e| e.to_string())?;
    }
    let _ = fs::remove_dir_all(work);
    let (width, height) = grid.card_size();
    let window = if extra_w != 0.0 || extra_h != 0.0 {
        format!(
            ", window x{} (+{} wide, +{} tall)",
            py_g(grid.window_scale),
            py_g(extra_w),
            py_g(extra_h)
        )
    } else {
        String::new()
    };
    Ok(format!(
        "Layout {} by {} built, cards {} by {}{window}: {}",
        grid.columns,
        grid.rows,
        py_g(width),
        py_g(height),
        mod_dir.display()
    ))
}

pub fn rewrite(root: &mut Element, grid: &Grid) -> Result<(), String> {
    let inv_path = find_inventory(root)
        .ok_or_else(|| "POPUP_INVENTORY has no INVENTORY panel".to_string())?;
    let (slot01, navprev, navnext) = {
        let inventory = get(root, &inv_path)
            .ok_or_else(|| "POPUP_INVENTORY has no INVENTORY panel".to_string())?;
        let children = prop(data_of(inventory), "Children")
            .ok_or_else(|| "INVENTORY has no Children".to_string())?;
        for needed in ["SLOT01", "NAVPREV", "NAVNEXT"] {
            if child_by_id(children, needed).is_none() {
                return Err(format!("INVENTORY has no {needed}"));
            }
        }
        (
            child_by_id(children, "SLOT01").unwrap().clone(),
            child_by_id(children, "NAVPREV").unwrap().clone(),
            child_by_id(children, "NAVNEXT").unwrap().clone(),
        )
    };
    let (top_i, bottom_i) = arrow_backgrounds(root)?;
    let (width, height) = grid.card_size();
    let (extra_w, extra_h) = grid.extra_size();
    {
        let lay = layout_mut(root)?;
        add_field(lay, "Width", extra_w)?;
        add_field(lay, "Height", extra_h)?;
    }
    {
        let children = prop_mut(root, "Children").ok_or("expected Children on popup root")?;
        let top = nth_el_mut(children, top_i).ok_or("missing top arrow background")?;
        add_field(layout_mut(top)?, "Position X", extra_w)?;
    }
    {
        let children = prop_mut(root, "Children").ok_or("expected Children on popup root")?;
        let bottom = nth_el_mut(children, bottom_i).ok_or("missing bottom arrow background")?;
        let lay = layout_mut(bottom)?;
        add_field(lay, "Position X", extra_w)?;
        add_field(lay, "Position Y", extra_h)?;
    }
    let (top_layout, bottom_layout) = {
        let children = prop(root, "Children").ok_or("expected Children on popup root")?;
        let top = nth_el(children, top_i).ok_or("missing top arrow background")?;
        let bottom = nth_el(children, bottom_i).ok_or("missing bottom arrow background")?;
        (layout_of(top)?.clone(), layout_of(bottom)?.clone())
    };

    let mut ordered: Vec<Element> = Vec::new();
    for number in 1..=grid.items() {
        let mut card = slot01.clone();
        set_id(&mut card, &format!("SLOT{number:02}"))?;
        set_size(&mut card, width, height)?;
        let starts_row = number > 1 && (number - 1) % grid.columns == 0;
        let same = if starts_row { "false" } else { "true" };
        set_named(layout_mut(&mut card)?, "SameLine", same)?;
        ordered.push(card);
    }
    let mut prev_arrow = navprev;
    let mut next_arrow = navnext;
    set_layout(&mut prev_arrow, &top_layout)?;
    set_layout(&mut next_arrow, &bottom_layout)?;
    ordered.push(prev_arrow);
    ordered.push(next_arrow);

    let inventory = get_mut(root, &inv_path)
        .ok_or_else(|| "POPUP_INVENTORY has no INVENTORY panel".to_string())?;
    let data = data_of_mut(inventory);
    let children = prop_mut(data, "Children").ok_or("INVENTORY has no Children")?;
    children.children.clear();
    for entry in ordered {
        children.children.push(XMLNode::Element(entry));
    }
    Ok(())
}

pub fn grow_popup(
    root: &mut Element,
    owner: &str,
    extra_w: f64,
    extra_h: f64,
) -> Result<Vec<(String, String, Element)>, String> {
    let idxs = path_to(root, "UI/POPUP_INVENTORY.MBIN")
        .ok_or_else(|| "popup does not embed UI/POPUP_INVENTORY.MBIN".to_string())?;
    let top_percent = {
        let top = along_children(root, &[])?;
        is_percent(layout_of(top)?, "Width Percentage")?
    };
    if top_percent {
        let mut base = None;
        for n in 1..=idxs.len() {
            let e = along_children(root, &idxs[..n])?;
            let lay = layout_of(e)?;
            if !is_percent(lay, "Width Percentage")? {
                base = Some(parse_f64(
                    prop(lay, "Width").ok_or("missing Width on path")?,
                )?);
                break;
            }
        }
        let base = base.ok_or_else(|| "no fixed width below percent root".to_string())?;
        let top = along_children_mut(root, &[])?;
        let lay = layout_mut(top)?;
        let width = prop_mut(lay, "Width").ok_or("missing Width on popup root")?;
        let cur = parse_f64(width)?;
        set_value(width, format!("{:.6}", cur * (base + extra_w) / base));
    }
    for n in 0..=idxs.len() {
        let e = along_children_mut(root, &idxs[..n])?;
        let lay = layout_mut(e)?;
        let add_w = !is_percent(lay, "Width Percentage")?;
        let add_h = !is_percent(lay, "Height Percentage")?;
        if add_w {
            add_field(lay, "Width", extra_w)?;
        }
        if add_h {
            add_field(lay, "Height", extra_h)?;
        }
    }

    let mut copies: Vec<(String, String, Element)> = Vec::new();
    for depth in 0..idxs.len() {
        let child_index = idxs[depth];
        let child_y = {
            let child = along_children(root, &idxs[..=depth])?;
            parse_f64(prop(layout_of(child)?, "Position Y").ok_or("missing Position Y")?)?
        };
        let parent = along_children_mut(root, &idxs[..depth])?;
        let data = data_of_mut(parent);
        let children = prop_mut(data, "Children").ok_or("no Children on popup path")?;
        let count = el_count(children);
        for si in 0..count {
            if si == child_index {
                continue;
            }
            let sibling = nth_el_mut(children, si).ok_or("missing sibling")?;
            grow_sibling(sibling, child_y, extra_w, extra_h, owner, &mut copies)?;
        }
    }
    Ok(copies)
}

fn grow_sibling(
    sibling: &mut Element,
    child_y: f64,
    extra_w: f64,
    extra_h: f64,
    owner: &str,
    copies: &mut Vec<(String, String, Element)>,
) -> Result<(), String> {
    let mut moved = false;
    {
        let lay = layout_mut(sibling)?;
        let anchor = attr(prop(lay, "Anchor").ok_or("missing Anchor")?, "value");
        if anchor != Some("true") || is_percent(lay, "AnchorPercent")? {
            return Ok(());
        }
        let y = parse_f64(prop(lay, "Position Y").ok_or("missing Position Y")?)?;
        if y > child_y {
            add_field(lay, "Position Y", extra_h)?;
            moved = true;
        }
        let horizontal = attr(
            prop(
                prop(lay, "Align").ok_or("missing Align")?,
                "Horizontal",
            )
            .ok_or("missing Horizontal")?,
            "value",
        )
        .unwrap_or("")
        .to_string();
        if horizontal == "Center" {
            add_field(lay, "Position X", extra_w / 2.0)?;
            moved = true;
        } else if horizontal == "Right" {
            add_field(lay, "Position X", extra_w)?;
            moved = true;
        }
    }
    let source = prop(data_of(sibling), "DataFilename")
        .and_then(|e| attr(e, "value").filter(|v| !v.is_empty()))
        .map(str::to_string);
    if moved {
        if let Some(source) = source {
            let file = source.rsplit('/').next().unwrap_or(source.as_str());
            let target = format!("{PRIVATE}/{owner}_{file}");
            if let Some(embedded) = prop_mut(data_of_mut(sibling), "DataFilename") {
                set_value(embedded, target.clone());
            }
            copies.push((target, source, layout_of(sibling)?.clone()));
        }
    }
    Ok(())
}

fn set_layout(entry: &mut Element, layout: &Element) -> Result<(), String> {
    let data = data_of_mut(entry);
    let element = prop_mut(data, "Element Data").ok_or("no Element Data")?;
    let at = element
        .children
        .iter()
        .position(|n| n.as_element().map(|e| attr(e, "name") == Some("Layout")).unwrap_or(false))
        .ok_or("no Layout")?;
    element.children[at] = XMLNode::Element(layout.clone());
    Ok(())
}

fn path_to(entry: &Element, data_filename: &str) -> Option<Vec<usize>> {
    let data = data_of(entry);
    if let Some(embedded) = prop(data, "DataFilename") {
        if attr(embedded, "value") == Some(data_filename) && entry.name != "Data" {
            return Some(Vec::new());
        }
    }
    let children = prop(data, "Children")?;
    for (i, child) in element_children(children).enumerate() {
        if let Some(mut rest) = path_to(child, data_filename) {
            rest.insert(0, i);
            return Some(rest);
        }
    }
    None
}

fn find_inventory(root: &Element) -> Option<Vec<usize>> {
    let mut found = None;
    walk_elements(root, &mut Vec::new(), &mut |el, path| {
        if found.is_none()
            && el.name == "Property"
            && attr(el, "value") == Some("GcNGuiLayerData")
            && element_id(el) == Some("INVENTORY")
        {
            found = Some(path.to_vec());
        }
    });
    found
}

fn arrow_backgrounds(root: &Element) -> Result<(usize, usize), String> {
    let children = prop(root, "Children").ok_or("expected Children on popup root")?;
    let mut found: Vec<(f64, usize)> = Vec::new();
    for (i, entry) in element_children(children).enumerate() {
        if element_id(entry) == Some("INVENTORY") {
            continue;
        }
        let lay = match layout_of(entry) {
            Ok(l) => l,
            Err(_) => continue,
        };
        let Some(anchor) = prop(lay, "Anchor") else {
            continue;
        };
        if attr(anchor, "value") != Some("true") {
            continue;
        }
        let Some(pos_y) = prop(lay, "Position Y") else {
            continue;
        };
        let y = parse_f64(pos_y)?;
        found.push((y, i));
    }
    if found.len() != 2 {
        return Err(format!(
            "expected 2 arrow backgrounds in POPUP_INVENTORY, found {}",
            found.len()
        ));
    }
    found.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok((found[0].1, found[1].1))
}

fn walk_elements(el: &Element, path: &mut Vec<usize>, visit: &mut impl FnMut(&Element, &[usize])) {
    visit(el, path);
    let mut i = 0usize;
    for node in &el.children {
        if let Some(child) = node.as_element() {
            path.push(i);
            walk_elements(child, path, visit);
            path.pop();
            i += 1;
        }
    }
}

fn get<'a>(root: &'a Element, path: &[usize]) -> Option<&'a Element> {
    let mut cur = root;
    for &i in path {
        cur = nth_el(cur, i)?;
    }
    Some(cur)
}

fn get_mut<'a>(root: &'a mut Element, path: &[usize]) -> Option<&'a mut Element> {
    let mut cur = root;
    for &i in path {
        cur = nth_el_mut(cur, i)?;
    }
    Some(cur)
}

fn along_children<'a>(root: &'a Element, idxs: &[usize]) -> Result<&'a Element, String> {
    let mut cur = root;
    for &i in idxs {
        let kids = prop(data_of(cur), "Children").ok_or("no Children on popup path")?;
        cur = nth_el(kids, i).ok_or("broken popup path")?;
    }
    Ok(cur)
}

fn along_children_mut<'a>(root: &'a mut Element, idxs: &[usize]) -> Result<&'a mut Element, String> {
    let mut cur = root;
    for &i in idxs {
        let data = data_of_mut(cur);
        let kids = prop_mut(data, "Children").ok_or("no Children on popup path")?;
        cur = nth_el_mut(kids, i).ok_or("broken popup path")?;
    }
    Ok(cur)
}

fn data_of(entry: &Element) -> &Element {
    if let Some(first) = nth_el(entry, 0) {
        if attr(first, "name").unwrap_or("").starts_with("GcNGui") {
            return first;
        }
    }
    entry
}

fn data_of_mut(entry: &mut Element) -> &mut Element {
    let unwrap = nth_el(entry, 0)
        .map(|first| attr(first, "name").unwrap_or("").starts_with("GcNGui"))
        .unwrap_or(false);
    if unwrap {
        nth_el_mut(entry, 0).unwrap()
    } else {
        entry
    }
}

fn element_id(entry: &Element) -> Option<&str> {
    let element = prop(data_of(entry), "Element Data")?;
    let ident = prop(element, "ID")?;
    attr(ident, "value")
}

fn layout_of(entry: &Element) -> Result<&Element, String> {
    let element = prop(data_of(entry), "Element Data").ok_or("no Element Data")?;
    prop(element, "Layout").ok_or_else(|| "no Layout".to_string())
}

fn layout_mut(entry: &mut Element) -> Result<&mut Element, String> {
    let data = data_of_mut(entry);
    let element = prop_mut(data, "Element Data").ok_or("no Element Data")?;
    prop_mut(element, "Layout").ok_or_else(|| "no Layout".to_string())
}

fn set_id(entry: &mut Element, value: &str) -> Result<(), String> {
    let data = data_of_mut(entry);
    let element = prop_mut(data, "Element Data").ok_or("no Element Data")?;
    let ident = prop_mut(element, "ID").ok_or("no ID")?;
    set_value(ident, value.to_string());
    Ok(())
}

fn set_size(entry: &mut Element, width: f64, height: f64) -> Result<(), String> {
    let lay = layout_mut(entry)?;
    set_named(lay, "Width", &format!("{width:.6}"))?;
    set_named(lay, "Height", &format!("{height:.6}"))?;
    Ok(())
}

fn set_named(layout: &mut Element, name: &str, value: &str) -> Result<(), String> {
    let field = prop_mut(layout, name).ok_or_else(|| format!("no {name}"))?;
    set_value(field, value.to_string());
    Ok(())
}

fn add_field(layout: &mut Element, name: &str, amount: f64) -> Result<(), String> {
    let field = prop_mut(layout, name).ok_or_else(|| format!("no {name}"))?;
    let cur = parse_f64(field)?;
    set_value(field, format!("{:.6}", cur + amount));
    Ok(())
}

fn is_percent(layout: &Element, name: &str) -> Result<bool, String> {
    let field = prop(layout, name).ok_or_else(|| format!("no {name}"))?;
    Ok(attr(field, "value") == Some("true"))
}

fn child_by_id<'a>(children: &'a Element, id: &str) -> Option<&'a Element> {
    element_children(children).find(|e| element_id(e) == Some(id))
}

fn attr<'a>(el: &'a Element, key: &str) -> Option<&'a str> {
    el.attributes.get(key).map(String::as_str)
}

fn set_value(el: &mut Element, value: String) {
    el.attributes.insert("value".into(), value);
}

fn parse_f64(el: &Element) -> Result<f64, String> {
    let v = attr(el, "value").ok_or("missing value")?;
    v.parse()
        .map_err(|_| format!("not a float: {v}"))
}

fn prop<'a>(el: &'a Element, name: &str) -> Option<&'a Element> {
    element_children(el).find(|c| attr(c, "name") == Some(name))
}

fn prop_mut<'a>(el: &'a mut Element, name: &str) -> Option<&'a mut Element> {
    el.children
        .iter_mut()
        .filter_map(|n| n.as_mut_element())
        .find(|c| attr(c, "name") == Some(name))
}

fn element_children(el: &Element) -> impl Iterator<Item = &Element> {
    el.children.iter().filter_map(|n| n.as_element())
}

fn nth_el(parent: &Element, i: usize) -> Option<&Element> {
    parent.children.iter().filter_map(|n| n.as_element()).nth(i)
}

fn nth_el_mut(parent: &mut Element, i: usize) -> Option<&mut Element> {
    parent
        .children
        .iter_mut()
        .filter_map(|n| n.as_mut_element())
        .nth(i)
}

fn el_count(parent: &Element) -> usize {
    parent.children.iter().filter(|n| n.as_element().is_some()).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sizes::grid::Grid;

    const SNIPPET: &str = r#"<?xml version='1.0' encoding='utf-8'?>
<Data template="GcNGuiLayerData">
	<Property name="Element Data" value="GcNGuiElementData">
		<Property name="ID" value="POPUP" />
		<Property name="Layout" value="GcNGuiLayoutData">
			<Property name="Width" value="630.000000" />
			<Property name="Height" value="360.000000" />
		</Property>
	</Property>
	<Property name="Children">
		<Property value="GcNGuiLayerData">
			<Property name="Element Data" value="GcNGuiElementData">
				<Property name="ID" value="ARROWBG_TOP" />
				<Property name="Layout" value="GcNGuiLayoutData">
					<Property name="Position X" value="549.000000" />
					<Property name="Position Y" value="10.000000" />
					<Property name="Anchor" value="true" />
				</Property>
			</Property>
		</Property>
		<Property value="GcNGuiLayerData">
			<Property name="Element Data" value="GcNGuiElementData">
				<Property name="ID" value="ARROWBG_BOT" />
				<Property name="Layout" value="GcNGuiLayoutData">
					<Property name="Position X" value="549.000000" />
					<Property name="Position Y" value="200.000000" />
					<Property name="Anchor" value="true" />
				</Property>
			</Property>
		</Property>
		<Property value="GcNGuiLayerData">
			<Property name="Element Data" value="GcNGuiElementData">
				<Property name="ID" value="INVENTORY" />
				<Property name="Layout" value="GcNGuiLayoutData">
					<Property name="Width" value="540.000000" />
				</Property>
			</Property>
			<Property name="Children">
				<Property value="GcNGuiLayerData">
					<Property name="Element Data" value="GcNGuiElementData">
						<Property name="ID" value="SLOT01" />
						<Property name="Layout" value="GcNGuiLayoutData">
							<Property name="Width" value="112.000000" />
							<Property name="Height" value="131.000000" />
							<Property name="SameLine" value="true" />
						</Property>
					</Property>
				</Property>
				<Property value="GcNGuiLayerData">
					<Property name="Element Data" value="GcNGuiElementData">
						<Property name="ID" value="NAVPREV" />
						<Property name="Layout" value="GcNGuiLayoutData">
							<Property name="Position X" value="0.000000" />
						</Property>
					</Property>
				</Property>
				<Property value="GcNGuiLayerData">
					<Property name="Element Data" value="GcNGuiElementData">
						<Property name="ID" value="NAVNEXT" />
						<Property name="Layout" value="GcNGuiLayoutData">
							<Property name="Position X" value="0.000000" />
						</Property>
					</Property>
				</Property>
			</Property>
		</Property>
	</Property>
</Data>
"#;

    #[test]
    fn rewrite_handmade_snippet() {
        let mut doc = Mxml::parse(SNIPPET).expect("parse snippet");
        let grid = Grid::new(2, 2, 1.0);
        rewrite(doc.root_mut(), &grid).expect("rewrite");
        let root = doc.root();
        let inv = find_inventory(root).and_then(|p| get(root, &p)).expect("inventory");
        let children = prop(data_of(inv), "Children").expect("children");
        let ids: Vec<String> = element_children(children)
            .filter_map(|e| element_id(e).map(str::to_string))
            .collect();
        assert_eq!(
            ids,
            ["SLOT01", "SLOT02", "SLOT03", "SLOT04", "NAVPREV", "NAVNEXT"]
        );
        let slot03 = child_by_id(children, "SLOT03").unwrap();
        let same = attr(prop(layout_of(slot03).unwrap(), "SameLine").unwrap(), "value");
        assert_eq!(same, Some("false"));
        let slot01 = child_by_id(children, "SLOT01").unwrap();
        let same01 = attr(prop(layout_of(slot01).unwrap(), "SameLine").unwrap(), "value");
        assert_eq!(same01, Some("true"));
        let w = attr(prop(layout_of(slot01).unwrap(), "Width").unwrap(), "value");
        assert_eq!(w, Some("102.500000"));
        let nav = child_by_id(children, "NAVPREV").unwrap();
        let px = attr(prop(layout_of(nav).unwrap(), "Position X").unwrap(), "value");
        assert_eq!(px, Some("549.000000"));
        let root_w = attr(prop(layout_of(root).unwrap(), "Width").unwrap(), "value");
        assert_eq!(root_w, Some("630.000000"));
    }
}
