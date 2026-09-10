#!/usr/bin/env python3
"""Render reverie's ANSI output to images, the way a truecolor terminal would.

Terminal cells are drawn 8x16 px: '▀' = top half fg / bottom half bg (so every
half-block pixel is an 8x8 square), braille = 2x4 dots, other glyphs via DejaVu Sans Mono.
Usage:
  ansi2png.py FILE.ansi OUT.png [--cols C --rows R]          # one frame
  ansi2png.py STREAM.raw OUT.gif --cols C --rows R --every N  # live pty capture -> GIF
"""
import sys, re, argparse
import numpy as np
from PIL import Image, ImageDraw, ImageFont

CW, CH = 8, 16
FONT = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", 13)
CSI = re.compile(rb"\x1b\[([?0-9;]*)([A-Za-z])")

def c256(n):
    if n < 16:
        base = [(0,0,0),(205,0,0),(0,205,0),(205,205,0),(0,0,238),(205,0,205),(0,205,205),(229,229,229),
                (127,127,127),(255,0,0),(0,255,0),(255,255,0),(92,92,255),(255,0,255),(0,255,255),(255,255,255)]
        return base[n]
    if n >= 232:
        v = 8 + (n - 232) * 10; return (v, v, v)
    n -= 16; lv = [0, 95, 135, 175, 215, 255]
    return (lv[n // 36], lv[(n // 6) % 6], lv[n % 6])

class Screen:
    def __init__(s, cols, rows):
        s.cols, s.rows = cols, rows
        s.ch = [[" "] * cols for _ in range(rows)]
        s.fg = np.zeros((rows, cols, 3), np.uint8); s.bg = np.zeros((rows, cols, 3), np.uint8)
        s.x = s.y = 0; s.cfg = (229, 229, 229); s.cbg = (0, 0, 0); s.frames = []
    def sgr(s, params):
        p = [int(x) if x else 0 for x in params.split(";")] if params else [0]
        i = 0
        while i < len(p):
            v = p[i]
            if v == 0: s.cfg, s.cbg = (229, 229, 229), (0, 0, 0)
            elif v in (38, 48) and i + 1 < len(p):
                if p[i+1] == 2: col = tuple(p[i+2:i+5]); i += 4
                else: col = c256(p[i+2]); i += 2
                if v == 38: s.cfg = col
                else: s.cbg = col
            i += 1
    def put(s, ch):
        if 0 <= s.y < s.rows and 0 <= s.x < s.cols:
            s.ch[s.y][s.x] = ch; s.fg[s.y, s.x] = s.cfg; s.bg[s.y, s.x] = s.cbg
        s.x = min(s.x + 1, s.cols - 1) if s.x + 1 < s.cols else s.cols
    def feed(s, data, on_frame=None):
        i, n = 0, len(data)
        while i < n:
            b = data[i]
            if b == 0x1b:
                m = CSI.match(data, i)
                if not m:
                    i += 1; continue
                params, cmd = m.group(1).decode(), m.group(2).decode()
                if cmd == "H":
                    a = [int(x) if x else 1 for x in params.split(";")] if params else [1, 1]
                    s.y, s.x = a[0] - 1, (a[1] - 1 if len(a) > 1 else 0)
                elif cmd == "C": s.x += int(params or 1)
                elif cmd == "m": s.sgr(params)
                elif cmd == "J" and params == "2":
                    s.ch = [[" "] * s.cols for _ in range(s.rows)]; s.fg[:] = 0; s.bg[:] = s.cbg
                elif cmd == "l" and params == "?2026" and on_frame: on_frame(s)
                i = m.end(); continue
            if b < 0x20: i += 1; continue
            l = 1 if b < 0x80 else 2 if b < 0xe0 else 3 if b < 0xf0 else 4
            s.put(data[i:i+l].decode("utf-8", "replace")); i += l
    def image(s):
        img = np.zeros((s.rows * CH, s.cols * CW, 3), np.uint8)
        top = np.repeat(np.repeat(s.fg, CW, 1), CH, 0); bot = np.repeat(np.repeat(s.bg, CW, 1), CH, 0)
        img[:] = bot
        glyphs = []
        for r in range(s.rows):
            for c in range(s.cols):
                ch = s.ch[r][c]
                if ch == "▀":
                    img[r*CH:r*CH+CH//2, c*CW:(c+1)*CW] = s.fg[r, c]
                elif ch != " ":
                    glyphs.append((r, c, ch))
        im = Image.fromarray(img); d = ImageDraw.Draw(im)
        for r, c, ch in glyphs:
            fg = tuple(int(v) for v in s.fg[r, c]); o = ord(ch)
            if 0x2800 <= o <= 0x28ff:
                bits = o - 0x2800
                pos = [(0,0,0x01),(0,1,0x02),(0,2,0x04),(1,0,0x08),(1,1,0x10),(1,2,0x20),(0,3,0x40),(1,3,0x80)]
                for dx, dy, bit in pos:
                    if bits & bit:
                        x0, y0 = c*CW + 1 + dx*4, r*CH + 1 + dy*4
                        d.ellipse([x0, y0, x0 + 2.2, y0 + 2.2], fill=fg)
            else:
                d.text((c*CW, r*CH + 1), ch, font=FONT, fill=fg)
        return im

if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("inp"); ap.add_argument("out")
    ap.add_argument("--cols", type=int, default=120); ap.add_argument("--rows", type=int, default=36)
    ap.add_argument("--every", type=int, default=1); ap.add_argument("--scale", type=float, default=1.0)
    ap.add_argument("--fps", type=float, default=15); ap.add_argument("--max", type=int, default=10**9)
    a = ap.parse_args()
    data = open(a.inp, "rb").read()
    scr = Screen(a.cols, a.rows)
    if a.out.endswith(".gif"):
        frames = []; count = [0]
        def grab(sc):
            count[0] += 1
            if count[0] % a.every == 0 and len(frames) < a.max:
                im = sc.image()
                if a.scale != 1.0: im = im.resize((int(im.width * a.scale), int(im.height * a.scale)), Image.LANCZOS)
                frames.append(im.convert("P", palette=Image.ADAPTIVE, colors=255, dither=Image.NONE))
        scr.feed(data, grab)
        frames[0].save(a.out, save_all=True, append_images=frames[1:], duration=int(1000 / a.fps), loop=0, optimize=False, disposal=1)
        print(f"{a.out}: {len(frames)} frames from {count[0]} terminal frames")
    else:
        scr.feed(data)
        im = scr.image()
        if a.scale != 1.0: im = im.resize((int(im.width * a.scale), int(im.height * a.scale)), Image.LANCZOS)
        im.save(a.out); print(a.out, im.size)
