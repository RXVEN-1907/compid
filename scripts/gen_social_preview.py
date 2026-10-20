#!/usr/bin/env python3
"""Generate .github/social-preview.png (1280x640) deterministically.

This is the card GitHub renders on the repository page and on every shared
link. Composition:

  - near-black green-tinted ground (the README's dark palette)
  - the fingerprint-of-small-squares motif from docs/compid.svg, rendered
    large across the left two-thirds in lavender opacity steps that
    brighten toward the pipeline arrow
  - the motif flows through the pipeline row into the arrow, which points
    at the wordmark
  - right third: "compid" set large ("id" picked out in lavender) above a
    one-line tagline, "which compiler built this binary?"

The square pattern comes from the same generator and seed as the logo
(scripts/gen_logo.py); the card draws more rings at the same relative
packing so the whorl stays legible at 4x the logo's scale. Everything is
drawn programmatically with Pillow at 4x supersampling and downscaled, so
edges and rounded corners stay clean. Type is DejaVu Sans (searched from a
candidate list; falls back to Pillow's bundled default). Re-run any time:

    python3 scripts/gen_social_preview.py [--thumb PATH]
"""

from __future__ import annotations

import argparse
import math
import random
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

REPO_ROOT = Path(__file__).resolve().parent.parent
OUT_PATH = REPO_ROOT / ".github" / "social-preview.png"

W, H = 1280, 640
SS = 4  # supersample factor

# Near-black ground with the same faint green cast as the README's dark theme.
GROUND = (10, 16, 14)        # #0a100e
LAVENDER = (167, 139, 250)   # #a78bfa
NEUTRAL = (148, 163, 184)    # #94a3b8
INK = (233, 239, 235)        # near-white, same faint green cast

WORDMARK = "compid"
TAGLINE = "which compiler built this binary?"

# Same seed and pattern family as the logo (scripts/gen_logo.py): identical
# wobble, dropout, and accent-band rules. The card draws more rings and
# tightens the packing ratio (pitch/square 1.36 vs the logo's 1.57) so the
# whorl reads as ridges at social-card scale.
SEED = 20261020
CARD_RINGS = 8
CARD_PITCH = 30.0
CARD_SQ = 20.5
CARD_CENTER = (296.0, 320.0)
SKIP_P = 0.085

FONT_CANDIDATES_BOLD = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
    "/usr/local/share/fonts/DejaVuSans-Bold.ttf",
    "/Library/Fonts/DejaVuSans-Bold.ttf",
    "C:/Windows/Fonts/DejaVuSans-Bold.ttf",
    "C:/Windows/Fonts/arialbd.ttf",
]
FONT_CANDIDATES_REGULAR = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/local/share/fonts/DejaVuSans.ttf",
    "/Library/Fonts/DejaVuSans.ttf",
    "C:/Windows/Fonts/DejaVuSans.ttf",
    "C:/Windows/Fonts/arial.ttf",
]

_font_cache = {}


def load_font(px, bold=True):
    key = (round(px), bold)
    if key in _font_cache:
        return _font_cache[key]
    candidates = FONT_CANDIDATES_BOLD if bold else FONT_CANDIDATES_REGULAR
    font = None
    for cand in candidates:
        if Path(cand).is_file():
            try:
                font = ImageFont.truetype(cand, round(px))
                break
            except OSError:
                continue
    if font is None:
        font = ImageFont.load_default(round(px))
    _font_cache[key] = font
    return font


def in_band(ang):
    """Right-facing band of the outer ring that feeds the pipeline arrow."""
    a = ang % (2 * math.pi)
    upper = 2 * math.pi - 1.45 < a < 2 * math.pi - 0.25
    lower = 0.25 < a < 1.45
    return upper or lower


ANGULAR_SLOTS = 120  # shared angular grid so adjacent rings echo radially


def fingerprint(center, pitch, rings, seed=SEED, skip_p=SKIP_P, swirl=0.10):
    """[(x, y, ring)] square centers - mirrors scripts/gen_logo.py.

    Rings share one angular grid (ANGULAR_SLOTS slots); ring k places a
    square every `stride` slots, sized so the along-arc step stays near
    0.82 * pitch. Strides that divide each other line up radially, which
    is what makes the whorl read as concentric ridges.
    """
    cx0, cy0 = center
    rng = random.Random(seed)
    points = [(cx0, cy0, 0)]
    step = 2 * math.pi / ANGULAR_SLOTS
    for k in range(1, rings + 1):
        radius = k * pitch
        stride = max(1, round(pitch * 0.82 / (radius * step)))
        n = ANGULAR_SLOTS // stride
        phase = swirl * k  # gentle swirl; rings stay traceable
        for i in range(n):
            ang = phase + i * stride * step
            if not (k == rings and in_band(ang)) and rng.random() < skip_p:
                continue
            wobble = 0.04 * pitch * math.sin(3 * ang + 1.7 * k)
            r = radius + wobble
            points.append((cx0 + r * math.cos(ang), cy0 + r * math.sin(ang), k))
    return points


