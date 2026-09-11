#!/usr/bin/env python3
"""dogfight arena: two small language models command the two saucer teams.

Spawned by `dogfight` when `ufo_pilots = "lm"` (or `dogfight arena`). Backends:
  cuda  torch + transformers, weights in fp16/bf16 (or 8bit/4bit via bitsandbytes)
  mlx   mlx-lm on Apple silicon (unified memory; no VRAM cap needed)
  auto  mlx if importable on macOS/arm64, else cuda

Line protocol on stdin/stdout (stdout is reserved for it; everything else goes to stderr,
which dogfight redirects to ~/.local/state/dogfight/arena.log):

  dogfight -> arena   {"t":"obs","team":0,"tick":7,...}   the situation for one team, one JSON line
                     {"t":"loss","kind":"ship"|"round",...} a saucer (or the whole fleet) was lost: write a lesson
                     {"t":"quit"}
  arena -> dogfight   STATUS <text>                        progress (loading, ...)
                     READY <team> <label> <mem_gb>        that team's commander is online
                     ORDERS <team> <tick> S0:attack:3 S1:flee ... | <battle cry, only after a loss>
                     LESSON <team> <text>                 what the commander learned from a loss
                     ERROR <text>                         fatal; the arena exits

Evolve mode (--evolve): after each loss the losing commander gets the post-mortem and writes one
lesson; its lessons are kept in --memory/<TEAM>-<model>.txt and prepended to every later prompt,
so the commanders change their play while the match runs (and across matches).

  arena.py --model-a ID --model-b ID [--backend auto|cuda|mlx] [--vram-gb 6] [--quant none|8bit|4bit]
           [--evolve|--no-evolve] [--memory DIR]
  arena.py ... --check [--load]     environment report (`dogfight arena check`)
  arena.py ... --pull               download both models (`dogfight arena pull`)
"""
import argparse
import json
import os
import platform
import re
import sys
import time

TEAM_NAMES = ["ZORB", "KRELL"]
# the optional argument must stay on the same line, or "flee\nS2: ..." would eat "S2"
ORDER_RE = re.compile(r"S\s*(\d+)\s*[:\-=]\s*(attack|hunt|flee|abduct|guard|patrol|idle|wait|hold)\b(?:[ \t]*[ESC]?[ \t]*(\d+))?", re.I)
SAY_RE = re.compile(r"SAY\s*[:\-=]\s*(.+)", re.I)
SAY_OK = re.compile(r"[^A-Za-z0-9 .,!?'\-:;()]")
LESSON_RE = re.compile(r"LESSON\s*[:\-=]\s*(.+)", re.I)
TEMPLATE_ECHO = re.compile(r"battle cry|defiant|own words|characters|<|>", re.I)
STOPWORDS = {"always", "never", "prioritize", "prioritise", "the", "a", "an", "to", "of", "and", "or", "over", "when", "is",
             "are", "in", "on", "with", "for", "your", "our", "them", "they", "it", "should", "must", "instead", "rather", "than"}
MAX_LESSONS = 8
# lessons must speak the game's language; anything about verbs that don't exist is noise
BAD_LESSON = re.compile(r"\b(guard|patrol|warp|repair|heal|shield|reload|hide|deploy)\w*", re.I)


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, file=sys.stderr, flush=True)


def emit(line):
    sys.stdout.write(line.replace("\n", " ") + "\n")
    sys.stdout.flush()


def split_rev(model_id):
    """`org/name@revision` -> (org/name, revision). Pin a commit for reproducible weights."""
    mid, _, rev = model_id.partition("@")
    return mid, (rev or "main")


def label_of(model_id):
    return split_rev(model_id)[0].rstrip("/").split("/")[-1][:24]


# ------------------------------------------------------------------ backends

