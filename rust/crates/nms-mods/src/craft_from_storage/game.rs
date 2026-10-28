//! Addresses read out of the game code, plus chest and view helpers.
#![allow(clippy::missing_transmute_annotations)]

use std::time::Instant;

use nms_core::log::Log;
use nms_core::mem::{self, read_i32, read_u32, read_u64, readable, MIN_ADDR};
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::craft_from_storage as sigs;

use super::lists::{
    chest_kinds, dedup_keep, kinds, CHESTS, ELEMENT_TYPE, FISH_CATEGORY, FISH_KIND, LIST_COUNT, LIST_DATA, PANTRY_KIND,
    POPUP_VECTOR_MAX, PRODUCT, PRODUCT_CATEGORY, REACH_SECONDS, STORE_NAME_AT, VIEW_OFFSET,
};

const PUSH_AT: usize = 0x89;
const PUSH_BYTES: &[u8] = &[0x48, 0x8D, 0x55, 0x30, 0x44, 0x89, 0x7D, 0x30, 0x48, 0x8B, 0xCB, 0xE8];
const REACHABLE_PUSH_AT: usize = 0x1C2;
const REACHABLE_AT: usize = 0x1CA;
const REACHABLE_BYTES: &[u8] = &[0x48, 0x8B, 0xCB, 0xE8];
const FREE_AT: usize = 0x6CD;
const FREE_BYTES: &[u8] = &[
    0x41, 0xB8, 0xFF, 0xFF, 0xFF, 0xFF, 0x48, 0x8D, 0x4D, 0xF0, 0x48, 0x8B, 0xD7, 0xE8,
];
const OWNED_LISTS_AT: [usize; 2] = [0x49, 0x55];
const NEARBY_IDS_AT: usize = 0x03;
const NEARBY_COUNT_AT: usize = 0x09;
// Build 183791 (BASE block grew by 1 byte before the mask and a 10-byte nop before the loop).
const BASE_TYPES_AT: usize = 0x25;
const OBJECT_TYPE_CALL_AT: usize = 0x46;
const BASE_CHESTS_CALL_AT: usize = 0x77;
const VIEW_GLOBAL_AT: usize = 0x64;
const VIEW_GLOBAL_BYTES: &[u8] = &[0x48, 0x8B, 0x0D];
const VIEW_CALL_AT: usize = 0x6B;
const VIEW_CALL_BYTES: &[u8] = &[0x48, 0x81, 0xC1, 0x40, 0xC2, 0x00, 0x00, 0xE8];
const VIEW_PUSH_AT: usize = 0x81;
const VIEW_PUSH_BYTES: &[u8] = &[0x48, 0x8B, 0xCB, 0xE8];
const COOK_PRODUCT_GLOBAL_AT: usize = 0x34;
const COOK_PRODUCT_GLOBAL_BYTES: &[u8] = &[0x48, 0x8B, 0x0D];
const COOK_PRODUCT_CALL_AT: usize = 0x3B;
const COOK_PRODUCT_CALL_BYTES: &[u8] = &[0x48, 0x83, 0xC1, 0x60, 0xE8];

type PushFn = unsafe extern "system" fn(u64, u64);
type ReachableFn = unsafe extern "system" fn(u64);
type FreeFn = unsafe extern "system" fn(u64, u64, i32);
type ObjectTypeFn = unsafe extern "system" fn(u64) -> i32;
type BaseChestsFn = unsafe extern "system" fn(u64, u64, u64) -> i32;
type ViewFn = unsafe extern "system" fn(u64, i32, i32) -> u64;
type PushViewFn = unsafe extern "system" fn(u64, u64);
type ProductFn = unsafe extern "system" fn(u64, u64) -> u64;

pub struct Game {
    push_va: usize,
    reachable_va: usize,
    free_va: usize,
    nearby_ids: usize,
    nearby_count: usize,
    base_types: u32,
    object_type_va: usize,
    base_chests_va: usize,
    owned_lists: [usize; 2],
    pub owned_counts: Vec<i32>,
    reach_at: Option<Instant>,
    reach_list: Vec<i32>,
    view_va: Option<usize>,
    view_owner: usize,
    push_view_va: Option<usize>,
    product_data_va: Option<usize>,
    product_root: usize,
    pub why: String,
}

