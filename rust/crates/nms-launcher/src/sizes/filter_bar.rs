//! Port of `filter_bar.py`: extra category buttons and the locker sort column.

use super::grid::py_g;
use super::inventory::xml;
use nms_amumss::exml::Element;

pub const SIDE_BAR: &str = "UI/COMPONENTS/PAGESELECTBARSIDE.MBIN";
pub const VANILLA_BUTTONS: i32 = 5;
pub const VANILLA_SIZE: f64 = 60.0;
pub const PITCH: f64 = 70.0;
pub const COLUMNS: i32 = 2;
pub const COLUMN_PITCH: f64 = 62.0;
pub const HOST_WIDTH: f64 = 66.0;
pub const EXTRA: f64 = COLUMNS as f64 * COLUMN_PITCH;
pub const CANVAS: f64 = 3840.0;
pub const SCREEN: f64 = 1920.0;
pub const SHIFT: f64 = 24.0;
pub const CENTRE: f64 = 50.0;
pub const PAGE_CHILDREN: usize = 13;
pub const ICON_ROOT: &str = "TEXTURES/UI/FRONTEND/ICONS/";

const MOVES_RIGHT: &[&str] = &[
    "TABTITLE",
    "SQU_TECH_BIG",
    "SQU_ITEM_BIG",
    "SQU_INV_TECH",
    "SQU_INV_REGULAR",
    "FILTERS",
    "CARGOHEADER",
    "TECHHEADER",
];

/// (cGcProductCategory, icon). Treasures (5), Creature eggs (8) and Bones (10) use the mod's own
/// glyphs from `dev/make_filter_icons.py`, shipped in the SmallerInventorySlots folder.
const ICONS: &[(i32, &str)] = &[
    (0, "WIKI/WIKI.CRAFTEDPROD.OFF.DDS"),
    (2, "WIKI/WIKI.TRADEPRODS.OFF.DDS"),
    (3, "WIKI/WIKI.CURIO.OFF.DDS"),
    (5, "CATEGORIES/FILTERTREASURE.DDS"),
    (8, "CATEGORIES/FILTEREGG.DDS"),
    (9, "MISSIONS/MISSION.FISHROUND.OFF.DDS"),
    (10, "CATEGORIES/FILTERBONE.DDS"),
    (11, "MISSIONS/MISSION.SPACEHULK.OFF.DDS"),
];

/// Icons the mod ships itself (everything else is a vanilla texture). A side bar built before
/// these existed lacks some of them; `mods::side_bar_outdated` looks for all of them.
pub const OWN_ICONS: &[&str] = &[
    "CATEGORIES/FILTERTREASURE.DDS",
    "CATEGORIES/FILTEREGG.DDS",
    "CATEGORIES/FILTERBONE.DDS",
    "CATEGORIES/FILTERFOODS.DDS",
    "CATEGORIES/FILTERCUSTOM.DDS",
];

/// Buttons after the category block, by button number, placed after it in order: FILTER18
/// (Foods, filter id 17) and FILTER19 (Custom, filter id 18) fill row 5 beside FILTER5. The
/// slot mod hides them until their switch is on; SmallerInventorySlots.lua adds the same two.
const EXTRAS: &[(i32, &str)] = &[
    (18, "CATEGORIES/FILTERFOODS.DDS"),
    (19, "CATEGORIES/FILTERCUSTOM.DDS"),
];

/// Every added button as (button number, icon), in placement order.
fn numbered() -> Vec<(i32, &'static str)> {
    ICONS
        .iter()
        .map(|(category, icon)| (VANILLA_BUTTONS + 1 + category, *icon))
        .chain(EXTRAS.iter().copied())
        .collect()
}

pub const SORT_NAMES: &[&str] = &["SORTBYVALUE", "SORTBYTYPE", "SORTBYNAME", "SORTBYCOLOUR"];
pub const LOCKER_SORT_X: f64 = 1891.0;
pub const LOCKER_SORT_BOTTOM: f64 = 1440.0;
pub const LOCKER_SORT_STEP: f64 = 68.0;
pub const LOCKER_SORT_SIZE: f64 = 60.0;
pub const LOCKER_SORT_ORDER: &[&str] = &["SORTBYTYPE", "SORTBYCOLOUR", "SORTBYNAME", "SORTBYVALUE"];

