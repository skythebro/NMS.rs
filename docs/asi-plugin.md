# NMS.rs as an ASI plugin (design, not released)

Status: research and prototype on branch `asi-plugin`. Nothing here is released, uploaded or
merged. Facts are marked **Confirmed** (read in our code or in the loader's source, or tested
here), **Inferred** (follows from confirmed facts, not tested) or **Unknown**.

## Why

Players who use Ultimate ASI Loader (ThirteenAG, "UAL") as `Binaries\xinput9_1_0.dll` for other
mods (Imperial Galactic Map, for example) cannot also have our `xinput9_1_0.dll`. Today they must
use our `winmm.dll` or `version.dll` "DLL only" file. One Nexus user (DaninFuchs) instead renamed
our loader to `nms_rs.asi` and reports that NMS.rs and Imperial Galactic Map both work. We have not
tested that ourselves.

## How Ultimate ASI Loader loads a plugin

Source: `ThirteenAG/Ultimate-ASI-Loader` master, `source/loader/main.cpp`, `startup/startup.cpp`,
`core/loader.cpp`, `plugins/plugins.cpp`, `proxy/proxy.cpp`, read 2026-11-26. All **Confirmed**
from source unless marked.

- x64 builds exist for xinput9_1_0.dll and 18 other names (d3d9/10/11/12, dxgi, dinput8, dsound,
  version, wininet, winmm, winhttp, binkw64, bink2w64, xinput1_1..1_4, xinputuap). The download
  is a rolling `x64-latest` tag, so a test must record the file hash.
- When: with the default `DontLoadFromDllMain=1`, UAL's DllMain only patches kernel32 imports
  (GetStartupInfo, GetModuleHandle, GetSystemTimeAsFileTime, CreateFile and others) of the exe
  and of DLLs in the exe folder. The first such call made outside the loader lock (in practice
  the CRT start-up of NMS.exe, before WinMain; **Inferred**) loads the plugins. With
  `DontLoadFromDllMain=0` it loads them from its own DllMain, under the loader lock.
- How: `LoadLibraryW(full path)`, no flags, current directory set to the plugin's folder. Then
  `InitializeASI()` (no arguments) if the plugin exports it. A module already loaded is not
  loaded twice.
- Failure: logged, the next plugin loads anyway. A DllMain that returns FALSE or a wrong
  architecture gives no dialog; other errors show a dialog.
