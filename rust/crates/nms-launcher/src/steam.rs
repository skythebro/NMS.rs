//! Steam launch option: find Steam and edit every account's `localconfig.vdf`.
//!
//! Ports `loader/lib/steam_launch.py`. The option is
//! `"<this exe>" play %command%`. Steam rewrites the VDF on exit, so `set` /
//! `remove` close Steam, edit, then start it again. Tests only touch temp folders.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const GAME_ID: &str = "275850";
const STEAM_ID64_BASE: u64 = 7_656_119_796_026_5728;
const APPS_PATH: &[&str] = &["userlocalconfigstore", "software", "valve", "steam", "apps"];
const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

type Vdf = Vec<(String, VdfValue)>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum VdfValue {
    Str(String),
    Section(Vdf),
}

pub(crate) fn launch_option(exe: &Path) -> String {
    format!("\"{}\" play %command%", exe.display())
}

pub(crate) fn cli(args: &[&str]) -> Result<String, String> {
    let yes = args.iter().any(|a| *a == "--yes");
    let action: Vec<&str> = args.iter().copied().filter(|a| *a != "--yes").collect();
    match action.as_slice() {
        ["status"] => cli_status(),
        ["set"] => cli_apply(true, !yes),
        ["remove"] => cli_apply(false, !yes),
        _ => Err("usage: nms-rs steam-option status | set | remove [--yes]".into()),
    }
}

fn cli_status() -> Result<String, String> {
    match steam_dir() {
        Err(_) => Ok("Steam not found".into()),
        Ok(steam) => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            match option_is_set(&steam, &exe) {
                Ok(true) => Ok("set".into()),
                Ok(false) | Err(_) => Ok("not set".into()),
            }
        }
    }
}

pub(crate) fn cli_apply(install: bool, ask: bool) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(apply_for(&exe, install, ask)?.join("\n"))
}

/// Setup passes `<game>\NMS.rs\nms-rs.exe`, not whatever copy ran first.
pub(crate) fn apply_for(exe: &Path, install: bool, ask: bool) -> Result<Vec<String>, String> {
    let steam = steam_dir()?;
    apply(&steam, exe, install, ask)
}

pub(crate) fn option_is_set_for(exe: &Path) -> bool {
    match steam_dir() {
        Ok(steam) => option_is_set(&steam, exe).unwrap_or(false),
        Err(_) => false,
    }
}

pub(crate) fn launch_option_strings() -> Vec<String> {
    let Ok(steam) = steam_dir() else {
        return Vec::new();
    };
    let Ok(paths) = config_files(&steam) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for path in paths {
        let Ok(text) = read_replace(&path) else {
            continue;
        };
        let Ok(tree) = parse(&text) else {
            continue;
        };
        if let Some(node) = app_node(&tree, false) {
            out.push(get(node, "LaunchOptions").unwrap_or_default());
        }
    }
    out
}

/// One of the launch options still holds a launch option that `strip_launch` would remove.
pub(crate) fn has_our_option(options: &[String]) -> bool {
    options.iter().any(|o| find_ours(o, 0).is_some())
}

/// One of the launch options still runs the version 3 loader (`NMS.py\launch.cmd` or `.sh`).
pub(crate) fn has_old_nms_py_option(options: &[String]) -> bool {
    options.iter().any(|o| {
        let lower = o.replace('/', "\\").to_ascii_lowercase();
        lower.contains("nms.py\\launch.cmd") || lower.contains("nms.py\\launch.sh")
    })
}

