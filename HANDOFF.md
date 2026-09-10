# reverie — engineering handoff

Reference for an agent or engineer taking over. `CLAUDE.md` holds the rules; this file holds the
detail. State at handoff: **v0.2 (2026-09-10)** — one scene (`ufo`) with two kinds of pilots:
built-in heuristics, or two small language models (`ufo-battle`) in a Python sidecar.

---

## 1. What it is and why it is shaped this way

A screensaver *inside* the terminal. When the interactive shell has been idle at its prompt for
`idle_seconds`, the terminal is taken over by an animated scene; any key restores the prompt.

Design drivers, in priority order:
1. **Target = VTE** (GNOME Terminal on Ubuntu ≤ 25.04, Ptyxis on 25.10+). VTE stable builds ship
   without sixel and without the kitty graphics protocol, so there is no pixel path: everything is
   character cells with 24-bit colour. The bottleneck is how many bytes the terminal must parse.
2. **Lightweight** (explicit user requirement) → Rust, `libc` only; the watcher that lives in every
   shell is ~1 MB RSS and sleeps until its deadline. The language models live in a **separate
   Python process** that exists only while `ufo-battle` is on screen; the binary has no ML code.
3. **`ufo-battle` fits an 8 GB CUDA card** (the user's 20 GB card is explicitly not the yardstick),
   defaults to CUDA, and is one word away from MLX on a Mac.
4. **Never damage the user's terminal session**: termios, screen, cursor and the half-typed line
   must survive every exit path, including SIGTERM, SIGHUP and panic.

History: v0.1 (same day) had four scenes (galaxy, garden, meadow, portrait) and an image
importer. The user asked to keep only `ufo` and make it LM-controlled; the others were deleted
(commit history has them) together with the `image` dependency.

## 2. Runtime architecture

```
 ~/.bashrc  ──eval "$(reverie init bash)"──►  trap __reverie_alrm ALRM
                                              reverie watch --pid $$ --tty "$(tty)" --daemon
                                                    │
     every ≤10 s: at prompt?  Linux: /proc/<shell>/stat (tpgid == pgrp && state 'S')
                              macOS: ps -o pgid,tpgid,stat -p <shell>
                  stat(tty).atime → idle ≥ idle_seconds?
                                                    │ yes
     write $XDG_RUNTIME_DIR/reverie-$UID/fire-<pid>  then kill(shell, SIGALRM)
                                                    ▼
 shell runs trap → reverie run --idle-trigger <pid>
     claim_trigger(): marker must exist and be ≤ 5 s old (else exit 0 silently)
     acquire_slot(): flock on slot-0..N (max_instances) (else exit 0)
     Term::enter(): raw mode, alt screen, hide cursor, no autowrap, focus reports
     loop: update(dt) → render(canvas) → encode(diff) → write → poll(stdin, until next frame)
     any key → Term::leave(): tcflush input, restore sequence, restore termios
 bash trap then runs `kill -WINCH $$` → readline repaints the prompt line
```

Why each piece exists:
- **SIGALRM, not USR1**: measured in a pty against bash 5.2.21: USR1/USR2 traps are deferred
  while readline waits for input; WINCH and ALRM run immediately; INT runs but wipes the line.
  zsh 5.9 (`TRAPALRM`) and fish 3.7 (`--on-signal SIGALRM`) also work.
- **Marker file**: makes a late or foreign ALRM (e.g. zsh `TMOUT`) a no-op.
- **tpgid == pgrp**: if a foreground job runs, the terminal's foreground group isn't the shell's,
  so the watcher stays silent during `vim`, builds, etc.
- **`--tty` from the snippet**: `/proc/<pid>/fd/0` is Linux-only; the shell knows its tty.
  Without `--tty` the Linux fallback still reads /proc.
- **tty atime**: updated when the shell reads keystrokes; kernel granularity 8 s on Linux.
- **One watcher per shell**: `runtime_dir/watch-<pid>` pidfile; a new watcher for the same shell
  SIGTERMs the old one. `uninstall` stops watchers via the pidfiles (+ a /proc sweep on Linux).
- **Instance slots**: at most `max_instances` terminals animate at once (flock, auto-released).

## 3. Rendering model

`Canvas` (src/canvas.rs) is `cols × rows*2` RGB pixels plus two overlays:

| layer | resolution | cell output | use for |
|---|---|---|---|
| `px` | cols × 2·rows | `▀` fg=top pixel, bg=bottom pixel (space if equal) | everything |
| `dots` | 2·cols × 4·rows | braille char, one colour per cell | stars, sparks |
| `glyphs` | cols × rows | any char, fg colour | HUD, comms, lessons |

Priority per cell: glyph > dots > half-block. Primitives take float coordinates and anti-alias.

`Encoder` (src/encode.rs) diffs against what was last emitted (per-channel `tolerance`), tracks
cursor and SGR state, truecolor or 256-colour, wraps frames in DEC 2026 synchronized update.

`Term` (src/term.rs): opens `/dev/tty` directly; restore happens in `leave()`, in `Drop`, in the
panic hook, and on SIGTERM/INT/HUP/QUIT. `app::run` (src/app.rs) paces frames, applies the
`--pilots`/backend/evolve overrides to a cloned config, and rotates scenes (only relevant if the
config lists both `ufo` and `ufo-battle`).

## 4. Module map

| file | role |
|---|---|
| `src/main.rs` | CLI (hand-rolled parser): `run [cuda|mlx] [ufo|ufo-battle]`, `arena [check|pull|lessons|forget]`, `bench`, `snapshot`, install/uninstall/pause/status |
| `src/app.rs` | the screensaver loop |
| `src/term.rs` | raw mode, signals, restore guarantees, input |
| `src/canvas.rs`, `src/encode.rs` | framebuffer + AA primitives; diff encoder |
| `src/idle.rs` | watcher daemon (Linux /proc, macOS `ps`), trigger claim, slots, pause/resume |
| `src/shell.rs` | bash/zsh/fish snippets, install/uninstall with backup + marked block |
| `src/config.rs` | TOML-subset parser (no deps), `DEFAULT_TOML`, XDG paths |
| `src/arena.rs` | sidecar link: spawn, reader thread, line protocol parser, `Order` enum, lessons dir |
| `src/scenes/mod.rs` | **locked** `Scene` trait, `make()`, `NAMES`, shared `Starfield` |
| `src/scenes/ufo.rs` | the scene: orders → steering, built-in commander, LM plumbing, HUD |
| `agents/arena.py` | the sidecar (embedded via `include_str!`, written to `~/.local/share/reverie/arena.py`) |
| `src/math.rs`, `src/rng.rs` | `Rgb`, value noise, gradients; splitmix64 RNG |

## 5. The ufo scene

**Orders** (`arena::Order`): `Attack(enemy)`, `Hunt` (nearest), `Flee`, `Abduct(cow)`,
`Guard(ally)`, `Patrol`. `Ufo::execute` turns an order into a desired direction + a fire target,
degrades invalid orders (dead target, taken cow, dead ally) to `Hunt`, then applies ally
separation, soft world bounds and acceleration limits. Attack/Hunt use lead pursuit until 75% of
range, then orbit at half range; Guard/Patrol fire opportunistically at anything in range;
Flee/Abduct don't fire.

**Built-in commander** (`builtin_order`, used by `ufo` and by every team whose model isn't ready):
the v0.1 heuristics expressed as an order per frame: keep abducting unless disturbed; abduct when
bored and no enemy within 1.6× range; flee below 35 hp with an enemy within 0.8× range; else attack
the sticky target (replaced only by an enemy 40% closer); else patrol. Automatic respawn 2.5–4.5 s.

**LM mode** (`Ufo::with_arena`): each team has `LM_FLEET = 4` slots. Destroyed saucers stay
down until the commander answers `DEPLOY: n`; after 25 s with no ship alive one is launched
anyway (`no_ships` guard). Per team, every `lm_think_seconds` (1.0) and only when no request is
pending, `observation()` serialises the situation as one JSON line (coordinates in percent of the
field width; `mine`, `foes`, `cows`, `events` since the last orders, the enemy's last cry,
`free_slots`, `cry` = one of ours died since the last orders). `lm_poll()` applies `ORDERS`,
`READY`, `STATUS`, `LESSON`, `ERROR`/exit (→ built-in pilots + a HUD note). Replies older than
60 s are dropped as stuck.

**Post-mortems** (evolve): on a kill, `post_mortem()` sends `{"t":"loss",...}` for the victim's
team: killer, killer hp, distance vs range, the victim's order, enemies/allies within range,
seconds flown below 35 hp, seconds since warp-in, height. The sidecar reflects and answers
`LESSON <team> <text>`, shown in the HUD for 12 s.

**HUD** (glyph layer): row 0 `ZORB n  <model>` left and `<model>  KRELL n` right (`..` while a
request is pending); row 1+ status while loading, then battle cries (20 s) and lessons (12 s).
Only ASCII 0x20–0x7E reaches the glyph layer (cell widths).

Key numbers: ship size `s = w/26` clamped 3.2..8 px; weapon range `16 s`; built-in fleet
`w/45` clamped 2..5 per team (4 at 200 cols, matching LM mode).

## 6. The sidecar (agents/arena.py)

Line protocol (stdout is reserved for it; stderr → `~/.local/state/reverie/arena.log`):

```
reverie → arena   {"t":"obs",...}  {"t":"loss",...}  {"t":"quit"}
arena → reverie   STATUS <text>
                  READY <team> <label> <mem_gb>
                  ORDERS <team> <tick> S0:attack:3 S1:flee ... D:<n> | <cry>
                  LESSON <team> <text>
                  ERROR <text>
```

Backends: `TorchBackend` (transformers `AutoModelForCausalLM`, bf16 if supported else fp16,
`torch.cuda.set_per_process_memory_fraction(lm_vram_gb/total)` as a hard cap, optional
bitsandbytes 8/4-bit) and `MlxBackend` (`mlx_lm.load/generate` with `make_sampler(temp 0.7,
top_p 0.9)`). `--backend auto` = mlx on darwin/arm64 else cuda; the config default is `cuda`.
`enable_thinking=False` is passed to every chat template (Qwen3 honours it, others ignore it)
and `<think>…</think>` is stripped defensively.

Prompting: system = role + tactics + the team's lessons; user = the situation as text, the order
menu, and the exact reply skeleton (`S<id>: <order>` lines, `DEPLOY: <0-n>` only when slots are
free, `SAY:` only when `cry` is set). `parse_reply` is regex-based and forgiving; `ORDER_RE`'s
optional argument cannot cross a newline (a bug found by `eval/test_arena.py`). Sampling:
temperature 0.7, top-p 0.9, top-k 40, `max_new_tokens = 30 + 14·ships` (≤ 110); reflections 60.

Lessons: `Lessons` keeps ≤ 8 per (team, model) in `~/.local/state/reverie/lessons/<TEAM>-<label>.txt`
(dedup, oldest dropped), injected into the system prompt. `reverie arena lessons|forget`.

Measured here (RTX 4000 Ada, torch 2.6 cu124, transformers 5.7): load 2 s + 1 s; torch peak
4.6 GB for both models, 4.9 GB by nvidia-smi (includes the CUDA context); decisions 0.2–0.8 s
(Qwen3-0.6B ~120 tok/s, SmolLM2-1.7B ~70 tok/s); reflections 0.1–0.25 s. A 76 s match at 200×55
ran at 57 fps (fps=60 config) with the sidecar answering ~1 request/s per team.

Model choice notes: ungated small instruct models checked on 2026-09-10 (bf16 safetensors):
Qwen3-0.6B 1.5 GB, Qwen3-1.7B 4.1 GB, Qwen3.5-0.8B 1.75 GB, Qwen3.5-2B 4.6 GB (multimodal
class), SmolLM2-1.7B-Instruct 3.4 GB, SmolLM3-3B 6.2 GB; gemma-3-1b-it and Llama-3.2-1B are
gated; gemma-4-E2B-it is 10 GB. Two 1.7B models in bf16 (7.5 GB) do **not** fit the 8 GB target
with the CUDA context; use `lm_quant = "8bit"` for such pairs. MLX users can point the model
keys at `mlx-community/*-4bit` repos.

## 7. File formats (on users' disks — stay backward compatible)

**Config** `~/.config/reverie/config.toml`: `key = value` lines; unknown keys ignored (v0.1's
`garden_speed`, `day_cycle_seconds`, `character` are silently accepted). Keys: `idle_seconds`,
`fps`, `unfocused_fps`, `scenes`, `rotate_minutes`, `max_instances`, `color`, `tolerance`,
`ufo_pilots`, `lm_backend`, `lm_model_a`, `lm_model_b`, `lm_vram_gb`, `lm_quant`, `lm_python`,
`lm_think_seconds`, `lm_evolve`.

**Lessons** `~/.local/state/reverie/lessons/<TEAM>-<model label>.txt`: one lesson per line.

**Runtime** `${XDG_RUNTIME_DIR:-/tmp}/reverie-$UID/` (mode 0700): `fire-<pid>`, `watch-<pid>`,
`slot-<n>`. **Pause**: `~/.local/state/reverie/paused-until` (unix secs).
**Sidecar copy** `~/.local/share/reverie/arena.py` (rewritten whenever the embedded one differs).

## 8. Evaluation harness

| tool | checks |
|---|---|
| `reverie bench [--scene ufo] [--size 200x55] [--frames 600] [--seed 42]` | JSON: frame mean/p95/max ms, bytes mean/p95 KB, first-frame KB, peak RSS |
| `eval/check_bench.py` | compares bench JSON to `eval/thresholds.toml` |
| `eval/test_terminal.py` | 18 pty checks (bash/zsh/fish): saver starts; exit ≤ 300 ms; restore screen/termios; restore after SIGTERM; silent during a command; fires at prompt; partial line survives; wake key not leaked; watcher RSS ≤ 4 MB |
| `eval/test_arena.py` | sidecar prompt/parse/lessons/backend selection, no GPU |
| `reverie arena check --load` | the GPU side: loads both models, times a decision and a reflection, prints peak memory |
| `reverie snapshot` + `eval/ansi2png.py` | deterministic frame → PNG for visual review |

Results at handoff (200×55, seed 42): ufo mean 0.15 ms, p95 0.17 ms, 22.6 KB/frame mean,
29.2 KB p95 (limits 4 ms / 8 ms / 45 KB / 110 KB). Binary 0.69 MB (≤ 4), watcher 1.05 MB RSS
(≤ 4). pty harness: 14/14 bash checks pass here; zsh and fish are not installed on this machine
(the 4 shell-specific checks could not run — not a reverie failure). CI runs all three shells.

## 9. Known issues and backlog

Known, accepted:
- bash: cursor at column 0 after waking until the first keystroke (line content correct).
- ±8 s idle timing (kernel atime granularity).
- The commanders are tiny: they drop ships from their reply (those keep their previous order),
  answer `DEPLOY: 0` with nothing in the air (the 25 s guard covers it), and occasionally echo the
  enemy's cry.
- macOS watcher + MLX backend: cross-compiled (`cargo check --target aarch64-apple-darwin`) and
  unit-tested, never run on a Mac. First things to verify there: `ps -o pgid,tpgid,stat` output
  parsing, tty atime updating on input, `mlx_lm` chat-template kwargs.
- `.github/workflows/ci.yml` has not run yet (macos job added, untested).

Backlog, roughly by value:
1. A real GNOME Terminal pass (colours, braille widths, CPU of `gnome-terminal-server`).
2. Mac verification pass (above).
3. Smarter reflections: merge/replace similar lessons instead of dedup-by-string; let the
   winning team learn from kills too.
4. kitty graphics pixel mode for kitty/Ghostty (VTE path must remain default).
5. Packaging: `.deb`, musl binaries, a `pip`-installable sidecar.

## 10. Decision history (short)

- Language: Rust after "the tool should not be heavy". GPU/CUDA rejected for *rendering* (no
  pixel protocol; bytes are the bottleneck) but used for the *commanders* via a sidecar.
- Sidecar over in-process inference: keeps the binary at one crate and ~0.7 MB, lets CUDA and
  MLX be swapped without touching Rust, and the GPU is released the moment the saver exits.
- Line protocol over pipes rather than HTTP/ollama: no client library, no daemon to manage,
  works identically on Linux and macOS, and `arena.log` shows every prompt outcome.
- Default models Qwen3-0.6B vs SmolLM2-1.7B-Instruct: two families (more fun), ungated, 4.6 GB
  together — the 8 GB target with real margin. Qwen3-1.7B + SmolLM2-1.7B (7.5 GB) was rejected.
- Orders instead of direct control: LMs answer once a second; the steering layer keeps the
  30–60 fps motion and the original look, so "graphics as used" holds.
- 4 saucers per team, deploy-on-decision, battle cry only after a loss, evolve as a toggle:
  user instructions on 2026-09-10.
- Harness scene names changed from galaxy/meadow/garden to ufo when those scenes were removed
  (pass criteria unchanged; logged in BUILD_LOG).
