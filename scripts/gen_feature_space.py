#!/usr/bin/env python3
"""Generate docs/feature-space.png: every binary in the seed corpus mapped
to 2D with TruncatedSVD over its raw fingerprint vector - computed fully
unsupervised (no labels), then colored by ground truth afterward to show
the structure the classifier feeds on. Two panels: by toolchain, by OS.

Usage: .venv/bin/python scripts/gen_feature_space.py
"""
import json
from pathlib import Path

import numpy as np
from sklearn.decomposition import TruncatedSVD
from sklearn.feature_extraction import DictVectorizer
from sklearn.preprocessing import normalize
from PIL import Image, ImageDraw, ImageFont

REPO = Path(__file__).resolve().parent.parent
FEATURES = REPO / "dataset" / "features" / "train.jsonl"
OUT = REPO / "docs" / "feature-space.png"

W, H = 1160, 560
BG = (10, 16, 14)
PANEL = (22, 27, 34)
PANEL_EDGE = (48, 54, 61)
TEXT = (230, 239, 243)
DIM = (139, 148, 158)
LAVENDER = (167, 139, 250)
AMBER = (217, 180, 80)
TEAL = (94, 200, 180)
ROSE = (240, 130, 150)
GRAY = (122, 136, 148)

MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
MONO_B = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"
SANS_B = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
F = lambda sz: ImageFont.truetype(MONO, sz)
FB = lambda sz: ImageFont.truetype(MONO_B, sz)
FS = lambda sz: ImageFont.truetype(SANS_B, sz)


def load():
    recs = []
    with FEATURES.open(encoding="utf-8") as fh:
        for line in fh:
            rec = json.loads(line)
            recs.append((rec["features"], rec["labels"]))
    return recs


def project(recs):
    vec = DictVectorizer(sparse=True)
    x = vec.fit_transform([r[0] for r in recs])
    x = normalize(x, norm="l2", copy=False)
    svd = TruncatedSVD(n_components=2, random_state=42)
    xy = svd.fit_transform(x)
    return xy, float(svd.explained_variance_ratio_.sum())


def render_panel(d, xy, colors, x0, y0, w, h, title, subtitle):
    d.text((x0, y0 - 46), title, font=FB(19), fill=TEXT)
    d.text((x0, y0 - 22), subtitle, font=F(13), fill=DIM)
    d.rounded_rectangle((x0 - 1, y0 - 1, x0 + w + 1, y0 + h + 1),
                        radius=8, fill=(16, 22, 20), outline=PANEL_EDGE, width=1)
    xs, ys = xy[:, 0], xy[:, 1]
    lo_x, hi_x = xs.min(), xs.max()
    lo_y, hi_y = ys.min(), ys.max()
    span_x = max(hi_x - lo_x, 1e-9)
    span_y = max(hi_y - lo_y, 1e-9)
    for (px, py), color in zip(xy, colors):
        sx = x0 + 10 + (px - lo_x) / span_x * (w - 20)
        sy = y0 + 10 + (1 - (py - lo_y) / span_y) * (h - 20)
        r = 2.4
        d.ellipse((sx - r, sy - r, sx + r, sy + r), fill=color)
    return (lo_x, hi_x, lo_y, hi_y)


def main():
    recs = load()
    xy, evr = project(recs)

    family_colors = {"gcc": AMBER, "clang": LAVENDER}
    os_colors = {"linux": LAVENDER, "macos": TEAL, "windows": ROSE}
    fam_colors = [family_colors[r[1]["compiler_family"]] for r in recs]
    os_col = [os_colors[r[1]["os"]] for r in recs]

    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((8, 8, W - 9, H - 9), radius=16, fill=PANEL,
                        outline=PANEL_EDGE, width=2)
    d.text((40, 30), "the fingerprint space", font=FS(30), fill=TEXT)
    d.text((40, 72),
           f"all {len(recs)} seed binaries projected to 2D by TruncatedSVD over raw fingerprints "
           f"- unsupervised, labels only color the dots",
           font=F(15), fill=DIM)

    pw, ph = 520, 360
    render_panel(d, xy, fam_colors, 60, 150, pw, ph,
                 "colored by compiler family",
                 "amber = gcc 392   lavender = clang 763")
    render_panel(d, xy, os_col, 620, 150, pw, ph,
                 "colored by operating system",
                 "lavender = linux 1057   teal = macos 204   rose = windows 98")

    ly = 560
    d.text((60, ly), f"explained variance (2 components): {evr:.1%}", font=F(15), fill=DIM)
    d.text((60, ly + 24), "clusters this clean are why a linear model reaches the numbers above",
           font=F(15), fill=DIM)

    img.save(OUT, optimize=True)
    print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KiB, {W}x{H}) from {len(recs)} binaries")


if __name__ == "__main__":
    main()