fn option_is_set(steam: &Path, exe: &Path) -> Result<bool, String> {
    let want = launch_option(exe).to_ascii_lowercase();
    let paths = config_files(steam)?;
    for path in paths {
        let tree = parse(&read_replace(&path)?)?;
        let Some(node) = app_node(&tree, false) else {
            return Ok(false);
        };
        let current = get(node, "LaunchOptions").unwrap_or_default();
        if !current.to_ascii_lowercase().contains(&want) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn apply(steam: &Path, exe: &Path, install: bool, ask: bool) -> Result<Vec<String>, String> {
    if install && option_is_set(steam, exe).unwrap_or(false) {
        return Ok(vec!["Steam launch option is already set.".into()]);
    }
    let mut reopen = false;
    if steam_running() {
        if ask {
            print!("Steam has to close for a moment to change the launch option. Close Steam now? [Y/n] ");
            let _ = io::stdout().flush();
            let mut answer = String::new();
            io::stdin().read_line(&mut answer).map_err(|e| e.to_string())?;
            if !matches!(answer.trim().to_ascii_lowercase().as_str(), "" | "y" | "yes") {
                return Ok(vec![format!(
                    "Skipped. To do it by hand: Steam > No Man's Sky > Properties > Launch Options: {}",
                    launch_option(exe)
                )]);
            }
        }
        println!("Closing Steam...");
        close_steam(steam)?;
        reopen = true;
    }
    let result = edit(steam, exe, install, default_verify);
    if reopen {
        println!("Starting Steam again...");
        start_steam(steam)?;
    }
    result
}

fn default_verify(written: &str, path: &Path) -> Result<bool, String> {
    let back = read_replace(path)?;
    Ok(dump(&parse(&back)?) == written)
}

fn edit(
    steam: &Path,
    exe: &Path,
    install: bool,
    verify: fn(&str, &Path) -> Result<bool, String>,
) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    for path in config_files(steam)? {
        let text = read_replace(&path)?;
        let mut tree = parse(&text)?;
        let account = path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "?".into());
        let message = if install {
            change_install(&mut tree, exe)
        } else {
            change_remove(&mut tree)
        };
        match message {
            None => lines.push(format!("Steam account {account}: already set.")),
            Some(msg) if msg.starts_with('!') => {
                lines.push(format!("Steam account {account}: {}", &msg[1..]));
            }
            Some(msg) => {
                let backup = path.with_extension("vdf.bak");
                std::fs::copy(&path, &backup).map_err(|e| format!("backup failed: {e}"))?;
                let written = dump(&tree);
                std::fs::write(&path, written.as_bytes()).map_err(|e| format!("write failed: {e}"))?;
                if !verify(&written, &path)? {
                    std::fs::copy(&backup, &path).map_err(|e| format!("restore failed: {e}"))?;
                    return Err(format!(
                        "Steam account {account}: the edited settings did not read back correctly, so the original was restored."
                    ));
                }
                let _ = std::fs::remove_file(&backup);
                lines.push(format!("Steam account {account}: {msg}"));
            }
        }
    }
    Ok(lines)
}

fn change_install(tree: &mut Vdf, exe: &Path) -> Option<String> {
    let node = app_node_mut(tree, true).expect("create");
    let current = get(node, "LaunchOptions").unwrap_or_default();
    let option = launch_option(exe);
    if current.to_ascii_lowercase().contains(&option.to_ascii_lowercase()) {
        return None;
    }
    let current = strip_ours(&current).trim().to_string();
    if current.to_ascii_lowercase().contains("%command%") {
        return Some(format!(
            "!left alone: its launch options already use %command% ({}). Add the mod to them yourself: {option}",
            current
        ));
    }
    set(node, "LaunchOptions", &format!("{option} {current}").trim());
    Some("launch option added.".into())
}