pub fn copy_sort_buttons(root: &Element) -> Result<Vec<Element>, String> {
    let mut out = Vec::new();
    for name in SORT_NAMES {
        let found = xml::find(root, name);
        if found.len() != 1 {
            return Err(
                "the side bar does not have exactly one of each sort button: the game changed its layout"
                    .into(),
            );
        }
        out.push(found[0].clone());
    }
    Ok(out)
}

pub fn add_locker_sort(root: &mut Element, mut buttons: Vec<Element>) -> Result<(), String> {
    if buttons.len() != SORT_NAMES.len() {
        return Err(format!(
            "expected {} sort buttons, got {}",
            SORT_NAMES.len(),
            buttons.len()
        ));
    }
    let pages = xml::find_paths(root, "PAGECONTENT");
    if pages.len() != 1 {
        return Err(format!(
            "expected one PAGECONTENT on the locker page, found {}",
            pages.len()
        ));
    }
    {
        let page = xml::along_children(root, &pages[0]).ok_or("PAGECONTENT")?;
        if !xml::find(page, "FILTERS").is_empty() {
            return Err(
                "the locker page already has a FILTERS layer: the game changed its layout".into(),
            );
        }
    }
    let mut host = buttons[0].clone();
    xml::set_id(&mut host, "FILTERS")?;
    if let Some(children) = xml::children_mut(&mut host) {
        xml::clear_children(children);
    }
    xml::set_flag(&mut host, "Is Hidden", "false");
    xml::set_flag(&mut host, "IgnoreInput", "false");
    let height = LOCKER_SORT_STEP * (buttons.len() as f64 - 1.0) + LOCKER_SORT_SIZE;
    xml::pin(
        &mut host,
        LOCKER_SORT_X,
        LOCKER_SORT_BOTTOM - height,
        LOCKER_SORT_SIZE,
        height,
    )?;
    {
        let lay = xml::layout_mut(&mut host)?;
        for name in ["Constrain Proportions", "Force Aspect"] {
            if let Some(flag) = xml::prop_mut(lay, name) {
                xml::set_value(flag, "false".into());
            }
        }
    }
    for button in &mut buttons {
        let id = xml::element_id(button).ok_or("sort button has no ID")?;
        let row = LOCKER_SORT_ORDER
            .iter()
            .position(|n| *n == id)
            .ok_or_else(|| format!("unknown sort button {id}"))?;
        xml::pin(
            button,
            0.0,
            row as f64 * LOCKER_SORT_STEP,
            LOCKER_SORT_SIZE,
            LOCKER_SORT_SIZE,
        )?;
        xml::set_flag(button, "Is Hidden", "false");
    }
    {
        let kids = xml::children_mut(&mut host).ok_or("FILTERS host has no Children")?;
        for button in buttons {
            xml::append_child(kids, button);
        }
    }
    let page = xml::along_children_mut(root, &pages[0]).ok_or("PAGECONTENT")?;
    let kids = xml::children_mut(page).ok_or("PAGECONTENT has no Children")?;
    xml::append_child(kids, host);
    Ok(())
}

pub fn place(index: i32) -> (f64, f64) {
    let column = index % COLUMNS;
    let row = index / COLUMNS;
    (
        -(COLUMNS - column) as f64 * COLUMN_PITCH,
        row as f64 * PITCH,
    )
}