def rounded_square(draw, cx, cy, sq, color, alpha, radius_ratio=0.23):
    r = sq * radius_ratio
    half = sq / 2
    a = int(round(255 * alpha))
    draw.rounded_rectangle(
        [(cx - half) * SS, (cy - half) * SS, (cx + half) * SS, (cy + half) * SS],
        radius=r * SS,
        fill=color + (a,),
    )


def fit_px(text, target_w, start_px, min_px=8):
    """Largest font pixel size (at 1x) whose advance width fits target_w.

    Measures with 1x-sized fonts; callers load the draw font at px * SS.
    """
    px = start_px
    while px > min_px and load_font(px, bold=True).getlength(text) > target_w:
        px -= 2
    return px


def fit_px_regular(text, target_w, start_px, min_px=8):
    px = start_px
    while px > min_px and load_font(px, bold=False).getlength(text) > target_w:
        px -= 2
    return px


def draw_card():
    img = Image.new("RGBA", (W * SS, H * SS), GROUND + (255,))
    draw = ImageDraw.Draw(img)

    # ---- fingerprint motif, left two thirds -------------------------------
    pitch = CARD_PITCH
    sq = CARD_SQ
    unit = pitch / 11.0  # logo-unit offsets scale with the ring pitch
    cx0, cy0 = CARD_CENTER
    pts = fingerprint(CARD_CENTER, pitch, rings=CARD_RINGS, swirl=0.0)

    xs = [p[0] for p in pts]
    xmin, xmax = min(xs), max(xs)
    for (x, y, ring) in pts:
        t = ((x - xmin) / (xmax - xmin)) ** 1.15
        alpha = 0.22 + 0.78 * t
        ang = math.atan2(y - cy0, x - cx0)
        if ring == CARD_RINGS and in_band(ang):
            alpha = 1.0  # the strand that hands off to the arrow
        rounded_square(draw, x, y, sq, LAVENDER, alpha)

    # ---- pipeline row and arrow -------------------------------------------
    row_first = cx0 + CARD_RINGS * pitch + 14 * unit
    for i in range(3):
        rounded_square(draw, row_first + i * pitch, cy0, sq, LAVENDER, 1.0)

    back = row_first + 2 * pitch + sq / 2 + 8 * unit
    tip = row_first + 2 * pitch + sq / 2 + 41 * unit
    half = 16 * unit
    stroke = 5 * unit
    tri = [(back * SS, (cy0 - half) * SS), (tip * SS, cy0 * SS),
           (back * SS, (cy0 + half) * SS)]
    draw.polygon(tri, fill=LAVENDER + (255,))
    rad = stroke * SS / 2
    for (vx, vy) in tri:  # round joins, matching the logo's round linejoin
        draw.ellipse([vx - rad, vy - rad, vx + rad, vy + rad],
                     fill=LAVENDER + (255,))
    draw.line([tri[0], tri[1]], fill=LAVENDER + (255,), width=int(stroke * SS))
    draw.line([tri[1], tri[2]], fill=LAVENDER + (255,), width=int(stroke * SS))

    # ---- wordmark block, right third ---------------------------------------
    text_x = 818.0
    avail = (W - 64.0) - text_x

    word_px = fit_px(WORDMARK, avail, 120)
    font_word = load_font(word_px * SS, bold=True)
    baseline = 308.0
    comp_w = font_word.getlength("comp")
    draw.text((text_x * SS, baseline * SS), "comp", font=font_word,
              fill=INK + (255,), anchor="ls")
    draw.text((text_x * SS + comp_w, baseline * SS), "id", font=font_word,
              fill=LAVENDER + (255,), anchor="ls")

    # divider: three motif squares fading in, echoing the pipeline row
    div_sq = 13.0
    div_y = baseline + 40.0
    for i, a in enumerate((0.35, 0.65, 1.0)):
        rounded_square(draw, text_x + div_sq / 2 + i * 22.0, div_y, div_sq,
                       LAVENDER, a)

    tag_px = fit_px_regular(TAGLINE, avail, 34)
    font_tag = load_font(tag_px * SS, bold=False)
    tag_baseline = div_y + div_sq / 2 + 44.0
    draw.text((text_x * SS, tag_baseline * SS), TAGLINE, font=font_tag,
              fill=NEUTRAL + (255,), anchor="ls")

    return img.resize((W, H), Image.LANCZOS).convert("RGB")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--thumb", metavar="PATH",
                        help="also write a 300px-wide thumbnail for review")
    args = parser.parse_args()

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    card = draw_card()
    card.save(OUT_PATH, format="PNG", optimize=True)

    # validate: exact size, decodable, opaque
    with Image.open(OUT_PATH) as check:
        assert check.format == "PNG", "not a PNG"
        assert check.size == (W, H), f"bad size {check.size}"
        check.verify()
    with Image.open(OUT_PATH) as check:
        assert check.mode == "RGB", f"expected opaque RGB, got {check.mode}"

    if args.thumb:
        thumb = card.resize((300, 150), Image.LANCZOS)
        thumb.save(args.thumb, format="PNG")

    print(f"wrote {OUT_PATH.relative_to(REPO_ROOT)} ({W}x{H}, RGB)")
    if args.thumb:
        print(f"wrote thumbnail {args.thumb} (300x150)")


if __name__ == "__main__":
    main()