impl Game {
    pub fn new(image: &ProcessImage, log: &Log) -> Result<Self, String> {
        let choices_rva = find_sig(image, sigs::CHOICES)
            .ok_or_else(|| "the crafting inventory list code was not found (game update?)".to_string())?;
        let start = image.va(choices_rva);
        let push = call_after(start, PUSH_AT, PUSH_BYTES)?;
        if call_after(start, REACHABLE_PUSH_AT, REACHABLE_BYTES)? != push {
            return Err("unexpected game code before the Storage Container lookup (game update?)".into());
        }
        let reachable = call_after(start, REACHABLE_AT, REACHABLE_BYTES)?;
        let free = call_after(start, FREE_AT, FREE_BYTES)?;
        let block_rva = find_sig(image, sigs::BASE)
            .ok_or_else(|| "the base Storage Container lookup was not found (game update?)".to_string())?;
        let block = image.va(block_rva);
        let nearby_ids = rel32_va(block.wrapping_add(NEARBY_IDS_AT))
            .ok_or_else(|| "the base Storage Container lookup was not found (game update?)".to_string())?;
        let nearby_count = rel32_va(block.wrapping_add(NEARBY_COUNT_AT))
            .ok_or_else(|| "the base Storage Container lookup was not found (game update?)".to_string())?;
        let base_types = read_u32(block.wrapping_add(BASE_TYPES_AT)).unwrap_or(0);
        let object_type = rel32_va(block.wrapping_add(OBJECT_TYPE_CALL_AT))
            .ok_or_else(|| "the base Storage Container lookup was not found (game update?)".to_string())?;
        let base_chests = rel32_va(block.wrapping_add(BASE_CHESTS_CALL_AT))
            .ok_or_else(|| "the base Storage Container lookup was not found (game update?)".to_string())?;
        let owned_rva = find_sig(image, sigs::OWNED_LISTS)
            .ok_or_else(|| "the charge inventory lists were not found (game update?)".to_string())?;
        let owned_base = image.va(owned_rva);
        let owned_lists = [
            rel32_va(owned_base.wrapping_add(OWNED_LISTS_AT[0]))
                .ok_or_else(|| "the charge inventory lists were not found (game update?)".to_string())?,
            rel32_va(owned_base.wrapping_add(OWNED_LISTS_AT[1]))
                .ok_or_else(|| "the charge inventory lists were not found (game update?)".to_string())?,
        ];

        let mut game = Self {
            push_va: push,
            reachable_va: reachable,
            free_va: free,
            nearby_ids,
            nearby_count,
            base_types,
            object_type_va: object_type,
            base_chests_va: base_chests,
            owned_lists,
            owned_counts: Vec::new(),
            reach_at: None,
            reach_list: Vec::new(),
            view_va: None,
            view_owner: 0,
            push_view_va: None,
            product_data_va: None,
            product_root: 0,
            why: String::new(),
        };

        match resolve_view(image) {
            Ok((owner, view, push_view)) => {
                game.view_owner = owner;
                game.view_va = Some(view);
                game.push_view_va = Some(push_view);
            }
            Err(error) => {
                log.info(&format!("Fossil picker stays vanilla: {error} (game update?)"));
            }
        }
        match resolve_product(image) {
            Ok((root, data)) => {
                game.product_root = root;
                game.product_data_va = Some(data);
            }
            Err(error) => {
                log.info(&format!("Quick Transfer Cold Storage stays vanilla: {error} (game update?)"));
            }
        }
        Ok(game)
    }

    fn push(&self, list: u64, value: u64) {
        if self.push_va == 0 {
            return;
        }
        let f: PushFn = unsafe { std::mem::transmute(self.push_va) };
        unsafe { f(list, value) };
    }

    pub(crate) fn push_kind(&self, list: u64, kind: i32) {
        let mut value = kind;
        self.push(list, (&mut value as *mut i32) as u64);
    }

