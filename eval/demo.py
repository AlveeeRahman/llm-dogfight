#!/usr/bin/env python3
"""Record a real reverie session in a pty (timestamped) and render it to MP4.

Everything shown is reverie's actual terminal output replayed through a small
VT emulator — nothing is mocked. Waiting periods are time-compressed and labelled.
"""
import os, sys, time, json, re, tempfile, subprocess
import numpy as np
from PIL import Image, ImageDraw, ImageFont

sys.path.insert(0, os.path.dirname(__file__))
import test_terminal as T
from ansi2png import c256

COLS, ROWS = 110, 32
T.COLS, T.ROWS = COLS, ROWS
CW, CH = 8, 16
MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
SANS = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
FONT = ImageFont.truetype(MONO, 13)

# ------------------------------------------------------------------ record
def record(path):
    tmp = tempfile.mkdtemp()
    os.chdir(tmp)
    env = T.make_env(tmp, "bash", 6)
    cfg = os.path.join(tmp, "cfg", "reverie", "config.toml")
    open(cfg, "w").write('idle_seconds = 6\nscenes = ["meadow", "ufo", "galaxy", "garden"]\nrotate_minutes = 0.16\n'
                         'garden_speed = 60\nday_cycle_seconds = 45\nfps = 30\n')
    sh = T.Shell("bash", env)
    t0 = time.time(); log = []; events = []
    def pump(secs, until=None):
        end = time.time() + secs
        while time.time() < end:
            import select
            r, _, _ = select.select([sh.fd], [], [], 0.01)
            if r:
                try: d = os.read(sh.fd, 1 << 16)
                except OSError: return
                log.append((time.time() - t0, d))
                if until and until in d: return True
    def ev(name): events.append((time.time() - t0, name))
    def type_(s, cps=14):
        for ch in s:
            os.write(sh.fd, ch.encode()); pump(1.0 / cps)
    pump(1.5)
    ev("start")
    type_("ls ~/projects\r"); pump(0.6)
    ev("typing")
    type_("sleep 12   # a long build...\r")
    ev("cmd")
    pump(12.3)
    ev("typing2")
    type_("git commit -m \"wip\"")
    ev("idle")
    pump(30, b"\x1b[?1049h")
    ev("saver")
    pump(44.0)
    ev("key")
    os.write(sh.fd, b"q")
    pump(1.2)
    ev("back")
    pump(1.2)
    os.write(sh.fd, b"\x15")  # (don't actually commit) — clear line with ^U
    pump(0.4)
    type_("reverie list\r"); pump(1.5)
    ev("end")
    sh.close()
    os.system("pkill -f 'reverie watch' 2>/dev/null")
    json.dump({"events": events, "chunks": [(t, d.hex()) for t, d in log]}, open(path, "w"))
    print(f"recorded {len(log)} chunks, {sum(len(d) for _, d in log)/1e6:.1f} MB of terminal output, {events[-1][0]:.1f}s")

# ------------------------------------------------------------------ tiny VT emulator (enough for bash + reverie)
CSI = re.compile(rb"\x1b\[([?0-9;]*)([@A-Za-z])")
OSC = re.compile(rb"\x1b\][^\x07\x1b]*(\x07|\x1b\\)")
DEF_FG, DEF_BG = (222, 222, 222), (23, 23, 23)
BASIC = [(23,23,23),(204,0,0),(78,154,6),(196,160,0),(52,101,164),(117,80,123),(6,152,154),(211,215,207),
         (85,87,83),(239,41,41),(138,226,52),(252,233,79),(114,159,207),(173,127,168),(52,226,226),(238,238,236)]