pub fn side_bar(root: &mut Element) -> Result<(), String> {
    let paths = xml::find_paths(root, "FILTERS");
    if paths.len() != 1 {
        return Err(format!(
            "expected one FILTERS layer in the side bar, found {}: the game changed its layout",
            paths.len()
        ));
    }
    let fpath = paths[0].clone();
    check_vanilla_filters(root, &fpath)?;
    shift_holder(root, &fpath)?;
    let extra_ids: Vec<(String, String, f64, f64)> = {
        numbered()
            .into_iter()
            .enumerate()
            .map(|(index, (number, icon))| {
                let (x, y) = place(index as i32);
                (
                    format!("FILTER{number}"),
                    format!("{ICON_ROOT}{icon}"),
                    x + EXTRA,
                    y,
                )
            })
            .collect()
    };
    let layer = xml::along_children_mut(root, &fpath).ok_or("FILTERS")?;
    xml::set_layout_field(layer, "Width", xml::get_f(layer, "Width")? + EXTRA)?;
    let n = xml::child_count(layer);
    let mut model_at = None;
    for i in 0..n {
        let child = xml::child_mut(layer, i).ok_or("FILTERS child")?;
        xml::set_layout_field(child, "Position X", xml::get_f(child, "Position X")? + EXTRA)?;
        if xml::element_id(child) == Some(&format!("FILTER{VANILLA_BUTTONS}")) {
            model_at = Some(i);
        }
    }
    let model_at = model_at.ok_or("FILTER5 missing after shift")?;
    let model = xml::child_mut(layer, model_at)
        .ok_or("FILTER5")?
        .clone();
    let at = model_at + 1;
    for (index, (id, icon, x, y)) in extra_ids.into_iter().enumerate() {
        let mut button = model.clone();
        xml::set_id(&mut button, &id)?;
        xml::set_layout_field(&mut button, "Position X", x)?;
        xml::set_layout_field(&mut button, "Position Y", y)?;
        let icon_paths = xml::find_paths(&button, "ICON");
        for ip in icon_paths {
            let image = xml::along_children_mut(&mut button, &ip).ok_or("ICON")?;
            let data = xml::data_of_mut(image);
            if let Some(img) = xml::prop_mut(data, "Image") {
                xml::set_value(img, icon.clone());
            }
        }
        let kids = xml::children_mut(layer).ok_or("FILTERS Children")?;
        xml::insert_child(kids, at + index, button);
    }
    Ok(())
}

fn check_vanilla_filters(root: &Element, fpath: &[usize]) -> Result<(), String> {
    let layer = xml::along_children(root, fpath).ok_or("FILTERS")?;
    let kids = xml::children_of(layer);
    let names: Vec<Option<&str>> = kids.iter().map(|c| xml::element_id(c)).collect();
    for n in 1..=VANILLA_BUTTONS {
        let want = format!("FILTER{n}");
        if !names.iter().any(|id| *id == Some(want.as_str())) {
            return Err(
                "the side bar does not have exactly FILTER1-FILTER5: the game changed its layout"
                    .into(),
            );
        }
    }
    if names.iter().any(|id| *id == Some("FILTER6")) {
        return Err(
            "the side bar does not have exactly FILTER1-FILTER5: the game changed its layout".into(),
        );
    }
    let model = kids
        .iter()
        .find(|c| xml::element_id(c) == Some("FILTER5"))
        .ok_or("FILTER5")?;
    let size = (xml::get_f(model, "Width")?, xml::get_f(model, "Height")?);
    if size != (VANILLA_SIZE, VANILLA_SIZE) {
        return Err(
            "the side buttons are not 60 by 60 any more: the game changed its layout".into(),
        );
    }
    for state in ["ON", "OFF"] {
        let found = xml::find(model, state);
        if found.len() != 1 || xml::find(found[0], "ICON").len() != 1 {
            return Err(format!(
                "a side button has no single {state} state with an ICON: the game changed its layout"
            ));
        }
    }
    Ok(())
}

fn shift_holder(root: &mut Element, fpath: &[usize]) -> Result<(), String> {
    let holder_path: Vec<usize> = if fpath.is_empty() {
        Vec::new()
    } else {
        fpath[..fpath.len() - 1].to_vec()
    };
    let filter_index = fpath.last().copied();
    {
        let holder = if holder_path.is_empty() {
            &*root
        } else {
            xml::along_children(root, &holder_path).ok_or("FILTERS holder")?
        };
        let lay = xml::layout_of(holder)?;
        let pct = xml::prop(lay, "Width Percentage").ok_or("no Width Percentage")?;
        if xml::attr(pct, "value") != Some("true") {
            return Err(
                "FILTERS is not in one full-width layer of the side bar: the game changed its layout"
                    .into(),
            );
        }
    }
    let n = {
        let holder = if holder_path.is_empty() {
            &*root
        } else {
            xml::along_children(root, &holder_path).ok_or("FILTERS holder")?
        };
        xml::child_count(holder)
    };
    for i in 0..n {
        if Some(i) == filter_index {
            continue;
        }
        let child = {
            let holder = if holder_path.is_empty() {
                &mut *root
            } else {
                xml::along_children_mut(root, &holder_path).ok_or("FILTERS holder")?
            };
            xml::child_mut(holder, i).ok_or("holder child")?
        };
        if xml::horizontal(child) != Some("Left") {
            continue;
        }
        let step = if xml::is_percent(child)? {
            EXTRA / (HOST_WIDTH + EXTRA) * 100.0
        } else {
            EXTRA
        };
        xml::set_layout_field(child, "Position X", xml::get_f(child, "Position X")? + step)?;
    }
    Ok(())
}

