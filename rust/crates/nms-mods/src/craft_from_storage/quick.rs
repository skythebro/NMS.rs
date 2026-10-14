//! Quick Transfer per-box rows, All Containers, and Cold Storage.

use std::collections::HashMap;
use std::time::Instant;

use nms_core::log::Log;
use nms_core::mem::{self, read_i32, read_u32, read_u64, readable, MIN_ADDR};

use super::game::Game;
use super::lists::{
    parse_dest_kind, place_cooker_dests, quick_window, shown_pairs_text, DestRow, ShownKind, CHESTS, FISH_KIND,
    QUICK_ALL_NAME, QUICK_COLD_NAME, QUICK_FOCUS_AT, QUICK_MAGIC, QUICK_MARK_AT, QUICK_ROW_SIZE, QUICK_SLOTS,
    QUICK_SOURCE_AT,
};
use super::patches::{cave_or_alloc, quick_set_name};

// Local u16 read: mem has no read_u16.
fn read_u16(addr: usize) -> Option<u16> {
    let lo = nms_core::mem::read_u8(addr)?;
    let hi = nms_core::mem::read_u8(addr.wrapping_add(1))?;
    Some(u16::from_le_bytes([lo, hi]))
}

type QuickRowFn = unsafe extern "system" fn(u64, u64, u64) -> u64;

#[derive(Default)]
pub struct QuickState {
    pub busy: bool,
    pub broken: bool,
    pub diag: Option<(u16, i32, u16)>,
    pub diag_at: Option<Instant>,
    pub at: Option<Instant>,
    pub page: usize,
    pub off: i32,
    pub focus: Option<i32>,
    pub seen: Option<Instant>,
    pub forced: i32,
    pub kinds: HashMap<i32, i32>,
    pub push: Option<QuickRowFn>,
    pub slot: i32,
    pub state: Option<(u16, String)>,
    pub qt_element: u64,
}

impl QuickState {
    pub fn log_list(&mut self, log: &Log, rows: usize, object_id: usize) {
        if rows < MIN_ADDR {
            return;
        }
        let count = read_u32(rows.wrapping_add(4)).unwrap_or(0);
        let data = read_u64(rows.wrapping_add(8)).unwrap_or(0);
        let ident = if object_id >= MIN_ADDR {
            read_u16(object_id).unwrap_or(0)
        } else {
            0
        };
        let mut last_type = -1i32;
        let mut last_sub = 0u16;
        if data >= MIN_ADDR as u64 && (1..64).contains(&count) {
            let last = (data as usize).wrapping_add((count as usize).saturating_sub(1).wrapping_mul(QUICK_ROW_SIZE));
            last_type = read_i32(last).unwrap_or(-1);
            last_sub = read_u16(last.wrapping_add(4)).unwrap_or(0);
        }
        let key = (ident, last_type, last_sub);
        let now = Instant::now();
        if self.diag == Some(key) {
            if let Some(at) = self.diag_at {
                if now.duration_since(at).as_secs_f64() < 0.3 {
                    return;
                }
            }
        }
        self.diag = Some(key);
        self.diag_at = Some(now);
        log.info(&format!(
            "Quick Transfer list: ident {ident}, rows {count}, last type {last_type}, last +4 {last_sub}"
        ));
    }

    pub fn force_from_row(&mut self, row: u64) {
        self.forced = 0;
        if row < MIN_ADDR as u64 {
            return;
        }
        let row = row as usize;
        if read_i32(row) != Some(7) {
            return;
        }
        let mark = read_u32(row.wrapping_add(QUICK_MARK_AT)).unwrap_or(0).wrapping_sub(QUICK_MAGIC);
        if mark < QUICK_SLOTS as u32 {
            self.forced = self.kinds.get(&(mark as i32)).copied().unwrap_or(0);
        }
    }

    pub fn filter_object_chests(&self, out: usize) {
        let kind = self.forced;
        if kind == 0 || out < MIN_ADDR {
            return;
        }
        let count = read_u32(out.wrapping_add(4)).unwrap_or(0);
        let data = read_u64(out.wrapping_add(8)).unwrap_or(0);
        let capacity = read_u32(out).unwrap_or(0);
        if kind == FISH_KIND {
            if data >= MIN_ADDR as u64 && capacity >= 1 {
                let _ = unsafe { mem::write(data as usize, FISH_KIND) };
                let _ = unsafe { mem::write(out.wrapping_add(4), 1u32) };
            } else {
                let _ = unsafe { mem::write(out.wrapping_add(4), 0u32) };
            }
            return;
        }
        if data >= MIN_ADDR as u64 && (1..64).contains(&count) {
            let mut present = false;
            for i in 0..count {
                let k = read_u32((data as usize).wrapping_add(4usize.wrapping_mul(i as usize))).unwrap_or(0) as i32;
                if k == kind {
                    present = true;
                    break;
                }
            }
            if present {
                let _ = unsafe { mem::write(data as usize, kind) };
                let _ = unsafe { mem::write(out.wrapping_add(4), 1u32) };
            } else {
                let _ = unsafe { mem::write(out.wrapping_add(4), 0u32) };
            }
        }
    }

