"""Generates assets/jot.ico (a "j" in the Alacritty palette) with no third-party packages."""
import struct
import zlib
from pathlib import Path

BG = (0x18, 0x18, 0x18)
FG = (0x82, 0xB8, 0xC8)
SIZES = [16, 24, 32, 48, 64, 128, 256]
SS = 4  # supersampling factor for smooth edges


def coverage(px, py, size):
    """Returns (bg_alpha, fg_alpha) for a sample point in a 64-unit design space."""
    s = 64.0 / size
    x, y = px * s, py * s
    r = 12.0
    cx = min(max(x, r), 64 - r)
    cy = min(max(y, r), 64 - r)
    in_bg = (x - cx) ** 2 + (y - cy) ** 2 <= r * r
    rects = [(34, 12, 42, 20), (34, 26, 42, 46), (22, 46, 42, 54), (22, 40, 30, 54)]
    in_fg = any(x0 <= x < x1 and y0 <= y < y1 for x0, y0, x1, y1 in rects)
    return in_bg, in_fg


def render(size):
    rows = []
    for j in range(size):
        row = bytearray([0])  # PNG filter type 0
        for i in range(size):
            n_bg = n_fg = 0
            for sj in range(SS):
                for si in range(SS):
                    b, f = coverage(i + (si + 0.5) / SS, j + (sj + 0.5) / SS, size)
                    n_bg += b
                    n_fg += b and f
            total = SS * SS
            a = n_bg / total
            if n_bg == 0:
                row += bytes([0, 0, 0, 0])
                continue
            f_frac = n_fg / n_bg
            rgb = [round(BG[k] * (1 - f_frac) + FG[k] * f_frac) for k in range(3)]
            row += bytes(rgb + [round(a * 255)])
        rows.append(bytes(row))
    return rows


def png(size):
    raw = b"".join(render(size))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def main():
    images = [(s, png(s)) for s in SIZES]
    out = bytearray(struct.pack("<HHH", 0, 1, len(images)))
    offset = 6 + 16 * len(images)
    for size, data in images:
        dim = 0 if size >= 256 else size
        out += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    for _, data in images:
        out += data
    path = Path(__file__).resolve().parent.parent / "assets" / "jot.ico"
    path.write_bytes(out)
    print(f"wrote {path} ({len(out)} bytes, sizes {SIZES})")


main()
