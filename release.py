"""Build the NMS.rs release zip.

    py -3 release.py rust-build --version 4.0.0       build the shipped exe and DLL (cargo --profile dist:
                                                      no debug info, no symbols, no build-machine paths)
    py -3 release.py rust --version 4.0.0 --out DIR   write NMS.rs-<version>.zip and its .sha256 into DIR

Run it with any python.org Python 3.9 or newer, from anywhere: every path is relative to this file.
The binaries are read from <target>/dist, where <target> is CARGO_TARGET_DIR when it is set and
rust/target otherwise. Nothing here touches a game install.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
RUST = ROOT / "rust"
BUILD = ROOT / "build"
MODS_SRC = ROOT / "mods"
# Default-size game files of the four UI mods (modified MBINs). The manager's "Apply sizes" rebuilds
# BiggerItemGrids and SmallerInventorySlots to the player's pick on their own machine.
PRESETS = ROOT / "presets"
DEFAULT_PRESETS = {
    "BiggerItemGrids": PRESETS / "BiggerItemGrids" / "8x3-window1.4",
    "SmallerInventorySlots": PRESETS / "SmallerInventorySlots" / "70pct",
    "CompactTooltips": PRESETS / "CompactTooltips" / "default",
    "CompactQuickTransfer": PRESETS / "CompactQuickTransfer" / "default",
}


def _target_dir() -> Path:
    env = os.environ.get("CARGO_TARGET_DIR")
    return Path(env).resolve() if env else RUST / "target"


RUST_TARGET = _target_dir() / "dist"

# Every folder the manager installs from (NMS.rs/stock/<Mod>). Mods with only Rust code still get a
# stock.json entry so the manager knows their version. NMSrsMenu is lua only (the pause-menu row of
# the in-game Mods page).
RUST_STOCK_MODS = (
    "BiggerItemGrids",
    "SmallerInventorySlots",
    "CompactTooltips",
    "CompactQuickTransfer",
    "CraftFromStorage",
    "QuickTransferAll",
    "SphereSizedMining",
    "TradeUITweaks",
    "QuickRecharge",
    "HideEmptyHover",
    "NMSrsMenu",
    "GalaxyMapFilters",
    "PickerTweaks",
    "RefinerGuard",
    "ToolModeBinds",
)
RUST_README = (
    "NMS.rs {version}\n\n"
    "Extract this zip into your No Man's Sky game folder (the one that holds Binaries\n"
    "and GAMEDATA), so that Binaries\\xinput9_1_0.dll and NMS.rs\\nms-rs.exe land there.\n\n"
    "1. Run NMS.rs\\nms-rs.exe once to open the manager. Install the mods you want,\n"
    "   park the ones you do not, and pick your grid and slot sizes.\n"
    "2. Press Play in Steam. No launch option is needed.\n\n"
    "Coming from version 3 (NMS.py)? The manager finds the old setup and asks first.\n"
    "Migrate now backs up your settings to NMS.rs\\backup, then removes the old Python\n"
    "files, the old Steam launch option and the NMS.py folder. Your mod settings stay.\n\n"
    "Mod folders in NMS.rs\\stock are the copies the manager installs from.\n"
    "To remove everything, delete Binaries\\xinput9_1_0.dll and the NMS.rs folder.\n"
    "\nLinux / Proton (beta): the manager shows the one Steam launch option you need,\n"
    "WINEDLLOVERRIDES=\"xinput9_1_0=n,b\" %command%\n"
    "Without it the game starts but the mods do not load. If anything fails, use Pack logs in\n"
    "the manager's Settings tab and send the zip.\n"
)


def _user_file(path: Path) -> bool:
    """Settings and logs a mod writes for itself; size presets (*_preset.json) are shipped."""
    name = path.name.lower()
    return (
        name.endswith(".log")
        or (name.endswith(".json") and not name.endswith("_preset.json"))
        or name in ("disabled.txt", "merge_order.txt", "conflicts.txt")
    )


def _rust_keep(path: Path) -> bool:
    """Shipped stock file: no caches, logs, user json, Python files or secrets."""
    if "__pycache__" in path.parts or path.suffix.lower() in (".py", ".pyc", ".log"):
        return False
    if path.name == ".env":
        return False
    return not _user_file(path)


def _sources(folder: Path) -> dict[str, Path]:
    if not folder.is_dir():
        return {}
    return {p.relative_to(folder).as_posix(): p for p in sorted(folder.rglob("*")) if p.is_file()}


def rust_files(version: str) -> dict[str, Path]:
    """{path inside the zip: source}. Paths are relative to the game folder."""
    dll, exe = RUST_TARGET / "xinput9_1_0.dll", RUST_TARGET / "nms-rs.exe"
    for path in (dll, exe):
        if not path.is_file():
            raise SystemExit(f"Missing {path}. Build it first: py -3 release.py rust-build --version {version}")
    files: dict[str, Path] = {"Binaries/xinput9_1_0.dll": dll, "NMS.rs/nms-rs.exe": exe}
    versions: dict[str, str] = {}
    for name in RUST_STOCK_MODS:
        sources = _sources(MODS_SRC / name)
        preset = DEFAULT_PRESETS.get(name)
        if preset is not None:
            if not (preset / "UI").is_dir():
                raise SystemExit(f"Missing the default preset {preset}")
            sources.update(_sources(preset))
        for relative, source in sources.items():
            # Per-mod README.txt is for this repo; players read the manager and README.txt.
            if relative == "README.txt" or not _rust_keep(source):
                continue
            files[f"NMS.rs/stock/{name}/{relative}"] = source
        versions[name] = version
    BUILD.mkdir(parents=True, exist_ok=True)
    info = BUILD / "stock_rust.json"
    info.write_text(json.dumps(versions, indent=2) + "\n", encoding="utf-8")
    files["NMS.rs/stock/stock.json"] = info
    # No default settings files: every Rust mod writes its own defaults on first load.
    readme = BUILD / "README-rust.txt"
    readme.write_text(RUST_README.format(version=version), encoding="utf-8")
    files["README.txt"] = readme
    sums = BUILD / "SHA256SUMS-rust.txt"
    sums.write_text(
        "".join(f"{_sha256(source)} *{name}\n" for name, source in sorted(files.items())),
        encoding="utf-8",
    )
    files["NMS.rs/SHA256SUMS.txt"] = sums
    return files


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _zip(path: Path, files: dict[str, Path]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, source in sorted(files.items()):
            archive.write(source, name)
    print(f"Zipped {path.name} ({path.stat().st_size / 1e6:.1f} MB)")


def make_rust_zip(version: str, out: Path) -> Path:
    path = out / f"NMS.rs-{version}.zip"
    _zip(path, rust_files(version))
    # The hash to post with the release; anyone can compare it with their download.
    (out / f"{path.name}.sha256").write_text(f"{_sha256(path)} *{path.name}\n", encoding="utf-8")
    return path


def rust_build(version: str) -> None:
    """Build the files the zip ships, without debug info or this machine's folder names in them."""
    sysroot = subprocess.run(["rustc", "--print", "sysroot"], capture_output=True, text=True, check=True).stdout.strip()
    cargo_home = Path(os.environ.get("CARGO_HOME") or Path.home() / ".cargo")
    flags = [
        f"--remap-path-prefix={RUST}=src",
        f"--remap-path-prefix={_target_dir()}=target",
        f"--remap-path-prefix={cargo_home}=cargo",
        f"--remap-path-prefix={sysroot}=rustc",
        "-C", "link-arg=/PDBALTPATH:%_PDB%",
    ]
    # CARGO_ENCODED_RUSTFLAGS, not RUSTFLAGS: folder names may have spaces in them.
    env = dict(os.environ, CARGO_ENCODED_RUSTFLAGS="\x1f".join(flags), NMS_RS_VERSION=version)
    env.pop("RUSTFLAGS", None)
    subprocess.run(
        ["cargo", "build", "--profile", "dist", "-p", "nms-launcher", "-p", "nms-proxy"],
        cwd=RUST, env=env, check=True,
    )
    leaks = rust_path_leaks()
    if leaks:
        raise SystemExit("Build paths still in the shipped files: " + ", ".join(leaks))
    for name in ("nms-rs.exe", "xinput9_1_0.dll"):
        print(f"{_sha256(RUST_TARGET / name)} *{name}")


def rust_path_leaks() -> list[str]:
    """Names of this machine (user folder, repo folder) found inside the shipped exe and DLL."""
    needles = {str(ROOT).lower(), str(Path.home()).lower(), Path.home().name.lower()}
    found = []
    for name in ("nms-rs.exe", "xinput9_1_0.dll"):
        data = (RUST_TARGET / name).read_bytes().lower()
        for needle in sorted(needles):
            if needle and (needle.encode() in data or needle.encode("utf-16-le") in data):
                found.append(f"{name}: {needle}")
    return found


def main() -> int:
    args = sys.argv[1:]
    if args and args[0] == "rust-build":
        opts = args[1:]
        if len(opts) != 2 or opts[0] != "--version":
            raise SystemExit(__doc__)
        rust_build(opts[1])
        return 0
    if args and args[0] == "rust":
        opts = args[1:]
        if len(opts) != 4 or opts[0] != "--version" or opts[2] != "--out":
            raise SystemExit(__doc__)
        make_rust_zip(opts[1], Path(opts[3]).resolve())
        return 0
    raise SystemExit(__doc__)


if __name__ == "__main__":
    raise SystemExit(main())