def chat_text(tok, messages):
    """Render the chat with the model's template. Templates without a system role (h2o-danube,
    gemma-2 style) raise; fold the system prompt into the first user turn and try again."""
    kw = dict(tokenize=False, add_generation_prompt=True, enable_thinking=False)
    try:
        return tok.apply_chat_template(messages, **kw)
    except Exception:  # noqa: BLE001 - jinja2.TemplateError, ValueError, ... depending on the template
        system = " ".join(m["content"] for m in messages if m["role"] == "system")
        rest = [dict(m) for m in messages if m["role"] != "system"]
        if rest and system:
            rest[0]["content"] = system + "\n\n" + rest[0]["content"]
        return tok.apply_chat_template(rest, **kw)


class Commander:
    def __init__(self, model_id, model, tok):
        self.model_id, self.model, self.tok = model_id, model, tok
        self.label = label_of(model_id)


class TorchBackend:
    name = "cuda"

    def __init__(self, vram_gb, quant, cpu=False):
        import torch
        self.torch = torch
        if cpu:
            # no GPU: both models on the CPU in float32 (small models only; a few tokens per
            # second on a laptop, enough for one order per saucer every few seconds)
            self.name = "cpu"
            self.ngpu = 0
            self.devs = [torch.device("cpu"), torch.device("cpu")]
            self.dev = self.devs[0]
            self.total_gb = 0.0
            self.cap_gb = 0.0
            self.dtype = torch.float32
            self.quant = "none"
            self._slot = 0
            torch.set_num_threads(max(1, os.cpu_count() or 1))
            return
        if not torch.cuda.is_available():
            if sys.platform == "darwin":
                raise RuntimeError("no CUDA on macOS: use `dogfight run mlx ufo-battle` (pip install mlx-lm)")
            raise RuntimeError("torch has no CUDA device (install a CUDA build of torch: https://pytorch.org/get-started/locally/)")
        # two GPUs: one commander per card; otherwise both share cuda:0
        self.ngpu = torch.cuda.device_count()
        self.devs = [torch.device("cuda:0"), torch.device("cuda:1" if self.ngpu >= 2 else "cuda:0")]
        self.dev = self.devs[0]
        total = torch.cuda.get_device_properties(0).total_memory
        self.total_gb = total / 2**30
        # Hard cap per device: the caching allocator refuses to grow past the budget instead of
        # spilling into memory the desktop or another program needs. 0 = auto: the card
        # minus 2 GB, so an 8 GB card gets 6 GB and a 24 GB card gets 22 GB.
        self.cap_gb = vram_gb if vram_gb > 0 else max(4.0, self.total_gb - 2.0)
        for i in range(min(self.ngpu, 2)):
            t = torch.cuda.get_device_properties(i).total_memory
            torch.cuda.set_per_process_memory_fraction(min(1.0, self.cap_gb * 2**30 / t), i)
        self._slot = 0
        self.dtype = torch.bfloat16 if torch.cuda.is_bf16_supported() else torch.float16
        self.quant = quant

    def load(self, model_id):
        from transformers import AutoModelForCausalLM, AutoTokenizer
        dev = self.devs[self._slot % 2]
        self._slot += 1
        model_id, rev = split_rev(model_id)
        tok = AutoTokenizer.from_pretrained(model_id, revision=rev)
        kw = {}
        if self.quant in ("8bit", "4bit"):
            from transformers import BitsAndBytesConfig  # needs `pip install bitsandbytes`
            kw["quantization_config"] = BitsAndBytesConfig(load_in_8bit=(self.quant == "8bit"), load_in_4bit=(self.quant == "4bit"),
                                                           bnb_4bit_compute_dtype=self.dtype)
            kw["device_map"] = {"": dev.index or 0}
        try:
            model = AutoModelForCausalLM.from_pretrained(model_id, revision=rev, dtype=self.dtype, **kw)
        except TypeError:  # transformers < 5 spelling
            model = AutoModelForCausalLM.from_pretrained(model_id, revision=rev, torch_dtype=self.dtype, **kw)
        if "device_map" not in kw:
            model.to(dev)
        model.eval()
        c = Commander(model_id, model, tok)
        c.dev = dev
        return c

    def generate(self, c, messages, max_new):
        torch = self.torch
        text = chat_text(c.tok, messages)
        ids = c.tok(text, return_tensors="pt").to(getattr(c, "dev", self.dev))
        pad = c.tok.pad_token_id if c.tok.pad_token_id is not None else c.tok.eos_token_id
        with torch.inference_mode():
            out = c.model.generate(**ids, max_new_tokens=max_new, do_sample=True, temperature=0.7, top_p=0.9, top_k=40,
                                   pad_token_id=pad)
        n_in = ids["input_ids"].shape[1]
        return c.tok.decode(out[0, n_in:], skip_special_tokens=True), int(out.shape[1] - n_in)

    def mem_gb(self):
        if self.ngpu == 0:
            return 0.0
        return sum(self.torch.cuda.max_memory_reserved(i) for i in range(min(self.ngpu, 2))) / 2**30

    def device_desc(self):
        if self.ngpu == 0:
            return f"CPU, {os.cpu_count()} threads, float32"
        p = self.torch.cuda.get_device_properties(0)
        gpus = f"{p.name} x{self.ngpu} (one model per GPU)" if self.ngpu >= 2 else p.name
        return f"{gpus}, {self.total_gb:.1f} GB, cap {self.cap_gb:.1f} GB per GPU, dtype {str(self.dtype).split('.')[-1]}"


