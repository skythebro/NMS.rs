//! Live filter patches: dim cave, side-button loop, hide pass.

use nms_core::log::Log;
use nms_core::mem::{alloc_exec_near, read_bytes, read_i32, read_u8};
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::smaller_inventory_slots as sigs;
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use super::cave::{cave_bytes, custom_cave_bytes, hide_image, icon_stub, skip_stub, tip_table};
use super::layout::{
    CUSTOM_AT, CUSTOM_BYTES, CUSTOM_HEAD, CUSTOM_JBE, CUSTOM_JBE_AT, CUSTOM_KIND_AT,
    CATEGORIES, CAVE_HEAD, CLICK_AT, CLICK_BYTES, COUNT_AT, DIM_AT, DIM_BYTES, FILTER_FIELD_AT, FILTER_OFFSET_HIGH,
    FILTER_OFFSET_LOW, HIDE_AT, HIDE_BYTES, HIDE_CHILD_AT, HIDE_CURSOR_AT, HIDE_FIND_AT, HIDE_HASH_AT, ICON_AFTER,
    ICON_BYTES, ICON_LEN, ICON_SITES, JOIN_AT, LOOP_TOP, SKIP_AT, SKIP_BACK, SKIP_BYTES, SKIP_MISS, STEP_AT, STEP_BYTES,
    TIP_COPY_AT, TIP_COPY_BYTES, TIP_SIZE, TIPS_AT, VANILLA_FILTERS,
};
use super::ui::{rel32_target_va, FilterHover};

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

fn write_exec(addr: usize, bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() {
        return Ok(());
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len());
        FlushInstructionCache(GetCurrentProcess(), addr as _, bytes.len());
    }
    Ok(())
}

fn write_site(image: &mut ProcessImage, va: usize, bytes: &[u8]) -> Result<(), String> {
    let rva = va.wrapping_sub(image.base());
    image
        .write(rva, bytes)
        .map_err(|_| format!("the game code at {va:#x} could not be made writable"))
}

fn find_unique(image: &ProcessImage, pattern: &str) -> Result<usize, String> {
    let pat = Pattern::parse(pattern).map_err(|e| e.to_string())?;
    image
        .find_unique(&pat)
        .unique()
        .ok_or_else(|| "the inventory filter code was not found (game update?)".into())
}

pub fn filter_field_offset(image: &ProcessImage) -> Option<i32> {
    let rva = find_unique(image, sigs::FILTER).ok()?;
    read_i32(image.va(rva.wrapping_add(FILTER_FIELD_AT)))
}

pub fn field_in_range(field: i32) -> bool {
    (FILTER_OFFSET_LOW..=FILTER_OFFSET_HIGH).contains(&field)
}

fn check_buttons(head: usize, filter_field: i32) -> Result<(), String> {
    for site in ICON_SITES {
        let Some(found) = read_bytes(head.wrapping_add(site), ICON_LEN + ICON_AFTER.len()) else {
            return Err("a side button icon load is not where it was (found )".into());
        };
        let patched = found.first() == Some(&0xE9) && found.get(5..ICON_LEN) == Some(&[0x90, 0x90][..]);
        let icon_ok = found.get(..ICON_BYTES.len()) == Some(ICON_BYTES);
        let after_ok = found.get(ICON_LEN..) == Some(ICON_AFTER);
        if (!patched && !icon_ok) || !after_ok {
            return Err(format!(
                "a side button icon load is not where it was (found {})",
                hex_bytes(&found)
            ));
        }
    }
    let Some(click) = read_bytes(head.wrapping_add(CLICK_AT), CLICK_BYTES.len()) else {
        return Err("the side button click does not set the filter the dim test reads".into());
    };
    let field = read_i32(head.wrapping_add(CLICK_AT).wrapping_add(3));
    if click != CLICK_BYTES || field != Some(filter_field) {
        return Err("the side button click does not set the filter the dim test reads".into());
    }
    let Some(skip) = read_bytes(head.wrapping_add(SKIP_AT), SKIP_BYTES.len()) else {
        return Err("the side button click skip is not where it was (found )".into());
    };
    if skip != SKIP_BYTES && skip.first() != Some(&0xE9) {
        return Err(format!(
            "the side button click skip is not where it was (found {})",
            hex_bytes(&skip)
        ));
    }
    let Some(tips) = read_bytes(head.wrapping_add(TIP_COPY_AT), TIP_COPY_BYTES.len()) else {
        return Err("the side button tooltip copy is not where it was".into());
    };
    if tips != TIP_COPY_BYTES {
        return Err("the side button tooltip copy is not where it was".into());
    }
    let Some(mut step) = read_bytes(head.wrapping_add(STEP_AT), STEP_BYTES.len()) else {
        return Err("the side button loop does not step through five buttons as it did".into());
    };
    let count_off = COUNT_AT.wrapping_sub(STEP_AT);
    if let Some(slot) = step.get_mut(count_off) {
        *slot = VANILLA_FILTERS;
    }
    let back = rel32_target_va(head.wrapping_add(STEP_AT).wrapping_add(STEP_BYTES.len()));
    if step != STEP_BYTES || back != Some(head.wrapping_add(LOOP_TOP)) {
        return Err("the side button loop does not step through five buttons as it did".into());
    }
    Ok(())
}