fn change_remove(tree: &mut Vdf) -> Option<String> {
    let Some(node) = app_node_mut(tree, false) else {
        return None;
    };
    let current = get(node, "LaunchOptions").unwrap_or_default();
    if find_ours(&current, 0).is_none() {
        return None;
    }
    set(node, "LaunchOptions", strip_ours(&current).trim());
    Some("launch option removed.".into())
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn unescape(raw: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            out.push(chars[i + 1]);
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

enum Token {
    Brace(char),
    Str(String),
}

fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let n = chars.len();
    let mut out = Vec::new();
    while i < n {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && i + 1 < n && chars[i + 1] == '/' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '{' || c == '}' {
            out.push(Token::Brace(c));
            i += 1;
        } else if c == '"' {
            i += 1;
            let start = i;
            while i < n && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i > n {
                return Err("unterminated string in VDF".into());
            }
            let raw: String = chars[start..i.min(n)].iter().collect();
            out.push(Token::Str(raw));
            i += 1;
        } else {
            let start = i;
            while i < n && !chars[i].is_whitespace() && chars[i] != '{' && chars[i] != '}' && chars[i] != '"' {
                i += 1;
            }
            out.push(Token::Str(chars[start..i].iter().collect()));
        }
    }
    Ok(out)
}

fn parse(text: &str) -> Result<Vdf, String> {
    fn section(toks: &[Token], i: &mut usize, nested: bool) -> Result<Vdf, String> {
        let mut node = Vec::new();
        let mut key: Option<String> = None;
        while *i < toks.len() {
            match &toks[*i] {
                Token::Brace('}') => {
                    if !nested {
                        return Err("unbalanced } in VDF".into());
                    }
                    if key.is_some() {
                        return Err("VDF ended early".into());
                    }
                    *i += 1;
                    return Ok(node);
                }
                Token::Brace('{') => {
                    *i += 1;
                    let k = key.take().ok_or("VDF { without key")?;
                    let inner = section(toks, i, true)?;
                    node.push((k, VdfValue::Section(inner)));
                }
                Token::Str(s) => {
                    *i += 1;
                    if key.is_none() {
                        key = Some(s.clone());
                    } else {
                        node.push((key.take().unwrap(), VdfValue::Str(s.clone())));
                    }
                }
                Token::Brace(_) => return Err("unbalanced } in VDF".into()),
            }
        }
        if nested || key.is_some() {
            return Err("VDF ended early".into());
        }
        Ok(node)
    }
    let toks = tokens(text)?;
    let mut i = 0;
    section(&toks, &mut i, false)
}

fn dump(node: &Vdf) -> String {
    dump_depth(node, 0)
}

fn dump_depth(node: &Vdf, depth: usize) -> String {
    let pad = "\t".repeat(depth);
    let mut lines = String::new();
    for (key, value) in node {
        match value {
            VdfValue::Section(inner) => {
                lines.push_str(&format!("{pad}\"{key}\"\n{pad}{{\n{}{pad}}}\n", dump_depth(inner, depth + 1)));
            }
            VdfValue::Str(v) => {
                lines.push_str(&format!("{pad}\"{key}\"\t\t\"{v}\"\n"));
            }
        }
    }
    lines
}

fn child<'a>(node: &'a Vdf, name: &str) -> Option<&'a Vdf> {
    for (key, value) in node {
        if let VdfValue::Section(inner) = value {
            if unescape(key).eq_ignore_ascii_case(name) {
                return Some(inner);
            }
        }
    }
    None
}

fn child_mut<'a>(node: &'a mut Vdf, name: &str, create: bool) -> Option<&'a mut Vdf> {
    let found = node.iter().position(|(key, value)| {
        matches!(value, VdfValue::Section(_)) && unescape(key).eq_ignore_ascii_case(name)
    });
    if let Some(i) = found {
        match &mut node[i].1 {
            VdfValue::Section(inner) => return Some(inner),
            _ => return None,
        }
    }
    if !create {
        return None;
    }
    node.push((name.to_string(), VdfValue::Section(Vec::new())));
    match &mut node.last_mut().unwrap().1 {
        VdfValue::Section(inner) => Some(inner),
        _ => None,
    }
}

fn walk_mut<'a>(node: &'a mut Vdf, names: &[&str], create: bool) -> Option<&'a mut Vdf> {
    let Some((first, rest)) = names.split_first() else {
        return Some(node);
    };
    let next = child_mut(node, first, create)?;
    walk_mut(next, rest, create)
}

fn app_node(tree: &Vdf, _create: bool) -> Option<&Vdf> {
    let mut node = tree;
    for name in APPS_PATH.iter().copied().chain(std::iter::once(GAME_ID)) {
        node = child(node, name)?;
    }
    Some(node)
}

fn app_node_mut(tree: &mut Vdf, create: bool) -> Option<&mut Vdf> {
    let mut names: Vec<&str> = APPS_PATH.to_vec();
    names.push(GAME_ID);
    walk_mut(tree, &names, create)
}

fn get(node: &Vdf, name: &str) -> Option<String> {
    for (key, value) in node {
        if let VdfValue::Str(v) = value {
            if unescape(key).eq_ignore_ascii_case(name) {
                return Some(unescape(v));
            }
        }
    }
    None
}

