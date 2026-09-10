# reverie

A tiny terminal screensaver. When your shell sits idle at the prompt, two saucer
factions dogfight over a sleeping city — then any key puts you back exactly where
you were, half-typed command included.

| scene | what happens |
|---|---|
| `ufo` | the baseline: built-in pilots pursue, orbit, flee when damaged and abduct cows when bored. No GPU, nothing to install. |
| `ufo-battle` | the same graphics, but **two small language models command the teams** (one per faction). They read the situation, give each saucer an order, decide when to warp in reinforcements, taunt each other in the HUD — and, with *evolve* on, write a lesson after every saucer they lose and play by it from then on. |

Built for the default Ubuntu terminal (GNOME Terminal / Ptyxis, both VTE):
truecolor half-block + braille rendering, no graphics protocol needed. Works in any
truecolor terminal (256-colour fallback). Linux and macOS.

## Install

```sh
cargo install --path .          # or grab a release binary and put it on your PATH
reverie run ufo                 # preview the baseline now (any key exits)
reverie install                 # auto-start when your prompt idles (bash, zsh, fish)
```

`install` appends a marked block to your rc file (with a timestamped backup);
`reverie uninstall` removes it and stops the watchers. Editing the source does not
touch the installed binary: run `cargo install --path .` again (and open a new
terminal, or `pkill -f "reverie watch"`) to pick up changes.

## The language-model battle

```sh
pip install torch transformers          # CUDA GPU (Linux/Windows-WSL); a CUDA build of torch
pip install mlx-lm                      # Apple silicon Mac
reverie arena check --load              # verifies python/GPU/models, times one decision per model
reverie run cuda ufo-battle             # play (downloads the two models on first run, ~5 GB)
reverie run mlx ufo-battle              # same on a Mac
reverie arena lessons                   # what each commander has learned so far
reverie arena forget                    # wipe their memories
```

Defaults are chosen for an **8 GB CUDA card**: team ZORB is `Qwen/Qwen3-0.6B`, team
KRELL is `HuggingFaceTB/SmolLM2-1.7B-Instruct`; together they take about 5 GB of GPU
memory (bf16/fp16), and the sidecar caps itself at `lm_vram_gb` (6 GB) so it can never
crowd out the desktop. Decisions take 0.2–0.8 s on an RTX 4000 Ada; each team gets new
orders every 1–2 s while the low-level steering keeps flying smoothly in between.

How it works: `reverie` spawns `agents/arena.py` (embedded in the binary, written to
`~/.local/share/reverie/arena.py`) and talks a line protocol over pipes. Each team
starts with 4 saucers. Every second or so a commander receives a compact text
situation (its ships, hp, nearest enemy, cows, recent events, the enemy's last taunt)
and answers with one order per ship — `attack E4`, `hunt`, `flee`, `abduct C1`,
`guard S2`, `patrol` — plus `DEPLOY: n` to launch replacements for destroyed saucers
and a `SAY:` line shown in the HUD. Orders are executed by the same steering
behaviours as the baseline pilots, so the graphics and the feel of the fight stay the same.

**Evolve** (`lm_evolve = true`, or `--evolve on|off`): when a saucer dies, its
commander gets the post-mortem (who killed it, from what range, what it was doing, how
long it had been flying damaged, how outnumbered it was) and writes one rule to avoid
that death next time. Rules go into the commander's prompt for the rest of the match
and persist in `~/.local/state/reverie/lessons/`, so the models keep evolving across
sessions. Nothing is fine-tuned: this is in-context learning, which is what fits next
to a screensaver on an 8 GB card.

Config keys (`reverie config` prints them all): `ufo_pilots`, `lm_backend`
(`cuda` | `mlx` | `auto`), `lm_model_a`, `lm_model_b`, `lm_vram_gb`, `lm_quant`
(`8bit`/`4bit` via bitsandbytes for bigger pairs), `lm_python`, `lm_think_seconds`,
`lm_evolve`. Any Hugging Face causal LM with a chat template works; on MLX the
`mlx-community/*-4bit` repos are the small, fast choice. The sidecar's log is
`~/.local/state/reverie/arena.log`.

## Use

```sh
reverie run ufo                            # baseline
reverie run cuda ufo-battle                # or: reverie arena
reverie list                               # scenes
reverie pause 60 / reverie resume          # e.g. during a screen share
reverie config > ~/.config/reverie/config.toml           # then edit
```

Config highlights: `idle_seconds` (300), `scenes` (`["ufo"]`, or `["ufo-battle"]` to
have the models play while you're away), `rotate_minutes`, `fps` (30),
`unfocused_fps` (12), `max_instances` (3), `tolerance` (5).

## How it stays light

* **Watcher: ~1 MB RSS per shell**, sleeps until the idle deadline, polls nothing.
* **Saver: ~0.15 ms per frame, ~23 KB of output per frame** at 200×55 (see bench).
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

MIT licensed.