class MlxBackend:
    name = "mlx"

    def __init__(self, vram_gb, quant):
        import mlx.core as mx  # noqa: F401  (pip install mlx-lm)
        from mlx_lm import generate, load
        from mlx_lm.sample_utils import make_sampler
        self._load, self._generate, self._sampler = load, generate, make_sampler(temp=0.7, top_p=0.9)
        self.mx = mx
        if quant != "none":
            log("mlx: --quant is ignored; pick a pre-quantized repo such as mlx-community/Qwen3-0.6B-4bit")

    def load(self, model_id):
        model, tok = self._load(split_rev(model_id)[0])  # mlx-lm resolves the repo itself; no revision pinning
        return Commander(model_id, model, tok)

    def generate(self, c, messages, max_new):
        text = chat_text(c.tok, messages)
        out = self._generate(c.model, c.tok, prompt=text, max_tokens=max_new, sampler=self._sampler, verbose=False)
        return out, len(c.tok.encode(out))

    def mem_gb(self):
        mx = self.mx
        get = getattr(mx, "get_peak_memory", None) or getattr(getattr(mx, "metal", None), "get_peak_memory", None)
        return get() / 2**30 if get else 0.0

    def device_desc(self):
        return f"Apple silicon via MLX ({platform.machine()})"


def oom_hint(e):
    """One line for the HUD/log: known failures get an actionable hint, anything else its first line."""
    msg = str(e).splitlines()[0] if str(e) else repr(e)
    low = msg.lower()
    if "out of memory" in low:
        return "GPU memory cap reached: this pair does not fit; raise lm_vram_gb (or set it to \"auto\"), use lm_quant = \"8bit\", or pick a smaller model"
    if "429" in msg or "too many requests" in low or "rate limit" in low:
        return "Hugging Face is rate-limiting anonymous downloads (HTTP 429): wait a few minutes, or set HF_TOKEN (free account, read token) for a higher limit"
    if "401" in msg or "403" in msg or "gated" in low:
        return "this model is gated or private on Hugging Face: accept its licence there and set HF_TOKEN, or pick an ungated one (dogfight models)"
    if "does not appear to have" in low or "not a valid model identifier" in low or "404" in msg:
        return "no such model on Hugging Face: check the id (org/name) or pick one from `dogfight models`"
    return msg[:160]


def with_retries(fn, what, tries=4):
    """Downloads hit transient 429s; wait and try again before giving up."""
    for k in range(tries):
        try:
            return fn()
        except Exception as e:  # noqa: BLE001
            if k == tries - 1 or not ("429" in str(e) or "too many requests" in str(e).lower()):
                raise
            wait = 15 * (k + 1)
            log(f"{what}: rate-limited by Hugging Face, retrying in {wait}s")
            emit(f"STATUS rate-limited by Hugging Face, retrying {what} in {wait}s")
            time.sleep(wait)
    return None


