"""Build the Quick Deposit and Exosuit lock glyphs for QuickTransferAll.

Both are white on transparent 128x128 32 bpp DDS (no mips) that sit on the
QUICKDEPOSITBG.DDS hexagon (unchanged). Each file keeps its own 128 byte
header, so the size, format and channel masks stay as shipped and
QuickDeposit.lua needs no change.

The lua draws the two glyphs at different sizes on the same 58 px hexagon:
QUICKDEPOSIT.DDS at 52 px, SUITLOCK.DDS at 36 px. Both glyphs are written in
one design space where 512 units = 36 px on screen (the lock size); the
deposit glyph is drawn at 36/52 of that, so both come out the same size and
line weight in game (about 30 px across, details 2 px or more).

- QUICKDEPOSIT: an open cardboard-style crate (front face, two open flaps)
  with a thick down arrow dropping into it: "put items into storage".
- SUITLOCK: a 3x3 inventory slot grid with a closed padlock over its lower
  right corner: "this inventory is locked".

Shapes reuse the mask from make_filter_icons.py. Writes the DDS into
mods/QuickTransferAll/TEXTURES/UI/FRONTEND/ICONS/CATEGORIES and previews on
the in-game hexagon under build/previews/quick-icons/.

    py -3 dev\\make_quick_icons.py
"""

from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path

from make_filter_icons import SIZE, Mask, write_dds

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
OUT_DIR = ROOT / "mods" / "QuickTransferAll" / "TEXTURES" / "UI" / "FRONTEND" / "ICONS" / "CATEGORIES"
HEX_BG = OUT_DIR / "QUICKDEPOSITBG.DDS"
PREVIEW = ROOT / "build" / "previews" / "quick-icons"

# Display sizes from QuickDeposit.lua (BG 58, deposit glyph 52, lock glyph 36).
BG_PX, DEPOSIT_PX, LOCK_PX = 58, 52, 36
# Gap cut around overlapping parts, in design units (about 1.8 px on screen).
GAP = 26.0


def header_of(path: Path) -> bytes:
    """The existing file's header; checks it is an uncompressed 128x128 32 bpp DDS."""
    data = path.read_bytes()
    height, width = struct.unpack("<II", data[12:20])
    bpp = struct.unpack("<I", data[88:92])[0]
    if (data[:4] != b"DDS " or (width, height) != (SIZE, SIZE) or bpp != 32
            or data[84:88] != b"\0\0\0\0" or len(data) != 128 + SIZE * SIZE * 4):
        raise SystemExit(f"{path} is not an uncompressed {SIZE}x{SIZE} 32 bpp DDS")
    return data[:128]


def arrow(m: Mask, g: float, v: int) -> None:
    """Down arrow, grown by g on every side (g > 0 with v=0 cuts the gap)."""
    shaft, head, neck, tip, top = 34.0, 104.0, 196.0, 330.0, 30.0
    slope = math.hypot(head, tip - neck) / head  # how far the slanted edge moves per unit of g
    m.poly([(256 - shaft - g, top - g), (256 + shaft + g, top - g), (256 + shaft + g, neck - g),
            (256 + head + g * slope, neck - g), (256, tip + g * slope),
            (256 - head - g * slope, neck - g), (256 - shaft - g, neck - g)], v)


def deposit_mask() -> Mask:
    """Open crate with two flaps, a down arrow dropping into it."""
    m = Mask()
    m.place(LOCK_PX / DEPOSIT_PX, 256, 256)
    top, bottom, left, right = 280.0, 470.0, 84.0, 428.0
    # Front face, slightly rounded at the bottom.
    m.round_rect(left, top, right, bottom, 6, 26)
    # Flaps folded open to both sides, hinged on the top corners.
    m.poly([(left, top - 14), (left + 92, top - 14), (left + 40, top - 92), (left - 64, top - 92)])
    m.poly([(right, top - 14), (right - 92, top - 14), (right - 40, top - 92), (right + 64, top - 92)])
    # A slat line across the front so it reads as a crate, not a block.
    m.rect(left + 40, 404, right - 40, 404 + 22, 0)
    # Arrow over everything, with a cut gap so its tip shows inside the crate.
    arrow(m, GAP, 0)
    arrow(m, 0.0, 255)
    return m


