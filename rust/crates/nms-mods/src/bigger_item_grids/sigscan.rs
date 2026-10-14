//! Page-size planning from string anchors in NMS.exe (port of `bigger_item_grids_sigscan.py`).

use std::collections::{HashMap, HashSet};

use nms_core::image::{parse_headers, Code, FileImage};

const PAGE_SCALE: f32 = 0.125;
const TRANSFER_ANCHOR: &str = "TRANSFER_RECEIVE_DESC";
const SLOT_NAME_FORMAT: &str = "SLOT%02d";
const NEXT_PAGE: &[u8] = &[0xC1, 0xE1, 0x03, 0x83, 0xC0, 0x08, 0x3B, 0xC8, 0x0F, 0x4C, 0xC1];
const PREV_PAGE: &[u8] = &[0x83, 0xC1, 0xF8];
const RIP_LEA_RM: [u8; 8] = [0x05, 0x0D, 0x15, 0x1D, 0x25, 0x2D, 0x35, 0x3D];

#[derive(Debug, Clone, PartialEq)]
pub struct ScaleRedirect {
    pub cave_rva: usize,
    pub scale: f32,
    pub patches: Vec<(usize, Vec<u8>, Vec<u8>)>,
}

/// NUL-terminated ASCII copies of `text` in the image (RVAs).
pub fn find_c_strings(data: &[u8], text: &str) -> Vec<usize> {
    let raw = text.as_bytes();
    let mut hits = Vec::new();
    let mut start = 0usize;
    while let Some(index) = data[start..].windows(raw.len()).position(|w| w == raw) {
        let index = start + index;
        let end = index + raw.len();
        if end < data.len() && data[end] == 0 {
            hits.push(index);
        }
        start = index + 1;
    }
    hits
}

