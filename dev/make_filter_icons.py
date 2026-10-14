"""Build the Foods, Custom, Creature egg, Bones and Treasures filter glyphs as 128x128 A8R8G8B8 DDS.

Same format as QUICKDEPOSIT.DDS / SUITLOCK.DDS: uncompressed, no mips, white
on transparent (the round button chrome is the game's, the DDS is only the
glyph). No extra packages: the shapes are filled on a supersampled mask and
box-filtered down. Regenerates
mods/SmallerInventorySlots/TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/FILTER{FOODS,CUSTOM,EGG,BONE,TREASURE}.DDS
and PNG previews under build/previews/filter-icons/.

Egg (FILTER14, CreatureEgg, was the DNA atom PETS/DNA.ICON) is a slightly
tilted egg with a hatching crack across the top and three speckles. Bone
(FILTER16, ExhibitBone, was MISSION.BONES.OFF, which is the same shield as the
digging mission) is a classic two-knob bone on the diagonal. Treasure (FILTER11,
Procedural, tooltip "Treasures": the buried items you dig up; was the compass
WIKI.EXPLORATION.OFF) is a cut gem on a dug mound with a spade stuck in beside
it, so it is not the vanilla chest (FILTER3) or the refined-gem icon. The scales
put their ink() between the vanilla chest (735) and the pot (842).

Foods is a stock pot with a lid line, two side handles and two steam wisps:
solid like the vanilla chest and money bag, and not the vanilla skillet
(WIKI.COOKING.OFF) that the Consumables side of the game already uses.
Custom is a bookmark ribbon with a check mark cut out: "the items I marked".
It is neither the yellow map pin nor a favourite star.

The pot is drawn at FOODS_SCALE (0.90). Measured in game (2026-10-13 side bar
screenshot, white ink inside each 64 px button) the full-size pot had about
1055 units of ink against 844 for the bookmark, 826 atom, 795 chip, 707 chest
and 440 to 650 for the rest; at 0.90 it lands next to the bookmark.
The Quick Deposit and Exosuit lock glyphs are built by make_quick_icons.py.

    py -3 dev\\make_filter_icons.py
"""

from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
REF = ROOT / "mods" / "QuickTransferAll" / "TEXTURES" / "UI" / "FRONTEND" / "ICONS" / "CATEGORIES" / "SUITLOCK.DDS"
OUT_DIR = ROOT / "mods" / "SmallerInventorySlots" / "TEXTURES" / "UI" / "FRONTEND" / "ICONS" / "CATEGORIES"
PREVIEW = ROOT / "build" / "previews" / "filter-icons"
SIZE = 128
SCALE = 6
BIG = SIZE * SCALE
# Shapes are written on a 512 unit canvas and scaled to the mask.
UNIT = BIG / 512.0
# Linear scale of the pot; see foods_mask().
FOODS_SCALE = 0.90
# Linear scales of the egg, bone and treasure glyphs. ink() of the DDS tracks the in-game
# measurement (bookmark 801 vs 844, atom 874 vs 826, chip 839 vs 795, chest 735 vs 707), so
# these put egg 826, bone 703 (thin, already as wide as the atom) and treasure 775.
EGG_SCALE = 0.80
BONE_SCALE = 0.98
TREASURE_SCALE = 0.92


def header_from_ref() -> bytes:
    """The 128 byte header of SUITLOCK.DDS: 128x128, 32 bpp RGBA, no mips."""
    data = REF.read_bytes()
    if data[:4] != b"DDS " or len(data) != 128 + SIZE * SIZE * 4:
        raise SystemExit(f"unexpected DDS in {REF}")
    height, width = struct.unpack("<II", data[12:20])
    if (width, height) != (SIZE, SIZE) or data[84:88] != b"\0\0\0\0":
        raise SystemExit(f"{REF} is not an uncompressed {SIZE}x{SIZE} DDS")
    return data[:128]


