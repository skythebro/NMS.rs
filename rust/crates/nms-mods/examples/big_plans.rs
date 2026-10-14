//! Print page-size plans from the Rust sigscan (compare with `dev/big_plans.py`).

use std::env;
use std::path::{Path, PathBuf};

use nms_core::FileImage;
use nms_mods::bigger_item_grids::sigscan::{self, ScaleRedirect};
use serde_json::Number;

fn find_exe(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|d| d.join("Binaries").join("NMS.exe"))
        .find(|p| p.is_file())
}

fn format_scale(items: u32) -> String {
    let v = 1.0f64 / f64::from(items);
    Number::from_f64(v)
        .and_then(|n| serde_json::to_string(&n).ok())
        .unwrap_or_else(|| v.to_string())
}

fn format_plan(label: &str, plan: Option<&ScaleRedirect>, items: u32) -> String {
    let Some(plan) = plan else {
        return format!("{label} none");
    };
    let mut parts = vec![format!("{label} cave={:x} scale={}", plan.cave_rva, format_scale(items))];
    for (rva, old, new) in &plan.patches {
        parts.push(format!("{rva:x}:{}->{}", hex_bytes(old), hex_bytes(new)));
    }
    parts.join(" ")
}

fn hex_bytes(b: &[u8]) -> String {
    b.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn main() {
    let items: u32 = env::args()
        .nth(1)
        .unwrap_or_else(|| {
            eprintln!("usage: big_plans <items>");
            std::process::exit(1);
        })
        .parse()
        .unwrap_or_else(|_| {
            eprintln!("items must be an integer");
            std::process::exit(1);
        });
    let start = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let exe = find_exe(&start).unwrap_or_else(|| {
        eprintln!("Binaries/NMS.exe not found (walk up from cwd)");
        std::process::exit(1);
    });
    let img = FileImage::load(&exe).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    let slot = sigscan::plan_slot_filler(&img, items);
    println!("{}", format_plan("Slot filler", slot.as_ref(), items));
    let transfer = sigscan::plan_scale_redirect(&img, "TRANSFER_RECEIVE_DESC", items);
    println!("{}", format_plan("Transfer Items dots", transfer.as_ref(), items));
    let craft = sigscan::plan_scale_redirect(&img, "PRODUCTS_ALL", items);
    println!("{}", format_plan("Craft Product dots", craft.as_ref(), items));
}