def pick_backend(name):
    """auto: Apple silicon Macs always use MLX; everything else uses torch + CUDA."""
    if name == "auto":
        return "mlx" if (sys.platform == "darwin" and platform.machine() == "arm64") else "cuda"
    return name


def make_backend(name, vram_gb, quant):
    try:
        if name == "cpu":
            return TorchBackend(vram_gb, quant, cpu=True)
        return (MlxBackend if name == "mlx" else TorchBackend)(vram_gb, quant)
    except ImportError as e:
        if name == "mlx":
            hint = "pip install mlx-lm"
        elif sys.platform == "darwin":
            hint = "on a Mac use `dogfight run mlx ufo-battle` after `pip install mlx-lm`"
        else:
            hint = "pip install torch transformers (a CUDA build of torch)"
        raise RuntimeError(f"{e}; {hint}") from e


# ------------------------------------------------------------------ prompting

SYSTEM = ("You are the commander of team {me} in an arcade flying-saucer dogfight against team {foe}. "
          "Each turn you give one order per ship: attack, hunt, flee or abduct; there is no idle order. "
          "Score = enemy saucers destroyed + cows abducted; a team whose saucers are all destroyed loses the round. "
          "Destroyed saucers are replaced automatically after a few seconds. Focus fire on damaged enemies, "
          "flee with ships below 35 hp when an enemy is in range, and send a ship to abduct a cow whenever the cow "
          "is close (within 20) and no enemy is within laser range. Answer only in the required format.")


class Lessons:
    """What a commander learned from its losses: a short list kept in the prompt and on disk."""

    def __init__(self, path):
        self.path = path
        self.items = []
        if path and os.path.exists(path):
            with open(path, encoding="utf-8") as f:
                self.items = [line.strip() for line in f if line.strip() and not BAD_LESSON.search(line)][-MAX_LESSONS:]

    @staticmethod
    def words(text):
        return {w for w in re.findall(r"[a-z]+", text.lower()) if w not in STOPWORDS}

    def add(self, text):
        """Keep a lesson unless it is (nearly) the same as one already known, or talks about
        orders that do not exist."""
        text = text.strip()
        if not text or BAD_LESSON.search(text):
            return
        w = self.words(text)
        for old in self.items:
            ow = self.words(old)
            if w == ow or (w and ow and len(w & ow) / len(w | ow) >= 0.6):
                return
        self.items.append(text)
        self.items = self.items[-MAX_LESSONS:]
        if self.path:
            os.makedirs(os.path.dirname(self.path), exist_ok=True)
            with open(self.path, "w", encoding="utf-8") as f:
                f.write("\n".join(self.items) + "\n")

    def prompt(self):
        if not self.items:
            return ""
        return " Lessons from your past losses, obey them: " + " ".join(f"({i + 1}) {item}" for i, item in enumerate(self.items))


def system_prompt(team, lessons):
    return SYSTEM.format(me=TEAM_NAMES[team], foe=TEAM_NAMES[1 - team]) + (lessons.prompt() if lessons else "")


def describe_loss(loss):
    """The post-mortem the losing commander reflects on."""
    if loss.get("kind") == "round":
        return (f"You lost game {loss['round']}: no saucer left in the air. You destroyed {loss['kills']} enemy saucers and lost "
                f"{loss['losses']}; you had {loss.get('regens_left', 0)} reinforcements left. The enemy still had "
                f"{loss['foes_left'] or 'no'} saucers in the air. You abducted {loss['cows']} cows. The next game starts in "
                f"10 seconds and you get one extra saucer.")
    parts = [f"Your saucer S{loss['ship']} was destroyed by enemy E{loss['killer']} (which had {loss['killer_hp']} hp)",
             f"at distance {loss['dist']} (laser range {loss['range']}), while its order was '{loss['order']}'.",
             f"At that moment {loss['enemies_near']} enemies and {loss['allies_near']} allies were within range of it."]
    if loss.get("low_for", 0) >= 3:
        parts.append(f"It had been flying below 35 hp for {loss['low_for']:.0f} s without retreating.")
    if loss.get("alive_for", 0) < 8:
        parts.append(f"It had warped in only {loss['alive_for']:.0f} s earlier.")
    if loss.get("y", 0) > loss.get("ground", 100) - 12:
        parts.append("It was low, near the ground.")
    return " ".join(parts)