pub fn widen_page(root: &mut Element) -> Result<(), String> {
    let filters = xml::find_paths(root, "FILTERS");
    if filters.len() != 2 {
        return Err(format!(
            "the inventory page does not have two FILTERS layers {} wide: the game changed its layout",
            py_g(HOST_WIDTH)
        ));
    }
    for path in &filters {
        let layer = xml::along_children(root, path).ok_or("FILTERS")?;
        if xml::get_f(layer, "Width")? != HOST_WIDTH {
            return Err(format!(
                "the inventory page does not have two FILTERS layers {} wide: the game changed its layout",
                py_g(HOST_WIDTH)
            ));
        }
    }
    for path in filters {
        let layer = xml::along_children_mut(root, &path).ok_or("FILTERS")?;
        xml::set_layout_field(layer, "Position X", xml::get_f(layer, "Position X")? - EXTRA)?;
        xml::set_layout_field(layer, "Width", xml::get_f(layer, "Width")? + EXTRA)?;
    }
    let canvases = xml::find_paths(root, "PAGECONTENT");
    if canvases.len() != 2 {
        return Err(format!(
            "expected two PAGECONTENT layers on the inventory page, found {}: the game changed its layout",
            canvases.len()
        ));
    }
    for path in canvases {
        widen_canvas(root, &path)?;
    }
    Ok(())
}

fn widen_canvas(root: &mut Element, path: &[usize]) -> Result<(), String> {
    {
        let canvas = xml::along_children(root, path).ok_or("PAGECONTENT")?;
        if xml::get_f(canvas, "Position X")? != CENTRE
            || xml::get_f(canvas, "Width")? != CANVAS
            || !xml::is_percent(canvas)?
        {
            return Err(
                "PAGECONTENT is not a centred 3840 wide canvas anchored by percent: the game changed its layout"
                    .into(),
            );
        }
        let children = xml::children_of(canvas);
        if children.len() != PAGE_CHILDREN {
            return Err(
                "PAGECONTENT does not hold the elements it used to: the game changed its layout"
                    .into(),
            );
        }
        let mut moving_n = 0;
        for child in &children {
            if is_moving(child) {
                moving_n += 1;
            }
        }
        if moving_n != MOVES_RIGHT.len() {
            return Err(
                "PAGECONTENT does not hold the elements it used to: the game changed its layout"
                    .into(),
            );
        }
    }
    let n = {
        let canvas = xml::along_children(root, path).ok_or("PAGECONTENT")?;
        xml::child_count(canvas)
    };
    let mut moving = vec![false; n];
    {
        let canvas = xml::along_children(root, path).ok_or("PAGECONTENT")?;
        for (i, child) in xml::children_of(canvas).into_iter().enumerate() {
            moving[i] = is_moving(child);
        }
    }
    {
        let canvas = xml::along_children_mut(root, path).ok_or("PAGECONTENT")?;
        xml::set_layout_field(canvas, "Position X", CENTRE + SHIFT / SCREEN * 100.0)?;
    }
    for i in 0..n {
        if moving[i] {
            continue;
        }
        let child = {
            let canvas = xml::along_children_mut(root, path).ok_or("PAGECONTENT")?;
            xml::child_mut(canvas, i).ok_or("PAGECONTENT child")?
        };
        let step = if xml::is_percent(child)? {
            SHIFT / CANVAS * 100.0
        } else {
            SHIFT
        };
        xml::set_layout_field(child, "Position X", xml::get_f(child, "Position X")? - step)?;
    }
    Ok(())
}

