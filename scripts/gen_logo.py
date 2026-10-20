#!/usr/bin/env python3
"""Regenerate the compid brand SVGs deterministically.

Writes, relative to the repository root (resolved from this file's location):

  docs/compid.svg       fingerprint-to-pipeline-arrow lockup, 256x160
  docs/compid-icon.svg  square favicon mark, fingerprint only, 128x128

Both files share one fingerprint generator so the mark is identical across
assets: concentric rings of small squares with a fixed dropout pattern
(seeded PRNG), a gentle radial wobble, and a left-to-right opacity ramp that
reads as motion toward the pipeline arrow. Palette is exactly two colors -
#a78bfa lavender and #94a3b8 neutral gray - on a transparent background.

Usage: python3 scripts/gen_logo.py
"""

from __future__ import annotations

import math
import random
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

LAVENDER = "#a78bfa"
NEUTRAL = "#94a3b8"

# One seed drives every asset so the fingerprint pattern matches everywhere
# (scripts/gen_social_preview.py scales this same pattern for the social card).
SEED = 20261020
RINGS = 5
SKIP_P = 0.085          # per-square dropout probability on each ring
CORNER_RATIO = 0.23     # square corner radius as a fraction of square size


ANGULAR_SLOTS = 120  # shared angular grid so adjacent rings echo radially


def fingerprint(center, pitch, rings=RINGS, seed=SEED, skip_p=SKIP_P,
                swirl=0.10):
    """Return [(x, y, ring)] square-center points for the fingerprint mark.

    Deterministic: the dropout pattern depends only on `seed`, the ring
    stagger is a fixed swirl per ring, and the wobble is a fixed sinusoid.
    Rings share one angular grid (ANGULAR_SLOTS slots); ring k places a
    square every `stride` slots, sized so the along-arc step stays near
    0.82 * pitch. Strides that divide each other line up radially, which
    is what makes the whorl read as concentric ridges.
    """
    cx0, cy0 = center
    rng = random.Random(seed)

    def in_band(ang):
        """Right-facing band that feeds the pipeline arrow (y grows downward,
        so upper-right is just below 2*pi)."""
        a = ang % (2 * math.pi)
        upper = 2 * math.pi - 1.45 < a < 2 * math.pi - 0.25
        lower = 0.25 < a < 1.45
        return upper or lower

    points = [(cx0, cy0, 0)]
    step = 2 * math.pi / ANGULAR_SLOTS
    for k in range(1, rings + 1):
        radius = k * pitch
        stride = max(1, round(pitch * 0.82 / (radius * step)))
        n = ANGULAR_SLOTS // stride
        phase = swirl * k  # gentle swirl; rings stay traceable
        for i in range(n):
            ang = phase + i * stride * step
            # Never drop squares in the accent band: the lavender strands
            # must stay continuous to carry the eye to the arrow.
            if not (k == rings and in_band(ang)) and rng.random() < skip_p:
                continue
            wobble = 0.04 * pitch * math.sin(3 * ang + 1.7 * k)
            r = radius + wobble
            points.append((cx0 + r * math.cos(ang), cy0 + r * math.sin(ang), k))
    return points


def x_ramp(points, lo, hi, gamma=1.15):
    """Per-point opacity ramp: dimmer on the left, full strength to the right."""
    xs = [p[0] for p in points]
    xmin, xmax = min(xs), max(xs)
    out = []
    for (x, y, ring) in points:
        t = (x - xmin) / (xmax - xmin)
        t = t ** gamma
        out.append((x, y, ring, lo + (hi - lo) * t))
    return out


def is_accent(x, y, ring, rings):
    """True for the outer ring's right-facing arcs: the lavender strands
    that pick up the pipeline arrow."""
    if ring != rings:
        return False
    ang = math.atan2(y, x) % (2 * math.pi)
    upper = 2 * math.pi - 1.45 < ang < 2 * math.pi - 0.25
    lower = 0.25 < ang < 1.45
    return upper or lower


def rect(cx, cy, sq, fill, alpha, rx):
    x = cx - sq / 2
    y = cy - sq / 2
    return (f'<rect x="{x:.2f}" y="{y:.2f}" width="{sq:g}" height="{sq:g}" '
            f'rx="{rx:g}" fill="{fill}" fill-opacity="{alpha:.2f}"/>')


def svg_lockup(path):
    """docs/compid.svg: fingerprint + pipeline row + arrow, 256x160."""
    cx0, cy0 = 86.0, 80.0
    pitch, sq = 11.0, 7.0
    rx = round(sq * CORNER_RATIO, 2)
    rings = RINGS

    pts = fingerprint((cx0, cy0), pitch, rings=rings)
    pts = x_ramp(pts, 0.55, 1.0)

    parts = []
    parts.append('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 160" '
                 'width="512" height="320" role="img">')
    parts.append("  <title>compid</title>")
    parts.append("  <desc>A fingerprint drawn with tiny squares, brightening "
                 "toward a compiler-pipeline arrow.</desc>")
    for (x, y, ring, alpha) in pts:
        if is_accent(x - cx0, y - cy0, ring, rings):
            parts.append("  " + rect(x, y, sq, LAVENDER, alpha, rx))
        else:
            parts.append("  " + rect(x, y, sq, NEUTRAL, alpha, rx))

    # Pipeline row: three squares carrying the fingerprint into the arrow.
    row_x = cx0 + rings * pitch + 14
    for i in range(3):
        parts.append("  " + rect(row_x + i * pitch, cy0, sq, LAVENDER, 1.0, rx))

    tip_x = row_x + 2 * pitch + sq / 2 + 41
    half = 16
    back = row_x + 2 * pitch + sq / 2 + 8
    parts.append(f'  <path d="M{back:g} {cy0 - half:g} L{tip_x:g} {cy0:g} '
                 f'L{back:g} {cy0 + half:g} Z" fill="{LAVENDER}" '
                 f'stroke="{LAVENDER}" stroke-width="5" stroke-linejoin="round"/>')
    parts.append("</svg>")
    path.write_text("\n".join(parts) + "\n", encoding="utf-8")
    return len(pts) + 3 + 1


def svg_icon(path):
    """docs/compid-icon.svg: the fingerprint mark alone, square, 128x128."""
    cx0, cy0 = 64.0, 64.0
    pitch, sq = 10.5, 7.0
    rx = round(sq * CORNER_RATIO, 2)
    rings = RINGS

    pts = fingerprint((cx0, cy0), pitch, rings=rings)
    pts = x_ramp(pts, 0.55, 1.0)

    parts = []
    parts.append('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" '
                 'width="128" height="128" role="img">')
    parts.append("  <title>compid mark</title>")
    parts.append("  <desc>The compid mark: a fingerprint drawn with tiny "
                 "squares in lavender and neutral gray.</desc>")
    for (x, y, ring, alpha) in pts:
        if is_accent(x - cx0, y - cy0, ring, rings):
            parts.append("  " + rect(x, y, sq, LAVENDER, 0.82 + 0.18 * (alpha - 0.35) / 0.45, rx))
        else:
            parts.append("  " + rect(x, y, sq, NEUTRAL, alpha, rx))
    parts.append("</svg>")
    path.write_text("\n".join(parts) + "\n", encoding="utf-8")
    return len(pts)


def main():
    lockup = svg_lockup(REPO_ROOT / "docs" / "compid.svg")
    icon = svg_icon(REPO_ROOT / "docs" / "compid-icon.svg")
    print(f"wrote docs/compid.svg ({lockup} squares + arrow)")
    print(f"wrote docs/compid-icon.svg ({icon} squares)")


if __name__ == "__main__":
    main()
