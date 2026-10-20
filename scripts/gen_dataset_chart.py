#!/usr/bin/env python3
"""Generate docs/dataset.png: what the seed dataset is made of - builds by
container format and OS (donut) plus builds per architecture (bars), on the
brand's dark ground. Numbers mirror dataset/manifest.csv; refresh alongside
docs/EVALUATION.md after the matrix grows.

Usage: .venv/bin/python scripts/gen_dataset_chart.py
"""
from PIL import Image, ImageDraw, ImageFont
from pathlib import Path
import math

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "docs" / "dataset.png"

W, H = 1080, 560
BG = (10, 16, 14)
PANEL = (22, 27, 34)
PANEL_EDGE = (48, 54, 61)
TEXT = (230, 239, 243)
DIM = (139, 148, 158)
LAVENDER = (167, 139, 250)
TEAL = (94, 200, 180)
AMBER = (217, 180, 80)
GRAY = (122, 136, 148)

# keep in sync with dataset/manifest.csv
BY_OS = [("linux (ELF)", 1091, LAVENDER), ("macos (Mach-O)", 204, TEAL), ("windows (PE)", 100, AMBER)]
BY_ARCH = [("x86_64", 889), ("riscv64", 204), ("aarch64", 200), ("arm", 102)]
TOTAL = sum(v for _, v, _ in BY_OS)

MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
MONO_B = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"
SANS_B = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
F = lambda sz: ImageFont.truetype(MONO, sz)
FB = lambda sz: ImageFont.truetype(MONO_B, sz)
FS = lambda sz: ImageFont.truetype(SANS_B, sz)

CX, CY, R = 250, 300, 150


def donut(draw):
    start = -90.0
    for name, value, color in BY_OS:
        sweep = 360.0 * value / TOTAL
        draw.pieslice((CX - R, CY - R, CX + R, CY + R), start, start + sweep,
                      fill=color)
        start += sweep
    hole = 88
    draw.ellipse((CX - hole, CY - hole, CX + hole, CY + hole), fill=PANEL)
    tw = draw.textlength(str(TOTAL), font=FB(44))
    draw.text((CX - tw / 2, CY - 42), str(TOTAL), font=FB(44), fill=TEXT)
    lab = "builds"
    lw = draw.textlength(lab, font=F(18))
    draw.text((CX - lw / 2, CY + 12), lab, font=F(18), fill=DIM)


def main():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((8, 8, W - 9, H - 9), radius=16, fill=PANEL,
                        outline=PANEL_EDGE, width=2)

    d.text((40, 34), "the seed dataset", font=FS(30), fill=TEXT)
    d.text((40, 76), "every binary compiled from 57 seed programs across 9 targets - resumable, deterministic, regenerable",
           font=F(16), fill=DIM)

    donut(d)

    # OS legend next to the donut
    ly = 190
    for name, value, color in BY_OS:
        d.rounded_rectangle((470, ly, 494, ly + 24), radius=5, fill=color)
        d.text((508, ly + 1), name, font=FB(19), fill=TEXT)
        vw = d.textlength(str(value), font=F(19))
        d.text((760 - vw, ly + 2), str(value), font=F(19), fill=DIM)
        ly += 46

    # per-arch bars on the right half
    bx, by, bw = 760, 190, 260
    d.text((bx, by - 44), "builds per architecture", font=FB(19), fill=TEXT)
    maxv = max(v for _, v in BY_ARCH)
    for i, (name, value) in enumerate(BY_ARCH):
        y = by + i * 62
        d.text((bx, y), name, font=F(17), fill=TEXT)
        frac = value / maxv
        d.rounded_rectangle((bx, y + 26, bx + int(bw * frac), y + 48),
                            radius=6, fill=LAVENDER if name == "x86_64" else GRAY)
        d.text((bx + int(bw * frac) + 12, y + 24), str(value), font=F(17), fill=DIM)

    # container formats strip
    fx = 40
    d.text((fx, H - 74), "containers:", font=FB(17), fill=TEXT)
    fx += 160
    for name, value, color in [("ELF", 1087, LAVENDER), ("PE", 98, AMBER), ("Mach-O", 204, TEAL)]:
        d.rounded_rectangle((fx, H - 78, fx + 16, H - 60), radius=4, fill=color)
        label = f"{name} {value}"
        d.text((fx + 26, H - 78), label, font=F(17), fill=DIM)
        fx += 26 + d.textlength(label, font=F(17)) + 40

    img.save(OUT, optimize=True)
    print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KiB, {W}x{H})")


if __name__ == "__main__":
    main()