def reflect(backend, cmdr, lessons, loss):
    team = loss["team"]
    messages = [{"role": "system", "content": system_prompt(team, lessons)},
                {"role": "user", "content": describe_loss(loss) + " What should you do differently so this does not happen again? "
                 "There is no repair or healing: a damaged saucer stays damaged until it is destroyed; only flee, guard, "
                 "attack, hunt, abduct, patrol and DEPLOY exist. Reply with one concrete rule for your future orders, "
                 "max 25 words, in the form:\nLESSON: <rule>"}]
    t0 = time.time()
    text, n_tok = backend.generate(cmdr, messages, 60)
    text = re.sub(r"<think>.*?</think>", "", text, flags=re.S)
    m = LESSON_RE.search(text)
    lesson = (m.group(1) if m else text.strip().splitlines()[0] if text.strip() else "").strip()
    lesson = SAY_OK.sub("", lesson)[:120].strip()
    log(f"team {team} {cmdr.label} reflects ({n_tok} tok, {time.time() - t0:.2f}s): {lesson!r}")
    return lesson


def describe(obs):
    team = obs["team"]
    me, foe = TEAM_NAMES[team], TEAM_NAMES[1 - team]
    sc, cows = obs["score"], obs.get("cows_taken", [0, 0])
    rounds = obs.get("rounds", [0, 0])
    lines = [f"Game {obs.get('round', 1)} (games won: {me} {rounds[team]}, {foe} {rounds[1 - team]}). "
             f"This game: {me} {sc[team]} kills + {cows[team]} cows, {foe} {sc[1 - team]} kills + {cows[1 - team]} cows. "
             f"Field: x 0-100 left to right, y 0-{obs['fh']} top to bottom, ground at y={obs['ground']}. Laser range {obs['range']}."]
    if "regens" in obs:
        rg, al = obs["regens"], obs.get("alive", [0, 0])
        lines.append(f"Saucers in the air: {me} {al[team]}, {foe} {al[1 - team]} (max {obs.get('max_alive', 4)} each). "
                     f"Reinforcements left this game: {me} {rg[team]}, {foe} {rg[1 - team]}. A team with no saucer in the air loses the game.")
    if obs.get("doctrine"):
        gen = obs.get("gen", 0)
        head = f"Your doctrine (generation {gen}, " if gen else "Your doctrine ("
        lines.append(head + "enforced by your ship AI; orders outside it are corrected): "
                     f"{obs['doctrine']}." + (f" {obs['corrected']} of your last orders were corrected." if obs.get("corrected") else ""))
    lines.append("Your ships (S):")
    for s in obs["mine"]:
        near = f", nearest enemy E{s['near']} at distance {s['dist']}" if s.get("near", -1) >= 0 else ", no enemy in the air"
        cow = f", nearest free cow C{s['cow']} at distance {s['cowd']}" if s.get("cow", -1) >= 0 else ""
        lines.append(f"- S{s['id']} at ({s['x']},{s['y']}) hp {s['hp']}{near}{cow}, current order: {s['order']}")
    if not obs["mine"]:
        lines.append("- (all respawning)")
    lines.append("Enemy ships (E):")
    for e in obs["foes"]:
        tg = f", targeting S{e['target']}" if e.get("target", -1) >= 0 else ""
        lines.append(f"- E{e['id']} at ({e['x']},{e['y']}) hp {e['hp']}{tg}")
    if not obs["foes"]:
        lines.append("- (none in the air)")
    cows = ", ".join(f"C{c['id']} at x={c['x']} {c['state']}" for c in obs["cows"]) or "none"
    lines.append(f"Cows (C) on the ground: {cows}")
    # spell out the openings: small models rarely infer them from the numbers alone
    openings = [f"S{s['id']} could abduct C{s['cow']} now (cow at distance {s['cowd']}, "
                + (f"nearest enemy at {s['dist']})" if s.get("near", -1) >= 0 else "no enemy in the air)")
                for s in obs["mine"] if s.get("cow", -1) >= 0 and s["cowd"] <= 25 and (s.get("near", -1) < 0 or s["dist"] >= 40)]
    if openings:
        lines.append("Abduction openings, each worth a point: " + "; ".join(openings) + ".")
    if obs.get("events"):
        lines.append("Recent events: " + "; ".join(obs["events"]))
    if obs.get("foe_say"):
        lines.append(f'Enemy commander said: "{obs["foe_say"]}"')
    lines.append("Orders available: attack E<n> (chase and shoot that enemy), hunt (attack the nearest enemy), "
                 "flee (retreat and dodge), abduct C<n> (fly to that cow and beam it up).")
    if obs.get("cry"):
        lines.append("One of your saucers was just destroyed. End your reply with a SAY line: a defiant battle cry "
                     "to the enemy in your own words, max 40 characters.")
    lines.append("Reply with exactly this format and nothing else:")
    for s in obs["mine"]:
        lines.append(f"S{s['id']}: <order>")
    if obs.get("cry"):
        lines.append("SAY: <battle cry>")
    return "\n".join(lines)


