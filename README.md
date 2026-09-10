# reverie

![reverie: two saucer teams over a sleeping city](eval/snapshots/ufo.png)

A UFO dogfight in your terminal, flown by two small language models running on **your**
machine. Type `reverie`, and team ZORB (Qwen3-0.6B) and team KRELL (SmolLM2-1.7B) start
commanding their saucers over a sleeping city: attack, hunt, flee, steal cows, shout at
each other, learn from every saucer they lose. Any key ends it.

```sh
cargo install --path .        # or a release binary on your PATH
pip install torch transformers        # CUDA GPU (8 GB is enough)  -- or --
pip install mlx-lm                    # Apple silicon
reverie                               # that's it (first run downloads ~5 GB of models)
```

| command | what happens |
|---|---|
| `reverie` | the battle; picks CUDA, or MLX on Apple silicon |
| `reverie cuda` / `reverie mlx` | the battle with the backend chosen by hand |
| `reverie evolve` | the battle, plus a genetic algorithm evolving each team's tactical doctrine |
| `reverie ufo` | the same dogfight with the built-in pilots: no models, no GPU |
| `reverie install` | optional screensaver mode: play whenever your shell prompt sits idle |

The scene: a dithered night sky with a slow aurora and moonlit clouds over a city, a farm
with a barn, a fence, a road with passing cars and ten grazing cows (a new one walks out of
the barn after every abduction). Saucers are shaded and bank into turns, leave motion
trails, shimmer when hit; bolts, explosions and abduction beams light up the buildings and
the field. Rendering is plain truecolor text (half-blocks and braille), so it works in the
default Ubuntu terminal (GNOME Terminal / Ptyxis) and any other truecolor terminal, Linux
or macOS, with no graphics protocol.

## Screensaver mode and uninstalling

```sh
reverie install                 # play when the prompt idles (bash, zsh, fish); idle_seconds in the config
reverie uninstall               # remove the hook
reverie remove                  # remove everything reverie put on the machine
```

`install` appends a marked block to your rc file (with a timestamped backup);
`reverie uninstall` removes it and stops the watchers but keeps your config, the
commanders' memories and the models. **`reverie remove`** deletes everything reverie
put on the machine: the hook, watchers, `~/.config/reverie`, `~/.local/state/reverie`
(memories, scores, logs), `~/.local/share/reverie` (the sidecar copy), the runtime dir
and the binary itself; it lists all of it and asks first (`--yes` skips the question,
`--models` also deletes the two downloaded models from the Hugging Face cache). Only
the rc-file backups stay. Editing the source does not touch the installed binary: run
`cargo install --path .` again (and open a new terminal, or `pkill -f "reverie watch"`)
to pick up changes.

## Choosing the models

```sh
reverie models                          # the catalogue: aliases, sizes, what fits 8 GB next to your other model
reverie --zorb qwen3-1.7b --krell llama3.2-1b        # any alias, or any Hugging Face id (`id@commit` pins the weights)
reverie arena pull                      # download them ahead of time
reverie arena check --load              # load both, time a decision each, print the peak GPU memory
```

Catalogue (bf16 sizes, 2026-09): Qwen3 0.6B / 1.7B, Qwen2.5 0.5B / 1.5B, SmolLM2 360M / 1.7B,
SmolLM3 3B, Llama 3.2 1B / 3B (gated), Gemma 3 1B (gated), TinyLlama 1.1B, Granite 3.3 2B,
DeepSeek-R1-Distill 1.5B, Phi-4-mini. Anything with a chat template and safetensors works;
`lm_quant = "8bit"` makes the 3B-class pairs fit 8 GB. To keep a pair, set `lm_model_a` /
`lm_model_b` in the config. Gated models need the licence accepted on Hugging Face and
`HF_TOKEN` in the environment.

## The battle in detail

```sh
reverie arena lessons                   # what each commander has learned, and the evolved doctrines
reverie reset model                     # wipe lessons + doctrines;  reverie reset score  wipes games won
```

Defaults are chosen for an **8 GB CUDA card**: team ZORB is `Qwen/Qwen3-0.6B`, team
KRELL is `HuggingFaceTB/SmolLM2-1.7B-Instruct`; together they take about 5 GB of GPU
memory (bf16/fp16), and the sidecar caps itself at `lm_vram_gb` (6 GB) so it can never
crowd out the desktop. Decisions take 0.2–0.8 s on an RTX 4000 Ada; each team gets new
orders every 1–2 s while the low-level steering keeps flying smoothly in between.

