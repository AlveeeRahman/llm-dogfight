# LLM Dogfight — architecture

How the tool is built and why, for contributors. State: **v0.3.0** — a terminal UFO dogfight flown
by two small local language models (`dogfight`), with the built-in pilots as the no-GPU baseline
(`dogfight ufo`) and an optional idle-screensaver mode (`dogfight install`). One scene, two kinds of
pilots. Working rules: `scripts/qa.sh --full` must be green before a change is done; the files
under `eval/` are the measuring instruments and are not edited to make something pass.

---

## 1. What it is and why it is shaped this way

A game *inside* the terminal: `dogfight` takes the terminal over with an animated battle, and any
key gives it back exactly as it was.

Design drivers, in priority order:
1. **Target = VTE** (GNOME Terminal on Ubuntu ≤ 25.04, Ptyxis on 25.10+). VTE stable builds ship
   without sixel and without the kitty graphics protocol, so there is no pixel path: everything is
   character cells with 24-bit colour. The bottleneck is how many bytes the terminal must parse.
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

`dogfight` opens `/dev/tty`, enters raw mode and the alternate screen, and runs the frame loop
(`app::run`): update the scene, render it to a cell canvas, diff-encode it, write it, then wait
for the next frame while polling stdin. Any key, SIGTERM/INT/HUP/QUIT, or `--duration` ends the
loop; `Term::leave` restores the screen and termios on every path, including panics. In
`ufo-battle` the scene owns an `Arena` (src/arena.rs): a Python sidecar that runs the two models
and talks a line protocol over pipes.

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
`--pilots`/backend/evolve overrides to a cloned config.

## 4. Module map

| file | role |
|---|---|
| `src/main.rs` | CLI (hand-rolled parser): `dogfight [cuda|mlx|evolve|ufo]`, `run ...` long form, `arena check|pull|lessons`, `reset`, `remove`, `bench`, `snapshot`, `status` |
| `src/app.rs` | the battle loop: frame pacing, lag guard, perf log, `--shot` |
| `src/term.rs` | raw mode, signals, restore guarantees, input |
| `src/canvas.rs`, `src/encode.rs` | framebuffer + AA primitives; diff encoder |
| `src/config.rs` | TOML-subset parser (no deps), `DEFAULT_TOML`, XDG paths |
| `src/arena.rs` | sidecar link: spawn, reader thread, line protocol parser, `Order` enum, lessons dir |
| `src/scenes/mod.rs` | **locked** `Scene` trait, `make()`, `NAMES`, shared `Starfield` |
| `src/scenes/ufo.rs` | the scene: orders → steering, built-in commander, LM plumbing, HUD |
| `agents/arena.py` | the sidecar (embedded via `include_str!`, written to `~/.local/share/dogfight/arena.py`) |
| `src/math.rs`, `src/rng.rs` | `Rgb`, value noise, gradients; splitmix64 RNG |

## 5. The ufo scene

**Orders** (`arena::Order`): `Attack(enemy)`, `Hunt` (nearest), `Flee`, `Abduct(cow)`; `Guard` and
`Patrol` exist in the enum for the built-in commander only (the LM menu has no idle order and the
sidecar maps anything else to hunt). `Ufo::execute` turns an order into a desired direction + a
fire target, degrades invalid orders (dead target, taken cow) to `Hunt`, then applies ally
separation, soft world bounds and acceleration limits. Attack/Hunt use lead pursuit until 75% of
range, then orbit at half range; a beaming saucer fires back at anything within half range.

