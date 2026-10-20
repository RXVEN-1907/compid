#!/usr/bin/env python3
"""Generate docs/accuracy.png: held-out top-1 accuracy per field as
horizontal lavender bars on the brand's dark ground. The numbers mirror
docs/EVALUATION.md and the README table - refresh all three together
after a retrain.

Usage: .venv/bin/python scripts/gen_accuracy_chart.py
"""
from PIL import Image, ImageDraw, ImageFont
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "docs" / "accuracy.png"

W, H = 1080, 780
BG = (10, 16, 14)
PANEL = (22, 27, 34)
PANEL_EDGE = (48, 54, 61)
TEXT = (230, 239, 243)
DIM = (139, 148, 158)
LAVENDER = (167, 139, 250)
LAVENDER_DIM = (122, 102, 180)
GREEN = (74, 222, 128)

# (field, classes, top1, top3) - keep in sync with docs/EVALUATION.md
DATA = [
    ("compiler_family", 2, 1.0000, 1.0000),
    ("compiler_version", 2, 1.0000, 1.0000),
    ("arch", 4, 1.0000, 1.0000),
    ("os", 3, 0.9964, 1.0000),
    ("pie", 3, 0.9928, 1.0000),
    ("static", 3, 0.9890, 1.0000),
    ("abi", 3, 0.9568, 1.0000),
    ("lto", 2, 0.8273, 1.0000),
    ("hardening", 2, 0.9281, 1.0000),
    ("opt_level", 6, 0.8561, 0.9964),
]

MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
MONO_B = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"
SANS_B = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
F = lambda sz: ImageFont.truetype(MONO, sz)
FB = lambda sz: ImageFont.truetype(MONO_B, sz)
FS = lambda sz: ImageFont.truetype(SANS_B, sz)

BAR_Y0 = 130
BAR_H = 40
BAR_GAP = 14
LABEL_X = 40
BAR_X = 320
BAR_W = 620


def main():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((8, 8, W - 9, H - 9), radius=16, fill=PANEL,
                        outline=PANEL_EDGE, width=2)

    d.text((40, 34), "held-out accuracy per field", font=FS(30), fill=TEXT)
    d.text((40, 76), "top-1 (bar) and top-3 (tick) - 1389-build seed, stratified split, seed 42",
           font=F(16), fill=DIM)

    # gridlines at 0/25/50/75/100 %
    for pct in (0, 25, 50, 75, 100):
        x = BAR_X + BAR_W * pct / 100
        d.line((x, BAR_Y0 - 14, x, H - 100), fill=(38, 46, 52), width=1)
        lw = d.textlength(f"{pct}%", font=F(14))
        d.text((x - lw / 2, H - 90), f"{pct}%", font=F(14), fill=DIM)

    y = BAR_Y0
    for name, classes, top1, top3 in DATA:
        d.text((LABEL_X, y + 8), name, font=FB(19), fill=TEXT)
        # top-3 marker: small green tick where top3 lands
        x3 = BAR_X + BAR_W * top3
        d.rounded_rectangle((BAR_X, y, BAR_X + BAR_W * top1, y + BAR_H),
                            radius=6, fill=LAVENDER if top1 > 0.99 else LAVENDER_DIM)
        d.rounded_rectangle((x3 - 3, y - 4, x3 + 3, y + BAR_H + 4),
                            radius=2, fill=GREEN)
        val = f"{top1:.4f}"
        vw = d.textlength(val, font=F(18))
        d.text((BAR_X + BAR_W * top1 + 12, y + 8), val,
               font=F(18), fill=TEXT if top1 > 0.99 else DIM)
        y += BAR_H + BAR_GAP

    # legend
    ly = H - 56
    d.rounded_rectangle((BAR_X, ly, BAR_X + 26, ly + 14), radius=4, fill=LAVENDER)
    d.text((BAR_X + 34, ly - 3), "top-1", font=F(15), fill=TEXT)
    d.rounded_rectangle((BAR_X + 110, ly - 2, BAR_X + 116, ly + 16), fill=GREEN)
    d.text((BAR_X + 126, ly - 3), "top-3", font=F(15), fill=TEXT)
    d.text((BAR_X + 210, ly - 3),
           "lto / hardening: skewed axes, class-weighted fit", font=F(15), fill=DIM)

    img.save(OUT, optimize=True)
    print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KiB, {W}x{H})")


if __name__ == "__main__":
    main()
