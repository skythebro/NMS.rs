"""Build the NMS.rs app icon: a hex badge (light-blue rim, NMS blue face, white N, amber ".rs" bar and dot).

Same approach as make_filter_icons.py: no extra packages, shapes are filled on a supersampled
canvas (512 unit coordinates) and box-filtered down. Writes

    rust/crates/nms-launcher/assets/nms-rs.ico        16, 24, 32, 48, 64, 128, 256 (BMP up to 48, PNG above)
    rust/crates/nms-launcher/assets/nms-rs-256.png    preview and README use
    rust/crates/nms-launcher/assets/nms-rs-64.rgba    raw 64x64 RGBA for the egui window icon
    build/previews/app-icon/nms-rs-<size>.png  one PNG per size for looking at

    py -3 dev\\make_app_icon.py
"""

from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
ASSETS = ROOT / "rust" / "crates" / "nms-launcher" / "assets"
PREVIEW = ROOT / "build" / "previews" / "app-icon"
SIZES = (16, 24, 32, 48, 64, 128, 256)
BIG = 768  # divisible by 3 (256), 6 (128), 12 (64), 16 (48), 24 (32), 32 (24), 48 (16)
UNIT = BIG / 512.0

# Same blue and amber as rust/crates/nms-launcher/src/palette.rs.
AMBER_TOP, AMBER_BOT = (255, 208, 110), (238, 170, 40)    # amber (the ".rs" mark)
RIM_TOP, RIM_BOT = (150, 200, 250), (45, 125, 210)        # light blue rim
RIM_DARK = (10, 20, 38)
TEAL_TOP, TEAL_BOT = (52, 140, 228), (16, 52, 108)        # NMS blue face
WHITE = (255, 255, 255)


def lerp(a: tuple[int, int, int], b: tuple[int, int, int], t: float) -> tuple[int, int, int]:
    t = max(0.0, min(1.0, t))
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(3))  # type: ignore[return-value]


class Canvas:
    """RGBA canvas at BIG x BIG; shapes paint opaque colour (alpha 255) with an optional y gradient."""

    def __init__(self) -> None:
        self.pix = bytearray(BIG * BIG * 4)

    def _span(self, y: int, xa: float, xb: float, color_at) -> None:
        if not 0 <= y < BIG:
            return
        a = max(0, int(math.ceil(xa - 0.5)))
        b = min(BIG, int(math.floor(xb - 0.5)) + 1)
        if b > a:
            r, g, bl = color_at(y)
            i = (y * BIG + a) * 4
            self.pix[i:i + (b - a) * 4] = bytes((r, g, bl, 255)) * (b - a)

    def poly(self, pts: list[tuple[float, float]], top=WHITE, bottom=None) -> None:
        """Even-odd scanline fill. ``top``/``bottom`` give a vertical gradient over the polygon."""
        p = [(x * UNIT, y * UNIT) for x, y in pts]
        ys = [q[1] for q in p]
        y0, y1 = min(ys), max(ys)
        bottom = top if bottom is None else bottom

        def color_at(y: int) -> tuple[int, int, int]:
            return lerp(top, bottom, (y + 0.5 - y0) / max(1e-6, y1 - y0))

        for y in range(max(0, int(y0) - 1), min(BIG, int(y1) + 2)):
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
                self._span(y, xs[i], xs[i + 1], color_at)

    def circle(self, cx: float, cy: float, r: float, top=WHITE, bottom=None) -> None:
        self.poly([(cx + r * math.cos(2 * math.pi * i / 64), cy + r * math.sin(2 * math.pi * i / 64))
                   for i in range(64)], top, bottom)

    def round_rect(self, x0: float, y0: float, x1: float, y1: float, r: float, top=WHITE, bottom=None) -> None:
        pts: list[tuple[float, float]] = []
        for cx, cy, a0 in ((x1 - r, y0 + r, -90), (x1 - r, y1 - r, 0), (x0 + r, y1 - r, 90), (x0 + r, y0 + r, 180)):
            for i in range(13):
                a = math.radians(a0 + 90 * i / 12)
                pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
        self.poly(pts, top, bottom)

    def downscale(self, size: int) -> bytes:
        """Box filter to size x size RGBA (colour weighted by coverage, straight alpha out)."""
        k = BIG // size
        out = bytearray()
        for y in range(size):
            rows = [self.pix[(y * k + dy) * BIG * 4:(y * k + dy + 1) * BIG * 4] for dy in range(k)]
            for x in range(size):
                r = g = b = a = 0
                for row in rows:
                    seg = row[x * k * 4:(x + 1) * k * 4]
                    r += sum(seg[0::4])
                    g += sum(seg[1::4])
                    b += sum(seg[2::4])
                    a += sum(seg[3::4])
                n = k * k
                if a == 0:
                    out += b"\0\0\0\0"
                else:
                    cover = a / (255 * n)
                    out += bytes((min(255, round(r / n / cover)), min(255, round(g / n / cover)),
                                  min(255, round(b / n / cover)), round(255 * cover)))
        return bytes(out)