How it works: `reverie` spawns `agents/arena.py` (embedded in the binary, written to
`~/.local/share/reverie/arena.py`) and talks a line protocol over pipes. Each team
fields 5 saucers. Every second or so a commander receives a compact text situation
(its ships, hp, nearest enemy and nearest cow with distances, recent events, the
enemy's last cry) and answers with one order per ship — `attack E4`, `hunt`, `flee`,
`abduct C1`; there is no idle order. Orders are executed by the same steering
behaviours as the baseline pilots, so the feel of the fight stays the same.

Rules of a game: 4 saucers per team on screen (`lm_max_alive`). A destroyed saucer is
replaced by a reinforcement 4 s later, out of a budget of 20 per team per game
(`lm_regens`); when the budget is gone it is a fight to the death. A team with no saucer
in the air **and** no reinforcement left loses the game (a game lasts about 90 s and 40
kills with the defaults). 10 s later the next game starts and the loser fields one
extra saucer. Every kill, reinforcement, abduction and game result is one line in
`~/.local/state/reverie/match.log`. Score = kills + cows abducted per game; games won are persisted per model
pair in `~/.local/state/reverie/score-<A>-vs-<B>.txt` (`reverie reset score` clears them).
When a team loses a saucer its commander is asked for a one-line battle cry, shown in
the HUD for 20 s.

**Evolve** (`lm_evolve = true`, or `--evolve on|off`): when a saucer dies, its
commander gets the post-mortem (who killed it, from what range, what it was doing, how
long it had been flying damaged, how outnumbered it was) and writes one rule to avoid
that death next time; a wiped-out fleet gets a bigger-picture post-mortem as well. Rules go into the commander's prompt for the rest of the match
and persist in `~/.local/state/reverie/lessons/`, so the models keep evolving across
sessions. Nothing is fine-tuned: this is in-context learning, which is what fits next
to a screensaver on an 8 GB card.

**Evolve, the genetic kind** (`reverie run ufo-battle evolve`, or `lm_genetic = true`):
each team also carries a *doctrine*, six tactical parameters with hard bounds (flee
below N hp, never flee above M hp, abduct only a cow within D when no enemy is within
R laser ranges, focus-fire probability, how much of the fleet may flee at once). The
doctrine is spelled out in the commander's prompt and **enforced** on every order it gives:
an order outside the bounds is corrected and the model is told how many were corrected.
A genetic algorithm (population of 6 per team, in Rust, no extra model calls) scores the
active doctrine every 90 s of play or at game end by kills − losses + ½ cows (± a win
bonus) per minute, then breeds the next generation by crossover and bounded mutation.
Both teams evolve in parallel; populations persist in `~/.local/state/reverie/doctrine-*.txt`
and show up in `reverie arena lessons`. This follows the AutoSafe idea (an explicit
threat model + safe-action correction + reflection): the language model explores tactics
only inside a heuristic envelope, and what evolves is the envelope. `reverie reset model`
forgets lessons and doctrines; `reverie reset all` also resets the scoreboard.

Config keys (`reverie config` prints them all): `ufo_pilots`, `lm_backend`
(`auto` | `cuda` | `mlx`), `lm_model_a`, `lm_model_b`, `lm_vram_gb`, `lm_quant`
(`8bit`/`4bit` via bitsandbytes for bigger pairs), `lm_python`, `lm_think_seconds`,
`lm_evolve` (lessons), `lm_genetic` (doctrine GA), `lm_max_alive`, `lm_regens`. Any Hugging Face causal LM with a chat template works; on MLX the
`mlx-community/*-4bit` repos are the small, fast choice. The sidecar's log is
`~/.local/state/reverie/arena.log`.

## Config

```sh
reverie config > ~/.config/reverie/config.toml           # then edit
reverie pause 60 / reverie resume                        # screensaver mode, e.g. during a screen share
```

Screensaver keys: `idle_seconds` (300), `scenes` (`["ufo"]` needs no GPU; `["ufo-battle"]`
lets the models play while you're away), `rotate_minutes`, `fps` (30), `unfocused_fps`
(12), `max_instances` (3), `tolerance` (5).

## How it stays light

* **Watcher (screensaver mode): ~1 MB RSS per shell**, sleeps until the idle deadline, polls nothing.
* **Saver: ~0.35 ms per frame, ~30 KB of output per frame** at 200×55 (see bench).
  The encoder only redraws changed cells, ignores sub-threshold colour drift, and
  tracks cursor/SGR state — terminal CPU scales with bytes, so bytes are the budget.
* **0.66 MB static-ish binary**, one dependency (`libc`). The language models live in a
  separate Python process that exists only while `ufo-battle` is on screen.

## How idle detection works (and why SIGALRM)

Measured, not assumed: bash **defers USR1/USR2 traps while idle in readline** (they
only fire after the next Enter), but services **SIGALRM immediately**. So a watcher
checks that the shell owns the terminal (no command running) and the tty has had
no input for `idle_seconds`, writes a marker, and sends SIGALRM; the shell's trap
runs `reverie run --idle-trigger`, which verifies the marker (stray alarms are no-ops).
Verified in bash 5.2, zsh 5.9, fish 3.7. Your half-typed line survives; the wake
key is swallowed. On Linux the watcher reads `/proc`; on macOS it asks `ps` for the
tty's foreground process group (the shell snippet passes `--tty "$(tty)"`).

## Verification

```sh
reverie bench --size 200x55                    # vs locked eval/thresholds.toml
python3 eval/test_terminal.py                  # 18 pty checks across bash/zsh/fish
python3 eval/test_arena.py                     # sidecar prompt/parsing/lessons, no GPU
reverie arena check --load                     # the GPU side: load both models, time a decision
reverie snapshot --scene ufo --out u.ansi && python3 eval/ansi2png.py u.ansi u.png
```

## Performance and lag

The whole frame loop costs about 0.5 ms (update 0.01, render 0.3, encode 0.15 ms) and 3 percent
of one core at 60 fps on a 200×55 terminal, so lag can only come from the terminal draining
bytes. The saver writes a diff (only changed cells, 25–35 KB per frame); if the terminal cannot
keep up, `write()` blocks. A **lag guard** watches for that and halves the frame rate for 5 s
while raising the encoder's colour tolerance (fewer changed cells, fewer bytes), relaxing back
once writes are fast again. Measured under a terminal throttled to 600 KB/s: 25 fps with the
guard, 15 fps without it, 58 fps on a fast terminal either way. `reverie --perf FILE` (or `REVERIE_PERF=FILE`) writes one line per second with fps,
per-stage timings, bytes per frame, the longest frame gap, CPU share and lag-guard events;
that is how the numbers above were taken. The sidecar runs at lower CPU priority (`nice 5`).
If your terminal struggles, `fps = 30` in the config halves the byte rate.

## Why Rust, and what is enforced

The binary must live inside every shell for hours and take over the terminal without
ever damaging it, so it is written to be small, memory-safe and auditable:

* one dependency (`libc`); `cargo audit` runs in CI against the RustSec database; builds are
  `--locked`;
* `cargo clippy -- -D warnings` and `cargo fmt --check` are CI gates, 0 warnings; integer
  `overflow-checks` stay on in release (this caught a real overflow in the noise sampler);
* `unsafe` appears only where POSIX requires it (termios, ioctl, signals, fork/setsid,
  flock, getuid/chmod); every site carries a `// SAFETY:` justification, enforced by clippy;
* the Python sidecar passes `bandit` and `ruff`; model downloads can be pinned to a commit;
* `scripts/qa.sh` runs the whole gate locally; see `SECURITY.md` for the threat model;
* everything that comes back from the language models is filtered to printable ASCII
  before it can reach the terminal or a file (no escape-sequence injection from a model);
* the sidecar's stdout is a fixed line protocol parsed by hand; its stderr goes to a log;
  the binary never opens a network connection (models are fetched by the Python side);
* the runtime dir is created `0700`; state lives under XDG paths only; `reverie remove`
  deletes exactly those paths and nothing else;
* release builds use `panic = "abort"` with a panic hook that restores the terminal first.

## Known limits

* GNOME Terminal/Ptyxis have no pixel graphics protocol, so art is at half-block
  resolution (a maximised 200×55 terminal = 200×110 pixels).
* bash: after waking, the cursor shows at column 0 until your first keystroke
  (the line itself is intact and typing continues correctly).
* tty idle time has 8-second kernel granularity; `idle_seconds` is honoured ±8 s.
* zsh users with `TMOUT` + their own `TRAPALRM` are chained, not replaced.
* The commanders are 0.6B–1.7B models: they misread the field, forget a ship, or copy
  the enemy's taunt now and then. That is part of the show; the steering layer keeps
  the saucers flying sensibly under any order.
* The macOS watcher and the MLX backend were written against the documented APIs and
  cross-compiled here, not run on a Mac yet.

MIT licensed. Architecture and internals: `docs/ARCHITECTURE.md`. Security posture: `SECURITY.md`. History: `CHANGELOG.md`.