/// Returns the address of the Foods switch byte inside the dim cave, or `None` when an older cave
/// from a previous load is in place (categories work, Foods stays off).
fn install_dim_patch(image: &mut ProcessImage, block: usize, log: &Log) -> Result<Option<usize>, String> {
    let site = block.wrapping_add(DIM_AT);
    let join = block.wrapping_add(JOIN_AT);
    let deploy = site.wrapping_add(DIM_BYTES.len());
    let Some(current) = read_bytes(site, DIM_BYTES.len()) else {
        return Err("the product filter test is not where it was (found )".into());
    };
    if current.first() == Some(&0xE9) && current.get(5) == Some(&0x90) {
        if let Some(cave) = rel32_target_va(site.wrapping_add(1)) {
            if read_bytes(cave, CAVE_HEAD.len()).as_deref() == Some(CAVE_HEAD) {
                log.info("Filters: the dim test was already patched (mod reloaded)");
                let (body, foods) = cave_bytes(cave, join, deploy)?;
                let same = read_bytes(cave, foods).as_deref() == body.get(..foods);
                return Ok(same.then(|| cave.wrapping_add(foods)));
            }
        }
    }
    if current != DIM_BYTES {
        return Err(format!(
            "the product filter test is not where it was (found {})",
            hex_bytes(&current)
        ));
    }
    let cave = alloc_exec_near(site, 0x1000).ok_or_else(|| "no memory free near the game code for the filter test".to_string())?;
    let (body, foods) = cave_bytes(cave, join, deploy)?;
    write_exec(cave, &body)?;
    let mut jump = vec![0xE9];
    jump.extend_from_slice(&super::cave::rel32_bytes(site.wrapping_add(1), cave)?);
    jump.push(0x90);
    write_site(image, site, &jump)?;
    Ok(Some(cave.wrapping_add(foods)))
}

/// Live data of the Custom cave (absolute addresses).
#[derive(Clone, Copy, Debug)]
pub struct CustomCave {
    pub on: usize,
    pub count: usize,
    /// Store being drawn (8 bytes), for per-slot marks.
    pub store: usize,
    pub table: usize,
}

/// Head of the FILTER block, bytes 5 to 14: `xor bl, bl; mov r14d, [rdi+filter_field]`.
fn custom_site_ok(block: usize, filter_field: i32) -> Result<(), String> {
    let first = read_bytes(block, 5).unwrap_or_default();
    // QuickTransferAll's lock dim swaps the first five bytes for a jump while lock mode is on.
    let element_type = first == [0x41, 0x8B, 0x44, 0x24, 0x24] || first.first() == Some(&0xE9);
    let mut load = vec![0x32, 0xDB, 0x44, 0x8B, 0xB7];
    load.extend_from_slice(&filter_field.to_le_bytes());
    let loads = read_bytes(block.wrapping_add(5), load.len()).as_deref() == Some(load.as_slice());
    let jbe = read_bytes(block.wrapping_add(CUSTOM_JBE_AT), CUSTOM_JBE.len()).as_deref() == Some(CUSTOM_JBE)
        && rel32_target_va(block.wrapping_add(CUSTOM_JBE_AT).wrapping_add(2)) == Some(block.wrapping_add(JOIN_AT));
    let kind_test = read_bytes(block.wrapping_add(CUSTOM_KIND_AT), 3).as_deref() == Some(&[0x83, 0xF8, 0x01][..]);
    if element_type && loads && jbe && kind_test {
        Ok(())
    } else {
        Err("the filter test head is not what this mod knows (game update?)".into())
    }
}