    pub fn append_views(&mut self, vector: usize, kinds_in: &[i32]) -> Vec<(String, i32)> {
        self.why.clear();
        let Some(view_va) = self.view_va else {
            self.why = "no view function or vector".into();
            return Vec::new();
        };
        if vector < MIN_ADDR {
            self.why = "no view function or vector".into();
            return Vec::new();
        }
        let Some(capacity) = read_u32(vector) else {
            self.why = "no view function or vector".into();
            return Vec::new();
        };
        let Some(count) = read_u32(vector.wrapping_add(4)) else {
            self.why = "no view function or vector".into();
            return Vec::new();
        };
        let data = read_u64(vector.wrapping_add(8)).unwrap_or(0);
        self.why = format!("vector capacity {capacity} count {count}");
        if (count > 0 && data < MIN_ADDR as u64) || count > capacity || capacity > POPUP_VECTOR_MAX {
            self.why.push_str(", unusable");
            return Vec::new();
        }
        let owner = read_u64(self.view_owner).unwrap_or(0);
        if owner < MIN_ADDR as u64 {
            self.why.push_str(", no owner pointer");
            return Vec::new();
        }
        let owner = owner.wrapping_add(VIEW_OFFSET);
        let mut views = Vec::new();
        if data >= MIN_ADDR as u64 {
            for n in 0..count {
                if let Some(v) = read_u64((data as usize).wrapping_add((n as usize).wrapping_mul(8))) {
                    views.push(v);
                }
            }
        }
        let view_fn: ViewFn = unsafe { std::mem::transmute(view_va) };
        let Some(push_view_va) = self.push_view_va else {
            return Vec::new();
        };
        let push_view: PushViewFn = unsafe { std::mem::transmute(push_view_va) };
        let mut added = Vec::new();
        for &kind in kinds_in {
            let view = unsafe { view_fn(owner, kind, -1) };
            if view < MIN_ADDR as u64 || views.contains(&view) {
                let name = if kind == PANTRY_KIND {
                    "pantry".to_string()
                } else {
                    format!("container {}", kind - CHESTS.start)
                };
                self.why.push_str(&format!(
                    ", {name} view {}",
                    if view >= MIN_ADDR as u64 { "duplicate" } else { "null" }
                ));
                continue;
            }
            views.push(view);
            let mut value = view;
            unsafe { push_view(vector as u64, (&mut value as *mut u64) as u64) };
            let items = read_i32((view as usize).wrapping_add(0x9C)).unwrap_or(0);
            let label = if kind == PANTRY_KIND {
                "pantry".to_string()
            } else {
                format!("{}", kind - CHESTS.start)
            };
            added.push((label, items));
        }
        let now = read_u32(vector.wrapping_add(4)).unwrap_or(count);
        self.why.push_str(&format!(", now count {now}"));
        added
    }

    pub fn make_room(&mut self) -> Result<(), String> {
        for &owned in &self.owned_lists {
            let count = read_i32(owned.wrapping_add(LIST_COUNT)).unwrap_or(0);
            let present = kinds(owned);
            if present.contains(&CHESTS.start) || !(0 < count && count < 64) {
                return Err("unexpected charge inventory list (game update?)".into());
            }
            for kind in CHESTS {
                let mut value = kind;
                self.push(owned as u64, (&mut value as *mut i32) as u64);
            }
            let _ = unsafe { mem::write(owned.wrapping_add(LIST_COUNT), count) };
            self.owned_counts.push(count);
        }
        Ok(())
    }

    pub fn set_owned_chests(&self, chests: &[i32]) {
        for (owned, count) in self.owned_lists.iter().copied().zip(self.owned_counts.iter().copied()) {
            let data = read_u64(owned.wrapping_add(LIST_DATA)).unwrap_or(0);
            if data >= MIN_ADDR as u64 {
                for (n, &kind) in chests.iter().enumerate() {
                    let off = (data as usize).wrapping_add(4usize.wrapping_mul(count.wrapping_add(n as i32) as usize));
                    let _ = unsafe { mem::write(off, kind) };
                }
            }
            let new_count = count.wrapping_add(chests.len() as i32);
            let _ = unsafe { mem::write(owned.wrapping_add(LIST_COUNT), new_count) };
        }
    }

