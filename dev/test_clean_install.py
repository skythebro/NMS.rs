r"""Install the release zip the way a new player does, into an empty stand-in game folder.

    py -3 -I dev\test_clean_install.py [NMS.rs-<version>.zip]

Without a zip it builds one from the current build (like dev\test_release_rust.py). The stand-in
folder has Binaries\NMS.exe (an empty file: nothing here starts the game), GAMEDATA\MODS and nothing
else. Checks, with the manager's command line:
  - every mod in stock.json is listed, none installed yet
  - "mods install-all" installs every one of them, and nothing is offered as an update after that
  - the loader DLL lands next to NMS.exe
  - the zip extracted into Binaries by mistake makes the manager say it is in the wrong folder
"""
import json
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import release  # noqa: E402

# The zip is made from <target>\dist (release.py rust-build) or, when that is missing or older than
# the dev build (cargo build --release), from <target>\release.
_dist, _dev = release.RUST_TARGET / "nms-rs.exe", release._target_dir() / "release" / "nms-rs.exe"
if not _dist.is_file() or (_dev.is_file() and _dev.stat().st_mtime > _dist.stat().st_mtime):
    release.RUST_TARGET = _dev.parent
    release.PROXY_VARIANT_DIR = release.RUST_TARGET / "variants"


def game_folder(base: Path) -> Path:
    (base / "Binaries").mkdir(parents=True)
    (base / "Binaries" / "NMS.exe").write_bytes(b"")
    (base / "GAMEDATA" / "MODS").mkdir(parents=True)
    return base


def manager(folder: Path, *args: str) -> subprocess.CompletedProcess:
    exe = folder / "nms-rs.exe"
    run = subprocess.run([str(exe), *args], cwd=folder, capture_output=True, text=True, timeout=300)
    return run


def states(text: str) -> dict[str, str]:
    found = {}
    for line in text.splitlines():
        # "<Mod> <state>" lines; other lines ("python leftover: no", notes) have a colon.
        name, _, state = line.partition(" ")
        if name and state and ":" not in line:
            found[name] = state.strip()
    return found


def check(zip_path: Path, tmp: Path) -> None:
    with zipfile.ZipFile(zip_path) as z:
        wanted = sorted(json.loads(z.read("NMS.rs/stock/stock.json")))
        root = game_folder(tmp / "right")
        z.extractall(root)
        wrong = game_folder(tmp / "wrong")
        z.extractall(wrong / "Binaries")
    loader = root / "NMS.rs"

    fresh = manager(loader, "mods", "status")
    assert fresh.returncode == 0, fresh.stderr
    listed = states(fresh.stdout)
    assert sorted(listed) == wanted, f"listed {sorted(listed)}, stock.json has {wanted}"
    assert all(s == "missing" for s in listed.values()), listed

    done = manager(loader, "mods", "install-all")
    assert done.returncode == 0, done.stderr
    after = states(manager(loader, "mods", "status").stdout)
    assert after == {name: "live" for name in wanted}, after
    for name in wanted:
        assert (root / "GAMEDATA" / "MODS" / name).is_dir(), f"{name} not in GAMEDATA\\MODS"
    update = manager(loader, "mods", "update-all")
    assert update.returncode == 0 and "nothing to update" in update.stdout, update.stdout + update.stderr
    assert (root / "Binaries" / "xinput9_1_0.dll").is_file(), "loader DLL not next to NMS.exe"
    assert "wrong folder" not in fresh.stderr, fresh.stderr

    misplaced = manager(wrong / "Binaries" / "NMS.rs", "mods", "status")
    assert "wrong folder" in misplaced.stderr, f"no wrong-folder warning: {misplaced.stderr!r}"
    print(f"ok: clean install of {zip_path.name}: {len(wanted)} mods listed and installed, wrong folder reported")


def main() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        if len(sys.argv) > 1:
            zip_path = Path(sys.argv[1])
        else:
            zip_path = release.make_rust_zip("4.0.0", tmp / "zip")
        check(zip_path, tmp)
    return 0


if __name__ == "__main__":
    sys.exit(main())
