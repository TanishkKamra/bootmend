"""Pack the rendered PNGs (design/logo/png/icon-<size>.png) into assets/bootmend.ico.

Every entry is stored as PNG, which Windows Vista and later read at all sizes.
"""
import struct
import sys
from pathlib import Path

SIZES = [16, 20, 24, 30, 32, 36, 40, 48, 64, 256]
here = Path(__file__).resolve().parent
out = here.parent.parent / "assets" / "bootmend.ico"

blobs = []
for z in SIZES:
    p = here / "png" / f"icon-{z}.png"
    data = p.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        sys.exit(f"{p} is not a PNG")
    w, h = struct.unpack(">II", data[16:24])
    if (w, h) != (z, z):
        sys.exit(f"{p} is {w}x{h}, expected {z}x{z}")
    blobs.append(data)

header = struct.pack("<HHH", 0, 1, len(SIZES))
offset = 6 + 16 * len(SIZES)
entries = b""
for z, data in zip(SIZES, blobs):
    dim = 0 if z >= 256 else z
    entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
    offset += len(data)
out.write_bytes(header + entries + b"".join(blobs))
print(f"wrote {out} ({out.stat().st_size} bytes, {len(SIZES)} sizes)")