class VT:
    def __init__(s):
        s.main = s.blank(); s.alt = s.blank(); s.scr = s.main
        s.x = s.y = 0; s.fg, s.bg = DEF_FG, DEF_BG; s.saved = (0, 0); s.in_alt = False; s.wrap = True
        s.buf = b""
    def blank(s):
        return {"ch": [[" "] * COLS for _ in range(ROWS)], "fg": np.full((ROWS, COLS, 3), DEF_FG, np.uint8), "bg": np.full((ROWS, COLS, 3), DEF_BG, np.uint8)}
    def scroll(s):
        sc = s.scr
        sc["ch"].pop(0); sc["ch"].append([" "] * COLS)
        sc["fg"][:-1] = sc["fg"][1:]; sc["fg"][-1] = DEF_FG
        sc["bg"][:-1] = sc["bg"][1:]; sc["bg"][-1] = DEF_BG
    def put(s, ch):
        if s.x >= COLS:
            if not s.wrap: s.x = COLS - 1
            else:
                s.x = 0; s.y += 1
                if s.y >= ROWS: s.scroll(); s.y = ROWS - 1
        sc = s.scr
        sc["ch"][s.y][s.x] = ch; sc["fg"][s.y, s.x] = s.fg; sc["bg"][s.y, s.x] = s.bg
        s.x += 1
    def sgr(s, params):
        p = [int(x) if x else 0 for x in params.split(";")] if params else [0]
        i = 0
        while i < len(p):
            v = p[i]
            if v == 0: s.fg, s.bg = DEF_FG, DEF_BG
            elif 30 <= v <= 37: s.fg = BASIC[v - 30]
            elif 90 <= v <= 97: s.fg = BASIC[v - 82]
            elif 40 <= v <= 47: s.bg = BASIC[v - 40]
            elif v == 39: s.fg = DEF_FG
            elif v == 49: s.bg = DEF_BG
            elif v in (38, 48) and i + 1 < len(p):
                if p[i + 1] == 2: col = tuple(p[i + 2:i + 5]); i += 4
                else: col = c256(p[i + 2]); i += 2
                if v == 38: s.fg = col
                else: s.bg = col
            i += 1
    def feed(s, data):
        data = s.buf + data; s.buf = b""
        i, n = 0, len(data)
        while i < n:
            b = data[i]
            if b == 0x1b:
                m = CSI.match(data, i)
                if m:
                    prm, cmd = m.group(1).decode(), m.group(2).decode(); s.csi(prm, cmd); i = m.end(); continue
                m = OSC.match(data, i)
                if m: i = m.end(); continue
                if i + 1 < n and data[i + 1:i + 2] in (b"(", b")"): i += 3; continue
                if i + 1 >= n or (data[i + 1:i + 2] in (b"[", b"]") ): s.buf = data[i:]; return
                i += 2; continue
            if b == 0x0d: s.x = 0; i += 1; continue
            if b == 0x0a:
                s.y += 1
                if s.y >= ROWS: s.scroll(); s.y = ROWS - 1
                i += 1; continue
            if b == 0x08: s.x = max(0, s.x - 1); i += 1; continue
            if b < 0x20: i += 1; continue
            l = 1 if b < 0x80 else 2 if b < 0xe0 else 3 if b < 0xf0 else 4
            if i + l > n: s.buf = data[i:]; return
            s.put(data[i:i + l].decode("utf-8", "replace")); i += l
    def csi(s, prm, cmd):
        nums = [int(x) if x.isdigit() else 0 for x in prm.lstrip("?").split(";")] if prm else []
        n1 = nums[0] if nums and nums[0] else 1
        sc = s.scr
        if cmd == "H":
            s.y = (nums[0] if nums and nums[0] else 1) - 1; s.x = (nums[1] if len(nums) > 1 and nums[1] else 1) - 1
        elif cmd == "C": s.x = min(COLS, s.x + n1)
        elif cmd == "D": s.x = max(0, s.x - n1)
        elif cmd == "A": s.y = max(0, s.y - n1)
        elif cmd == "B": s.y = min(ROWS - 1, s.y + n1)
        elif cmd == "G": s.x = n1 - 1
        elif cmd == "m": s.sgr(prm)
        elif cmd == "K":
            mode = nums[0] if nums else 0
            rng = range(s.x, COLS) if mode == 0 else range(0, s.x + 1) if mode == 1 else range(COLS)
            for x in rng:
                if x < COLS: sc["ch"][s.y][x] = " "; sc["bg"][s.y, x] = s.bg
        elif cmd == "J":
            mode = nums[0] if nums else 0
            if mode == 2:
                for y in range(ROWS): sc["ch"][y] = [" "] * COLS
                sc["bg"][:] = s.bg
            elif mode == 0:
                s.csi("", "K")
                for y in range(s.y + 1, ROWS): sc["ch"][y] = [" "] * COLS; sc["bg"][y] = s.bg
        elif cmd in "hl" and prm.startswith("?"):
            on = cmd == "h"
            for v in nums:
                if v == 1049:
                    if on and not s.in_alt:
                        s.saved = (s.x, s.y); s.in_alt = True; s.alt = s.blank(); s.scr = s.alt
                    elif not on and s.in_alt:
                        s.in_alt = False; s.scr = s.main; s.x, s.y = s.saved
                elif v == 7: s.wrap = on
    def image(s):
        sc = s.scr
        img = np.repeat(np.repeat(sc["bg"], CW, 1), CH, 0).copy()
        glyphs = []
        for r in range(ROWS):
            row = sc["ch"][r]
            for c in range(COLS):
                ch = row[c]
                if ch == "▀": img[r*CH:r*CH+CH//2, c*CW:(c+1)*CW] = sc["fg"][r, c]
                elif ch != " ": glyphs.append((r, c, ch))
        im = Image.fromarray(img); d = ImageDraw.Draw(im)
        for r, c, ch in glyphs:
            fg = tuple(int(v) for v in sc["fg"][r, c]); o = ord(ch)
            if 0x2800 <= o <= 0x28ff:
                for dx, dy, bit in [(0,0,1),(0,1,2),(0,2,4),(1,0,8),(1,1,16),(1,2,32),(0,3,64),(1,3,128)]:
                    if (o - 0x2800) & bit:
                        x0, y0 = c*CW + 1 + dx*4, r*CH + 1 + dy*4; d.ellipse([x0, y0, x0 + 2.2, y0 + 2.2], fill=fg)
            else: d.text((c*CW, r*CH + 1), ch, font=FONT, fill=fg)
        if not s.in_alt and 0 <= s.y < ROWS and s.x < COLS:  # cursor block
            d.rectangle([s.x*CW, s.y*CH + 1, s.x*CW + CW - 1, s.y*CH + CH - 2], fill=(200, 200, 200))
        return im

# ------------------------------------------------------------------ render
CAPTIONS = [
    ("start", "Ubuntu terminal · bash with `reverie install` · idle_seconds = 6 (default 300)"),
    ("cmd", "A command is running → reverie stays silent (the shell doesn't own the terminal)"),
    ("typing2", "Back at the prompt. Type half a command… then walk away"),
    ("idle", "Prompt idle… the 1.5 MB watcher sends SIGALRM when the tty goes quiet"),
    ("saver", "reverie takes over — scenes rotate with crossfades (demo: 10 s each)"),
    ("key", "Any key → back instantly. The half-typed command is still there, key not leaked"),
    ("back", "Back at the prompt, exactly as you left it"),
]
SPEED = {"cmd": 4.0, "idle": 1.5}   # time compression for waiting phases (labelled in the video)

def render(rec_path, out_mp4, fps=30):
    rec = json.load(open(rec_path))
    events = rec["events"]; chunks = [(t, bytes.fromhex(h)) for t, h in rec["chunks"]]
    names = {n: t for t, n in events}
    segs = []  # (stream_t0, stream_t1, speed)
    bounds = [t for t, _ in events]
    for (ta, na), (tb, _) in zip(events, events[1:]):
        segs.append((ta, tb, SPEED.get(na, 1.0), na))
    # build video->stream time samples
    samples = []
    for ta, tb, sp, na in segs:
        n = int((tb - ta) / sp * fps)
        for k in range(n):
            samples.append((ta + k * sp / fps, na, sp))
    vt = VT(); ci = 0
    title_font = ImageFont.truetype(SANS, 15); cap_font = ImageFont.truetype(SANS, 17); small = ImageFont.truetype(SANS, 13)
    tw, th = COLS * CW, ROWS * CH
    S = 1.5
    W, H = int(tw * S) + 40, int(th * S) + 44 + 20 + 70
    W += W % 2; H += H % 2
    ff = subprocess.Popen(["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(fps),
                           "-i", "-", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", "-preset", "medium", out_mp4], stdin=subprocess.PIPE)
    cap_idx = {n: c for n, c in CAPTIONS}
    current_cap = CAPTIONS[0][1]
    for vi, (st, phase, sp) in enumerate(samples):
        while ci < len(chunks) and chunks[ci][0] <= st:
            vt.feed(chunks[ci][1]); ci += 1
        if phase in cap_idx: current_cap = cap_idx[phase]
        term = vt.image().resize((int(tw * S), int(th * S)), Image.NEAREST)
        frame = Image.new("RGB", (W, H), (14, 16, 20)); d = ImageDraw.Draw(frame)
        # GNOME-style window
        x0, y0 = 20, 20
        d.rounded_rectangle([x0, y0, x0 + term.width, y0 + 44 + term.height], radius=12, fill=(48, 48, 48))
        d.rectangle([x0, y0 + 32, x0 + term.width, y0 + 44], fill=(48, 48, 48))
        title = "demo@ubuntu: ~"
        d.text((x0 + term.width / 2 - d.textlength(title, font=title_font) / 2, y0 + 12), title, font=title_font, fill=(230, 230, 230))
        for k, sym in enumerate(["✕", "□", "–"]):
            cx = x0 + term.width - 22 - k * 34
            d.ellipse([cx - 11, y0 + 11, cx + 11, y0 + 33], fill=(70, 70, 70))
            d.text((cx - d.textlength(sym, font=small) / 2, y0 + 13), sym, font=small, fill=(220, 220, 220))
        frame.paste(term, (x0, y0 + 44))
        # caption + clock
        cy = y0 + 44 + term.height + 18
        d.text((x0 + 4, cy), current_cap, font=cap_font, fill=(235, 235, 235))
        tag = f"{sp:.0f}× speed" if sp > 1 else "real time"
        d.text((x0 + 4, cy + 28), f"real reverie output replayed from a pty · {tag}", font=small, fill=(140, 150, 165))
        ff.stdin.write(frame.tobytes())
        if vi % 300 == 0: print(f"  frame {vi}/{len(samples)}", flush=True)
    ff.stdin.close(); ff.wait()
    print(f"wrote {out_mp4}: {len(samples)} frames, {len(samples)/fps:.1f}s")

if __name__ == "__main__":
    if sys.argv[1] == "record": record(sys.argv[2])
    else: render(sys.argv[2], sys.argv[3])