def hexagon(cx: float, cy: float, r: float) -> list[tuple[float, float]]:
    """Pointy-top hexagon."""
    return [(cx + r * math.cos(math.radians(-90 + 60 * i)), cy + r * math.sin(math.radians(-90 + 60 * i)))
            for i in range(6)]


def draw() -> Canvas:
    c = Canvas()
    c.poly(hexagon(256, 256, 250), RIM_TOP, RIM_BOT)         # blue rim
    c.poly(hexagon(256, 256, 222), RIM_DARK)                 # thin dark gap
    c.poly(hexagon(256, 256, 206), TEAL_TOP, TEAL_BOT)       # blue face
    # A paler band across the top third reads as a soft highlight.
    c.poly([(256, 50), (434, 153), (78, 153)], lerp(TEAL_TOP, WHITE, 0.16), TEAL_TOP)
    # White N (two stems and a diagonal) with a dark shadow one step down and right.
    n = [(148, 340), (148, 130), (194, 130), (316, 270), (316, 130), (364, 130), (364, 340), (318, 340),
         (196, 200), (196, 340)]
    c.poly([(x + 9, y + 9) for x, y in n], (6, 28, 64))
    c.poly(n, WHITE)
    # ".rs": a bar and a dot in red.
    c.round_rect(148, 372, 296, 400, 13, AMBER_TOP, AMBER_BOT)
    c.circle(340, 386, 15, AMBER_TOP, AMBER_BOT)
    return c


def png_bytes(size: int, rgba: bytes) -> bytes:
    raw = bytearray()
    for y in range(size):
        raw.append(0)
        raw += rgba[y * size * 4:(y + 1) * size * 4]

    def chunk(tag: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")


def bmp_entry(size: int, rgba: bytes) -> bytes:
    """ICO image as a 32 bpp DIB: BITMAPINFOHEADER (double height), BGRA bottom-up, empty AND mask."""
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    pixels = bytearray()
    for y in range(size - 1, -1, -1):
        for x in range(size):
            r, g, b, a = rgba[(y * size + x) * 4:(y * size + x) * 4 + 4]
            pixels += bytes((b, g, r, a))
    mask_row = ((size + 31) // 32) * 4
    return header + bytes(pixels) + bytes(mask_row * size)


def ico_bytes(images: dict[int, bytes]) -> bytes:
    entries = []
    for size in SIZES:
        rgba = images[size]
        data = png_bytes(size, rgba) if size > 48 else bmp_entry(size, rgba)
        entries.append((size, data))
    out = struct.pack("<HHH", 0, 1, len(entries))
    offset = 6 + 16 * len(entries)
    for size, data in entries:
        out += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return out + b"".join(data for _, data in entries)


def main() -> None:
    canvas = draw()
    images = {size: canvas.downscale(size) for size in SIZES}
    ASSETS.mkdir(parents=True, exist_ok=True)
    PREVIEW.mkdir(parents=True, exist_ok=True)
    (ASSETS / "nms-rs.ico").write_bytes(ico_bytes(images))
    (ASSETS / "nms-rs-256.png").write_bytes(png_bytes(256, images[256]))
    (ASSETS / "nms-rs-64.rgba").write_bytes(images[64])
    for size, rgba in images.items():
        (PREVIEW / f"nms-rs-{size}.png").write_bytes(png_bytes(size, rgba))
    print(f"wrote {ASSETS / 'nms-rs.ico'} and previews in {PREVIEW}")


if __name__ == "__main__":
    main()