def parse_reply(text, obs):
    text = re.sub(r"<think>.*?</think>", "", text, flags=re.S)
    mine = {s["id"] for s in obs["mine"]}
    orders = {}
    for m in ORDER_RE.finditer(text):
        sid, verb, arg = int(m.group(1)), m.group(2).lower(), m.group(3)
        if sid not in mine:
            continue
        if verb in ("attack", "abduct"):
            orders[sid] = f"{verb}:{int(arg)}" if arg is not None else "hunt"
        elif verb == "flee":
            orders[sid] = "flee"
        else:  # hunt, and anything idle-ish the model invents
            orders[sid] = "hunt"
    say = ""
    m = SAY_RE.search(text) if obs.get("cry") else None
    if m:
        say = SAY_OK.sub("", m.group(1)).strip()[:40]
        if TEMPLATE_ECHO.search(say):  # the model parroted the instructions
            say = ""
    return orders, say, 0


def decide(backend, cmdr, obs, lessons=None):
    team = obs["team"]
    messages = [{"role": "system", "content": system_prompt(team, lessons)},
                {"role": "user", "content": describe(obs)}]
    max_new = min(110, 30 + 14 * max(1, len(obs["mine"])))
    t0 = time.time()
    text, n_tok = backend.generate(cmdr, messages, max_new)
    dt = time.time() - t0
    orders, say, deploy = parse_reply(text, obs)
    log(f"team {team} {cmdr.label}: {n_tok} tok in {dt:.2f}s ({n_tok / max(dt, 1e-3):.0f} tok/s) orders={orders} say={say!r}")
    log("   raw:", text.replace("\n", " / ")[:300])
    return orders, say, deploy


# ------------------------------------------------------------------ commands

def is_cached(model_id):
    try:
        from huggingface_hub import try_to_load_from_cache
        mid, rev = split_rev(model_id)
        r = try_to_load_from_cache(mid, "config.json", revision=rev)
        return isinstance(r, str)
    except Exception:
        return False


