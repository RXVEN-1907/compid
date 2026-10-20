#!/usr/bin/env python3
"""Generate docs/confusion-opt-level.png: the opt_level hold-out confusion
matrix as a heatmap - the honest view of where the model is weakest.
Data comes from docs/calibration.json (per_field.opt_level.confusion),
written by training/train.py on every run.

Usage: .venv/bin/python scripts/gen_confusion_chart.py
"""
import json
from PIL import Image, ImageDraw, ImageFont
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DATA = REPO / "docs" / "calibration.json"
OUT = REPO / "docs" / "confusion-opt-level.png"

W, H = 940, 780
BG = (10, 16, 14)
PANEL = (22, 27, 34)
PANEL_EDGE = (48, 54, 61)
TEXT = (230, 239, 243)
DIM = (139, 148, 158)
LAVENDER = (167, 139, 250)
AMBER = (217, 180, 80)
GREEN = (74, 222, 128)

MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
MONO_B = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"
SANS_B = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
F = lambda sz: ImageFont.truetype(MONO, sz)
FB = lambda sz: ImageFont.truetype(MONO_B, sz)
FS = lambda sz: ImageFont.truetype(SANS_B, sz)


def blend(fg, bg, t):
    return tuple(int(fg[i] * t + bg[i] * (1 - t)) for i in range(3))


def main():
    data = json.loads(DATA.read_text(encoding="utf-8"))
    per = data["per_field"]["opt_level"]
    labels = per["classes"]
    matrix = per["confusion"]
    n = len(labels)
    total = sum(sum(row) for row in matrix)

    cell = 96
    grid_x, grid_y = 260, 150

    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((8, 8, W - 9, H - 9), radius=16, fill=PANEL,
                        outline=PANEL_EDGE, width=2)

    d.text((40, 34), "opt_level - the honest confusion matrix", font=FS(30), fill=TEXT)
    d.text((40, 76), f"hold-out predictions vs ground truth ({total} binaries, seed {data['seed']})",
           font=F(16), fill=DIM)

    # column headers (predicted)
    for j, lab in enumerate(labels):
        d.text((grid_x + j * cell + cell / 2 - d.textlength(lab, font=FB(16)) / 2,
               grid_y - 32), lab, font=FB(16), fill=TEXT)
    d.text((grid_x + n * cell / 2 - d.textlength("predicted", font=FB(15)) / 2,
           grid_y - 58), "predicted", font=FB(15), fill=DIM)

    peak = max(max(row) for row in matrix)
    for i, lab in enumerate(labels):
        y = grid_y + i * cell
        d.text((grid_x - 20 - d.textlength(lab, font=FB(16)), y + cell / 2 - 10),
               lab, font=FB(16), fill=TEXT)
        for j, lab2 in enumerate(labels):
            v = matrix[i][j]
            t = (v / peak) ** 0.5
            fill = blend(LAVENDER, (32, 40, 48), 1 - t) if v else (30, 36, 44)
            d.rounded_rectangle((grid_x + j * cell + 3, y + 3,
                                 grid_x + (j + 1) * cell - 3, y + cell - 3),
                                radius=6, fill=fill)
            if v:
                # diagonal cells (correct) get green text; off-diagonal amber
                color = GREEN if i == j else AMBER
                vw = d.textlength(str(v), font=FB(18))
                d.text((grid_x + j * cell + cell / 2 - vw / 2, y + cell / 2 - 12),
                       str(v), font=FB(18), fill=color)
    d.text((grid_x + n * cell / 2 - d.textlength("ground truth", font=FB(15)) / 2,
            grid_y + n * cell + 12), "ground truth", font=FB(15), fill=DIM)

    d.text((40, grid_y + n * cell + 52),
           "reading: O0 is never confused with Os/Oz; the misses live almost entirely in the",
           font=F(16), fill=DIM)
    d.text((40, grid_y + n * cell + 76),
           "O2/O3 pair, where O3's code shape is a strict subset of what O2 emits on this corpus.",
           font=F(16), fill=DIM)

    img.save(OUT, optimize=True)
    print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KiB, {W}x{H})")


if __name__ == "__main__":
    main()
