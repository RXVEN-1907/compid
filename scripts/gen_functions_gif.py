#!/usr/bin/env python3
"""Generate docs/functions-demo.gif: an animated terminal showing the
mixed-toolchain proof - one binary linked from a gcc object and a zig cc
object, with `compid functions` attributing every function to its own
toolchain. All rows are the genuine output of scripts/mixed_demo.sh with
the committed model.

Deterministic: fixed frame schedule. Re-run to regenerate.

Usage: .venv/bin/python scripts/gen_functions_gif.py
"""
from PIL import Image, ImageDraw, ImageFont
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "docs" / "functions-demo.gif"

W, H = 980, 640
BG = (10, 16, 14)
PANEL = (22, 27, 34)
PANEL_EDGE = (48, 54, 61)
TEXT = (230, 239, 243)
DIM = (139, 148, 158)
LAVENDER = (167, 139, 250)
GREEN = (74, 222, 128)
AMBER = (217, 180, 80)

MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
MONO_B = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"
SANS_B = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
F = lambda sz: ImageFont.truetype(MONO, sz)
FB = lambda sz: ImageFont.truetype(MONO_B, sz)
FS = lambda sz: ImageFont.truetype(SANS_B, sz)

PAD = 30
LH = 28

# (name, addr, size, family, family_conf, opt, opt_conf) - genuine output
ROWS = [
    ("main",             "0x1070",  184, "gcc",   1.00, "O2", 0.58),
    ("_start",           "0x1130",   38, "gcc",   1.00, "O3", 0.74),
    ("bst_insert…prop",  "0x1220",  106, "gcc",   0.99, "O2", 0.45),
    ("walk",             "0x1290",  343, "gcc",   1.00, "O3", 0.99),
    ("height",           "0x13f0", 2075, "gcc",   1.00, "O3", 0.99),
    ("zig_half_main",    "0x1c10",  218, "clang", 1.00, "O2", 0.94),
    ("op_inc",           "0x1cf0",    9, "clang", 1.00, "O2", 0.97),
    ("op_sq",            "0x1d00",   30, "clang", 1.00, "O2", 0.91),
    ("op_fold",          "0x1d20",   15, "clang", 1.00, "O2", 0.97),
    ("op_rev",           "0x1d30",   95, "clang", 1.00, "O2", 0.97),
    ("op_mix",           "0x1d90",   39, "clang", 1.00, "O2", 0.98),
]
HALF = {"main": "gcc", "_start": "gcc", "bst_insert…prop": "gcc",
        "walk": "gcc", "height": "gcc",
        "zig_half_main": "zig", "op_inc": "zig", "op_sq": "zig",
        "op_fold": "zig", "op_rev": "zig", "op_mix": "zig"}
FAMILY_COLOR = {"gcc": (217, 180, 80), "clang": LAVENDER}
ZIG_COLOR = (94, 200, 180)


def span(draw, x, y, parts, font):
    for text, color in parts:
        draw.text((x, y), text, font=font, fill=color)
        x += draw.textlength(text, font=font)
    return x


def frame(draw, visible, conf_anim):
    draw.rounded_rectangle((0, 0, W - 1, H - 1), radius=14, fill=PANEL,
                           outline=PANEL_EDGE, width=2)
    for i, c in enumerate(((255, 95, 86), AMBER, GREEN)):
        draw.ellipse((22 + i * 26, 18, 38 + i * 26, 34), fill=c)
    title = "compid functions mixed-demo"
    tw = draw.textlength(title, font=FS(15))
    draw.text(((W - tw) / 2, 20), title, font=FS(15), fill=DIM)
    draw.line((0, 52, W, 52), fill=PANEL_EDGE, width=1)

    y = 76
    span(draw, PAD, y, [("$ ", GREEN),
                        ("compid functions mixed-demo", TEXT)], F(19))
    y += LH + 6
    span(draw, PAD, y, [("one binary, two toolchains: half linked from gcc, half from zig cc",
                         DIM)], F(15))
    y += LH + 10

    span(draw, PAD, y, [("name", DIM), (" " * 14, DIM), ("addr", DIM),
                        ("  size", DIM), ("      family", DIM),
                        ("        conf", DIM)], F(15))
    y += LH + 2

    for i, (name, addr, size, family, fconf, opt, oconf) in enumerate(ROWS):
        if i >= visible:
            break
        last = (i == visible - 1)
        fc = conf_anim if last else 1.0
        half_col = AMBER if HALF[name] == "gcc" else ZIG_COLOR
        parts = [
            (name[:22].ljust(22), TEXT),
            (f"{addr:>8}  ", DIM),
            (f"{size:>5}  ", DIM),
            (family, FAMILY_COLOR[family]),
        ]
        x = span(draw, PAD, y, parts, F(16))
        bar_w = int(60 * fconf * fc)
        draw.rounded_rectangle((x, y + 1, x + bar_w, y + 15), radius=3,
                               fill=LAVENDER if family == "clang" else AMBER)
        conf_s = f" {fconf:.2f}" if fc >= 1 else f" {fconf * fc:.2f}"
        span(draw, x + 70, y, [(conf_s, DIM)], F(16))
        y += LH

    y += 8
    if visible >= len(ROWS):
        span(draw, PAD, y, [("11 functions; predicted families: ", DIM),
                            ("clang=6", LAVENDER), (", ", DIM), ("gcc=5", AMBER)], F(17))
        y += LH + 4
        span(draw, PAD, y, [("gcc half: 4 functions -> ", DIM), ("gcc=4", AMBER),
                            ("   zig half: 6 -> ", DIM), ("clang=6", LAVENDER)], F(17))
        y += LH + 4
        span(draw, PAD, y, [("MIXED-DEMO SPLIT OK", GREEN)], FB(18))


def build_frames():
    frames = []
    total = len(ROWS)
    steps = [1] * 3                                   # header hold
    steps += list(range(1, total + 1))                # one row per frame
    steps += [total, total]                           # conf settle
    steps += [total + 1, total + 1]                   # tally
    steps += [total + 2] * 10                         # verdict + hold
    for s in steps:
        visible = min(s, total)
        conf_anim = 1.0 if s > total else min(1.0, (s - (total - visible) + 1) / 2.0)
        img = Image.new("RGB", (W, H), BG)
        d = ImageDraw.Draw(img)
        frame(d, min(visible, total), max(conf_anim, 0.999) if visible and s <= total else conf_anim)
        frames.append(img)
    # simplify: rebuild with plain visible-count schedule
    frames = []
    for s in steps:
        img = Image.new("RGB", (W, H), BG)
        d = ImageDraw.Draw(img)
        frame(d, min(s, total), 1.0)
        frames.append(img)
    return frames


def main():
    frames = build_frames()
    durations = [90] * len(frames)
    for i in range(max(0, len(frames) - 10), len(frames)):
        durations[i] = 350
    frames[0].save(OUT, save_all=True, append_images=frames[1:],
                   duration=durations, loop=0, optimize=True)
    print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KiB, {len(frames)} frames, {W}x{H})")


if __name__ == "__main__":
    main()
