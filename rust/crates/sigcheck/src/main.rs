//! `cargo run -p sigcheck --release [-- <NEW.exe>]`
//! `cargo run -p sigcheck --release [-- <NEW.exe>]`
//! `cargo run -p sigcheck --release -- --compare <OLD.exe> [<NEW.exe>] [--verbose]`
//!
//! Without a path it walks up from the current folder to `<game>\Binaries\NMS.exe`.
//! Exit code 1 if any signature is missing, matches twice, or a patch site has unexpected bytes.
//!
//! `--compare` checks both builds and lists what changed per signature: BROKE (found before, not
//! unique now), FIXED, NEW, GONE, and MOVED (same signature at a new RVA; only listed with
//! `--verbose`). Then it gives a verdict per mod. Exit code 1 if anything broke or the new build
//! has a failure. Old builds are in `nmsexebackups\` (`NMS_public 2810 183791.exe` is 183791,
//! `NMS_experimental 0810.exe` is 181442, `NMS_experimental 0110.exe` is 180836). The old build is checked against the current tables, so
//! a FIXED on an old build can just mean that signature was widened after that build.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nms_core::FileImage;
use sigcheck::{Delta, Line};

fn load(path: &Path) -> Result<FileImage, ExitCode> {
    FileImage::load(path).map_err(|e| {
        eprintln!("{e}");
        ExitCode::from(2)
    })
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = args.iter().any(|a| a == "--verbose");
    args.retain(|a| a != "--verbose");
    let (old_arg, new_arg) = match args.first().map(String::as_str) {
        Some("--compare") => match args.get(1) {
            Some(old) => (Some(PathBuf::from(old)), args.get(2).map(PathBuf::from)),
            None => {
                eprintln!("usage: sigcheck --compare <OLD.exe> [<NEW.exe>]");
                return ExitCode::from(2);
            }
        },
        Some(_) => (None, args.first().map(PathBuf::from)),
        None => (None, None),
    };
    let new_exe = match new_arg.or_else(|| std::env::current_dir().ok().and_then(|d| sigcheck::find_exe(&d))) {
        Some(p) => p,
        None => {
            eprintln!("NMS.exe not found; pass its path");
            return ExitCode::from(2);
        }
    };
    let new_img = match load(&new_exe) {
        Ok(i) => i,
        Err(code) => return code,
    };
    match old_arg {
        None => report(&new_exe, &new_img),
        Some(old_exe) => {
            let old_img = match load(&old_exe) {
                Ok(i) => i,
                Err(code) => return code,
            };
            compare_report(&old_exe, &old_img, &new_exe, &new_img, verbose)
        }
    }
}

fn report(exe: &Path, image: &FileImage) -> ExitCode {
    println!("{}", exe.display());
    let lines = sigcheck::check(image, nms_sdk::all());
    let mut bad = 0;
    let mut known = 0;
    for l in &lines {
        let (tag, note) = match (l.ok, sigcheck::known_reason(l)) {
            (true, _) => ("ok   ", String::new()),
            (false, Some(reason)) => {
                known += 1;
                ("KNOWN", format!(" ({reason})"))
            }
            (false, None) => {
                bad += 1;
                ("FAIL ", String::new())
            }
        };
        println!("{tag} {:<22} {:<38} {}{note}", l.owner, l.name, l.detail);
    }
    for o in sigcheck::overlaps(image, nms_sdk::all()) {
        let tag = if o.shared {
            "SHARE"
        } else {
            bad += 1;
            "FAIL "
        };
        println!(
            "{tag} {}::{} hooks RVA {:#x}, also matched by {}::{}",
            o.hook.0, o.hook.1, o.rva, o.other.0, o.other.1
        );
    }
    println!("{} checked, {} failed, {} known", lines.len(), bad, known);
    if bad == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn side(line: &Option<Line>) -> &str {
    line.as_ref().map_or("-", |l| l.detail.as_str())
}

fn compare_report(
    old_exe: &Path,
    old_img: &FileImage,
    new_exe: &Path,
    new_img: &FileImage,
    verbose: bool,
) -> ExitCode {
    let tables = nms_sdk::all();
    let old_lines = sigcheck::check(old_img, tables);
    let new_lines = sigcheck::check(new_img, tables);
    let changes = sigcheck::compare(&old_lines, &new_lines);

    println!("old: {}", old_exe.display());
    println!("new: {}", new_exe.display());
    let (mut broke, mut fixed, mut moved, mut added, mut gone) = (0, 0, 0, 0, 0);
    for c in &changes {
        let tag = match c.delta {
            Delta::Same => continue,
            Delta::Broke => {
                broke += 1;
                "BROKE"
            }
            Delta::Fixed => {
                fixed += 1;
                "FIXED"
            }
            Delta::Moved => {
                moved += 1;
                if !verbose {
                    continue;
                }
                "MOVED"
            }
            Delta::New => {
                added += 1;
                "NEW"
            }
            Delta::Gone => {
                gone += 1;
                "GONE"
            }
        };
        println!("{tag:<6} {:<22} {:<38} old: {} | new: {}", c.owner, c.name, side(&c.old), side(&c.new));
    }
    if !verbose && moved > 0 {
        println!("({moved} MOVED: same signature at a new RVA; add --verbose to list them)");
    }

    println!();
    println!("by mod:");
    let mut owners: Vec<&str> = Vec::new();
    for m in tables {
        // One owner can have several tables; list it once.
        if owners.contains(&m.owner) {
            continue;
        }
        owners.push(m.owner);
        let mine: Vec<_> = changes.iter().filter(|c| c.owner == m.owner && c.delta != Delta::Same).collect();
        let failing_now = new_lines
            .iter()
            .filter(|l| l.owner == m.owner && !l.ok && sigcheck::known_reason(l).is_none())
            .count();
        if mine.iter().all(|c| c.delta == Delta::Moved) && failing_now == 0 {
            continue;
        }
        let count = |d: Delta| mine.iter().filter(|c| c.delta == d).count();
        let verdict = if count(Delta::Broke) > 0 || failing_now > 0 { "BROKEN" } else { "ok" };
        println!(
            "  {verdict:<7} {:<22} broke {} fixed {} moved {} new {} gone {} failing now {}",
            m.owner,
            count(Delta::Broke),
            count(Delta::Fixed),
            count(Delta::Moved),
            count(Delta::New),
            count(Delta::Gone),
            failing_now
        );
    }

    let new_bad = new_lines.iter().filter(|l| !l.ok && sigcheck::known_reason(l).is_none()).count();
    let overlap_bad = sigcheck::overlaps(new_img, tables).iter().filter(|o| !o.shared).count();
    println!(
        "{} compared: {broke} broke, {fixed} fixed, {moved} moved, {added} new, {gone} gone. \
         New build: {new_bad} failing, {overlap_bad} unshared overlaps.",
        changes.len()
    );
    if broke == 0 && new_bad == 0 && overlap_bad == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