    fn collect<F: FnOnce(u64)>(&self, fill: F) -> Vec<i32> {
        let mut found = [0u64; 2];
        let addr = found.as_mut_ptr() as u64;
        fill(addr);
        let chests = chest_kinds(addr as usize);
        if found[1] != 0 {
            let free: FreeFn = unsafe { std::mem::transmute(self.free_va) };
            unsafe { free(addr, found[1], -1) };
        }
        chests
    }

    fn nearby_bases(&self) -> Vec<u16> {
        let ids = read_u64(self.nearby_ids).unwrap_or(0);
        let count = read_u32(self.nearby_count).unwrap_or(0);
        if ids < MIN_ADDR as u64 || count > 4096 {
            return Vec::new();
        }
        let ids = ids as usize;
        let mut bases = Vec::new();
        let object_type: ObjectTypeFn = unsafe { std::mem::transmute(self.object_type_va) };
        for n in 0..count {
            let off = ids.wrapping_add((n as usize).wrapping_mul(2));
            let lo = mem::read_u8(off).unwrap_or(0);
            let hi = mem::read_u8(off.wrapping_add(1)).unwrap_or(0);
            let oid = u16::from_le_bytes([lo, hi]);
            let mut object_id = oid;
            let kind = unsafe { object_type((&mut object_id as *mut u16) as u64) };
            if (0..=0x10).contains(&kind) && (self.base_types >> kind) & 1 == 1 {
                bases.push(oid);
            }
        }
        bases
    }

    pub fn reachable_chests(&mut self) -> Vec<i32> {
        if let Some(when) = self.reach_at {
            if when.elapsed().as_secs_f64() < REACH_SECONDS {
                return self.reach_list.clone();
            }
        }
        let mut chests = Vec::new();
        let base_chests: BaseChestsFn = unsafe { std::mem::transmute(self.base_chests_va) };
        for object_id in self.nearby_bases() {
            let mut key = object_id;
            let key_ptr = (&mut key as *mut u16) as u64;
            chests.extend(self.collect(|found| unsafe {
                base_chests(0, key_ptr, found);
            }));
        }
        let reachable: ReachableFn = unsafe { std::mem::transmute(self.reachable_va) };
        chests.extend(self.collect(|found| unsafe {
            reachable(found);
        }));
        let chests = dedup_keep(&chests);
        self.reach_at = Some(Instant::now());
        self.reach_list = chests.clone();
        chests
    }

    pub fn object_chests(&self, object_id: u16) -> Vec<i32> {
        let mut key = object_id;
        let key_ptr = (&mut key as *mut u16) as u64;
        let base_chests: BaseChestsFn = unsafe { std::mem::transmute(self.base_chests_va) };
        self.collect(|found| unsafe {
            base_chests(0, key_ptr, found);
        })
    }

    pub fn view(&self, kind: i32) -> u64 {
        let Some(view_va) = self.view_va else {
            return 0;
        };
        let owner = read_u64(self.view_owner).unwrap_or(0);
        if owner < MIN_ADDR as u64 {
            return 0;
        }
        let view_fn: ViewFn = unsafe { std::mem::transmute(view_va) };
        unsafe { view_fn(owner.wrapping_add(VIEW_OFFSET), kind, -1) }
    }

    pub fn fish_hold(&self) -> bool {
        self.view(FISH_KIND) >= MIN_ADDR as u64
    }