/// Patches `FILTER + CUSTOM_AT` into the Custom cave. Any mismatch leaves the game code as it is.
fn install_custom_patch(image: &mut ProcessImage, block: usize, filter_field: i32, log: &Log) -> Result<CustomCave, String> {
    let site = block.wrapping_add(CUSTOM_AT);
    let join = block.wrapping_add(JOIN_AT);
    let back = block.wrapping_add(CUSTOM_JBE_AT);
    let kind_test = block.wrapping_add(CUSTOM_KIND_AT);
    let current = read_bytes(site, CUSTOM_BYTES.len()).unwrap_or_default();
    let absolute = |cave: usize, l: super::cave::CustomLayout| CustomCave {
        on: cave.wrapping_add(l.on),
        count: cave.wrapping_add(l.count),
        store: cave.wrapping_add(l.store),
        table: cave.wrapping_add(l.table),
    };
    if current.first() == Some(&0xE9) && current.get(5..) == Some(&[0x90, 0x90, 0x90][..]) {
        let cave = rel32_target_va(site.wrapping_add(1)).ok_or_else(|| "the filter test head was changed by something else".to_string())?;
        if read_bytes(cave, CUSTOM_HEAD.len()).as_deref() != Some(CUSTOM_HEAD) {
            return Err("the filter test head was changed by something else".into());
        }
        let (body, layout) = custom_cave_bytes(cave, join, back, kind_test)?;
        if read_bytes(cave, layout.on).as_deref() == body.get(..layout.on) {
            log.info("Filters: the Custom test was already patched (mod reloaded)");
            return Ok(absolute(cave, layout));
        }
        return Err("the filter test head was changed by something else".into());
    }
    if current != CUSTOM_BYTES {
        return Err(format!("the filter test head is not where it was (found {})", hex_bytes(&current)));
    }
    custom_site_ok(block, filter_field)?;
    let cave = alloc_exec_near(site, super::cave::CUSTOM_CAVE_SIZE).ok_or_else(|| "no memory free near the game code for the filter test".to_string())?;
    let (body, layout) = custom_cave_bytes(cave, join, back, kind_test)?;
    write_exec(cave, &body)?;
    let mut jump = vec![0xE9];
    jump.extend_from_slice(&super::cave::rel32_bytes(site.wrapping_add(1), cave)?);
    jump.extend_from_slice(&[0x90, 0x90, 0x90]);
    write_site(image, site, &jump)?;
    Ok(absolute(cave, layout))
}

fn install_button_patch(image: &mut ProcessImage, head: usize, filter_field: i32, log: &Log) -> Result<(), String> {
    check_buttons(head, filter_field)?;
    let buttons = VANILLA_FILTERS.wrapping_add(CATEGORIES);
    let count = read_u8(head.wrapping_add(COUNT_AT)).unwrap_or(0);
    if count == buttons {
        log.info("Filters: the side buttons were already patched (mod reloaded)");
        return Ok(());
    }
    if count != VANILLA_FILTERS {
        return Err(format!("the side button loop counts to {count}, not {VANILLA_FILTERS}"));
    }
    let tips_va = rel32_target_va(head.wrapping_add(TIPS_AT)).ok_or_else(|| "the side button tooltips are not the game's filter keys".to_string())?;
    let vanilla = read_bytes(tips_va, (VANILLA_FILTERS as usize).saturating_mul(TIP_SIZE))
        .ok_or_else(|| "the side button tooltips are not the game's filter keys".to_string())?;
    if !vanilla.starts_with(b"UI_INV_FILTER_ALL_TIP\0") {
        return Err("the side button tooltips are not the game's filter keys".into());
    }
    let cave = alloc_exec_near(head, 0x1000).ok_or_else(|| "no memory free near the game code for the filter test".to_string())?;
    let table = tip_table(&vanilla)?;
    write_exec(cave, &table)?;
    let mut stub = cave.wrapping_add((table.len() + 0xF) & !0xF);
    let mut jumps = Vec::new();
    for site in ICON_SITES {
        let at = head.wrapping_add(site);
        let original = read_bytes(at, ICON_LEN).ok_or_else(|| "a side button icon load is not where it was (found )".to_string())?;
        let body = icon_stub(stub, &original, at.wrapping_add(ICON_LEN))?;
        write_exec(stub, &body)?;
        let mut jump = vec![0xE9];
        jump.extend_from_slice(&super::cave::rel32_bytes(at.wrapping_add(1), stub)?);
        jump.extend_from_slice(&[0x90, 0x90]);
        jumps.push((at, jump));
        stub = stub.wrapping_add((body.len() + 0xF) & !0xF);
    }
    write_exec(cave, &table)?;
    write_site(image, head.wrapping_add(TIPS_AT), &super::cave::rel32_bytes(head.wrapping_add(TIPS_AT), cave)?)?;
    for (at, jump) in jumps {
        write_site(image, at, &jump)?;
    }
    write_site(image, head.wrapping_add(COUNT_AT), &[buttons])?;
    Ok(())
}