**Built-in commander** (`builtin_order`, used by `ufo` and by every team whose model isn't ready):
the v0.1 heuristics expressed as an order per frame. Automatic respawn 2.5–4.5 s.

**LM mode** (`Ufo::with_arena`): `LM_SLOTS = 5` per team, `lm_max_alive` (4) on screen. `fleet()`
runs the match rules: a destroyed saucer's slot warps a reinforcement in after `REINFORCE`
(4 s) while `game.regens[team]` (20 per game) lasts and fewer than `max_alive` are up; a team
with no saucer alive *and no reinforcement left* loses the game (banner "X WINS GAME n", `game.wins`, cry due, game
post-mortem, scoreboard saved) and `GAME_PAUSE` (10 s) later `start_game()` fields the loser
with one extra saucer. Games won / lifetime kills / cows persist in
`score-<labelA>-vs-<labelB>.txt` (loaded once both READY labels are known). Per team,
every `lm_think_seconds` (1.0) and only when no request is pending, `observation()` serialises
the situation as one JSON line (percent coordinates; `mine` with nearest enemy *and* nearest free
cow distances, `foes`, `cows`, `events`, `foe_say`, `cry`, kills/cows/rounds). Wiped-out teams are
not asked for orders (except once for the cry).

**Post-mortems** (lessons): `post_mortem()` per destroyed saucer (kind `ship`) and
`post_mortem_game()` per lost game (kind `round` on the wire: kills, losses, enemy survivors,
cows, reinforcements left).

**Genetic doctrine** (`evolve` word / `lm_genetic`, src/evolve.rs): `Population` per team
(6 candidates, persisted in `doctrine-<TEAM>.txt`), one active `Doctrine` each. `Ufo::fleet`
closes an evaluation window every `SEGMENT` (90 s) or at game end: `window_fitness` = Δkills −
Δlosses + ½Δcows (± 6 on a decided game) per minute → `Population::score` → next candidate, or
`breed` (top half survives, uniform crossover + Gaussian mutation at 12% of each range, clamped
to `BOUNDS`) once all six are scored. `apply_doctrine` corrects every model order: forced flee
below `flee_hp` with an enemy in range, no flee above `brave_hp` or beyond `courage` × fleet,
abduct only within `abduct_dist` with no enemy inside `abduct_clear` × range, attack redirected
to the weakest enemy in range with probability `focus`. The doctrine text is in the prompt and
the correction count is fed back. Cost: a few float ops per game; no extra model calls.

**Farm and cows:** `NCOWS = 10` spread evenly at start; cows walk to a goal, graze 3–14 s, pick a
new goal; an abducted cow is replaced immediately by a new one walking out of the barn (so the
herd stays at 10). Score shows kills, cows and rounds per team.

**Graphics** (all in `render()` / `draw_saucer()`): Bayer-dithered sky gradient; aurora curtain
(two slow value-noise fields, upper 50% of the sky); three moonlit clouds; dynamic lights
(`Light` list rebuilt each frame from bolts, rings and beams, applied to the city/field band
below the tallest building); barn, fence posts, road with dashed centre line and cars
(headlights/taillights); animated cows (legs, tail, head bob); saucers with shaded hull, team
stripe, dome specular, chasing rim lights, speed-scaled engine glow, motion trail (7 samples at
60 ms), shield shimmer on hit, smoke tint when damaged; 3x5 pixel font (`glyph`) for the "ZORB VS KRELL" title card and the "X WINS GAME n" / "GAME n"
banners; HUD plate over the top rows (kills, cows, games won, reinforcements left, doctrine).

Key numbers: ship size `s = w/26` clamped 3.2..8 px; weapon range `16 s`; built-in fleet
`w/45` clamped 2..5 per team; bench 200×55: 0.34 ms/frame, 30.2 KB mean, 42.8 KB p95.

## 6. The sidecar (agents/arena.py)

Line protocol (stdout is reserved for it; stderr → `~/.local/state/dogfight/arena.log`):

```
dogfight → arena   {"t":"obs",...}  {"t":"loss",...}  {"t":"quit"}
arena → dogfight   STATUS <text>
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

Lessons: `Lessons` keeps ≤ 8 per (team, model) in `~/.local/state/dogfight/lessons/<TEAM>-<label>.txt`
(dedup, oldest dropped), injected into the system prompt. `dogfight arena lessons|forget`.

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

**Config** `~/.config/dogfight/config.toml`: `key = value` lines; unknown keys ignored (v0.1's
older screensaver keys are silently accepted). Keys:
`fps`, `unfocused_fps`, `scenes`, `rotate_minutes`, `max_instances`, `color`, `tolerance`,
`ufo_pilots`, `lm_backend`, `lm_model_a`, `lm_model_b`, `lm_vram_gb`, `lm_quant`, `lm_python`,
`lm_think_seconds`, `lm_evolve`.

**Lessons** `~/.local/state/dogfight/lessons/<TEAM>-<model label>.txt`: one lesson per line.
**Doctrines** `~/.local/state/dogfight/doctrine-<TEAM>.txt`: `gen n`, `active k`, then six lines
`flee_hp brave_hp abduct_dist abduct_clear focus courage fitness evals`.
**Scoreboard** `~/.local/state/dogfight/score-<A>-vs-<B>.txt`: `wins a b`, `kills a b`, `cows a b`, `games n n`.
**Match log** `~/.local/state/dogfight/match.log`: `<t>s game <n> | start|kill|reinforcement|cow|over: ...`.

**Runtime** `${XDG_RUNTIME_DIR:-/tmp}/dogfight-$UID/` (mode 0700): `fire-<pid>`, `watch-<pid>`,
`slot-<n>`. **Pause**: `~/.local/state/dogfight/paused-until` (unix secs).
**Sidecar copy** `~/.local/share/dogfight/arena.py` (rewritten whenever the embedded one differs).

## 8. Evaluation harness

| tool | checks |
|---|---|
| `dogfight bench [--scene ufo] [--size 200x55] [--frames 600] [--seed 42]` | JSON: frame mean/p95/max ms, bytes mean/p95 KB, first-frame KB, peak RSS |
| `eval/check_bench.py` | compares bench JSON to `eval/thresholds.toml` |
| `eval/test_arena.py` | sidecar prompt/parse/lessons/backend selection, no GPU |
| `dogfight arena check --load` | the GPU side: loads both models, times a decision and a reflection, prints peak memory |
| `dogfight snapshot` + `eval/ansi2png.py` | deterministic frame → PNG for visual review |

Results (200×55, seed 42): ufo mean 0.34 ms, p95 0.36 ms, 30.2 KB/frame mean,
(≤ 4). pty harness: 14/14 bash checks pass here; zsh and fish are not installed on this machine
(the 4 shell-specific checks could not run — not a dogfight failure). CI runs all three shells.

## 8b. Performance model and the lag guard

Measured with `--perf` in a pty at 200×55, fps 60: update 0.01 ms, render ~0.3 ms, encode
~0.15 ms, write ~0.35 ms (pty), 58 fps, 3 % CPU, 25–35 KB/frame. Under a pty throttled to
600 KB/s the write blocks 50–80 ms and the achievable rate is bytes-bound (~15 fps). `app::run`
therefore has a lag guard: four writes over half a frame period → fps halved for 5 s and the
encoder tolerance raised by 4 (cap 24); after 8 s of fast writes the tolerance steps back down
toward the config value. `Arena::send` goes through a writer thread with a 64-line bounded
queue (`try_send`), so a busy sidecar cannot block the loop; the sidecar itself runs `nice 5`.
`--perf FILE` / `DOGFIGHT_PERF` writes one line per second (fps, per-stage mean/max ms, KB/frame,
gap_max, CPU share, guard events) and is the first thing to look at for any lag report.

## 9. Security and code-quality posture

crash, leak or escape-sequence bug would be felt immediately. What is enforced:

| area | mechanism |
|---|---|
| dependencies | one crate (`libc`); `cargo audit` in CI |
| lints | `cargo clippy --release -- -D warnings` (with `undocumented_unsafe_blocks`), `cargo fmt --check`, 0 build warnings; `overflow-checks = true` in release; `bandit` + `ruff` on the sidecar |
| `unsafe` | 17 sites, each with a `// SAFETY:` comment, all thin wrappers over single libc calls: termios/ioctl/poll/read/write (term.rs), sigaction/signal (term.rs, main.rs), getuid/chmod (config.rs). No raw pointer arithmetic, no `transmute`, no `static mut`. |
| model output | `arena::sanitize` (cries), `Msg::Lesson` filter and the sidecar's `SAY_OK` regex allow printable ASCII only, so model text can never carry an escape sequence into the terminal, the HUD or a file |
| protocol | fixed line protocol parsed by hand (`arena::parse`); unknown lines are dropped; numbers are range-checked (team < 2) |
| files | XDG paths only; runtime dir `0700`; doctrine/score files parse numerically; the sidecar copy is rewritten from the embedded source on every start (a tampered copy is overwritten) |
| network | none in the binary; the Python side downloads models through `huggingface_hub` |
| terminal | restore on `leave()`, `Drop`, panic hook, SIGTERM/INT/HUP/QUIT; `panic = "abort"` after the hook; SIGPIPE default so a closed pipe cannot wedge output |

## 10. Known issues and backlog

Known, accepted:
- bash: cursor at column 0 after waking until the first keystroke (line content correct).
- The commanders are tiny: they drop ships from their reply (those keep their previous order),
  answer `DEPLOY: 0` with nothing in the air (the 25 s guard covers it), and occasionally echo the
  enemy's cry.
  unit-tested, never run on a Mac. First things to verify there: `ps -o pgid,tpgid,stat` output
  parsing, tty atime updating on input, `mlx_lm` chat-template kwargs.
- `.github/workflows/ci.yml`: lint/build/bench/harness on Linux, build + checks on macOS, release binaries on `v*` tags; `models.yml` checks the catalogue weekly.

Backlog, roughly by value:
1. A real GNOME Terminal pass (colours, braille widths, CPU of `gnome-terminal-server`).
2. Mac verification pass (above).
3. Smarter reflections: merge/replace similar lessons instead of dedup-by-string; let the
   winning team learn from kills too.
4. kitty graphics pixel mode for kitty/Ghostty (VTE path must remain default).
5. Packaging: `.deb`, musl binaries, a `pip`-installable sidecar.

## 11. Decision history (short)

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
- 5 saucers per team, automatic reinforcements, rounds on wipe-out (10 s return), no idle
  order, 10 cows replenished from the barn, battle cry only after a loss, evolve as a toggle:
  user instructions on 2026-09-10 (superseding the earlier 4-saucer / DEPLOY-on-decision rule).
- Harness scene names changed from galaxy/meadow/garden to ufo when those scenes were removed
  (pass criteria unchanged).