fn set(node: &mut Vdf, name: &str, text: &str) {
    for entry in node.iter_mut() {
        if matches!(entry.1, VdfValue::Str(_)) && unescape(&entry.0).eq_ignore_ascii_case(name) {
            entry.1 = VdfValue::Str(escape(text));
            return;
        }
    }
    node.push((name.to_string(), VdfValue::Str(escape(text))));
}

fn path_ends_ci(path: &str, suffix: &str) -> bool {
    let p = path.replace('/', "\\");
    let s = suffix.replace('/', "\\");
    p.len() >= s.len() && p.as_bytes().len() >= s.as_bytes().len() && p.to_ascii_lowercase().ends_with(&s.to_ascii_lowercase())
}

fn path_is_nms_rs(path: &str) -> bool {
    path_ends_ci(path, "\\nms-rs.exe") || path.eq_ignore_ascii_case("nms-rs.exe")
}

fn path_is_old_ours(path: &str) -> bool {
    path_ends_ci(path, "\\nms.py\\launch.cmd")
        || path_ends_ci(path, "\\nms.py\\launch.sh")
        || path_ends_ci(path, "\\biggeritemgrids.exe")
        || path_ends_ci(path, "\\play.bat")
}

fn ci_eq_at(s: &str, from: usize, needle: &str) -> bool {
    let rest = match s.get(from..) {
        Some(r) => r,
        None => return false,
    };
    if rest.len() < needle.len() {
        return false;
    }
    rest.as_bytes()
        .iter()
        .zip(needle.as_bytes())
        .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
        && rest.is_char_boundary(needle.len())
}

fn skip_ws(s: &str, mut i: usize) -> usize {
    while i < s.len() {
        let c = match s[i..].chars().next() {
            Some(c) => c,
            None => break,
        };
        if !c.is_whitespace() {
            break;
        }
        i += c.len_utf8();
    }
    i
}

fn match_ours_here(s: &str, start: usize) -> Option<usize> {
    if start >= s.len() || !s.is_char_boundary(start) {
        return None;
    }
    let mut i = start;
    if ci_eq_at(s, i, "bash") {
        let after = i + 4;
        if after < s.len() {
            if let Some(c) = s[after..].chars().next() {
                if c.is_whitespace() {
                    i = skip_ws(s, after);
                }
            }
        }
    }
    if !s.get(i..).is_some_and(|r| r.starts_with('"')) {
        return None;
    }
    i += 1;
    let close = s[i..].find('"')?;
    let path = &s[i..i + close];
    i += close + 1;
    i = skip_ws(s, i);
    let nms_rs = path_is_nms_rs(path);
    let old = path_is_old_ours(path);
    if !nms_rs && !old {
        return None;
    }
    if nms_rs {
        if !ci_eq_at(s, i, "play") {
            return None;
        }
        i += 4;
        i = skip_ws(s, i);
        if !ci_eq_at(s, i, "%command%") {
            return None;
        }
        i += 9;
        return Some(skip_ws(s, i));
    }
    if ci_eq_at(s, i, "--start") {
        i += 7;
        i = skip_ws(s, i);
    }
    if !ci_eq_at(s, i, "%command%") {
        return None;
    }
    i += 9;
    Some(skip_ws(s, i))
}

fn find_ours(s: &str, from: usize) -> Option<(usize, usize)> {
    let rest = s.get(from..)?;
    for (rel, _) in rest.char_indices() {
        let abs = from + rel;
        if let Some(end) = match_ours_here(s, abs) {
            return Some((abs, end));
        }
    }
    None
}

fn strip_ours(current: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < current.len() {
        match find_ours(current, i) {
            Some((a, b)) => {
                out.push_str(&current[i..a]);
                i = b;
            }
            None => {
                out.push_str(&current[i..]);
                break;
            }
        }
    }
    out
}

