//! UI element walks, layout floats, pad input, and probe helpers.

use std::collections::HashSet;
use std::time::Instant;

use nms_core::mem::{self, read_bytes, read_f32, read_i32, read_ptr, read_u32, read_u64, read_u8};

use super::layout::{
    CHILD_MAP, DATA_ID, ELEMENT_DATA, HOVER_NONE, HOVER_STALE, IS_HIDDEN, LAYOUT_H, LAYOUT_W,
    LAYOUT_X, LAYOUT_Y, MIN_ADDR, PROBE_CHUNK, PROBE_DUMP_DWORDS, STORE_NAME, STORE_NAME_LEN,
    STORE_STACK_GROUP,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Layout {
    pub y: f32,
    pub w: f32,
}

pub fn element_info(address: usize) -> (String, Option<Layout>) {
    let Some(data) = read_ptr(address.wrapping_add(ELEMENT_DATA)) else {
        return (String::new(), None);
    };
    let ident = read_ascii16(data.wrapping_add(DATA_ID));
    let _h = read_f32(data.wrapping_add(LAYOUT_H));
    let _x = read_f32(data.wrapping_add(LAYOUT_X));
    let layout = Layout {
        y: read_f32(data.wrapping_add(LAYOUT_Y)).unwrap_or(0.0),
        w: read_f32(data.wrapping_add(LAYOUT_W)).unwrap_or(0.0),
    };
    (ident, Some(layout))
}

fn read_ascii16(addr: usize) -> String {
    let Some(raw) = read_bytes(addr, 16) else {
        return String::new();
    };
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

pub fn element_name(element: usize) -> String {
    if element < MIN_ADDR {
        return String::new();
    }
    let Some(data) = read_ptr(element.wrapping_add(ELEMENT_DATA)) else {
        return String::new();
    };
    let Some(raw) = read_bytes(data.wrapping_add(DATA_ID), 16) else {
        return String::new();
    };
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let Ok(text) = std::str::from_utf8(&raw[..end]) else {
        return String::new();
    };
    if text.is_empty() || !text.chars().all(|ch| ch.is_ascii() && !ch.is_control()) {
        return String::new();
    }
    text.to_string()
}

pub fn children(element: usize) -> Vec<(String, usize)> {
    if element < MIN_ADDR {
        return Vec::new();
    }
    let Some(table) = read_ptr(element.wrapping_add(CHILD_MAP)) else {
        return Vec::new();
    };
    let Some(count) = read_i32(table.wrapping_add(0x1C)) else {
        return Vec::new();
    };
    let Some(buckets_n) = read_i32(table.wrapping_add(0x20)) else {
        return Vec::new();
    };
    let Some(buckets) = read_ptr(table.wrapping_add(8)) else {
        return Vec::new();
    };
    if !(0..=64).contains(&count) || !(1..=256).contains(&buckets_n) || buckets < MIN_ADDR {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut index = 0i32;
    while index < buckets_n {
        let off = (index as usize).wrapping_mul(8);
        let Some(mut node) = read_ptr(buckets.wrapping_add(off)) else {
            index = index.wrapping_add(1);
            continue;
        };
        let mut hops: i32 = 0;
        while node >= MIN_ADDR && hops < 8 && found.len() < 48 {
            if let Some(child) = read_ptr(node.wrapping_add(0x18)) {
                let ident = element_name(child);
                if !ident.is_empty() {
                    found.push((ident, child));
                }
            }
            node = read_ptr(node.wrapping_add(0x28)).unwrap_or(0);
            hops = hops.wrapping_add(1);
        }
        index = index.wrapping_add(1);
    }
    let extra = read_i32(element.wrapping_add(0x6C)).unwrap_or(0);
    let array = read_ptr(element.wrapping_add(0x70)).unwrap_or(0);
    if array >= MIN_ADDR && extra > 0 && extra <= 64 {
        let mut seen: HashSet<usize> = found.iter().map(|(_, c)| *c).collect();
        let mut index = 0i32;
        while index < extra {
            let Some(child) = read_ptr(array.wrapping_add((index as usize).wrapping_mul(8))) else {
                index = index.wrapping_add(1);
                continue;
            };
            if child < MIN_ADDR || seen.contains(&child) {
                index = index.wrapping_add(1);
                continue;
            }
            let ident = element_name(child);
            if !ident.is_empty() {
                found.push((ident, child));
                seen.insert(child);
            }
            index = index.wrapping_add(1);
        }
    }
    found
}

pub fn child_named(element: usize, name: &str) -> usize {
    children(element)
        .into_iter()
        .find(|(ident, _)| ident == name)
        .map(|(_, child)| child)
        .unwrap_or(0)
}

pub fn find_named(element: usize, name: &str, depth: usize, mut budget: usize) -> usize {
    let mut level = vec![element];
    let mut d = 0usize;
    while d <= depth {
        let mut nxt = Vec::new();
        for parent in level {
            if budget == 0 {
                return 0;
            }
            budget = budget.saturating_sub(1);
            for (ident, child) in children(parent) {
                if ident == name {
                    return child;
                }
                nxt.push(child);
            }
        }
        level = nxt;
        d = d.saturating_add(1);
    }
    0
}

pub fn set_hidden(element: usize, hidden: bool) {
    if element < MIN_ADDR {
        return;
    }
    let Some(data) = read_ptr(element.wrapping_add(ELEMENT_DATA)) else {
        return;
    };
    if data < MIN_ADDR {
        return;
    }
    let value = if hidden { 1u8 } else { 0u8 };
    let _ = unsafe { mem::write(data.wrapping_add(IS_HIDDEN), value) };
}

pub fn write_layer_y(layer: usize, y: f32) -> Result<(), String> {
    let Some(data) = read_ptr(layer.wrapping_add(ELEMENT_DATA)) else {
        return Err("inventory layer has no data".into());
    };
    if data < MIN_ADDR {
        return Err("inventory layer has no data".into());
    }
    if !unsafe { mem::write(data.wrapping_add(LAYOUT_Y), y) } {
        return Err("inventory layer has no data".into());
    }
    Ok(())
}

pub fn store_name(store: usize) -> String {
    if store < MIN_ADDR {
        return "?".into();
    }
    let Some(raw) = read_bytes(store.wrapping_add(STORE_NAME), STORE_NAME_LEN) else {
        return "?".into();
    };
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let text = String::from_utf8_lossy(&raw[..end]);
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_control() {
            out.push('?');
        } else {
            out.push(ch);
        }
    }
    if out.chars().count() > 64 {
        let mut s: String = out.chars().take(64).collect();
        s.push_str("...");
        s
    } else {
        out
    }
}

pub fn stack_group(store: usize) -> Option<u16> {
    if store < MIN_ADDR {
        return None;
    }
    let raw = read_bytes(store.wrapping_add(STORE_STACK_GROUP), 2)?;
    Some(u16::from_le_bytes([raw.first().copied().unwrap_or(0), raw.get(1).copied().unwrap_or(0)]))
}

pub fn page_dwords(page: usize, field: i32) -> String {
    let start = (page as i64).wrapping_add(field as i64) - (PROBE_DUMP_DWORDS as i64 / 2) * 4;
    if page < MIN_ADDR || start < MIN_ADDR as i64 {
        return "unreadable".into();
    }
    let start = start as usize;
    let mut words = Vec::new();
    let mut index = 0usize;
    while index < PROBE_DUMP_DWORDS {
        match read_u32(start.wrapping_add(index.wrapping_mul(4))) {
            Some(v) => words.push(format!("{v:08x}")),
            None => words.push("????????".into()),
        }
        index = index.wrapping_add(1);
    }
    words.join(" ")
}

pub fn read_dwords(address: usize, nbytes: usize) -> Vec<Option<u32>> {
    if nbytes < 4 || address < MIN_ADDR {
        return Vec::new();
    }
    let n = nbytes / 4;
    let mut words = vec![None; n];
    let mut offset = 0usize;
    while offset + 3 < nbytes {
        let mut size = PROBE_CHUNK.min(nbytes.saturating_sub(offset));
        size -= size % 4;
        if size < 4 {
            break;
        }
        let base = offset / 4;
        if let Some(raw) = read_bytes(address.wrapping_add(offset), size) {
            if raw.len() >= 4 {
                let count = raw.len() / 4;
                let mut index = 0usize;
                while index < count {
                    let at = base.wrapping_add(index);
                    if at < words.len() {
                        if let Some(chunk) = raw.get(index * 4..index * 4 + 4) {
                            words[at] = Some(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
                        }
                    }
                    index = index.wrapping_add(1);
                }
                offset = offset.wrapping_add(size);
                continue;
            }
        }
        let mut index = 0usize;
        while index < size / 4 {
            let at = base.wrapping_add(index);
            if at >= words.len() {
                break;
            }
            words[at] = read_u32(address.wrapping_add(offset).wrapping_add(index.wrapping_mul(4)));
            index = index.wrapping_add(1);
        }
        offset = offset.wrapping_add(size);
    }
    words
}

pub fn dword_text(value: Option<u32>) -> String {
    match value {
        Some(v) => format!("{:08x}", v),
        None => "????????".into(),
    }
}

pub struct FilterHover {
    hover: usize,
    ticks: usize,
    seen: u32,
    seen_at: Instant,
}

impl FilterHover {
    pub fn new(hover: usize, ticks: usize) -> Self {
        Self {
            hover,
            ticks,
            seen: read_u32(ticks).unwrap_or(0),
            seen_at: Instant::now(),
        }
    }

    pub fn hovered_filter(&mut self) -> Option<u8> {
        let now = Instant::now();
        let ticks = read_u32(self.ticks).unwrap_or(self.seen);
        if ticks != self.seen {
            self.seen = ticks;
            self.seen_at = now;
        }
        if now.duration_since(self.seen_at).as_secs_f64() > HOVER_STALE {
            return None;
        }
        let value = read_u8(self.hover).unwrap_or(HOVER_NONE);
        if value == HOVER_NONE {
            None
        } else {
            Some(value)
        }
    }
}

pub fn read_u64_checked(addr: usize) -> u64 {
    if addr < MIN_ADDR {
        0
    } else {
        read_u64(addr).unwrap_or(0)
    }
}

pub fn rel32_target_va(disp_at: usize) -> Option<usize> {
    let disp = read_i32(disp_at)?;
    usize::try_from(disp_at as i64 + 4 + disp as i64).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_ids_and_hover_none() {
        assert_eq!(super::super::layout::extra_filter_ids(), [5, 7, 8, 10, 13, 14, 15, 16, 17, 18]);
        assert_eq!(HOVER_NONE, 0xFF);
        assert_eq!(dword_text(None), "????????");
        assert_eq!(dword_text(Some(0x1a)), "0000001a");
    }
}