#[allow(dead_code)]
fn install_skip_patch(image: &mut ProcessImage, head: usize, log: &Log) -> Result<(), String> {
    let site = head.wrapping_add(SKIP_AT);
    let Some(found) = read_bytes(site, SKIP_BYTES.len()) else {
        return Err("the side button click skip is not where it was (found )".into());
    };
    if found.first() == Some(&0xE9) {
        if let Some(cave) = rel32_target_va(site.wrapping_add(1)) {
            let want = [0x41, 0x83, 0xFD, VANILLA_FILTERS];
            if read_bytes(cave, 4).as_deref() == Some(&want) {
                log.info("Filters: the controller click skip was already patched (mod reloaded)");
                return Ok(());
            }
        }
    }
    if found != SKIP_BYTES {
        return Err(format!(
            "the side button click skip is not where it was (found {})",
            hex_bytes(&found)
        ));
    }
    let cave = alloc_exec_near(site, 0x1000).ok_or_else(|| "no memory free near the game code for the filter test".to_string())?;
    let body = skip_stub(cave, head.wrapping_add(SKIP_BACK), head.wrapping_add(SKIP_MISS))?;
    write_exec(cave, &body)?;
    let mut jump = vec![0xE9];
    jump.extend_from_slice(&super::cave::rel32_bytes(site.wrapping_add(1), cave)?);
    while jump.len() < SKIP_BYTES.len() {
        jump.push(0x90);
    }
    write_site(image, site, &jump)?;
    log.info("Filters: extra buttons take a controller Confirm");
    Ok(())
}