    pub fn is_fish_product(&self, element: u64) -> bool {
        if element < MIN_ADDR as u64 || self.product_data_va.is_none() || self.product_root < MIN_ADDR {
            return false;
        }
        if read_i32((element as usize).wrapping_add(ELEMENT_TYPE)) != Some(PRODUCT) {
            return false;
        }
        let table = read_u64(self.product_root).unwrap_or(0);
        if table < MIN_ADDR as u64 {
            return false;
        }
        let Some(product_va) = self.product_data_va else {
            return false;
        };
        if product_va == 0 {
            return false;
        }
        let product: ProductFn = unsafe { std::mem::transmute(product_va) };
        let data = unsafe { product(table.wrapping_add(0x60), element) };
        if data < MIN_ADDR as u64 {
            return false;
        }
        read_i32((data as usize).wrapping_add(PRODUCT_CATEGORY)) == Some(FISH_CATEGORY)
    }

    pub fn container_name(&self, kind: i32) -> String {
        let view = self.view(kind);
        if view < MIN_ADDR as u64 {
            return String::new();
        }
        let addr = (view as usize).wrapping_add(STORE_NAME_AT);
        if !readable(addr, 64) {
            return String::new();
        }
        let Some(raw) = mem::read_bytes(addr, 64) else {
            return String::new();
        };
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        let text = String::from_utf8_lossy(raw.get(..end).unwrap_or(&[])).into_owned();
        if text.is_empty() || !text.chars().all(|c| !c.is_control()) || text == "BLD_STORAGE_NAME" {
            return String::new();
        }
        text
    }
}

fn find_sig(image: &ProcessImage, sig: &str) -> Option<usize> {
    Pattern::parse(sig).ok().and_then(|p| image.find_unique(&p).unique())
}

fn rel32_va(operand: usize) -> Option<usize> {
    let d = read_i32(operand)?;
    Some(operand.wrapping_add(4).wrapping_add(d as usize))
}

fn call_after(start: usize, at: usize, expected: &[u8]) -> Result<usize, String> {
    let addr = start.wrapping_add(at);
    let got = mem::read_bytes(addr, expected.len());
    if got.as_deref() != Some(expected) {
        return Err(format!("unexpected game code at +{at:#x} (game update?)"));
    }
    rel32_va(addr.wrapping_add(expected.len()))
        .ok_or_else(|| format!("unexpected game code at +{at:#x} (game update?)"))
}

fn resolve_view(image: &ProcessImage) -> Result<(usize, usize, usize), String> {
    let helper_rva = find_sig(image, sigs::VIEW).ok_or("the popup inventory view helper was not found")?;
    let helper = image.va(helper_rva);
    let got = mem::read_bytes(helper.wrapping_add(VIEW_GLOBAL_AT), VIEW_GLOBAL_BYTES.len());
    if got.as_deref() != Some(VIEW_GLOBAL_BYTES) {
        return Err("unexpected popup helper code".into());
    }
    let owner = rel32_va(helper.wrapping_add(VIEW_GLOBAL_AT).wrapping_add(VIEW_GLOBAL_BYTES.len()))
        .ok_or("unexpected popup helper code")?;
    let view = call_after(helper, VIEW_CALL_AT, VIEW_CALL_BYTES)?;
    let push_view = call_after(helper, VIEW_PUSH_AT, VIEW_PUSH_BYTES)?;
    Ok((owner, view, push_view))
}

fn resolve_product(image: &ProcessImage) -> Result<(usize, usize), String> {
    let cook_rva = find_sig(image, sigs::COOK_ITEM).ok_or("the cooking-item helper was not found")?;
    let cook = image.va(cook_rva);
    let got = mem::read_bytes(cook.wrapping_add(COOK_PRODUCT_GLOBAL_AT), COOK_PRODUCT_GLOBAL_BYTES.len());
    if got.as_deref() != Some(COOK_PRODUCT_GLOBAL_BYTES) {
        return Err("unexpected cooking-item helper code".into());
    }
    let root = rel32_va(
        cook.wrapping_add(COOK_PRODUCT_GLOBAL_AT)
            .wrapping_add(COOK_PRODUCT_GLOBAL_BYTES.len()),
    )
    .ok_or("unexpected cooking-item helper code")?;
    let data = call_after(cook, COOK_PRODUCT_CALL_AT, COOK_PRODUCT_CALL_BYTES)?;
    Ok((root, data))
}