class Mask:
    """Coverage mask in 512 unit coordinates; v=255 paints, v=0 cuts.

    place() scales everything drawn afterwards by k around (cx, cy), so a
    glyph can be resized without editing its coordinates.
    """

    def __init__(self) -> None:
        self.n = BIG
        self.pix = bytearray(BIG * BIG)
        self.k, self.cx, self.cy = 1.0, 256.0, 256.0
        self.cos, self.sin = 1.0, 0.0

    def place(self, k: float, cx: float = 256.0, cy: float = 256.0, turn: float = 0.0) -> None:
        """Scale by k and turn by ``turn`` degrees (clockwise on screen) around (cx, cy)."""
        self.k, self.cx, self.cy = k, cx, cy
        self.cos, self.sin = math.cos(math.radians(turn)), math.sin(math.radians(turn))

    def _xy(self, x: float, y: float) -> tuple[float, float]:
        dx, dy = x - self.cx, y - self.cy
        dx, dy = dx * self.cos - dy * self.sin, dx * self.sin + dy * self.cos
        return ((self.cx + dx * self.k) * UNIT, (self.cy + dy * self.k) * UNIT)

    def _span(self, y: int, xa: float, xb: float, v: int) -> None:
        if not 0 <= y < self.n:
            return
        a = max(0, int(math.ceil(xa - 0.5)))
        b = min(self.n, int(math.floor(xb - 0.5)) + 1)
        if b > a:
            row = y * self.n
            self.pix[row + a:row + b] = bytes([v]) * (b - a)

    def poly(self, pts: list[tuple[float, float]], v: int = 255) -> None:
        """Even-odd scanline fill, sampled at pixel centres."""
        p = [self._xy(x, y) for x, y in pts]
        ys = [q[1] for q in p]
        for y in range(max(0, int(min(ys)) - 1), min(self.n, int(max(ys)) + 2)):
            scan = y + 0.5
            xs: list[float] = []
            for i in range(len(p)):
                (xa, ya), (xb, yb) = p[i], p[(i + 1) % len(p)]
                if ya == yb:
                    continue
                if ya > yb:
                    xa, ya, xb, yb = xb, yb, xa, ya
                if ya <= scan < yb:
                    xs.append(xa + (scan - ya) / (yb - ya) * (xb - xa))
            xs.sort()
            for i in range(0, len(xs) - 1, 2):
                self._span(y, xs[i], xs[i + 1], v)

    def ellipse(self, cx: float, cy: float, rx: float, ry: float, v: int = 255) -> None:
        if self.sin and rx != ry:
            self.poly([(cx + rx * math.cos(a), cy + ry * math.sin(a))
                       for a in (2 * math.pi * i / 96 for i in range(96))], v)
            return
        cx, cy = self._xy(cx, cy)
        rx, ry = rx * self.k * UNIT, ry * self.k * UNIT
        for y in range(max(0, int(cy - ry) - 1), min(self.n, int(cy + ry) + 2)):
            t = (y + 0.5 - cy) / ry
            if abs(t) <= 1.0:
                half = rx * math.sqrt(1.0 - t * t)
                self._span(y, cx - half, cx + half, v)

    def circle(self, cx: float, cy: float, r: float, v: int = 255) -> None:
        self.ellipse(cx, cy, r, r, v)

    def rect(self, x0: float, y0: float, x1: float, y1: float, v: int = 255) -> None:
        self.poly([(x0, y0), (x1, y0), (x1, y1), (x0, y1)], v)

    def round_rect(self, x0: float, y0: float, x1: float, y1: float,
                   top: float, bottom: float, v: int = 255) -> None:
        """Rectangle with its own corner radius at the top and at the bottom."""
        pts: list[tuple[float, float]] = []
        for cx, cy, r, a0 in ((x1 - top, y0 + top, top, -90), (x1 - bottom, y1 - bottom, bottom, 0),
                              (x0 + bottom, y1 - bottom, bottom, 90), (x0 + top, y0 + top, top, 180)):
            for i in range(17):
                a = math.radians(a0 + 90 * i / 16)
                pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
        self.poly(pts, v)

    def stroke(self, pts: list[tuple[float, float]], r0: float, r1: float | None = None, v: int = 255) -> None:
        """Round-capped polyline whose radius runs from r0 at the start to r1 at the end."""
        r1 = r0 if r1 is None else r1
        total = sum(math.dist(pts[i], pts[i + 1]) for i in range(len(pts) - 1)) or 1.0
        done = 0.0
        for i in range(len(pts) - 1):
            (xa, ya), (xb, yb) = pts[i], pts[i + 1]
            step = math.dist(pts[i], pts[i + 1])
            ra = r0 + (r1 - r0) * done / total
            rb = r0 + (r1 - r0) * (done + step) / total
            done += step
            self.circle(xa, ya, ra, v)
            self.circle(xb, yb, rb, v)
            if step < 1e-6:
                continue
            nx, ny = -(yb - ya) / step, (xb - xa) / step
            self.poly([(xa + nx * ra, ya + ny * ra), (xb + nx * rb, yb + ny * rb),
                       (xb - nx * rb, yb - ny * rb), (xa - nx * ra, ya - ny * ra)], v)

    def downscale(self) -> list[int]:
        out = []
        n, s = self.n, SCALE
        for y in range(SIZE):
            rows = [self.pix[(y * s + dy) * n:(y * s + dy + 1) * n] for dy in range(s)]
            for x in range(SIZE):
                total = 0
                for row in rows:
                    total += sum(row[x * s:(x + 1) * s])
                out.append(total // (s * s))
        return out


def bezier(p0, p1, p2, p3, n: int = 32) -> list[tuple[float, float]]:
    pts = []
    for i in range(n + 1):
        t = i / n
        u = 1.0 - t
        pts.append((u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0],
                    u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1]))
    return pts