def check(args):
    name = pick_backend(args.backend)
    print(f"platform    {platform.system()} {platform.machine()}, python {platform.python_version()}")
    print(f"backend     {name} (requested {args.backend})")
    for mid in (args.model_a, args.model_b):
        print(f"model       {mid}  cached={'yes' if is_cached(mid) else 'no (run: dogfight arena pull)'}")
    try:
        if name == "mlx":
            import mlx_lm
            print(f"mlx-lm      {getattr(mlx_lm, '__version__', '?')}")
        else:
            import torch
            import transformers
            print(f"torch       {torch.__version__}  cuda={torch.cuda.is_available()}  transformers {transformers.__version__}")
            if name == "cpu":
                print(f"cpu         {os.cpu_count()} threads; small models only, a few tokens per second")
            if torch.cuda.is_available():
                p = torch.cuda.get_device_properties(0)
                free, total = torch.cuda.mem_get_info()
                cap = args.vram_gb if args.vram_gb > 0 else max(4.0, total / 2**30 - 2.0)
                print(f"gpu         {p.name}, {total / 2**30:.1f} GB total, {free / 2**30:.1f} GB free now; sidecar cap {cap:.1f} GB "
                      f"({'auto: card - 2 GB' if args.vram_gb <= 0 else 'lm_vram_gb'})")
    except ImportError as e:
        print(f"MISSING     {e}")
        print("            cuda: pip install torch transformers      mac: pip install mlx-lm, then `dogfight run mlx ufo-battle`")
        return 1
    if not args.load:
        print("(add --load to load both models and time one decision each)")
        return 0
    be = make_backend(name, args.vram_gb, args.quant)
    cmdrs = []
    for mid in (args.model_a, args.model_b):
        t0 = time.time()
        try:
            cmdrs.append(be.load(mid))
        except Exception as e:  # noqa: BLE001
            print(f"FAILED      {mid}: {oom_hint(e)}")
            return 1
        print(f"loaded      {mid} in {time.time() - t0:.1f}s, peak memory {be.mem_gb():.2f} GB")
    obs = {"team": 0, "tick": 1, "score": [0, 0], "cows_taken": [0, 0], "rounds": [0, 0], "round": 1, "fh": 55, "ground": 47,
           "range": 60, "cry": True,
           "mine": [{"id": 0, "x": 20, "y": 20, "hp": 100, "near": 3, "dist": 30, "cow": 0, "cowd": 12, "order": "hunt"},
                    {"id": 1, "x": 10, "y": 30, "hp": 40, "near": 3, "dist": 35, "cow": 0, "cowd": 18, "order": "hunt"}],
           "foes": [{"id": 3, "x": 50, "y": 22, "hp": 60, "target": 1}, {"id": 4, "x": 80, "y": 15, "hp": 100, "target": -1}],
           "cows": [{"id": 0, "x": 15, "state": "free"}, {"id": 1, "x": 70, "state": "free"}], "events": [], "foe_say": ""}
    for team, c in enumerate(cmdrs):
        obs["team"] = team
        t0 = time.time()
        orders, say, _ = decide(be, c, obs)
        print(f"decision    team {team} {c.label}: {time.time() - t0:.2f}s  orders={orders}  say={say!r}")
    loss = {"team": 1, "kind": "ship", "ship": 3, "killer": 0, "killer_hp": 80, "dist": 20, "range": 60, "order": "abduct C1",
            "enemies_near": 2, "allies_near": 0, "low_for": 6, "alive_for": 40, "x": 70, "y": 40, "ground": 47}
    t0 = time.time()
    print(f"reflection  team 1 {cmdrs[1].label}: {reflect(be, cmdrs[1], Lessons(None), loss)!r} ({time.time() - t0:.2f}s)")
    print(f"peak memory {be.mem_gb():.2f} GB for both models ({be.device_desc()})")
    return 0


def pull(args):
    from huggingface_hub import snapshot_download
    pats = ["*.json", "*.safetensors", "*.txt", "*.jinja", "*.model", "*.tiktoken"]
    for spec in (args.model_a, args.model_b):
        mid, rev = split_rev(spec)
        t0 = time.time()
        p = with_retries(lambda m=mid, r=rev: snapshot_download(m, revision=r, allow_patterns=pats), f"downloading {mid}")
        print(f"{mid} -> {p} ({time.time() - t0:.0f}s)")
    return 0


