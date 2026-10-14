//! Where things live, worked out from the running NMS.exe (`<game>\Binaries\NMS.exe`).

use std::path::{Path, PathBuf};

/// The game folder for an exe path: the parent of `Binaries`.
pub fn game_root_for(exe: &Path) -> Option<PathBuf> {
    let bin = exe.parent()?;
    if bin.file_name()?.to_string_lossy().eq_ignore_ascii_case("Binaries") {
        bin.parent().map(Path::to_path_buf)
    } else {
        None
    }
}

pub fn game_root() -> Option<PathBuf> {
    game_root_for(&std::env::current_exe().ok()?)
}

/// The first folder at or above `start` that holds `Binaries\NMS.exe`.
pub fn find_game_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find(|d| d.join("Binaries").join("NMS.exe").is_file()).map(Path::to_path_buf)
}

/// `GAMEDATA\MODS\<mod_name>` under the game folder.
pub fn mod_dir(root: &Path, mod_name: &str) -> PathBuf {
    root.join("GAMEDATA").join("MODS").join(mod_name)
}

/// The Rust loader's own folder, next to `NMS.py`.
pub fn loader_dir(root: &Path) -> PathBuf {
    root.join("NMS.rs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_is_parent_of_binaries() {
        let exe = Path::new(r"C:\Games\No Man's Sky\Binaries\NMS.exe");
        assert_eq!(game_root_for(exe), Some(PathBuf::from(r"C:\Games\No Man's Sky")));
        assert_eq!(game_root_for(Path::new(r"C:\Other\NMS.exe")), None);
    }

    #[test]
    fn mod_dir_layout() {
        let root = Path::new(r"C:\G");
        assert_eq!(mod_dir(root, "CraftFromStorage"), PathBuf::from(r"C:\G\GAMEDATA\MODS\CraftFromStorage"));
    }
}