fn read_f32(data: &[u8], rva: usize) -> Option<f32> {
    let b = data.get(rva..rva + 4)?;
    Some(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn primary_code(img: &FileImage) -> Option<(usize, usize)> {
    let headers = parse_headers(img.bytes()).ok()?;
    for s in &headers.sections {
        if s.name == ".text" && s.executable && s.raw_size != 0 {
            let end = s.rva.saturating_add(s.raw_size).min(img.bytes().len());
            return Some((s.rva, end));
        }
    }
    None
}

fn text_section(img: &FileImage) -> Option<&[u8]> {
    let (start, end) = primary_code(img)?;
    img.bytes().get(start..end)
}

fn try_rip_lea_match(code: &[u8], at: usize) -> Option<(usize, i32)> {
    let limit = code.len();
    let match_end = if code.get(at).is_some_and(|&b| (0x40..=0x4F).contains(&b)) && code.get(at + 1) == Some(&0x8D) {
        at + 3
    } else if code.get(at) == Some(&0x8D) {
        at + 2
    } else {
        return None;
    };
    let rm = *code.get(match_end - 1)?;
    if !RIP_LEA_RM.contains(&rm) {
        return None;
    }
    let match_len = match_end - at;
    let insn_len = match_len + 4;
    if at + insn_len > limit {
        return None;
    }
    let disp = i32::from_le_bytes([
        code[match_end],
        code[match_end + 1],
        code[match_end + 2],
        code[match_end + 3],
    ]);
    Some((match_len, disp))
}

fn iter_rip_leas(code: &[u8], code_rva: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
    let limit = code.len();
    let mut index = 0usize;
    std::iter::from_fn(move || {
        while index < limit {
            let at = index;
            if let Some((match_len, disp)) = try_rip_lea_match(code, at) {
                let insn_len = match_len + 4;
                index = at + match_len;
                let insn_rva = code_rva + at;
                let target = (insn_rva as i64 + insn_len as i64 + i64::from(disp)) as usize;
                return Some((insn_rva, target));
            }
            index = at + 1;
        }
        None
    })
}

fn anchor_xrefs_many(img: &FileImage, texts: &[&str]) -> HashMap<String, Vec<usize>> {
    let mut results: HashMap<String, Vec<usize>> =
        texts.iter().map(|t| ((*t).to_string(), Vec::new())).collect();
    let data = img.bytes();
    let mut targets: HashMap<usize, Vec<String>> = HashMap::new();
    for text in texts {
        for rva in find_c_strings(data, text) {
            targets.entry(rva).or_default().push((*text).to_string());
        }
    }
    if targets.is_empty() {
        return results;
    }
    let mut seen: HashSet<(String, usize)> = HashSet::new();
    for range in img.exec_ranges() {
        let Some(code) = data.get(range.clone()) else {
            continue;
        };
        for (insn_rva, target) in iter_rip_leas(code, range.start) {
            let Some(names) = targets.get(&target) else {
                continue;
            };
            for name in names {
                if seen.insert((name.clone(), insn_rva)) {
                    results.get_mut(name).expect("key").push(insn_rva);
                }
            }
        }
    }
    results
}

fn anchor_xrefs(img: &FileImage, text: &str) -> Vec<usize> {
    anchor_xrefs_many(img, &[text])
        .remove(text)
        .unwrap_or_default()
}

pub fn function_span(code: &[u8], code_rva: usize, inside_rva: usize) -> Option<(usize, usize)> {
    let offset = inside_rva.checked_sub(code_rva)?;
    if offset >= code.len() {
        return None;
    }
    let window = 0x9000usize;
    let back_from = offset.saturating_sub(window);
    let back = &code[back_from..offset];
    let mut start = None;
    let mut index = 0usize;
    while index < back.len() {
        if back[index] == 0xCC {
            let mut end = index;
            while end < back.len() && back[end] == 0xCC {
                end += 1;
            }
            if end - index >= 4 {
                start = Some(code_rva + back_from + end);
            }
            index = end;
        } else {
            index += 1;
        }
    }
    let start = start?;
    let forward = &code[offset..(offset + window).min(code.len())];
    index = 1;
    while index < forward.len() {
        if forward[index] == 0xCC {
            let mut end = index;
            while end < forward.len() && forward[end] == 0xCC {
                end += 1;
            }
            if end - index >= 4 {
                return Some((start, inside_rva + index));
            }
            index = end;
        } else {
            index += 1;
        }
    }
    None
}

pub fn page_scale_multiplies(
    code: &[u8],
    code_rva: usize,
    read_float: &mut dyn FnMut(usize) -> Option<f32>,
) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut loaded: HashMap<u8, usize> = HashMap::new();
    let mut index = 0usize;
    let limit = code.len();
    while index + 4 < limit {
        if code[index] == 0xF3 && code[index + 1] == 0x0F && index + 3 < limit {
            let opcode = code[index + 2];
            let modrm = code[index + 3];
            let mod_ = modrm >> 6;
            let reg = (modrm >> 3) & 7;
            let rm = modrm & 7;
            if opcode == 0x10 && mod_ == 0 && rm == 5 && index + 8 <= limit {
                let disp = i32::from_le_bytes([
                    code[index + 4],
                    code[index + 5],
                    code[index + 6],
                    code[index + 7],
                ]);
                let load_rva = code_rva + index;
                let value = read_float(load_rva + 8 + disp as usize);
                if value.is_some_and(|v| (v - PAGE_SCALE).abs() < 1e-6) {
                    loaded.insert(reg, load_rva);
                } else {
                    loaded.remove(&reg);
                }
                index += 8;
                continue;
            }
            if opcode == 0x59 && mod_ == 3 {
                if let Some(&load_rva) = loaded.get(&rm) {
                    found.push((load_rva, code_rva + index));
                }
                index += 4;
                continue;
            }
            if opcode == 0x10 && mod_ == 3 {
                loaded.remove(&reg);
                index += 4;
                continue;
            }
        }
        index += 1;
    }
    found
}

pub fn redirect_rip_float(old: &[u8], load_rva: usize, cave_rva: usize) -> Option<Vec<u8>> {
    if old.len() != 8 || old[0..2] != [0xF3, 0x0F] || (old[2] != 0x10 && old[2] != 0x59) || old[3] & 0xC7 != 0x05 {
        return None;
    }
    let disp = cave_rva as i64 - (load_rva as i64 + 8);
    if !(i32::MIN as i64..=i32::MAX as i64).contains(&disp) {
        return None;
    }
    let mut out = old[..4].to_vec();
    out.extend_from_slice(&(disp as i32).to_le_bytes());
    Some(out)
}

pub fn redirect_movss(old: &[u8], load_rva: usize, cave_rva: usize) -> Option<Vec<u8>> {
    if old.get(2) != Some(&0x10) {
        return None;
    }
    redirect_rip_float(old, load_rva, cave_rva)
}

fn rip_scale_reads(code: &[u8], code_rva: usize, data: &[u8]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut index = 0usize;
    let limit = code.len();
    while index + 8 <= limit {
        let opcode = code[index + 2];
        let modrm = code[index + 3];
        if code[index..index + 2] == [0xF3, 0x0F] && (opcode == 0x10 || opcode == 0x59) && modrm & 0xC7 == 0x05 {
            let disp = i32::from_le_bytes([
                code[index + 4],
                code[index + 5],
                code[index + 6],
                code[index + 7],
            ]);
            let load_rva = code_rva + index;
            let value = read_f32(data, load_rva + 8 + disp as usize);
            if value.is_some_and(|v| (v - PAGE_SCALE).abs() < 1e-6) {
                found.push(load_rva);
            }
            index += 8;
            continue;
        }
        index += 1;
    }
    found
}

fn plan_scale_in(
    img: &FileImage,
    code_rva: usize,
    code: &[u8],
    span: (usize, usize),
    items_per_page: u32,
) -> Option<ScaleRedirect> {
    if items_per_page == 0 {
        return None;
    }
    let (start, end) = span;
    let pad = end - code_rva;
    if code.get(pad..pad + 4)? != [0xCC, 0xCC, 0xCC, 0xCC] {
        return None;
    }
    let blob = code.get(start - code_rva..end - code_rva)?;
    let data = img.bytes();
    let loads = rip_scale_reads(blob, start, data);
    if loads.is_empty() {
        return None;
    }
    let scale = 1.0f32 / items_per_page as f32;
    let mut patches = Vec::new();
    for load_rva in loads {
        let load_off = load_rva - code_rva;
        let old = code.get(load_off..load_off + 8)?.to_vec();
        let new = redirect_rip_float(&old, load_rva, end)?;
        if new[..4] != old[..4] || new[4..] == old[4..] {
            return None;
        }
        patches.push((load_rva, old, new));
    }
    Some(ScaleRedirect { cave_rva: end, scale, patches })
}

pub fn plan_scale_redirect(img: &FileImage, anchor: &str, items: u32) -> Option<ScaleRedirect> {
    if items < 1 {
        return None;
    }
    let (code_rva, _) = primary_code(img)?;
    let code = text_section(img)?;
    let anchors = anchor_xrefs(img, anchor);
    if anchors.len() != 1 {
        return None;
    }
    let span = function_span(code, code_rva, anchors[0])?;
    plan_scale_in(img, code_rva, code, span, items)
}

fn call_targets(code: &[u8], code_rva: usize, start: usize, end: usize) -> HashSet<usize> {
    let mut targets = HashSet::new();
    let limit = code.len();
    for offset in (start - code_rva)..(end - code_rva).saturating_sub(5) {
        if code.get(offset) != Some(&0xE8) {
            continue;
        }
        let disp = i32::from_le_bytes([
            code[offset + 1],
            code[offset + 2],
            code[offset + 3],
            code[offset + 4],
        ]);
        let target = code_rva + offset + 5 + disp as usize;
        let target_off = target.saturating_sub(code_rva);
        if target_off > 0
            && target_off < limit
            && code.get(target_off - 1) == Some(&0xCC)
        {
            targets.insert(target);
        }
    }
    targets
}

pub fn find_slot_filler(img: &FileImage) -> Option<(usize, usize)> {
    let (code_rva, _) = primary_code(img)?;
    let code = text_section(img)?;
    let transfer = anchor_xrefs(img, TRANSFER_ANCHOR);
    if transfer.len() != 1 {
        return None;
    }
    let outer = function_span(code, code_rva, transfer[0])?;
    let slot_refs = anchor_xrefs(img, SLOT_NAME_FORMAT);
    let mut found = HashSet::new();
    for target in call_targets(code, code_rva, outer.0, outer.1) {
        let span = function_span(code, code_rva, target)?;
        if span.0 != target {
            continue;
        }
        if slot_refs.iter().any(|&r| span.0 <= r && r < span.1) {
            found.insert(span);
        }
    }
    if found.len() != 1 {
        return None;
    }
    found.into_iter().next()
}

pub fn plan_slot_filler(img: &FileImage, items: u32) -> Option<ScaleRedirect> {
    if !(1..=127).contains(&items) {
        return None;
    }
    let (code_rva, _) = primary_code(img)?;
    let code = text_section(img)?;
    let span = find_slot_filler(img)?;
    let scale = plan_scale_in(img, code_rva, code, span, items)?;
    if scale.patches.len() != 1 {
        return None;
    }
    let (start, end) = span;
    let body = code.get(start - code_rva..end - code_rva)?;
    if body.windows(NEXT_PAGE.len()).filter(|w| *w == NEXT_PAGE).count() != 1
        || body.windows(PREV_PAGE.len()).filter(|w| *w == PREV_PAGE).count() != 1
    {
        return None;
    }
    let step = items as u8;
    let next_idx = body.windows(NEXT_PAGE.len()).position(|w| w == NEXT_PAGE)?;
    let next_rva = start + next_idx;
    let next_old = NEXT_PAGE[..6].to_vec();
    let next_new = vec![0x6B, 0xC9, step, 0x83, 0xC0, step];
    let prev_idx = body.windows(PREV_PAGE.len()).position(|w| w == PREV_PAGE)?;
    let prev_rva = start + prev_idx;
    let prev_new = vec![0x83, 0xC1, (0u32.wrapping_sub(items) & 0xFF) as u8];
    let mut patches = scale.patches;
    patches.push((next_rva, next_old, next_new));
    patches.push((prev_rva, PREV_PAGE.to_vec(), prev_new));
    Some(ScaleRedirect {
        cave_rva: scale.cave_rva,
        scale: scale.scale,
        patches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lea_rcx(code_rva: usize, target_rva: usize) -> Vec<u8> {
        let insn_len = 7usize;
        let disp = target_rva as i64 - (code_rva as i64 + insn_len as i64);
        let mut v = vec![0x48, 0x8D, 0x0D];
        v.extend_from_slice(&(disp as i32).to_le_bytes());
        v
    }

    #[test]
    fn find_c_strings_requires_nul() {
        let data = b"xxInventoryTransferScreen\x00yyInventoryTransferScreenZ";
        assert_eq!(find_c_strings(data, "InventoryTransferScreen"), vec![2]);
    }

    #[test]
    fn rip_lea_hits_known_target() {
        let code_rva = 0x1000;
        let target = 0x2000;
        let mut code = vec![0x90, 0x90];
        code.extend(lea_rcx(code_rva + 2, target));
        code.push(0x90);
        let hits: Vec<_> = iter_rip_leas(&code, code_rva).collect();
        assert!(hits.contains(&(code_rva + 2, target)));
    }

    #[test]
    fn page_scale_tracks_one_xmm_load() {
        let code_rva = 0x1000;
        let const_rva = 0x2000;
        let disp = const_rva as i64 - (code_rva as i64 + 8);
        let mut code = vec![0xF3, 0x0F, 0x10, 0x35];
        code.extend_from_slice(&(disp as i32).to_le_bytes());
        code.extend_from_slice(&[0xF3, 0x0F, 0x59, 0xC6, 0xF3, 0x0F, 0x10, 0xF0, 0xF3, 0x0F, 0x59, 0xC6]);
        let mut read = |rva: usize| if rva == const_rva { Some(PAGE_SCALE) } else { None };
        assert_eq!(page_scale_multiplies(&code, code_rva, &mut read), vec![(code_rva, code_rva + 8)]);
    }

    #[test]
    fn redirect_movss_points_at_the_cave() {
        let load_rva = 0x6B816B;
        let cave_rva = 0x6B8834;
        let old = [0xF3, 0x0F, 0x10, 0x35, 0x2D, 0x04, 0x60, 0x44];
        let new = redirect_movss(&old, load_rva, cave_rva).expect("test bytes");
        assert_eq!(&new[..4], &old[..4]);
        let disp = i32::from_le_bytes([new[4], new[5], new[6], new[7]]);
        assert_eq!(load_rva + 8 + disp as usize, cave_rva);
    }
}