def serve(args):
    try:
        os.nice(5)  # the terminal animation has priority over token generation
    except (AttributeError, OSError):
        pass
    name = pick_backend(args.backend)
    emit(f"STATUS starting {name} backend")
    try:
        be = make_backend(name, args.vram_gb, args.quant)
    except Exception as e:
        emit(f"ERROR {name}: {e}")
        return 1
    cmdrs = []
    for team, mid in enumerate((args.model_a, args.model_b)):
        emit(f"STATUS {'loading' if is_cached(mid) else 'downloading (first run, a few minutes)'} {label_of(mid)} for {TEAM_NAMES[team]}")
        try:
            t0 = time.time()
            c = with_retries(lambda m=mid: be.load(m), f"loading {label_of(mid)}")
            log(f"loaded {mid} in {time.time() - t0:.1f}s, peak {be.mem_gb():.2f} GB")
        except Exception as e:  # noqa: BLE001
            emit(f"ERROR loading {label_of(mid)}: {oom_hint(e)}")
            return 1
        cmdrs.append(c)
        emit(f"READY {team} {c.label} {be.mem_gb():.2f}")
    lessons = []
    for team, c in enumerate(cmdrs):
        path = os.path.join(args.memory, f"{TEAM_NAMES[team]}-{c.label}.txt") if (args.evolve and args.memory) else None
        lessons.append(Lessons(path))
        if lessons[-1].items:
            log(f"team {team} remembers {len(lessons[-1].items)} lesson(s) from {path}")
    log("serving on", be.device_desc(), "evolve" if args.evolve else "no evolve")
    for raw in sys.stdin:
        raw = raw.strip()
        if not raw:
            continue
        try:
            obs = json.loads(raw)
        except json.JSONDecodeError:
            log("bad line:", raw[:120])
            continue
        if obs.get("t") == "quit":
            break
        if obs.get("t") == "loss" and args.evolve:
            team = int(obs["team"])
            try:
                lesson = reflect(be, cmdrs[team], lessons[team], obs)
            except Exception as e:
                log("reflect failed:", repr(e))
                lesson = ""
            if lesson:
                lessons[team].add(lesson)
                emit(f"LESSON {team} {lesson}")
            continue
        if obs.get("t") != "obs":
            continue
        team = int(obs["team"])
        try:
            orders, say, _ = decide(be, cmdrs[team], obs, lessons[team])
        except Exception as e:
            log("decide failed:", repr(e))
            orders, say = {}, ""
        body = " ".join(f"S{sid}:{o}" for sid, o in sorted(orders.items()))
        emit(f"ORDERS {team} {obs.get('tick', 0)} {body} | {say}")
    log("bye")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--model-a", default="Qwen/Qwen3-0.6B")
    ap.add_argument("--model-b", default="HuggingFaceTB/SmolLM2-360M-Instruct")
    ap.add_argument("--backend", default="cuda", choices=["auto", "cuda", "mlx", "cpu"])
    ap.add_argument("--vram-gb", type=float, default=0.0, help="CUDA memory cap for this process (both models); 0 = card memory - 2 GB")
    ap.add_argument("--quant", default="none", choices=["none", "8bit", "4bit"], help="bitsandbytes quantization (cuda)")
    ap.add_argument("--evolve", dest="evolve", action="store_true", default=True, help="learn a lesson from every loss (default)")
    ap.add_argument("--no-evolve", dest="evolve", action="store_false")
    ap.add_argument("--memory", default="", help="directory where lessons persist (evolve mode)")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--load", action="store_true", help="with --check: load the models and time a decision")
    ap.add_argument("--pull", action="store_true")
    args = ap.parse_args()
    if args.check:
        return check(args)
    if args.pull:
        return pull(args)
    return serve(args)


if __name__ == "__main__":
    sys.exit(main())
