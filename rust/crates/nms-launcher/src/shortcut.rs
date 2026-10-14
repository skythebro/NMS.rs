//! Desktop shortcut for GOG / no-Steam installs.
//!
//! Ports `loader/lib/desktop_shortcut.py`. Paths go to PowerShell as environment
//! variables so an apostrophe in the game folder is not spliced into the script.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use nms_core::spawn::command;

pub(crate) const SHORTCUT_NAME: &str = "No Man's Sky (NMS.rs).lnk";
pub(crate) const DESCRIPTION: &str = "Starts No Man's Sky with NMS.rs";

const PS_SCRIPT: &str = "\
$ws = New-Object -ComObject WScript.Shell; \
$sc = $ws.CreateShortcut($env:NMS_RS_LNK); \
$sc.TargetPath = $env:NMS_RS_TARGET; \
$sc.Arguments = $env:NMS_RS_ARGS; \
$sc.WorkingDirectory = $env:NMS_RS_WD; \
$sc.Description = $env:NMS_RS_DESC; \
$sc.WindowStyle = 1; \
if ($env:NMS_RS_ICON) { $sc.IconLocation = $env:NMS_RS_ICON }; \
$sc.Save()";

#[derive(Clone, Debug)]
pub(crate) struct Spec {
    pub lnk: PathBuf,
    pub target: PathBuf,
    pub arguments: String,
    pub working_dir: PathBuf,
    pub icon: Option<PathBuf>,
    pub description: String,
}

/// PowerShell argv and env pairs. Tests check this list; they do not run PowerShell.
pub(crate) fn powershell_invocation(spec: &Spec) -> (Vec<String>, Vec<(String, String)>) {
    let args = vec!["powershell".into(), "-NoProfile".into(), "-Command".into(), PS_SCRIPT.into()];
    let mut env = vec![
        ("NMS_RS_LNK".into(), spec.lnk.to_string_lossy().into_owned()),
        ("NMS_RS_TARGET".into(), spec.target.to_string_lossy().into_owned()),
        ("NMS_RS_ARGS".into(), spec.arguments.clone()),
        ("NMS_RS_WD".into(), spec.working_dir.to_string_lossy().into_owned()),
        ("NMS_RS_DESC".into(), spec.description.clone()),
    ];
    if let Some(icon) = &spec.icon {
        env.push(("NMS_RS_ICON".into(), format!("{},0", icon.display())));
    }
    (args, env)
}

pub(crate) fn spec_for_game(game: &Path, desktop: &Path) -> Spec {
    let nms_rs = game.join("NMS.rs");
    let icon = game.join("Binaries").join("NMS.exe");
    Spec {
        lnk: desktop.join(SHORTCUT_NAME),
        target: nms_rs.join("nms-rs.exe"),
        arguments: "play".into(),
        working_dir: nms_rs,
        icon: if icon.is_file() { Some(icon) } else { None },
        description: DESCRIPTION.into(),
    }
}

pub(crate) fn desktop_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let out = command("powershell")
            .args(["-NoProfile", "-Command", "[Environment]::GetFolderPath('Desktop')"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        if let Ok(out) = out {
            if out.status.success() {
                let line = String::from_utf8_lossy(&out.stdout);
                if let Some(last) = line.lines().rev().map(str::trim).find(|s| !s.is_empty()) {
                    return PathBuf::from(last);
                }
            }
        }
    }
    dirs_desktop_fallback()
}

fn dirs_desktop_fallback() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Desktop")
}

/// Write the shortcut. Returns an error string, or None on success.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn create(spec: &Spec) -> Option<String> {
    if let Some(parent) = spec.lnk.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return Some(e.to_string());
        }
    }
    let (args, env) = powershell_invocation(spec);
    let mut cmd = command(&args[0]);
    cmd.args(&args[1..]).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in &env {
        cmd.env(k, v);
    }
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) => return Some(e.to_string()),
    };
    if !out.status.success() || !spec.lnk.is_file() {
        let err = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr });
        let err = err.trim();
        return Some(if err.is_empty() { "shortcut was not created".into() } else { err.into() });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_keeps_apostrophe_path_in_env_not_script() {
        let game = PathBuf::from(r"D:\Games\Steam\steamapps\common\No Man's Sky");
        let desktop = PathBuf::from(r"C:\Users\Player\Desktop");
        let mut spec = spec_for_game(&game, &desktop);
        spec.icon = Some(game.join("Binaries").join("NMS.exe"));
        let (args, env) = powershell_invocation(&spec);
        assert_eq!(args[0], "powershell");
        assert_eq!(args[1], "-NoProfile");
        assert_eq!(args[2], "-Command");
        let script = &args[3];
        assert!(!script.contains("No Man's Sky"), "{script}");
        assert!(!script.contains('\''), "{script}");
        assert!(script.contains("$env:NMS_RS_TARGET"));
        assert!(script.contains("$env:NMS_RS_ARGS"));
        let env: std::collections::HashMap<_, _> = env.into_iter().collect();
        assert_eq!(env["NMS_RS_TARGET"], r"D:\Games\Steam\steamapps\common\No Man's Sky\NMS.rs\nms-rs.exe");
        assert_eq!(env["NMS_RS_ARGS"], "play");
        assert_eq!(env["NMS_RS_WD"], r"D:\Games\Steam\steamapps\common\No Man's Sky\NMS.rs");
        assert!(env["NMS_RS_LNK"].ends_with(SHORTCUT_NAME));
        assert!(env["NMS_RS_ICON"].contains("NMS.exe"));
        assert!(env["NMS_RS_ICON"].contains('\''));
    }
}
