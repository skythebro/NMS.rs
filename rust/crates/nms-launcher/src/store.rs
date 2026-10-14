//! Which store owns this game folder: Game Pass, GOG, Steam, or unknown.
//!
//! Marker files and folder-name checks match `loader/lib/game_store.py`.

use std::path::Path;

const GOG_DLLS: &[&str] = &["galaxy64.dll", "galaxy.dll"];
const GOG_SUFFIXES: &[&str] = &["info", "id", "hashdb"];
const STEAM_FILES: &[&str] = &["steam_api64.dll", "steam_api.dll", "steam_appid.txt"];
const GAMEPASS_FILES: &[&str] = &["appxmanifest.xml", "MicrosoftGame.config", "gamelaunchhelper.exe"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Store {
    Steam,
    Gog,
    GamePass,
    Unknown,
}

fn parts_lower(game: &Path) -> Vec<String> {
    game.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

fn has_gog_marker(game: &Path) -> bool {
    for folder in [game.to_path_buf(), game.join("Binaries")] {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let lower = name.to_string_lossy().to_ascii_lowercase();
            if GOG_DLLS.iter().any(|dll| *dll == lower) {
                return true;
            }
            if lower.starts_with("goggame-") {
                let ext = Path::new(name.as_os_str()).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                if GOG_SUFFIXES.iter().any(|s| *s == ext) {
                    return true;
                }
            }
        }
    }
    false
}

fn has_steam_marker(game: &Path) -> bool {
    if parts_lower(game).iter().any(|p| p == "steamapps") {
        return true;
    }
    let binaries = game.join("Binaries");
    for name in STEAM_FILES {
        if binaries.join(name).is_file() || game.join(name).is_file() {
            return true;
        }
    }
    false
}

fn has_gamepass_marker(game: &Path) -> bool {
    let parts = parts_lower(game);
    if parts.iter().any(|p| p == "xboxgames" || p == "windowsapps") {
        return true;
    }
    let binaries = game.join("Binaries");
    for name in GAMEPASS_FILES {
        if game.join(name).is_file() || binaries.join(name).is_file() {
            return true;
        }
    }
    false
}

pub(crate) fn kind(game: &Path) -> Store {
    let game = match game.canonicalize() {
        Ok(resolved) => resolved,
        Err(_) => game.to_path_buf(),
    };
    if has_gamepass_marker(&game) {
        return Store::GamePass;
    }
    if has_gog_marker(&game) {
        return Store::Gog;
    }
    if has_steam_marker(&game) {
        return Store::Steam;
    }
    Store::Unknown
}

pub(crate) fn uses_direct_exe(game: &Path) -> bool {
    match kind(game) {
        Store::Steam | Store::GamePass => false,
        Store::Gog | Store::Unknown => game.join("Binaries").join("NMS.exe").is_file(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_game(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nms-launcher-store-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Binaries")).expect("temp Binaries");
        dir
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(path, b"").expect("touch");
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn game_pass_from_folder_name_is_case_insensitive() {
        let root = std::env::temp_dir().join(format!("nms-launcher-XboxGames-{}", std::process::id()));
        let game = root.join("XboxGames").join("NMS");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(game.join("Binaries")).unwrap();
        assert_eq!(kind(&game), Store::GamePass);
        assert!(!uses_direct_exe(&game));
        cleanup(&root);
    }

    #[test]
    fn game_pass_from_marker_file() {
        let game = temp_game("gp-file");
        touch(&game.join("appxmanifest.xml"));
        touch(&game.join("Binaries").join("NMS.exe"));
        assert_eq!(kind(&game), Store::GamePass);
        assert!(!uses_direct_exe(&game));
        cleanup(&game);
    }

    #[test]
    fn gog_from_dll_and_goggame_suffix() {
        let dll = temp_game("gog-dll");
        touch(&dll.join("Binaries").join("Galaxy64.dll"));
        touch(&dll.join("Binaries").join("NMS.exe"));
        assert_eq!(kind(&dll), Store::Gog);
        assert!(uses_direct_exe(&dll));
        cleanup(&dll);

        let info = temp_game("gog-info");
        touch(&info.join("goggame-275850.info"));
        touch(&info.join("Binaries").join("NMS.exe"));
        assert_eq!(kind(&info), Store::Gog);
        cleanup(&info);
    }

    #[test]
    fn steam_from_steamapps_in_path_is_case_insensitive() {
        let root = std::env::temp_dir().join(format!("nms-launcher-SteamApps-{}", std::process::id()));
        let game = root.join("SteamApps").join("common").join("game");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(game.join("Binaries")).unwrap();
        touch(&game.join("Binaries").join("NMS.exe"));
        assert_eq!(kind(&game), Store::Steam);
        assert!(!uses_direct_exe(&game));
        cleanup(&root);
    }

    #[test]
    fn steam_from_api_dll() {
        let game = temp_game("steam-dll");
        touch(&game.join("Binaries").join("steam_api64.dll"));
        touch(&game.join("Binaries").join("NMS.exe"));
        assert_eq!(kind(&game), Store::Steam);
        assert!(!uses_direct_exe(&game));
        cleanup(&game);
    }

    #[test]
    fn unknown_uses_direct_exe_only_when_nms_exists() {
        let game = temp_game("unknown");
        assert_eq!(kind(&game), Store::Unknown);
        assert!(!uses_direct_exe(&game));
        touch(&game.join("Binaries").join("NMS.exe"));
        assert!(uses_direct_exe(&game));
        cleanup(&game);
    }

    #[test]
    fn game_pass_wins_over_gog_and_steam() {
        let game = temp_game("order");
        touch(&game.join("MicrosoftGame.config"));
        touch(&game.join("galaxy.dll"));
        touch(&game.join("steam_api.dll"));
        assert_eq!(kind(&game), Store::GamePass);
        cleanup(&game);
    }

    #[test]
    fn gog_wins_over_steam() {
        let game = temp_game("gog-first");
        touch(&game.join("galaxy.dll"));
        touch(&game.join("steam_api.dll"));
        assert_eq!(kind(&game), Store::Gog);
        cleanup(&game);
    }
}