fn read_replace(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn config_files(steam: &Path) -> Result<Vec<PathBuf>, String> {
    let userdata = steam.join("userdata");
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&userdata) {
        for entry in entries.flatten() {
            let path = entry.path().join("config").join("localconfig.vdf");
            if path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut played = Vec::new();
    for path in &files {
        if let Ok(tree) = parse(&read_replace(path)?) {
            if app_node(&tree, false).is_some() {
                played.push(path.clone());
            }
        }
    }
    if !played.is_empty() {
        return Ok(played);
    }
    if let Some(recent) = most_recent_account(steam) {
        let path = userdata.join(recent).join("config").join("localconfig.vdf");
        if path.is_file() {
            return Ok(vec![path]);
        }
    }
    Err("No Steam account config found. Start Steam, log in once, then try again.".into())
}

fn most_recent_account(steam: &Path) -> Option<String> {
    let users = steam.join("config").join("loginusers.vdf");
    if !users.is_file() {
        return None;
    }
    let tree = parse(&read_replace(&users).ok()?).ok()?;
    let list = child(&tree, "users")?;
    for (steam_id, info) in list {
        if let VdfValue::Section(info) = info {
            if get(info, "MostRecent").as_deref() == Some("1") {
                let id: u64 = unescape(steam_id).parse().ok()?;
                return Some(id.saturating_sub(STEAM_ID64_BASE).to_string());
            }
        }
    }
    None
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn nms_install_dir(steam: &Path) -> Result<PathBuf, String> {
    let mut libraries: Vec<PathBuf> = Vec::new();
    let vdf = steam.join("steamapps").join("libraryfolders.vdf");
    if vdf.is_file() {
        let tree = parse(&read_replace(&vdf)?)?;
        let root = child(&tree, "libraryfolders").unwrap_or(&tree);
        for (_key, node) in root {
            let VdfValue::Section(node) = node else { continue };
            let Some(raw) = get(node, "path") else { continue };
            let lib = PathBuf::from(&raw);
            libraries.push(lib.clone());
            let Some(apps) = child(node, "apps") else { continue };
            for (app_id, _value) in apps {
                if unescape(app_id) == GAME_ID {
                    let game = lib.join("steamapps").join("common").join("No Man's Sky");
                    if game.join("Binaries").join("NMS.exe").is_file() {
                        return Ok(game);
                    }
                }
            }
        }
    }
    if !libraries.iter().any(|p| p == steam) {
        libraries.insert(0, steam.to_path_buf());
    }
    for lib in libraries {
        let game = lib.join("steamapps").join("common").join("No Man's Sky");
        if game.join("Binaries").join("NMS.exe").is_file() {
            return Ok(game);
        }
    }
    Err(
        "Steam is installed, but No Man's Sky (app 275850) was not in libraryfolders.vdf. \
         In Steam: right-click No Man's Sky, Manage, Browse local files, and extract there."
            .into(),
    )
}

pub(crate) fn steam_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        steam_dir_registry()
    }
    #[cfg(not(windows))]
    {
        Err("Steam is not installed (no userdata or steamapps folder).".into())
    }
}

#[cfg(windows)]
fn steam_dir_registry() -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, REG_EXPAND_SZ, REG_SZ,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        let err = RegOpenKeyExW(HKEY_CURRENT_USER, wide(r"Software\Valve\Steam").as_ptr(), 0, KEY_QUERY_VALUE, &mut hkey);
        if err != 0 {
            return Err("Steam is not installed for this Windows user (no SteamPath in the registry).".into());
        }
        let mut ty = 0u32;
        let mut buf = vec![0u16; 4096];
        let mut nbytes = (buf.len() * 2) as u32;
        let err = RegQueryValueExW(
            hkey,
            wide("SteamPath").as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            buf.as_mut_ptr().cast(),
            &mut nbytes,
        );
        let _ = RegCloseKey(hkey);
        if err != 0 || (ty != REG_SZ && ty != REG_EXPAND_SZ) {
            return Err("Steam is not installed for this Windows user (no SteamPath in the registry).".into());
        }
        let nchars = (nbytes as usize) / 2;
        let slice = &buf[..nchars.min(buf.len())];
        let trimmed: Vec<u16> = slice.iter().copied().take_while(|&c| c != 0).collect();
        let s = std::ffi::OsString::from_wide(&trimmed);
        Ok(PathBuf::from(s))
    }
}

