#!/usr/bin/env python3
"""Generate docs/calibration.png: a reliability diagram for the whole-binary
model - observed hold-out accuracy per predicted-confidence bucket, pooled
over all label fields, next to the perfect-calibration diagonal. Data comes
from docs/calibration.json, which training/train.py writes on every run.

Usage: .venv/bin/python scripts/gen_calibration_chart.py
"""
import json
from PIL import Image, ImageDraw, ImageFont
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DATA = REPO / "docs" / "calibration.json"
OUT = REPO / "docs" / "calibration.png"

W, H = 1080, 640
BG = (10, 16, 14)
PANEL = (22, 27, 34)
PANEL_EDGE = (48, 54, 61)
TEXT = (230, 239, 243)
DIM = (139, 148, 158)
LAVENDER = (167, 139, 250)
GRAY = (122, 136, 148)
GREEN = (74, 222, 128)

MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
MONO_B = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"
SANS_B = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
F = lambda sz: ImageFont.truetype(MONO, sz)
FB = lambda sz: ImageFont.truetype(MONO_B, sz)
FS = lambda sz: ImageFont.truetype(SANS_B, sz)

PLOT_X, PLOT_Y, PLOT_W, PLOT_H = 120, 110, 820, 420


def main():
    data = json.loads(DATA.read_text(encoding="utf-8"))
    pooled = [b for b in data["pooled"] if b["n"] > 0]

    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((8, 8, W - 9, H - 9), radius=16, fill=PANEL,
                        outline=PANEL_EDGE, width=2)

    d.text((40, 34), "calibration - can you trust the confidence score?",
           font=FS(30), fill=TEXT)
    d.text((40, 76),
           f"observed accuracy per predicted-confidence bucket, pooled over all fields "
           f"({data['rows']}-binary hold-out, seed {data['seed']})",
           font=F(16), fill=DIM)

    # grid: 0-100% both axes, diagonal = perfect calibration
    for pct in range(0, 101, 25):
        x = PLOT_X + PLOT_W * pct / 100
        y = PLOT_Y + PLOT_H - PLOT_H * pct / 100
        d.line((x, PLOT_Y - 10, x, PLOT_Y + PLOT_H + 10), fill=(38, 46, 52), width=1)
        d.line((PLOT_X - 10, y, PLOT_X + PLOT_W + 10, y), fill=(38, 46, 52), width=1)
        d.text((x - 14, PLOT_Y + PLOT_H + 30), f"{pct}%", font=F(14), fill=DIM)
        d.text((PLOT_X - 58, y - 8), f"{pct}%", font=F(14), fill=DIM)
    # diagonal (perfect calibration)
    d.line((PLOT_X, PLOT_Y + PLOT_H, PLOT_X + PLOT_W, PLOT_Y), fill=GRAY, width=2)
    d.text((PLOT_X + 34, PLOT_Y + 38), "perfect calibration", font=F(14), fill=GRAY)

    d.text((PLOT_X + PLOT_W / 2 - 120, PLOT_Y + PLOT_H + 40),
           "predicted confidence", font=FB(17), fill=TEXT)
    d.text((PLOT_X - 84, PLOT_Y + PLOT_H / 2 - 60), "observed", font=FB(17), fill=TEXT)
    d.text((PLOT_X - 84, PLOT_Y + PLOT_H / 2 - 38), "accuracy", font=FB(17), fill=TEXT)

    # bucket columns: light lavender columns for sample volume, dots+line for
    # observed accuracy
    max_n = max(b["n"] for b in pooled)
    col_w = PLOT_W / 10
    for b in pooled:
        cx = PLOT_X + PLOT_W * ((b["bin_lo"] + b["bin_hi"]) / 2)
        col_h = 300 * b["n"] / max_n
        d.rounded_rectangle((cx - col_w / 2 + 8, PLOT_Y + PLOT_H - col_h,
                             cx + col_w / 2 - 8, PLOT_Y + PLOT_H),
                            radius=4, fill=(30, 40, 50))
        if b["n"]:
            d.text((cx - d.textlength(f"n={b['n']}", font=F(13)) / 2, PLOT_Y + PLOT_H - col_h - 20),
                   f"n={b['n']}", font=F(13), fill=DIM)

    pts = []
    for b in pooled:
        if b["accuracy"] is None or b["n"] == 0:
            continue
        cx = PLOT_X + PLOT_W * ((b["bin_lo"] + b["bin_hi"]) / 2)
        cy = PLOT_Y + PLOT_H - PLOT_H * b["accuracy"]
        pts.append((cx, cy))
        d.ellipse((cx - 7, cy - 7, cx + 7, cy + 7), fill=LAVENDER,
                  outline=PANEL, width=2)
    if len(pts) > 1:
        d.line(pts, fill=LAVENDER, width=3)

    # annotation: the highest populated bucket's observed accuracy
    top = max(pooled, key=lambda b: b["n"])
    if top["accuracy"] is not None:
        note = (f"at confidence >= {top['bin_lo']:.1f}: n={top['n']}, "
                f"observed accuracy {top['accuracy']:.4f}")
        d.text((40, H - 46), note, font=FB(17), fill=GREEN)

    img.save(OUT, optimize=True)
    print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KiB, {W}x{H}) from {DATA.name}")


if __name__ == "__main__":
    main()