def padlock(m: Mask, g: float, v: int) -> None:
    """Closed padlock body plus shackle, grown by g on every side."""
    x0, x1, y0, y1 = 252.0, 476.0, 300.0, 478.0
    m.round_rect(x0 - g, y0 - g, x1 + g, y1 + g, 20 + g, 24 + g, v)
    # Shackle: two legs going up into a half circle, as one round-capped stroke.
    cx, ring, r, legs = (x0 + x1) / 2, 64.0, 22.0, y0 - 50
    arc = [(cx + ring * math.cos(math.radians(a)), legs - ring * math.sin(math.radians(a)))
           for a in range(180, -1, -10)]
    m.stroke([(cx - ring, y0 + 10)] + arc + [(cx + ring, y0 + 10)], r + g, v=v)


def lock_mask() -> Mask:
    """3x3 slot grid, closed padlock over its lower right corner."""
    m = Mask()
    cell, gap, x0, y0 = 88.0, 26.0, 36.0, 36.0
    for row in range(3):
        for col in range(3):
            x, y = x0 + col * (cell + gap), y0 + row * (cell + gap)
            m.round_rect(x, y, x + cell, y + cell, 16, 16)
    padlock(m, GAP, 0)
    padlock(m, 0.0, 255)
    # Keyhole.
    m.circle(364, 372, 24, 0)
    m.poly([(352, 380), (376, 380), (384, 430), (344, 430)], 0)
    return m


# Previews ---------------------------------------------------------------

TEAL = (17, 80, 90)       # side bar backdrop in an in-game screenshot
HEX_ALPHA = 0.12          # the game draws the hexagon as faint white over it


def sample(alpha: list[int], u: float, v: float) -> float:
    """Bilinear alpha at texture coordinate (u, v) in 0..1, 0 outside."""
    x, y = u * SIZE - 0.5, v * SIZE - 0.5
    xi, yi = math.floor(x), math.floor(y)
    fx, fy = x - xi, y - yi

    def at(px: int, py: int) -> int:
        return alpha[py * SIZE + px] if 0 <= px < SIZE and 0 <= py < SIZE else 0

    top = at(xi, yi) * (1 - fx) + at(xi + 1, yi) * fx
    bot = at(xi, yi + 1) * (1 - fx) + at(xi + 1, yi + 1) * fx
    return (top * (1 - fy) + bot * fy) / 255.0


def dds_alpha(path: Path) -> list[int]:
    return list(path.read_bytes()[128 + 3::4])


def hex_preview(path: Path, glyph: list[int], glyph_px: int, zoom: int) -> None:
    """Glyph on the in-game hexagon, at zoom x the 58 px button."""
    bg = dds_alpha(HEX_BG)
    n = BG_PX * zoom
    pad = (BG_PX - glyph_px) / 2 / BG_PX
    gs = glyph_px / BG_PX
    raw = bytearray()
    for y in range(n):
        raw.append(0)
        v = (y + 0.5) / n
        for x in range(n):
            u = (x + 0.5) / n
            a_bg = sample(bg, u, v) * HEX_ALPHA
            a_gl = sample(glyph, (u - pad) / gs, (v - pad) / gs) if pad <= u < 1 - pad and pad <= v < 1 - pad else 0.0
            px = [c + (255 - c) * a_bg for c in TEAL]
            px = [c + (255 - c) * a_gl for c in px]
            raw.extend(int(c + 0.5) for c in px)
    write_png_rgb(path, n, n, raw)


def write_png_rgb(path: Path, w: int, h: int, raw: bytes) -> None:
    def chunk(tag: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b""))


def main() -> None:
    PREVIEW.mkdir(parents=True, exist_ok=True)
    for name, make, px in (("QUICKDEPOSIT", deposit_mask, DEPOSIT_PX), ("SUITLOCK", lock_mask, LOCK_PX)):
        target = OUT_DIR / f"{name}.DDS"
        header = header_of(target)
        alpha = make().downscale()
        write_dds(target, alpha, header)
        for zoom in (1, 4):
            hex_preview(PREVIEW / f"{name}-hex-{zoom}x.png", alpha, px, zoom)
        print(f"wrote {target}")


if __name__ == "__main__":
    main()
