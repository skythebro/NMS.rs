"""The ASI plugin build (nms_rs.asi) loads the way an ASI loader loads it, and does nothing else.

    py -3 -I dev\\test_asi_plugin.py [folder]

Reads <target>\\dist\\variants\\nms_rs.asi (written by `release.py rust-build`), or nms_rs.asi in
[folder]. Copies it into a temp folder and loads it with LoadLibraryW and no flags, as Ultimate ASI
Loader does (plugins/plugins.cpp). Checks: only our own exports and both markers; DllMain returns
TRUE (the load succeeds); the boot thread starts and ends by itself because this host is not
NMS.exe; no xinput9_1_0.dll gets mapped. The xinput build renamed to .asi (what players did before
an ASI build existed) is loaded the same way. Nothing touches a game install.
"""
import ctypes
import shutil
import sys
import tempfile
import time
from ctypes import wintypes as w
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import release  # noqa: E402

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
kernel32.LoadLibraryW.argtypes = [w.LPCWSTR]
kernel32.LoadLibraryW.restype = w.HMODULE
kernel32.GetModuleHandleW.argtypes = [w.LPCWSTR]
kernel32.GetModuleHandleW.restype = w.HMODULE
kernel32.GetProcAddress.argtypes = [w.HMODULE, w.LPCSTR]
kernel32.GetProcAddress.restype = ctypes.c_void_p
kernel32.CreateToolhelp32Snapshot.argtypes = [w.DWORD, w.DWORD]
kernel32.CreateToolhelp32Snapshot.restype = w.HANDLE
kernel32.CloseHandle.argtypes = [w.HANDLE]


class THREADENTRY32(ctypes.Structure):
    _fields_ = [("dwSize", w.DWORD), ("cntUsage", w.DWORD), ("th32ThreadID", w.DWORD),
                ("th32OwnerProcessID", w.DWORD), ("tpBasePri", w.LONG), ("tpDeltaPri", w.LONG), ("dwFlags", w.DWORD)]


kernel32.Thread32First.argtypes = [w.HANDLE, ctypes.POINTER(THREADENTRY32)]
kernel32.Thread32Next.argtypes = [w.HANDLE, ctypes.POINTER(THREADENTRY32)]


def thread_count() -> int:
    snap = kernel32.CreateToolhelp32Snapshot(0x4, 0)  # TH32CS_SNAPTHREAD
    entry = THREADENTRY32()
    entry.dwSize = ctypes.sizeof(entry)
    pid, count = kernel32.GetCurrentProcessId(), 0
    ok = kernel32.Thread32First(snap, ctypes.byref(entry))
    while ok:
        count += entry.th32OwnerProcessID == pid
        ok = kernel32.Thread32Next(snap, ctypes.byref(entry))
    kernel32.CloseHandle(snap)
    return count


def settle(baseline: int) -> int:
    """Thread count once the boot thread is gone (it returns at once when the host is not NMS.exe)."""
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        now = thread_count()
        if now <= baseline:
            return now
        time.sleep(0.05)
    return thread_count()


def load_like_ual(path: Path) -> int:
    baseline = thread_count()
    module = kernel32.LoadLibraryW(str(path))
    assert module, f"{path.name}: LoadLibraryW failed, error {ctypes.get_last_error()}"
    assert kernel32.GetModuleHandleW(str(path)) == module
    # UAL calls InitializeASI when exported; we do not export it, so DllMain is the only entry.
    assert not kernel32.GetProcAddress(module, b"InitializeASI"), "InitializeASI must not be exported"
    left = settle(baseline)
    assert left <= baseline, f"{path.name}: boot thread still running ({left} threads, {baseline} before)"
    return module


def main() -> int:
    folder = Path(sys.argv[1]) if len(sys.argv) > 1 else release.PROXY_VARIANT_DIR
    built = folder / release.ASI_FILE
    xinput = release.RUST_TARGET / release.PROXY_DLL
    if not built.is_file():
        raise SystemExit(f"Missing {built}. Build it first: py -3 release.py rust-build --version <v>")
    release.check_asi_exports(built)
    assert kernel32.GetModuleHandleW("xinput9_1_0.dll") is None, "this Python already has XInput loaded"
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as tmp:
        copy = Path(tmp) / "scripts" / release.ASI_FILE
        copy.parent.mkdir()
        shutil.copyfile(built, copy)
        module = load_like_ual(copy)
        for marker, want in (("NMS_RS_PROXY_MARKER", b"NMS.rs proxy marker"), ("NMS_RS_ASI_MARKER", b"NMS.rs asi marker")):
            address = kernel32.GetProcAddress(module, marker.encode())
            assert address and ctypes.string_at(address) == want, marker
        assert kernel32.GetModuleHandleW("xinput9_1_0.dll") is None, "the ASI build mapped xinput9_1_0.dll"
        print(f"ok: {release.ASI_FILE} loads like an ASI plugin, boot thread ends, no proxy exports")
        if xinput.is_file() and xinput.read_bytes() != built.read_bytes():
            renamed = Path(tmp) / "renamed.asi"
            shutil.copyfile(xinput, renamed)
            load_like_ual(renamed)
            print("ok: the xinput9_1_0.dll build renamed to .asi loads the same way")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
