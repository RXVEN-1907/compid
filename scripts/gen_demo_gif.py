#!/usr/bin/env python3
"""Generate docs/demo.gif: an animated terminal window showing a real
`compid classify` run (the numbers are the genuine model output for the
seed binary atoi_manual-gcc13-x86_64-linux-gnu-O0-nopie-nolto-dyn).

Deterministic: fixed frame schedule, no randomness. Re-run to regenerate
byte-identical output (Pillow quantization of palette frames is stable).

Usage: .venv/bin/python scripts/gen_demo_gif.py
"""
from PIL import Image, ImageDraw, ImageFont
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "docs" / "demo.gif"

W, H = 880, 880
BG = (10, 16, 14)          # brand near-black green tint
PANEL = (22, 27, 34)       # terminal panel
PANEL_EDGE = (48, 54, 61)
TEXT = (230, 239, 243)
DIM = (139, 148, 158)
LAVENDER = (167, 139, 250)
GREEN = (74, 222, 128)
RED = (255, 95, 86)
AMBER = (255, 189, 46)
GREEN_DOT = (39, 201, 63)
AMBER_DOT = (255, 189, 46)
RED_DOT = (255, 95, 86)

MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
MONO_B = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"
SANS_B = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
F = lambda sz: ImageFont.truetype(MONO, sz)
FB = lambda sz: ImageFont.truetype(MONO_B, sz)
FS = lambda sz: ImageFont.truetype(SANS_B, sz)

PAD = 30
BAR_X = 430          # bar start column
BAR_MAX = 330        # bar full width
LH = 27              # line height


def span(draw, x, y, parts, font):
    """Draw (text, color) runs left to right; return end x."""
    for text, color in parts:
        draw.text((x, y), text, font=font, fill=color)
        x += draw.textlength(text, font=font)
    return x


def bar(draw, x, y, frac, colored=True):
    w = int(BAR_MAX * frac)
    color = LAVENDER if colored else (0, 0, 0, 0)
    if colored:
        draw.rounded_rectangle((x, y + 2, x + w, y + 18), radius=4, fill=color)
    return w


def field_block(draw, y, name, rows, reveal, low=False):
    """rows: [(class, pct, rank)]; reveal 0..1 animates the top bar."""
    title_parts = [(name, TEXT)]
    if low:
        title_parts.append(("  [low confidence]", (217, 180, 80)))
    span(draw, PAD + 24, y, title_parts, FB(19))
    y += LH + 4
    for rank, (cls, pct, frac) in enumerate(rows):
        color = TEXT if rank == 0 else DIM
        prefix = f"  {rank}. "
        span(draw, PAD + 24, y, [(prefix, DIM)], F(17))
        x = PAD + 24 + draw.textlength(prefix, font=F(17))
        span(draw, x, y, [(cls, GREEN if rank == 0 else color)], F(17))
        x += draw.textlength(cls.ljust(12), font=F(17))
        pct_s = f"{pct:6.2f}%"
        span(draw, x, y, [(pct_s, color)], F(17))
        f = frac if rank == 0 else frac * (pct / 100.0)
        w = bar(draw, BAR_X, y, min(1.0, f * reveal), colored=True)
        if rank == 0 and reveal < 1.0 and w > 4:
            draw.rounded_rectangle((BAR_X + w, y + 2, BAR_X + w + 12, y + 18),
                                   radius=4, fill=LAVENDER)
        y += LH
    return y + 10


def frame(draw, step):
    """step counts through the animation timeline."""
    # terminal chrome
    draw.rounded_rectangle((0, 0, W - 1, H - 1), radius=14, fill=PANEL,
                           outline=PANEL_EDGE, width=2)
    for i, c in enumerate((RED_DOT, AMBER_DOT, GREEN_DOT)):
        draw.ellipse((22 + i * 26, 18, 38 + i * 26, 34), fill=c)
    title = "compid - classify"
    tw = draw.textlength(title, font=FS(15))
    draw.text(((W - tw) / 2, 20), title, font=FS(15), fill=DIM)
    draw.line((0, 52, W, 52), fill=PANEL_EDGE, width=1)

    y = 76
    prompt = "compid classify /tmp/sample"
    if step >= 0:
        shown = prompt[:max(0, min(len(prompt), step - 0))]
        span(draw, PAD, y, [("$ ", GREEN), (shown, TEXT)], F(19))
        if step <= len(prompt):
            cx = PAD + draw.textlength("$ ", font=F(19)) + draw.textlength(shown, font=F(19))
            draw.rounded_rectangle((cx + 2, y - 2, cx + 14, y + 22), fill=TEXT)
    y += LH + 8

    header = [
        ("binary: /tmp/sample", TEXT),
        ("model:   model/model.json (trained 2026-10-20, 68352 features)", DIM),
        ("build-id: d1294f531713bb3802b7f5a57fe5c1e6ab895e3f", DIM),
    ]
    hdr_reveal = max(0, step - len(prompt) - 1)
    for i, (text, color) in enumerate(header):
        if hdr_reveal > i:
            span(draw, PAD, y, [(text, color)], F(17 if i == 0 else 16))
            y += LH
    if hdr_reveal <= 2:
        if step > len(prompt) and hdr_reveal >= 0 and step % 2 == 0:
            draw.rounded_rectangle((PAD + 2, y - 2, PAD + 14, y + 22), fill=TEXT)
        return

    fields = [
        ("compiler_family", [("gcc", 99.90, 1.0), ("clang", 0.10, 1.0)]),
        ("compiler_version", [("13.3.0", 99.91, 1.0), ("19.1.7", 0.09, 1.0)]),
        ("opt_level", [("O0", 87.35, 0.874), ("O2", 6.71, 1.0), ("Os", 3.33, 1.0)]),
        ("os", [("linux", 98.57, 0.986), ("macos", 1.18, 1.0), ("windows", 0.26, 1.0)]),
        ("pie", [("no-pie", 75.62, 0.756), ("pie", 23.82, 1.0)]),
        ("static", [("no", 96.87, 0.969), ("n/a", 1.99, 1.0), ("yes", 1.14, 1.0)]),
    ]
    fr = 1.0
    for i, (name, rows) in enumerate(fields):
        block_reveal = max(0.0, min(1.0, (step - 22 - i * 3) / 2.0))
        if block_reveal <= 0:
            break
        y = field_block(draw, y, name, rows, block_reveal)


def build_frames():
    frames = []
    # typing the prompt: 1 char per frame
    prompt_len = len("$ compid classify /tmp/sample")
    steps = list(range(0, prompt_len + 1))
    steps += [prompt_len + 1, prompt_len + 2, prompt_len + 3]     # header reveal
    steps += list(range(prompt_len + 4, prompt_len + 4 + 3 * 6 + 2))  # blocks
    steps += [steps[-1]] * 8                                       # hold
    for s in steps:
        img = Image.new("RGB", (W, H), BG)
        d = ImageDraw.Draw(img)
        frame(d, s)
        frames.append(img)
    return frames


def main():
    frames = build_frames()
    durations = []
    for i in range(len(frames)):
        if i < 3:
            durations.append(60)
        elif i >= len(frames) - 8:
            durations.append(400)          # long hold at the end
        else:
            durations.append(90)
    frames[0].save(OUT, save_all=True, append_images=frames[1:],
                   duration=durations, loop=0, optimize=True)
    print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KiB, {len(frames)} frames, {W}x{H})")


if __name__ == "__main__":
    main()