fn is_moving(child: &Element) -> bool {
    let mut names = Vec::new();
    if let Some(id) = xml::element_id(child) {
        names.push(id);
    }
    for grand in xml::children_of(child) {
        if let Some(id) = xml::element_id(grand) {
            names.push(id);
        }
    }
    names.iter().any(|id| MOVES_RIGHT.contains(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nms_amumss::exml::Mxml;

    fn layer(id: &str, x: &str, w: &str, extra: &str) -> String {
        format!(
            r#"<Property value="GcNGuiLayerData">
			<Property name="Element Data" value="GcNGuiElementData">
				<Property name="ID" value="{id}" />
				<Property name="Layout" value="GcNGuiLayoutData">
					<Property name="Position X" value="{x}" />
					<Property name="Width" value="{w}" />
					<Property name="AnchorPercent" value="false" />
					<Property name="Align" value="GcNGuiAlignment">
						<Property name="Horizontal" value="Left" />
						<Property name="Vertical" value="Top" />
					</Property>
					{extra}
				</Property>
			</Property>
			<Property name="Children" />
		</Property>"#
        )
    }

    #[test]
    fn widen_page_handmade() {
        let moving: String = [
            "TABTITLE",
            "SQU_TECH_BIG",
            "SQU_ITEM_BIG",
            "SQU_INV_TECH",
            "SQU_INV_REGULAR",
            "FILTERS",
            "CARGOHEADER",
            "TECHHEADER",
        ]
        .iter()
        .map(|id| {
            let w = if *id == "FILTERS" { "66.000000" } else { "10.000000" };
            layer(id, "100.000000", w, "")
        })
        .collect();
        let rest: String = ["A", "B", "C", "D", "E"]
            .iter()
            .map(|id| layer(id, "100.000000", "10.000000", ""))
            .collect();
        let canvas = |suffix: &str| {
            format!(
                r#"<Property value="GcNGuiLayerData">
			<Property name="Element Data" value="GcNGuiElementData">
				<Property name="ID" value="PAGECONTENT" />
				<Property name="Layout" value="GcNGuiLayoutData">
					<Property name="Position X" value="50.000000" />
					<Property name="Width" value="3840.000000" />
					<Property name="AnchorPercent" value="true" />
				</Property>
			</Property>
			<Property name="Children">
				{moving}{rest}
			</Property>
		</Property>"#
            )
            .replace("PAGECONTENT", &format!("PAGECONTENT{suffix}"))
            .replace("PAGECONTENT", "PAGECONTENT")
        };
        // Keep both IDs as PAGECONTENT.
        let page = format!(
            r#"<?xml version='1.0' encoding='utf-8'?>
<Data template="GcNGuiLayerData">
	<Property name="Element Data" value="GcNGuiElementData">
		<Property name="ID" value="ROOT" />
		<Property name="Layout" value="GcNGuiLayoutData">
			<Property name="Width" value="1.000000" />
		</Property>
	</Property>
	<Property name="Children">
		{}
		{}
	</Property>
</Data>
"#,
            canvas("").replace("PAGECONTENT", "PAGECONTENT"),
            canvas("").replace("PAGECONTENT", "PAGECONTENT")
        );
        let mut doc = Mxml::parse(&page).expect("parse");
        widen_page(doc.root_mut()).expect("widen");
        let filters = xml::find(doc.root(), "FILTERS");
        assert_eq!(filters.len(), 2);
        assert_eq!(xml::get_f(filters[0], "Width").unwrap(), 190.0);
        assert_eq!(
            xml::attr(
                xml::prop(xml::layout_of(filters[0]).unwrap(), "Position X").unwrap(),
                "value"
            ),
            Some("-24.000000")
        );
        let pages = xml::find(doc.root(), "PAGECONTENT");
        assert_eq!(
            xml::attr(
                xml::prop(xml::layout_of(pages[0]).unwrap(), "Position X").unwrap(),
                "value"
            ),
            Some("51.250000")
        );
        let a = xml::find(doc.root(), "A");
        assert_eq!(
            xml::attr(
                xml::prop(xml::layout_of(a[0]).unwrap(), "Position X").unwrap(),
                "value"
            ),
            Some("76.000000")
        );
        let tab = xml::find(doc.root(), "TABTITLE");
        assert_eq!(
            xml::attr(
                xml::prop(xml::layout_of(tab[0]).unwrap(), "Position X").unwrap(),
                "value"
            ),
            Some("100.000000")
        );
    }

    fn filter_button(n: i32) -> String {
        format!(
            r#"<Property value="GcNGuiLayerData">
			<Property name="Element Data" value="GcNGuiElementData">
				<Property name="ID" value="FILTER{n}" />
				<Property name="Layout" value="GcNGuiLayoutData">
					<Property name="Position X" value="0.000000" />
					<Property name="Position Y" value="0.000000" />
					<Property name="Width" value="60.000000" />
					<Property name="Height" value="60.000000" />
					<Property name="AnchorPercent" value="false" />
					<Property name="Align" value="GcNGuiAlignment">
						<Property name="Horizontal" value="Left" />
						<Property name="Vertical" value="Top" />
					</Property>
				</Property>
			</Property>
			<Property name="Children">
				<Property value="GcNGuiLayerData">
					<Property name="Element Data" value="GcNGuiElementData">
						<Property name="ID" value="ON" />
						<Property name="Layout" value="GcNGuiLayoutData">
							<Property name="Width" value="60.000000" />
						</Property>
					</Property>
					<Property name="Children">
						<Property value="GcNGuiLayerData">
							<Property name="Element Data" value="GcNGuiElementData">
								<Property name="ID" value="ICON" />
								<Property name="Layout" value="GcNGuiLayoutData">
									<Property name="Width" value="60.000000" />
								</Property>
							</Property>
							<Property name="Image" value="OLD.DDS" />
						</Property>
					</Property>
				</Property>
				<Property value="GcNGuiLayerData">
					<Property name="Element Data" value="GcNGuiElementData">
						<Property name="ID" value="OFF" />
						<Property name="Layout" value="GcNGuiLayoutData">
							<Property name="Width" value="60.000000" />
						</Property>
					</Property>
					<Property name="Children">
						<Property value="GcNGuiLayerData">
							<Property name="Element Data" value="GcNGuiElementData">
								<Property name="ID" value="ICON" />
								<Property name="Layout" value="GcNGuiLayoutData">
									<Property name="Width" value="60.000000" />
								</Property>
							</Property>
							<Property name="Image" value="OLD.DDS" />
						</Property>
					</Property>
				</Property>
			</Property>
		</Property>"#
        )
    }

    #[test]
    fn side_bar_handmade() {
        let buttons: String = (1..=5).map(filter_button).collect();
        let xml_text = format!(
            r#"<?xml version='1.0' encoding='utf-8'?>
<Data template="GcNGuiLayerData">
	<Property name="Element Data" value="GcNGuiElementData">
		<Property name="ID" value="HOLDER" />
		<Property name="Layout" value="GcNGuiLayoutData">
			<Property name="Width Percentage" value="true" />
			<Property name="Width" value="100.000000" />
		</Property>
	</Property>
	<Property name="Children">
		<Property value="GcNGuiLayerData">
			<Property name="Element Data" value="GcNGuiElementData">
				<Property name="ID" value="FILTERS" />
				<Property name="Layout" value="GcNGuiLayoutData">
					<Property name="Width" value="66.000000" />
					<Property name="Position X" value="0.000000" />
					<Property name="AnchorPercent" value="false" />
					<Property name="Align" value="GcNGuiAlignment">
						<Property name="Horizontal" value="Left" />
						<Property name="Vertical" value="Top" />
					</Property>
				</Property>
			</Property>
			<Property name="Children">
				{buttons}
			</Property>
		</Property>
		<Property value="GcNGuiLayerData">
			<Property name="Element Data" value="GcNGuiElementData">
				<Property name="ID" value="SORTBYVALUE" />
				<Property name="Layout" value="GcNGuiLayoutData">
					<Property name="Position X" value="0.000000" />
					<Property name="Width" value="60.000000" />
					<Property name="AnchorPercent" value="false" />
					<Property name="Align" value="GcNGuiAlignment">
						<Property name="Horizontal" value="Left" />
						<Property name="Vertical" value="Top" />
					</Property>
				</Property>
			</Property>
			<Property name="Children" />
		</Property>
	</Property>
</Data>
"#
        );
        let mut doc = Mxml::parse(&xml_text).expect("parse");
        side_bar(doc.root_mut()).expect("side_bar");
        let six = xml::find(doc.root(), "FILTER6");
        assert_eq!(six.len(), 1);
        assert_eq!(
            xml::attr(
                xml::prop(xml::layout_of(six[0]).unwrap(), "Position X").unwrap(),
                "value"
            ),
            Some("0.000000")
        );
        let icons = xml::find(six[0], "ICON");
        assert_eq!(
            xml::attr(xml::prop(xml::data_of(icons[0]), "Image").unwrap(), "value"),
            Some("TEXTURES/UI/FRONTEND/ICONS/WIKI/WIKI.CRAFTEDPROD.OFF.DDS")
        );
        let five = xml::find(doc.root(), "FILTER5");
        assert_eq!(
            xml::attr(
                xml::prop(xml::layout_of(five[0]).unwrap(), "Position X").unwrap(),
                "value"
            ),
            Some("124.000000")
        );
        let sort = xml::find(doc.root(), "SORTBYVALUE");
        assert_eq!(
            xml::attr(
                xml::prop(xml::layout_of(sort[0]).unwrap(), "Position X").unwrap(),
                "value"
            ),
            Some("124.000000")
        );
        let layer = xml::find(doc.root(), "FILTERS");
        assert_eq!(xml::get_f(layer[0], "Width").unwrap(), 190.0);

        // Foods and Custom fill row 5 of the block, beside FILTER5, as the shipped lua puts them.
        for (id, x, icon) in [
            ("FILTER18", "0.000000", "CATEGORIES/FILTERFOODS.DDS"),
            ("FILTER19", "62.000000", "CATEGORIES/FILTERCUSTOM.DDS"),
        ] {
            let found = xml::find(doc.root(), id);
            assert_eq!(found.len(), 1, "{id}");
            let lay = xml::layout_of(found[0]).unwrap();
            assert_eq!(xml::attr(xml::prop(lay, "Position X").unwrap(), "value"), Some(x));
            assert_eq!(
                xml::attr(xml::prop(lay, "Position Y").unwrap(), "value"),
                Some("280.000000")
            );
            let icons = xml::find(found[0], "ICON");
            assert_eq!(icons.len(), 2, "{id} ON and OFF icons");
            for image in icons {
                assert_eq!(
                    xml::attr(xml::prop(xml::data_of(image), "Image").unwrap(), "value"),
                    Some(format!("{ICON_ROOT}{icon}").as_str())
                );
            }
        }
        // 5 vanilla + 8 categories + Foods + Custom, no duplicates, and nothing past FILTER19.
        let ids: Vec<String> = xml::children_of(layer[0])
            .iter()
            .filter_map(|c| xml::element_id(c).map(str::to_string))
            .collect();
        assert_eq!(ids.len(), 15);
        let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), 15);
        assert!(xml::find(doc.root(), "FILTER20").is_empty());
    }

    #[test]
    fn added_buttons_match_lua_slots() {
        let all = numbered();
        let numbers: Vec<i32> = all.iter().map(|(n, _)| *n).collect();
        assert_eq!(numbers, vec![6, 8, 9, 11, 14, 15, 16, 17, 18, 19]);
        // Positions after the shift by EXTRA, as SmallerInventorySlots.lua writes them.
        let placed: Vec<(f64, f64)> = (0..all.len() as i32)
            .map(|i| {
                let (x, y) = place(i);
                (x + EXTRA, y)
            })
            .collect();
        assert_eq!(placed[8], (0.0, 280.0));
        assert_eq!(placed[9], (62.0, 280.0));
        // Row 5 is level with FILTER5 (fifth vanilla button at the vanilla pitch).
        assert_eq!(placed[8].1, (VANILLA_BUTTONS - 1) as f64 * PITCH);
    }

    #[test]
    fn own_icons_are_the_categories_glyphs() {
        // Every CATEGORIES/ icon the bar uses is in OWN_ICONS and the other way round, so the
        // outdated check and the shipped DDS files cannot drift from the layout.
        let used: std::collections::BTreeSet<&str> = numbered()
            .into_iter()
            .map(|(_, icon)| icon)
            .filter(|icon| icon.starts_with("CATEGORIES/"))
            .collect();
        let own: std::collections::BTreeSet<&str> = OWN_ICONS.iter().copied().collect();
        assert_eq!(used, own);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../../mods/SmallerInventorySlots/TEXTURES/UI/FRONTEND/ICONS",
        );
        for icon in OWN_ICONS {
            let bytes = std::fs::read(root.join(icon)).unwrap_or_else(|e| panic!("{icon}: {e}"));
            assert_eq!(bytes.len(), 128 + 128 * 128 * 4, "{icon} is a 128x128 A8R8G8B8 DDS");
            assert_eq!(&bytes[..4], b"DDS ", "{icon}");
        }
        let ids: Vec<(i32, &str)> = numbered()
            .into_iter()
            .filter(|(_, icon)| icon.starts_with("CATEGORIES/FILTER") && !EXTRAS.iter().any(|e| e.1 == *icon))
            .collect();
        assert_eq!(
            ids,
            vec![
                (11, "CATEGORIES/FILTERTREASURE.DDS"),
                (14, "CATEGORIES/FILTEREGG.DDS"),
                (16, "CATEGORIES/FILTERBONE.DDS"),
            ]
        );
    }
}