    fn row_at(&self, rows: usize, ident: u16) -> usize {
        let count = read_u32(rows.wrapping_add(4)).unwrap_or(0);
        let data = read_u64(rows.wrapping_add(8)).unwrap_or(0);
        if data < MIN_ADDR as u64 || !(1..64).contains(&count) {
            return 0;
        }
        let row = (data as usize).wrapping_add((count as usize).saturating_sub(1).wrapping_mul(QUICK_ROW_SIZE));
        if read_i32(row) != Some(7) || read_u16(row.wrapping_add(4)) != Some(ident) {
            return 0;
        }
        row
    }

    fn source(&self, game: &Game, kinds: &[i32]) -> Option<i32> {
        if self.page < MIN_ADDR {
            return None;
        }
        let pointer = read_u64(self.page.wrapping_add(QUICK_SOURCE_AT)).unwrap_or(0);
        if pointer < MIN_ADDR as u64 {
            return None;
        }
        kinds.iter().copied().find(|&kind| game.view(kind) == pointer)
    }

    fn ensure_push(&mut self, image_base_row: Option<usize>, image: &nms_core::process::ProcessImage) -> bool {
        if self.push.is_none() {
            let Some(va) = image_base_row else {
                return false;
            };
            if va == 0 {
                return false;
            }
            self.push = Some(unsafe { std::mem::transmute::<usize, QuickRowFn>(va) });
        }
        cave_or_alloc(image).is_some()
    }

    pub fn place_cooker(&self, rows: usize) {
        if rows < MIN_ADDR {
            return;
        }
        let count = read_u32(rows.wrapping_add(4)).unwrap_or(0);
        let data = read_u64(rows.wrapping_add(8)).unwrap_or(0);
        if data < MIN_ADDR as u64 || !(2..=64).contains(&count) {
            return;
        }
        if !readable(data as usize, (count as usize).wrapping_mul(QUICK_ROW_SIZE)) {
            return;
        }
        let mut parsed = Vec::new();
        let slot_kinds: Vec<(i32, i32)> = self.kinds.iter().map(|(s, k)| (*s, *k)).collect();
        for i in 0..count {
            let addr = (data as usize).wrapping_add((i as usize).wrapping_mul(QUICK_ROW_SIZE));
            let Some(raw) = mem::read_bytes(addr, QUICK_ROW_SIZE) else {
                return;
            };
            let typ = i32::from_le_bytes([
                raw.first().copied().unwrap_or(0),
                raw.get(1).copied().unwrap_or(0),
                raw.get(2).copied().unwrap_or(0),
                raw.get(3).copied().unwrap_or(0),
            ]);
            let mark = u32::from_le_bytes([
                raw.get(QUICK_MARK_AT).copied().unwrap_or(0),
                raw.get(QUICK_MARK_AT + 1).copied().unwrap_or(0),
                raw.get(QUICK_MARK_AT + 2).copied().unwrap_or(0),
                raw.get(QUICK_MARK_AT + 3).copied().unwrap_or(0),
            ]);
            let kind = parse_dest_kind(mark, &slot_kinds);
            parsed.push(DestRow { typ, kind, raw });
        }
        let ordered = place_cooker_dests(&parsed);
        if ordered == parsed {
            return;
        }
        let mut blob = Vec::new();
        for row in &ordered {
            blob.extend_from_slice(&row.raw);
        }
        let _ = write_slice(data as usize, &blob);
    }