def wisp(x: float, bottom: float, top: float, sway: float) -> list[tuple[float, float]]:
    """One S-shaped steam wisp from bottom up to top."""
    mid = (bottom + top) / 2
    return (bezier((x, bottom), (x - sway, bottom - 30), (x - sway, mid + 22), (x, mid))[:-1]
            + bezier((x, mid), (x + sway, mid - 22), (x + sway, top + 30), (x, top)))


def foods_mask() -> Mask:
    """Stock pot: lid band, body with rounded bottom, two side handles, two steam wisps."""
    m = Mask()
    # Drawn at 90 percent around its own centre (x 256, y 240). At full size the
    # pot had about 1.25x the ink of the vanilla glyphs beside it in game; at
    # 0.90 it matches the bookmark, atom and chip.
    m.place(FOODS_SCALE, 256, 240)
    # Lid: a low dome with a knob, over a band a little wider than the pot.
    m.ellipse(256, 220, 118, 34)
    m.round_rect(236, 164, 276, 200, 14, 4)
    m.round_rect(112, 218, 400, 248, 15, 15)
    # Body with a rounded bottom, a gap under the lid band.
    m.round_rect(132, 262, 380, 430, 6, 58)
    m.rect(100, 248, 412, 262, 0)
    # Side handles (ears) set into the body.
    m.round_rect(80, 286, 150, 322, 18, 18)
    m.round_rect(362, 286, 432, 322, 18, 18)
    # Steam: two wisps, thick at the bottom and thinning out.
    m.stroke(wisp(204, 152, 60, 26), 18, 9)
    m.stroke(wisp(308, 152, 60, 26), 18, 9)
    return m


def custom_mask() -> Mask:
    """Bookmark ribbon with a notch, a check mark cut out of it."""
    m = Mask()
    left, right, top, bottom = 164.0, 348.0, 78.0, 434.0
    notch = 354.0
    r = 26.0
    pts: list[tuple[float, float]] = [(left, bottom), (left, top + r)]
    for i in range(17):
        a = math.radians(180 + 90 * i / 16)
        pts.append((left + r + r * math.cos(a), top + r + r * math.sin(a)))
    for i in range(17):
        a = math.radians(270 + 90 * i / 16)
        pts.append((right - r + r * math.cos(a), top + r + r * math.sin(a)))
    pts += [(right, bottom), ((left + right) / 2, notch)]
    m.poly(pts)
    m.stroke([(208, 216), (244, 254), (306, 182)], 19, v=0)
    return m


def egg_outline(cx: float, cy: float, rx: float, ry: float, narrow: float) -> list[tuple[float, float]]:
    """Egg: an ellipse whose half-width shrinks towards the top by ``narrow``."""
    pts = []
    for i in range(128):
        t = 2 * math.pi * i / 128
        pts.append((cx + rx * math.sin(t) * (1 - narrow * math.cos(t)), cy - ry * math.cos(t)))
    return pts


