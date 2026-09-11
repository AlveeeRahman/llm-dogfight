#!/usr/bin/env python3
"""Render docs/assets/social-preview.png (1280x640, the size GitHub asks for).

The editable source of the social preview is this script: it lays the real battle
screenshot under a dark gradient and sets the title over it, so the card shows the
game rather than a logo. Type is deliberately oversized because link previews show
the image at a fraction of its width. Re-run after replacing eval/snapshots/battle.png.

    python3 eval/social_preview.py            # writes docs/assets/social-preview.png
"""

from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
SHOT = ROOT / "eval" / "snapshots" / "battle.png"
OUT = ROOT / "docs" / "assets" / "social-preview.png"
W, H = 1280, 640
FONTS = [
    "/usr/share/fonts/truetype/noto/NotoSans-{w}.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans{d}.ttf",
    "/System/Library/Fonts/Supplemental/Arial{a}.ttf",
]


def font(size: int, bold: bool) -> ImageFont.FreeTypeFont:
    for pat in FONTS:
        path = pat.format(w="Bold" if bold else "Regular", d="-Bold" if bold else "", a=" Bold" if bold else "")
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    return ImageFont.load_default(size)  # type: ignore[return-value]


def main() -> int:
    shot = Image.open(SHOT).convert("RGB")
    # Cover the canvas with the screenshot, cropped from the centre, then dim it.
    scale = max(W / shot.width, H / shot.height)
    shot = shot.resize((round(shot.width * scale), round(shot.height * scale)), Image.LANCZOS)
    left, top = (shot.width - W) // 2, (shot.height - H) // 2
    bg = shot.crop((left, top, left + W, top + H)).filter(ImageFilter.GaussianBlur(1.2))
    shade = Image.new("L", (W, H), 0)
    sd = ImageDraw.Draw(shade)
    for x in range(W):  # darker on the left where the text sits, the battle shows through on the right
        sd.line([(x, 0), (x, H)], fill=int(215 - 95 * (x / W)))
    bg = Image.composite(Image.new("RGB", (W, H), (7, 9, 20)), bg, shade)
    d = ImageDraw.Draw(bg)
    d.rectangle([0, 0, W, 10], fill=(60, 240, 216))

    x = 84
    d.text((x, 150), "LLM Dogfight", font=font(88, True), fill=(248, 250, 252))
    d.text((x, 268), "A terminal UFO dogfight", font=font(42, True), fill=(60, 240, 216))
    d.text((x, 322), "between two local LLMs", font=font(42, True), fill=(60, 240, 216))
    d.text((x, 412), "CUDA · Apple MLX · CPU  ·  23 open models  ·  learns from every loss", font=font(27, False), fill=(190, 198, 222))
    d.text((x, 455), "0.8 MB Rust binary  ·  fits an 8 GB GPU  ·  MIT", font=font(27, False), fill=(190, 198, 222))
    d.text((x, 546), "github.com/AlveeeRahman/llm-dogfight", font=font(25, False), fill=(140, 150, 180))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    bg.save(OUT, optimize=True)
    print(f"wrote {OUT.relative_to(ROOT)} {bg.size[0]}x{bg.size[1]} {OUT.stat().st_size // 1024} KB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