fn install_hide_patch(image: &mut ProcessImage, log: &Log) -> Result<(FilterHover, usize), String> {
    let lookup_rva = Pattern::parse(sigs::HIDE_LOOKUP)
        .ok()
        .and_then(|p| image.find_unique(&p).unique());
    let hide_rva = Pattern::parse(sigs::HIDE)
        .ok()
        .and_then(|p| image.find_unique(&p).unique());
    let (lookup_rva, hide_rva) = match (lookup_rva, hide_rva) {
        (Some(a), Some(b)) => (a, b),
        _ => return Err("the side bar hide pass was not found (game update?)".into()),
    };
    let lookup = image.va(lookup_rva);
    let site = image.va(hide_rva.wrapping_add(HIDE_AT));
    let mut calls = [0usize; 4];
    for (i, (name, at)) in [
        ("find", HIDE_FIND_AT),
        ("hash", HIDE_HASH_AT),
        ("child", HIDE_CHILD_AT),
        ("cursor", HIDE_CURSOR_AT),
    ]
    .into_iter()
    .enumerate()
    {
        let Some(op) = read_bytes(lookup.wrapping_add(at), 1) else {
            return Err(format!("the side bar's {name} call is not where it was"));
        };
        if op != [0xE8] {
            return Err(format!("the side bar's {name} call is not where it was"));
        }
        calls[i] = rel32_target_va(lookup.wrapping_add(at).wrapping_add(1))
            .ok_or_else(|| format!("the side bar's {name} call is not where it was"))?;
    }
    let Some(found) = read_bytes(site, HIDE_BYTES.len()) else {
        return Err("the extra filter hide site is not where it was (found )".into());
    };
    let args = (
        site.wrapping_add(HIDE_BYTES.len()),
        calls[0],
        calls[1],
        calls[2],
        calls[3],
    );
    if found.first() == Some(&0xE9) {
        let cave = rel32_target_va(site.wrapping_add(1)).ok_or_else(|| "the extra filter hide pass was already changed".to_string())?;
        let (expected, labels) = hide_image(cave, args.0, args.1, args.2, args.3, args.4)?;
        let code_len = labels
            .get("page")
            .map(|va| va.wrapping_sub(cave))
            .ok_or_else(|| "hide page label missing".to_string())?;
        if read_bytes(cave, code_len).as_deref() == expected.get(..code_len) {
            log.info("Filters: extra button hide was already patched (mod reloaded)");
            let hover = *labels.get("hover").ok_or_else(|| "hide hover label missing".to_string())?;
            let ticks = *labels.get("ticks").ok_or_else(|| "hide ticks label missing".to_string())?;
            let on = *labels.get("on").ok_or_else(|| "hide on label missing".to_string())?;
            return Ok((FilterHover::new(hover, ticks), on));
        }
        return Err("the extra filter hide pass was already changed".into());
    }
    if found != HIDE_BYTES {
        return Err(format!(
            "the extra filter hide site is not where it was (found {})",
            hex_bytes(&found)
        ));
    }
    let cave = alloc_exec_near(site, 0x1000).ok_or_else(|| "no memory free near the game code for the filter test".to_string())?;
    let (blob, labels) = hide_image(cave, args.0, args.1, args.2, args.3, args.4)?;
    write_exec(cave, &blob)?;
    let mut jump = vec![0xE9];
    jump.extend_from_slice(&super::cave::rel32_bytes(site.wrapping_add(1), cave)?);
    write_site(image, site, &jump)?;
    log.info("Filters: extra buttons hide with the sort stack, and take a controller Confirm");
    let hover = *labels.get("hover").ok_or_else(|| "hide hover label missing".to_string())?;
    let ticks = *labels.get("ticks").ok_or_else(|| "hide ticks label missing".to_string())?;
    let on = *labels.get("on").ok_or_else(|| "hide on label missing".to_string())?;
    Ok((FilterHover::new(hover, ticks), on))
}

/// What `install_filters` put in place. Addresses are live cave data the switches write.
pub struct Installed {
    pub hover: Option<FilterHover>,
    /// First byte of the per-button show table in the hide cave (one byte per extra button).
    pub hide_on: Option<usize>,
    /// Foods switch byte in the dim cave.
    pub foods_on: Option<usize>,
    pub custom: Option<CustomCave>,
}

pub fn install_filters(image: &mut ProcessImage, log: &Log) -> Result<Installed, String> {
    let block_rva = find_unique(image, sigs::FILTER)?;
    let head_rva = find_unique(image, sigs::BUTTONS)?;
    let block = image.va(block_rva);
    let head = image.va(head_rva);
    let filter_field = read_i32(block.wrapping_add(FILTER_FIELD_AT))
        .ok_or_else(|| "the inventory filter code was not found (game update?)".to_string())?;
    check_buttons(head, filter_field)?;
    let foods_on = install_dim_patch(image, block, log)?;
    if foods_on.is_none() {
        log.info("Foods filter not changed: an older filter test is in place");
    }
    install_button_patch(image, head, filter_field, log)?;
    let custom = match install_custom_patch(image, block, filter_field, log) {
        Ok(cave) => Some(cave),
        Err(error) => {
            log.info(&format!("Custom filter not changed: {error}"));
            None
        }
    };
    let (hover, hide_on) = match install_hide_patch(image, log) {
        Ok((hover, on)) => (Some(hover), Some(on)),
        Err(error) => {
            log.info(&format!("Filters: extra button hide left vanilla: {error}"));
            (None, None)
        }
    };
    Ok(Installed {
        hover,
        hide_on,
        foods_on,
        custom,
    })
}