def egg_mask() -> Mask:
    """Creature egg: tilted egg, a hatching crack across the top, three speckles."""
    m = Mask()
    m.place(EGG_SCALE, 256, 262, 14)
    m.poly(egg_outline(256, 262, 150, 192, 0.16))
    # Crack: a zigzag cut from the left edge two thirds of the way across.
    m.stroke([(84, 214), (136, 182), (176, 230), (222, 186), (262, 230), (300, 196)], 12, 9, v=0)
    # Speckles low on the right.
    m.circle(300, 318, 22, v=0)
    m.circle(232, 374, 15, v=0)
    m.circle(330, 394, 13, v=0)
    return m


def bone_mask() -> Mask:
    """Bone: a shaft with two round knobs at each end, on the diagonal."""
    m = Mask()
    m.place(BONE_SCALE, 256, 256, -45)
    m.round_rect(130, 222, 382, 290, 6, 6)
    for x in (128, 384):
        m.circle(x, 210, 54)
        m.circle(x, 302, 54)
    return m


def treasure_mask() -> Mask:
    """Buried treasure: a cut gem resting on a dug mound, a spade stuck in beside it."""
    m = Mask()
    m.place(TREASURE_SCALE, 256, 268)
    # Mound: the top of a wide ellipse, flat underneath.
    m.ellipse(256, 446, 196, 112)
    m.rect(0, 440, 512, 640, 0)
    # Spade: T grip, shaft and a pointed blade going into the mound, outlined by a gap.
    m.stroke([(342, 112), (420, 112)], 15)
    m.stroke([(381, 112), (381, 300)], 15)
    blade = [(338, 290), (424, 290), (424, 362), (381, 414), (338, 362)]
    m.poly([(326, 278), (436, 278), (436, 368), (381, 432), (326, 368)], 0)
    m.poly(blade)
    # Gem: crown and pavilion, cut a little clear of the mound, with facet lines cut out.
    crown = [(96, 236), (140, 180), (276, 180), (320, 236)]
    gem = crown + [(208, 392)]
    m.poly([(84, 230), (134, 166), (282, 166), (332, 230), (208, 412)], 0)
    m.poly(gem)
    m.stroke([(104, 238), (312, 238)], 8, v=0)
    m.stroke([(170, 184), (150, 236), (208, 382)], 7, v=0)
    m.stroke([(246, 184), (266, 236), (208, 382)], 7, v=0)
    return m


def ink(alpha: list[int]) -> float:
    """White ink of a glyph as it sits in a 64 px button, the unit FOODS_SCALE was measured in."""
    return sum(alpha) / 255 / 4


def write_dds(path: Path, alpha: list[int], header: bytes) -> None:
    pixels = bytearray()
    for a in alpha:
        pixels.extend((255, 255, 255, a))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(header + pixels)


def write_png(path: Path, alpha: list[int], dark: bool) -> None:
    """Transparent white glyph, or the glyph on a dark round button like the side bar."""
    raw = bytearray()
    c = (SIZE - 1) / 2
    for y in range(SIZE):
        raw.append(0)
        for x in range(SIZE):
            a = alpha[y * SIZE + x]
            if not dark:
                raw.extend((255, 255, 255, a))
                continue
            d = math.hypot(x - c, y - c)
            base = (12, 14, 18) if d > 63 else (54, 58, 64) if d > 60 else (30, 33, 38)
            raw.extend(tuple(b + (240 - b) * a // 255 for b in base) + (255,))

    def chunk(tag: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
                     + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b""))


def main() -> None:
    header = header_from_ref()
    PREVIEW.mkdir(parents=True, exist_ok=True)
    for name, make in GLYPHS:
        alpha = make().downscale()
        write_dds(OUT_DIR / f"{name}.DDS", alpha, header)
        write_png(PREVIEW / f"{name}.png", alpha, False)
        write_png(PREVIEW / f"{name}-on-dark.png", alpha, True)
        print(f"wrote {OUT_DIR / (name + '.DDS')} (ink {ink(alpha):.0f})")


GLYPHS = (
    ("FILTERFOODS", foods_mask),
    ("FILTERCUSTOM", custom_mask),
    ("FILTEREGG", egg_mask),
    ("FILTERBONE", bone_mask),
    ("FILTERTREASURE", treasure_mask),
)


if __name__ == "__main__":
    main()