    fn mark_row(&mut self, row: usize, kind: i32, name: &str) {
        let slot = self.slot.rem_euclid(QUICK_SLOTS);
        self.slot = self.slot.wrapping_add(1);
        quick_set_name(slot, name);
        self.kinds.insert(slot, kind);
        let _ = unsafe { mem::write(row.wrapping_add(QUICK_MARK_AT), QUICK_MAGIC.wrapping_add(slot as u32)) };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn expand(
        &mut self,
        log: &Log,
        game: &mut Game,
        expand: bool,
        show_all: bool,
        rows: usize,
        key: usize,
        flag: u64,
        push_va: Option<usize>,
        image: &nms_core::process::ProcessImage,
    ) {
        if rows < MIN_ADDR || key < MIN_ADDR {
            return;
        }
        let ident = read_u16(key).unwrap_or(0);
        let mut kinds = if expand { game.object_chests(ident) } else { Vec::new() };
        let want_cold = game.fish_hold() && game.is_fish_product(self.qt_element);
        let mut source_kinds = kinds.clone();
        if want_cold {
            source_kinds.push(FISH_KIND);
        }
        let source = self.source(game, &source_kinds);
        if let Some(src) = source {
            kinds.retain(|k| *k != src);
        }
        if want_cold && source != Some(FISH_KIND) {
            kinds.push(FISH_KIND);
        }
        if kinds.is_empty() {
            return;
        }
        let first = self.row_at(rows, ident);
        if first == 0 {
            log.info("Quick Transfer rows: the storage row was not where expected, left as it is");
            return;
        }
        if !self.ensure_push(push_va, image) {
            return;
        }
        let Some(push) = self.push else {
            return;
        };
        if !expand {
            if !kinds.contains(&FISH_KIND) {
                return;
            }
            let _ = unsafe { mem::write(first.wrapping_add(QUICK_MARK_AT), 0u32) };
            unsafe { push(rows as u64, key as u64, flag) };
            let row = self.row_at(rows, ident);
            if row == 0 {
                log.info("Quick Transfer rows: could not add the Cold Storage row");
                return;
            }
            self.mark_row(row, FISH_KIND, QUICK_COLD_NAME);
            self.place_cooker(rows);
            let shown = format!("[('cold', '{QUICK_COLD_NAME}')]");
            let state = (ident, shown);
            if self.state.as_ref() != Some(&state) {
                self.state = Some(state.clone());
                log.info(&format!("Quick Transfer rows for object {ident}: {}", state.1));
            }
            return;
        }
        let boxes: Vec<i32> = kinds.iter().copied().filter(|&k| k != FISH_KIND).collect();
        let mut list: Vec<Option<i32>> = Vec::new();
        if kinds.contains(&FISH_KIND) {
            list.push(Some(FISH_KIND));
        }
        if show_all {
            list.push(None);
        }
        list.extend(boxes.into_iter().map(Some));
        let first_index = read_i32(rows.wrapping_add(4)).unwrap_or(0).saturating_sub(1);
        let now = Instant::now();
        let stale = self
            .seen
            .map(|t| now.duration_since(t).as_secs_f64() > 0.3)
            .unwrap_or(true);
        if stale {
            self.off = 0;
            self.focus = None;
        }
        self.seen = Some(now);
        let focus_addr = self.page.wrapping_add(QUICK_FOCUS_AT);
        let focus = if self.page >= MIN_ADDR {
            read_i32(focus_addr).unwrap_or(0)
        } else {
            0
        };
        let (off, new_focus, window) = quick_window(&list, first_index, self.page >= MIN_ADDR, self.off, self.focus, focus, stale);
        self.off = off;
        self.focus = Some(new_focus);
        if self.page >= MIN_ADDR && new_focus != focus {
            let _ = unsafe { mem::write(focus_addr, new_focus) };
        }
        let mut shown = Vec::new();
        for (position, kind) in window.iter().enumerate() {
            let mut row = first;
            if position > 0 {
                unsafe { push(rows as u64, key as u64, flag) };
                row = self.row_at(rows, ident);
                if row == 0 {
                    match kind {
                        None => log.info("Quick Transfer rows: could not add the All Containers row"),
                        Some(k) if *k == FISH_KIND => {
                            log.info("Quick Transfer rows: could not add the Cold Storage row")
                        }
                        Some(k) => log.info(&format!(
                            "Quick Transfer rows: could not add the row for container {}",
                            k - CHESTS.start
                        )),
                    }
                    break;
                }
            }
            match kind {
                None => {
                    let _ = unsafe { mem::write(row.wrapping_add(QUICK_MARK_AT), 0u32) };
                    shown.push((ShownKind::All, QUICK_ALL_NAME.to_string()));
                }
                Some(k) if *k == FISH_KIND => {
                    self.mark_row(row, FISH_KIND, QUICK_COLD_NAME);
                    shown.push((ShownKind::Cold, QUICK_COLD_NAME.to_string()));
                }
                Some(k) => {
                    let name = {
                        let custom = game.container_name(*k);
                        if custom.is_empty() {
                            super::patches::numbered_name(k - CHESTS.start + 1)
                        } else {
                            custom
                        }
                    };
                    self.mark_row(row, *k, &name);
                    shown.push((ShownKind::Box(k - CHESTS.start), name));
                }
            }
        }
        self.place_cooker(rows);
        let refs: Vec<(ShownKind, &str)> = shown.iter().map(|(k, n)| (*k, n.as_str())).collect();
        let text = shown_pairs_text(&refs);
        let state = (ident, text);
        if self.state.as_ref() != Some(&state) {
            self.state = Some(state.clone());
            log.info(&format!("Quick Transfer rows for object {ident}: {}", state.1));
        }
    }
}

fn write_slice(addr: usize, bytes: &[u8]) -> bool {
    if !readable(addr, bytes.len()) {
        return false;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len());
    }
    true
}

// Silence unused import if the helper name was a leftover idea.