- Order: extra plugins (`LoadExtraPlugins`), then `*.asi` in the loader's own folder (skipped
  when `LoadFromScriptsOnly=1`), then `scripts\`, then `plugins\`, then the VFS folders (`update`
  by default; **Inferred**). With `LoadRecursively=1` (default) the direct subfolders of
  `scripts`, `plugins` and the extra folders load before the folder itself, one level only.
  Inside a folder the order is directory enumeration order, not sorted (NTFS usually returns name
  order; **Unknown** in practice).
- Settings: `<loader stem>.ini`, `global.ini`, `scripts\global.ini`, `plugins\global.ini`,
  `update\global.ini`; later files win. Section `[GlobalSets]`.
- Real XInput: UAL loads `xinput9_1_0Hooked.dll` next to itself if present, otherwise
  `System32\xinput9_1_0.dll` (then xinput1_4, then xinput9_1_0 again), and forwards every export
  by name through a slot. Controllers keep working.
- Recognition: UAL exports `IsUltimateASILoader` (and `GetOverloadPathW` and other VFS
  functions). That export name is how a tool can tell UAL from any other DLL of that name.
- Wine/Proton: the README says nothing. **Unknown** whether its kernel32 import hooks fire under
  Wine; DaninFuchs is on Windows (**Inferred** from the report).

## Does our DLL behave as an .asi?

| Question | Answer | Status |
|---|---|---|
| DllMain | Stores the module, `DisableThreadLibraryCalls`, spawns the boot thread, returns TRUE. Nothing depends on how it was loaded. Under `DontLoadFromDllMain=0` it runs under the loader lock exactly like the proxy does today. | Confirmed (code) |
| Boot checks | `boot::run` checks the host exe name (`NMS.exe`) and finds the game root from the exe path, not from our file's path, so `Binaries\scripts\nms_rs.asi` finds the same `NMS.rs\` folder. | Confirmed (code) |
| Proxy exports | The xinput build keeps its four XInput exports when renamed to .asi. Nothing imports them (NMS.exe binds to UAL), they resolve lazily, so they are dead but harmless. The winmm/version builds would work too. | Confirmed (code) |
| `InitializeASI` | We do not export it, so UAL calls nothing; booting from DllMain works under every ASI loader. Adding it would give two boot paths. Keep it out. | Confirmed (code) |
| Double load | The `Local\NMS.rs-loader-<pid>` mutex is per process, not per file name, so `xinput9_1_0.dll` (ours) or `winmm.dll` (ours) plus `nms_rs.asi` still boots once; the second writes the "Keep one NMS.rs file" line. Two .asi copies: same. | Confirmed (code); in-game untested |
| Log name | `own_file_name()` gives `nms_rs.asi`, so `core.log` says "loaded ... as nms_rs.asi". | Confirmed (code) |
| Start time | Our proxy's DllMain runs while Windows resolves NMS.exe's static imports. As an .asi it runs at UAL's first hooked kernel32 call, in the CRT start-up. Both are before WinMain, so before the game opens any GAMEDATA file; the gap should be milliseconds. The UI merge (`nms-rs.exe prepare`) then races the game's first pak read exactly as it does today. | Inferred; measure in game |
| Code snapshot | `ProcessImage::current()` copies NMS.exe's code at boot so signatures see original bytes. As a proxy we boot before any ASI plugin. As an .asi, plugins loaded earlier (`LoadExtraPlugins`, earlier folders, earlier names) have already run their DllMain and maybe their `InitializeASI`, and may have hooked NMS.exe. A hooked prologue that one of our signatures covers fails closed ("not found"), it does not crash. This is the main real difference. | Inferred; measure in game |
| Near memory | `hold_near` reserves 128 x 1 MB near the image. Earlier plugins' trampolines may already sit there; we take what is left and log the count. | Inferred |
| Proton | The override belongs to the ASI loader's DLL (`WINEDLLOVERRIDES="xinput9_1_0=n,b" %command%` for UAL as xinput). The .asi needs none: it is loaded by full path. | Inferred; UAL's own hooks under Wine Unknown |

Mitigation for the snapshot: install our .asi where UAL loads it first. The loader's own folder
(`Binaries\`) loads before `scripts\` and `plugins\`, so `Binaries\nms_rs.asi` is the default,
and `scripts\nms_rs.asi` only when `LoadFromScriptsOnly=1`. Inside a folder we cannot force the
order (enumeration order); if a conflict ever shows up, a name that sorts early
(`!nms_rs.asi`) is a cheap extra lever, not adopted now.

## Separate `nms_rs.asi` build?

Two options:

1. Ship the xinput9_1_0.dll build under the name `nms_rs.asi`. Works per the user report; no new
   build. Explorer shows "XInput proxy" and the file has XInput exports nobody calls.
2. A Cargo feature `as-asi` (prototype): same crate, no proxy exports, version resource "ASI
   plugin", `OriginalFilename nms_rs.asi`, plus a second marker `NMS_RS_ASI_MARKER`.

Recommendation: option 2, because the file says what it is and the manager can tell the two
builds apart. The one new failure mode is the exportless build under a DLL name
(`xinput9_1_0.dll` without XInput exports stops NMS.exe from starting: unresolved import). The
manager refuses to install it under a DLL name and `status` flags it if found. Option 1 stays a
valid fallback (the manager accepts any NMS.rs file with a `.asi` name as ours).

Prototype result: the `as-asi` build exports `DllMain`, `NMS_RS_PROXY_MARKER`,
`NMS_RS_ASI_MARKER`, `nms_rs_detour_call_v1`, `nms_rs_detour_entry_v1` and nothing else.

## Manager

Prototype in `nms-launcher/src/proxy.rs` and `main.rs`:

- **Find our .asi**: any `*.asi` with the NMS.rs marker in `Binaries\`, `scripts\`, `plugins\`,
  `update\`, and one level of subfolders under the last three (UAL's own search). By marker, not
  by name, so a renamed file (DaninFuchs' `nms_rs.asi` made from the xinput build) counts.
- **Find an ASI loader**: a foreign file in `Binaries\` under any UAL name that exports
  `IsUltimateASILoader` (PE export table, not a byte search). Other ASI loaders without that
  export are not detected (**Unknown** how many NMS players use one).
- **Read `LoadFromScriptsOnly`** from the ini files above, last one wins.
- **Status lines** (`prepare`, Pack logs, `nms-rs status`-adjacent): every loader file of ours
  (DLLs and .asi paths), the ASI loader if any, and warnings: ".asi but no ASI loader, it will not
  load", ".asi in Binaries\ but LoadFromScriptsOnly=1", "ASI build under a DLL name".
- **Two copies**: `twice_warning` counts .asi files too.
- **`install --as asi`**: copies `nms_rs.asi` from next to `nms-rs.exe` to the folder above
  (updates an existing one of ours in place), removes our DLL loader files, refuses when no ASI
  loader is found unless `--force`.
- **Plain `install`** (and so Setup): when nothing of ours is installed and
  `Binaries\xinput9_1_0.dll` is an ASI loader, it picks the .asi instead of failing, if
  `nms_rs.asi` is next to the exe; otherwise the error names `--as asi`. When ours is already an
  .asi it updates that file.
- **`uninstall`** removes our .asi files too.
- **Proton line**: with our .asi, the Linux note shows the ASI loader's override, not ours.
  (egui part not in the prototype: `linux_note` and `loader_note` need `Vec<String>` loaders and
  two new strings in every language.)

Not done: an egui button "Install as ASI plugin" in the Mods tab next to the "another mod's
xinput9_1_0.dll" message. Setup's automatic choice covers the common path.

## Release (`release.py`)

- `rust-build` builds `--features as-asi` and copies it to `<target>\dist\variants\nms_rs.asi`;
  `check_asi_exports` requires exactly our own exports.
- New optional Nexus file `NMS.rs-<v>-asi.zip`: `Binaries\nms_rs.asi`, the same file as
  `NMS.rs\nms_rs.asi` (so `nms-rs install` can update or reinstall it) and a README (install the
  main zip but keep the ASI loader's xinput9_1_0.dll; no NMS.rs winmm/version next to it).
- Open question for Bryan: also ship `NMS.rs\nms_rs.asi` inside the main zip (about 2 MB), so
  Setup can install the .asi with no extra download. The main zip still contains
  `Binaries\xinput9_1_0.dll`, which overwrites UAL if the player extracts everything; that
  problem is the same one Snap Angles users have today and is not solved here.

## Tests

- Unit tests (`proxy.rs`, `main.rs`), passing (nms-launcher 151 passed): .asi found by marker in
  each folder UAL searches and not elsewhere, ASI loader found by its PE export (a fake PE in the
  test; System32's xinput is not one), ini `LoadFromScriptsOnly` with later files winning,
  install picks the folder, plain install picks the .asi next to an ASI loader, refuses `--as asi`
  without a loader, refuses to replace the loader with `--as xinput9_1_0` and names `--as asi`,
  refuses the ASI build under a DLL name, switching .asi to DLL and back removes the other,
  uninstall removes our .asi only, the Proton line names the ASI loader's DLL.
- `dev\test_asi_plugin.py`, passing: loads the built `nms_rs.asi` from a `scripts\` folder into
  Python with `LoadLibraryW` and no flags (as UAL does), checks the export list, both markers, no
  `InitializeASI`, that the load succeeds (DllMain returned TRUE), that the boot thread ends
  (host is not NMS.exe) and that no `xinput9_1_0.dll` gets mapped. Then the same for the xinput
  build renamed to `.asi`. It does not run UAL itself or the double-load mutex (that one is
  covered by `boot::tests::only_the_first_claim_boots`).
- In game (needs Bryan, not done): see below.

## In-game test plan (owed)

Needs UAL downloaded (from the GitHub release, x64 `xinput9_1_0-x64.zip`; record its SHA256) and
Bryan's go-ahead to put it in the game folder.

1. Back up `Binaries\xinput9_1_0.dll` (ours) to `%TEMP%\nmsasi-backup\`.
2. Put UAL's `xinput9_1_0.dll` in `Binaries\`, our `nms_rs.asi` in `Binaries\`.
3. Start the game to the main menu, then load a save. Compare `NMS.rs\logs\core.log` with a run
   on our proxy: first timestamp relative to process start, "near-code memory held: N of 128",
   every signature line (none newly "not found"), UI merge finished before the menu, the Mods
   page in Options, a setting change from the manager over the pipe, controller input.
4. Same with Imperial Galactic Map's `.asi` also in `Binaries\` (the reported pair), and once
   with our .asi in `scripts\` so another plugin loads first.
5. Double load: our `winmm.dll` plus `nms_rs.asi`: one boot, one "Keep one NMS.rs file" line.
6. Restore: delete UAL, our .asi and any `scripts\`, `.ini`, `.log` UAL wrote, put the backup
   back, run `@game-folder-guard`.

Until that passes, the design is not called sound for release.