fn steam_running() -> bool {
    #[cfg(windows)]
    {
        let out = nms_core::spawn::command("tasklist")
            .args(["/FI", "IMAGENAME eq steam.exe", "/NH"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        match out {
            Ok(out) => String::from_utf8_lossy(&out.stdout).to_ascii_lowercase().contains("steam.exe"),
            Err(_) => false,
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn close_steam(steam: &Path) -> Result<(), String> {
    let mut cmd = nms_core::spawn::command(steam.join("steam.exe"));
    cmd.arg("-shutdown").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    strip_python_env(&mut cmd);
    let _ = cmd.status();
    let deadline = Instant::now() + Duration::from_secs(90);
    while steam_running() {
        if Instant::now() > deadline {
            return Err("Steam did not close within 90 seconds. Close it yourself and try again.".into());
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

fn start_steam(steam: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // steam.exe is a GUI program; these flags replace CREATE_NO_WINDOW so Steam outlives us
        // and ignores Ctrl+C in the terminal that ran `steam-option`.
        let mut cmd = nms_core::spawn::command(steam.join("steam.exe"));
        cmd.current_dir(steam).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        strip_python_env(&mut cmd);
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
            .spawn()
            .map_err(|e| format!("could not start Steam: {e}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = steam;
        Err("starting Steam is not implemented on this OS".into())
    }
}

fn strip_python_env(cmd: &mut Command) {
    for (key, _) in std::env::vars() {
        if key.to_ascii_uppercase().starts_with("PYTHON") {
            cmd.env_remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEMP_N: AtomicU32 = AtomicU32::new(0);

    fn temp_dir() -> PathBuf {
        let n = TEMP_N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("nms-rs-t023-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_account(steam: &Path, account: &str, tree: &Vdf) -> PathBuf {
        let path = steam.join("userdata").join(account).join("config").join("localconfig.vdf");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, dump(tree).as_bytes()).unwrap();
        path
    }

    fn empty_store() -> Vdf {
        parse(
            "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t}\n\t\t}\n\t}\n}\n",
        )
        .unwrap()
    }

    fn store_with_launch(option: &str) -> Vdf {
        let mut tree = empty_store();
        let node = app_node_mut(&mut tree, true).unwrap();
        set(node, "LaunchOptions", option);
        tree
    }

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Games\No Man's Sky\nms-rs.exe")
    }

    #[test]
    fn parse_dump_round_trip() {
        let text = "\"root\"\n{\n\t\"k\"\t\t\"say \\\"hi\\\" and \\\\path\"\n\t\"child\"\n\t{\n\t\t\"n\"\t\t\"1\"\n\t}\n}\n";
        let tree = parse(text).unwrap();
        assert_eq!(dump(&tree), text);
        let again = dump(&parse(&dump(&tree)).unwrap());
        assert_eq!(again, text);
        assert_eq!(unescape(r#"say \"hi\" and \\path"#), "say \"hi\" and \\path");
        assert_eq!(escape("say \"hi\" and \\path"), r#"say \"hi\" and \\path"#);
    }

    #[test]
    fn matcher_apostrophe_and_old_forms() {
        let ours = launch_option(&exe());
        assert!(find_ours(&ours, 0).is_some(), "{ours}");
        let old_cmd = r#""D:\Games\Steam\steamapps\common\No Man's Sky\NMS.py\launch.cmd" %command%"#;
        assert!(find_ours(old_cmd, 0).is_some());
        let old_exe = r#""C:\mods\BiggerItemGrids.exe" %command%"#;
        assert!(find_ours(old_exe, 0).is_some());
        let old_bat = r#""C:\mods\BiggerItemGrids\Play.bat" --start %command%"#;
        assert!(find_ours(old_bat, 0).is_some());
        let old_rs = r#""D:\old\nms-rs.exe" play %command%"#;
        assert!(find_ours(old_rs, 0).is_some());
        let foreign = r#""C:\other\wrapper.exe" %command%"#;
        assert!(find_ours(foreign, 0).is_none());
        assert_eq!(strip_ours(old_cmd).trim(), "");
    }

    #[test]
    fn old_nms_py_option_is_found() {
        assert!(has_old_nms_py_option(&[r#""F:\Games\No Man's Sky\NMS.py\launch.cmd" %command%"#.into()]));
        assert!(has_old_nms_py_option(&[r#"WINEDLLOVERRIDES="x" bash "/home/d/NMS.py/launch.sh" %command%"#.into()]));
        assert!(!has_old_nms_py_option(&[launch_option(&exe())]));
        assert!(!has_old_nms_py_option(&[String::new(), "-skipintro".into()]));
    }

    #[test]
    fn install_into_empty_app_node() {
        let mut tree = empty_store();
        assert_eq!(change_install(&mut tree, &exe()).as_deref(), Some("launch option added."));
        let node = app_node(&tree, false).unwrap();
        assert_eq!(get(node, "LaunchOptions").unwrap(), launch_option(&exe()));
        assert!(change_install(&mut tree, &exe()).is_none());
    }

    #[test]
    fn install_replaces_old_forms() {
        let option = launch_option(&exe());
        for old in [
            r#""C:\NMS.py\launch.cmd" %command%"#,
            r#""C:\mods\BiggerItemGrids.exe" %command%"#,
            r#""C:\mods\Play.bat" %command%"#,
            r#""D:\old\nms-rs.exe" play %command%"#,
            r#"bash "/home/x/NMS.py/launch.sh" %command%"#,
        ] {
            let mut tree = store_with_launch(old);
            assert_eq!(change_install(&mut tree, &exe()).as_deref(), Some("launch option added."), "{old}");
            let node = app_node(&tree, false).unwrap();
            assert_eq!(get(node, "LaunchOptions").unwrap(), option, "{old}");
        }
    }

    #[test]
    fn install_leaves_foreign_command() {
        let mut tree = store_with_launch(r#""C:\other\wrapper.exe" %command%"#);
        let msg = change_install(&mut tree, &exe()).unwrap();
        assert!(msg.starts_with("!left alone:"), "{msg}");
        let node = app_node(&tree, false).unwrap();
        assert_eq!(get(node, "LaunchOptions").unwrap(), r#""C:\other\wrapper.exe" %command%"#);
    }

    #[test]
    fn remove_ours_only() {
        let mut tree = store_with_launch(&launch_option(&exe()));
        assert_eq!(change_remove(&mut tree).as_deref(), Some("launch option removed."));
        let node = app_node(&tree, false).unwrap();
        assert_eq!(get(node, "LaunchOptions").unwrap(), "");
        assert!(change_remove(&mut tree).is_none());
    }

    #[test]
    fn edit_on_temp_account() {
        let steam = temp_dir();
        let mut tree = empty_store();
        let _ = app_node_mut(&mut tree, true);
        write_account(&steam, "111", &tree);
        let lines = edit(&steam, &exe(), true, default_verify).unwrap();
        assert_eq!(lines, vec!["Steam account 111: launch option added.".to_string()]);
        let lines = edit(&steam, &exe(), false, default_verify).unwrap();
        assert_eq!(lines, vec!["Steam account 111: launch option removed.".to_string()]);
        let _ = fs::remove_dir_all(&steam);
    }

    fn fail_verify(_written: &str, _path: &Path) -> Result<bool, String> {
        Ok(false)
    }

    #[test]
    fn bak_restore_when_read_back_differs() {
        let steam = temp_dir();
        let mut tree = empty_store();
        let _ = app_node_mut(&mut tree, true);
        let original = dump(&tree);
        let path = write_account(&steam, "222", &tree);
        let err = edit(&steam, &exe(), true, fail_verify).unwrap_err();
        assert!(err.contains("did not read back correctly"), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap().replace("\r\n", "\n"), original);
        assert!(path.with_extension("vdf.bak").is_file());
        let _ = fs::remove_dir_all(&steam);
    }

    #[test]
    fn nms_install_dir_from_library_vdf() {
        let steam = temp_dir();
        let lib = steam.join("library");
        let game = lib.join("steamapps").join("common").join("No Man's Sky");
        fs::create_dir_all(game.join("Binaries")).unwrap();
        fs::write(game.join("Binaries").join("NMS.exe"), b"fake").unwrap();
        let lib_esc = escape(&lib.to_string_lossy());
        let vdf = format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{lib_esc}\"\n\t\t\"apps\"\n\t\t{{\n\t\t\t\"275850\"\t\t\"1\"\n\t\t}}\n\t}}\n}}\n"
        );
        fs::create_dir_all(steam.join("steamapps")).unwrap();
        fs::write(steam.join("steamapps").join("libraryfolders.vdf"), vdf.as_bytes()).unwrap();
        assert_eq!(nms_install_dir(&steam).unwrap(), game);
        let _ = fs::remove_dir_all(&steam);
    }
}
