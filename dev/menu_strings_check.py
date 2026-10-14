r"""List Mods-page strings that en.json lacks, and keys other languages lack.

  py -3 dev\menu_strings_check.py

Add new keys to rust/crates/nms-mods/src/common/menu_strings/en.json first (label.<Mod>.<key>,
desc.<Mod>.<key>, choice.<Mod>.<key>.<choice>; label.*/desc.* hold keys every mod has), then
add the same keys to every other language file (keep placeholders such as {n} and {name}), and run
cargo test -p nms-mods menu_text.
"""
import json, re
from pathlib import Path

root = Path(__file__).resolve().parent.parent / "rust" / "crates"
strings = root / "nms-mods" / "src" / "common" / "menu_strings"
en = json.loads((strings / "en.json").read_text(encoding="utf-8"))

# SettingSpec keys per source file; the mod folder name comes from `fn name`.
# The proxy's dev-only self-test (feature "selftest") is left out on purpose: it never ships, so it
# has no strings here and its rows show the English spec labels.
for src in (root / "nms-mods" / "src").rglob("*.rs"):
    text = src.read_text(encoding="utf-8")
    keys = re.findall(r'SettingSpec\s*\{\s*key:\s*"([^"]+)"', text)
    m = re.search(r'fn name\(&self\) -> &\'static str \{\s*"([^"]+)"', text)
    if not keys or not m:
        continue
    for k in keys:
        if f"label.{m[1]}.{k}" not in en and f"label.*.{k}" not in en:
            print("missing label:", m[1], k, "in", src.name)

for f in sorted(strings.glob("*.json")):
    d = json.loads(f.read_text(encoding="utf-8"))
    for k in en.keys() - d.keys():
        print(f.name, "missing", k)
