#!/usr/bin/env python3
"""Generate docs/boundary-map.png: the .text section of a real seed binary
rendered byte by byte, with function starts marked. Green ticks are the
symbol-table ground truth; amber ticks are what the stripped-binary
prologue + call-target scanner finds without any symbols. The gap between
the two tick rows is the scanner's honest error, visible at a glance.

Usage: .venv/bin/python scripts/gen_boundary_map.py
"""
import json
import struct
import subprocess
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

REPO = Path(__file__).resolve().parent.parent
FIXTURE = REPO / "dataset" / "binaries" / "bit_reverse-gcc13-x86_64-linux-gnu-O0-nopie-nolto-st"
COMPID = REPO / "target" / "release" / "compid"
OUT = REPO / "docs" / "boundary-map.png"

W = 1080
BG = (10, 16, 14)
PANEL = (22, 27, 34)
PANEL_EDGE = (48, 54, 61)
TEXT = (230, 239, 243)
DIM = (139, 148, 158)
LAVENDER = (167, 139, 250)
GREEN = (74, 222, 128)
AMBER = (217, 180, 80)
DARKBYTE = (30, 38, 46)
CODEBYTE = (58, 68, 82)

MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
MONO_B = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"
SANS_B = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
F = lambda sz: ImageFont.truetype(MONO, sz)
FB = lambda sz: ImageFont.truetype(MONO_B, sz)
FS = lambda sz: ImageFont.truetype(SANS_B, sz)

GRID_COLS = 192


def text_section(path):
    """Minimal ELF64 section-header walk: return (.text bytes, sh_addr)."""
    blob = path.read_bytes()
    e_shoff = struct.unpack_from("<Q", blob, 0x28)[0]
    e_shentsize = struct.unpack_from("<H", blob, 0x3A)[0]
    e_shnum = struct.unpack_from("<H", blob, 0x3C)[0]
    e_shstrndx = struct.unpack_from("<H", blob, 0x3E)[0]
    headers = []
    for i in range(e_shnum):
        off = e_shoff + i * e_shentsize
        name, typ, flags = struct.unpack_from("<IIQ", blob, off)
        addr, offset, size = struct.unpack_from("<QQQ", blob, off + 16)
        headers.append((name, typ, flags, addr, offset, size))
    str_off = headers[e_shstrndx][4]
    def name_of(idx):
        end = blob.index(b"\x00", str_off + idx)
        return blob[str_off + idx:end].decode()
    for name, typ, flags, addr, offset, size in headers:
        if name_of(name) == ".text" and size > 0:
            return blob[offset:offset + size], addr
    raise RuntimeError("no .text section")


def functions_json(path, extra=()):
    out = subprocess.run([str(COMPID), "functions", str(path), "--json", *extra],
                         capture_output=True, text=True, check=True)
    return json.loads(out.stdout)


BYTES_PER_ROW = 1000


def main():
    text, sh_addr = text_section(FIXTURE)
    n = len(text)

    truth = functions_json(FIXTURE)["functions"]
    truth_starts = sorted(int(f["addr"], 16) for f in truth)
    stripped = REPO / "tests" / "fixtures" / "tmp_boundary_stripped"
    stripped.write_bytes(FIXTURE.read_bytes())
    subprocess.run(["strip", str(stripped)], check=True)
    heur = functions_json(stripped)["functions"]
    heuristic_starts = sorted(int(f["addr"], 16) for f in heur)
    stripped.unlink()

    rows = (n + BYTES_PER_ROW - 1) // BYTES_PER_ROW
    map_y0 = 158
    map_h = rows  # 1px per byte row
    truth_y0 = map_y0 + map_h + 26
    heur_y0 = truth_y0 + 14
    total_h = heur_y0 + 14 + 78

    # ground-truth body ranges for tinting
    bodies = []
    for f in truth:
        a = int(f["addr"], 16)
        size = int(f["size"]) or 1
        bodies.append((a - sh_addr, a - sh_addr + size))
    bodies.sort()
    merged = []
    for lo, hi in bodies:
        if merged and lo <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], hi)
        else:
            merged.append([lo, hi])

    truth_set = set(t - sh_addr for t in truth_starts if 0 <= t - sh_addr < n)
    heur_set = set(h - sh_addr for h in heuristic_starts if 0 <= h - sh_addr < n)

    img = Image.new("RGB", (W, total_h), BG)
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((8, 8, W - 9, total_h - 9), radius=16, fill=PANEL,
                        outline=PANEL_EDGE, width=2)
    d.text((40, 30), "what the scanner sees", font=FS(30), fill=TEXT)
    d.text((40, 72),
           f".text of {FIXTURE.name} - {n} bytes, one pixel per byte, "
           f"{len(truth_starts)} symbol functions (a static build: everything the seed links)",
           font=F(16), fill=DIM)
    d.text((40, 98),
           "green hairlines: symbol-table ground truth (unstripped twin)   "
           "amber hairlines: prologue + call-target scan (stripped copy, no symbols)",
           font=F(14), fill=DIM)
    d.text((40, 128), ".text", font=FB(17), fill=TEXT)
    d.text((110, 129), f"0x{sh_addr:x}", font=F(15), fill=DIM)

    px = img.load()
    body_lo = 0
    for i in range(n):
        col = i % BYTES_PER_ROW
        row = i // BYTES_PER_ROW
        x = 40 + col
        y = map_y0 + row
        while body_lo < len(merged) and merged[body_lo][1] <= i:
            body_lo += 1
        in_body = body_lo < len(merged) and merged[body_lo][0] <= i
        if i in truth_set:
            c = (120, 200, 140)          # start byte: green-tinted
        elif in_body:
            c = (86, 76, 122)            # function body: muted violet
        else:
            c = (28, 34, 40)             # padding: near-black
        px[x, y] = c
    # hairlines drawn over the map for visibility
    for a in truth_starts:
        off = a - sh_addr
        if 0 <= off < n:
            x = 40 + (off % BYTES_PER_ROW)
            y = map_y0 + (off // BYTES_PER_ROW)
            d.line((x, max(map_y0, y - 2), x, min(map_y0 + map_h, y + 2)),
                   fill=GREEN, width=1)
    for a in heuristic_starts:
        off = a - sh_addr
        if 0 <= off < n:
            x = 40 + (off % BYTES_PER_ROW)
            y = map_y0 + (off // BYTES_PER_ROW)
            d.line((x, max(map_y0, y - 1), x, min(map_y0 + map_h, y + 3)),
                   fill=AMBER, width=1)

    matched = sum(1 for a in heuristic_starts
                  if any(abs(a - t) <= 8 for t in truth_starts))
    d.line((40, truth_y0 - 8, 40 + min(n, BYTES_PER_ROW), truth_y0 - 8), fill=PANEL_EDGE, width=1)
    d.text((40, truth_y0), f"symbol ground truth: {len(truth_starts)} starts", font=FB(16), fill=GREEN)
    d.text((520, truth_y0), f"scanner (no symbols): {len(heuristic_starts)} starts, {matched} matched within 8 bytes",
           font=FB(16), fill=AMBER)
    d.text((40, total_h - 62),
           "the map is the job: every amber hairline on a function entry is a boundary found without a single symbol.",
           font=F(15), fill=DIM)

    img.save(OUT, optimize=True)
    print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KiB) - {n} bytes, "
          f"{len(truth_starts)} truth starts, {len(heuristic_starts)} heuristic starts, {matched} matched")


if __name__ == "__main__":
    main()
