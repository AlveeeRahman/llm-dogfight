# reverie

![reverie: two saucer teams dogfighting over a sleeping city](eval/snapshots/ufo.png)

**A UFO dogfight in your terminal, flown by two small language models running on your own
machine.** Type `reverie`. Team ZORB and team KRELL, each commanded by a different local model,
start fighting over a sleeping city: they chase, retreat, steal cows, shout at each other, and
learn from every saucer they lose. Any key ends it.

Works in the default Ubuntu terminal (GNOME Terminal / Ptyxis) and any other truecolor
terminal, on Linux and macOS, with no graphics protocol: it is plain text, drawn with
half-blocks and braille. The binary is a 0.7 MB Rust program with one dependency; the models
run in a small Python sidecar on CUDA or Apple MLX and fit an 8 GB GPU.

## Quick start

```sh
# 1. the tool
cargo install --path .            # or drop a release binary on your PATH

# 2. a model runtime (one of these)
pip install torch transformers    # NVIDIA GPU with 8 GB or more (a CUDA build of torch)
pip install mlx-lm                # Apple silicon Mac

# 3. play
reverie                           # first run downloads the two default models (~5 GB)
```

No GPU? `reverie ufo` runs the same dogfight with the built-in pilots and needs nothing but
the binary.

## Commands

| command | what it does |
|---|---|
| `reverie` | start a battle; picks CUDA, or MLX on Apple silicon |
| `reverie cuda` / `reverie mlx` | start a battle with the backend chosen by hand |
| `reverie evolve` | start a battle where a genetic algorithm also evolves each team's tactics |
| `reverie ufo` | the built-in pilots: no models, no GPU |
| `reverie models` | the model catalogue and what fits your GPU |
| `reverie --zorb MODEL --krell MODEL` | pick the two commanders (an alias from the catalogue or any Hugging Face id) |
| `reverie arena check --load` | verify Python, GPU and models; load both and time a decision |
| `reverie arena pull` | download the models ahead of time |
| `reverie arena lessons` | what each commander has learned so far |
| `reverie reset model \| score \| all` | forget the lessons, the games won, or both |
| `reverie install` / `reverie uninstall` | optional screensaver mode: play whenever your shell sits idle |
| `reverie remove` | delete everything reverie put on the machine, including itself |

Options for a battle: `--lessons on|off`, `--fps N`, `--seed N`, `--duration SECS`,
`--perf FILE`. `reverie --help` lists everything.

## How a battle works

**Rules.** Each team has 4 saucers on screen. A destroyed saucer is replaced 4 s later from a
budget of 20 reinforcements per game; when a team has nothing in the air and nothing left to
send, it loses the game. Ten seconds later the next game starts, and the loser fields an extra
saucer. Score is kills plus cows abducted; games won are remembered per model pair. A game
lasts about a minute and a half. Every kill, reinforcement, abduction and result is one line in
`~/.local/state/reverie/match.log`.

**Commanders.** About once a second each model receives a compact text description of the
situation (its saucers with hit points, nearest enemy and nearest cow, the enemy's saucers,
recent events, the enemy's last battle cry) and answers with one order per saucer:
`attack E4`, `hunt`, `flee` or `abduct C1`. The orders are flown by the same steering code as
the built-in pilots, so the fight stays smooth no matter how slow or confused a model is.
When a team loses a saucer, its commander shouts a battle cry that shows in the HUD.

**Learning.** After every loss the commander is shown the post-mortem (who killed the saucer,
from what range, what it was doing, how long it had been flying damaged, how outnumbered it
was) and writes one rule to avoid that fate. Rules stay in its prompt for the rest of the
session and persist on disk, so a pair of models keeps evolving across battles. Nothing is
fine-tuned; this is in-context learning, which is what fits next to a game on an 8 GB card.

**Evolve.** With `reverie evolve`, each team also carries a *doctrine*: six tactical
parameters with hard bounds (when to flee, when fleeing is forbidden, how close a cow must be
and how far the enemy, how often to focus fire, how much of the fleet may retreat at once).
The doctrine is spelled out in the model's prompt and enforced on every order it gives, and a
genetic algorithm scores the active doctrine over each game and breeds the next generation.
The model explores tactics only inside a heuristic envelope, and the envelope is what evolves.
It runs in Rust with no extra model calls, so it costs nothing.

## Models

The defaults are `Qwen/Qwen3-0.6B` for ZORB and `HuggingFaceTB/SmolLM2-1.7B-Instruct` for
KRELL: two families, both ungated, 4.6 GB of GPU memory together. `reverie models` prints the
catalogue with sizes and whether each entry fits 8 GB next to your other model. Every listed
model is **ungated**: no licence click-through, no token.

| alias | model | bf16 | notes |
|---|---|---|---|
| `qwen3-0.6b` | Qwen/Qwen3-0.6B | 1.5 GB | default for ZORB; fast and decisive |
| `qwen3-1.7b` | Qwen/Qwen3-1.7B | 4.1 GB | stronger Qwen; pair it with a small partner |
| `qwen2.5-0.5b` | Qwen/Qwen2.5-0.5B-Instruct | 1.0 GB | tiny and quick |
| `qwen2.5-1.5b` | Qwen/Qwen2.5-1.5B-Instruct | 3.1 GB | |
| `smollm2-360m` | HuggingFaceTB/SmolLM2-360M-Instruct | 0.7 GB | under 1B: often skips the order format |
| `smollm2-1.7b` | HuggingFaceTB/SmolLM2-1.7B-Instruct | 3.4 GB | default for KRELL |
| `llama3.2-1b` | unsloth/Llama-3.2-1B-Instruct | 2.5 GB | Meta's Llama 3.2 1B, ungated mirror |
| `gemma3-1b` | unsloth/gemma-3-1b-it | 2.0 GB | Google's Gemma 3 1B, ungated mirror |
| `lfm2-1.2b` | LiquidAI/LFM2-1.2B | 2.3 GB | Liquid AI, built for on-device use |
| `lfm2-700m` | LiquidAI/LFM2-700M | 1.5 GB | under 1B: often skips the order format |
| `olmo2-1b` | allenai/OLMo-2-0425-1B-Instruct | 3.0 GB | fully open training recipe; likes to abduct |
| `falcon3-1b` | tiiuae/Falcon3-1B-Instruct | 3.3 GB | |
| `danube3-500m` | h2oai/h2o-danube3-500m-chat | 1.0 GB | under 1B: often skips the order format |
| `deepseek-r1-1.5b` | deepseek-ai/DeepSeek-R1-Distill-Qwen-1.5B | 3.6 GB | thinking model: slower, chatty |
| `granite3.3-2b` | ibm-granite/granite-3.3-2b-instruct | 5.1 GB | `lm_quant = "8bit"` next to a partner |
| `smollm3-3b` | HuggingFaceTB/SmolLM3-3B | 6.2 GB | `lm_quant = "8bit"` next to a partner |
| `qwen2.5-3b` | Qwen/Qwen2.5-3B-Instruct | 6.2 GB | `lm_quant = "8bit"` next to a partner |

```sh
reverie --zorb lfm2-1.2b --krell llama3.2-1b     # try a pair once
reverie arena check --load                        # confirm both load and see the peak GPU memory
```

To keep a pair, set `lm_model_a` and `lm_model_b` in the config. Any Hugging Face model with
a chat template and safetensors weights works; append `@<commit>` to pin the weights. On MLX
the same ids work through mlx-lm, and the `mlx-community/*-4bit` repos are smaller and faster.
Sizes above are bf16 safetensors as reported by the Hugging Face API in September 2026. The
defaults and the 1B-class entries (Llama 3.2, Gemma 3, LFM2, OLMo 2, Falcon3, danube3, SmolLM2
360M, Qwen2.5 0.5B) were each loaded and asked for orders; the 1B-and-up models answer in the
format almost every time, the sub-1B ones often don't (a saucer without a new order keeps its
last one, so the fight goes on either way). The 3B-class entries are listed on size alone. Models whose chat template has no system role
(h2o-danube, Gemma 2) get the system prompt folded into the user turn automatically.

## Screensaver mode

```sh
reverie install                  # bash, zsh or fish: play when the prompt has been idle
reverie pause 60                 # quiet for an hour, e.g. during a screen share
reverie uninstall                # remove the hook again
```

`install` appends a marked block to your shell rc file (with a timestamped backup). A tiny
watcher (about 1 MB) sleeps in each shell and wakes the saver after `idle_seconds`; it only
fires when the shell is sitting at its prompt, so builds and editors are never interrupted.
Any key restores your prompt with the half-typed line intact. By default the screensaver runs
the built-in pilots (`scenes = ["ufo"]`); set `scenes = ["ufo-battle"]` to let the models play
while you are away.

## Configuration

`reverie config` prints the default file; copy it to `~/.config/reverie/config.toml` and edit.

| key | default | meaning |
|---|---|---|
| `lm_backend` | `auto` | `cuda`, `mlx`, or `auto` (MLX on Apple silicon, CUDA elsewhere) |
| `lm_model_a`, `lm_model_b` | Qwen3-0.6B, SmolLM2-1.7B | the two commanders |
| `lm_vram_gb` | 6 | GPU memory cap for the sidecar (both models) |
| `lm_quant` | `none` | `8bit` or `4bit` via bitsandbytes for bigger pairs (CUDA) |
| `lm_evolve` | true | write and use lessons after each loss |
| `lm_genetic` | false | doctrine evolution (same as the `evolve` word) |
| `lm_max_alive`, `lm_regens` | 4, 20 | saucers on screen; reinforcements per game |
| `lm_think_seconds` | 1.0 | minimum pause between a team's orders |
| `lm_python` | `python3` | the interpreter that has torch or mlx-lm |
| `idle_seconds`, `fps`, `scenes` | 300, 30, `["ufo"]` | screensaver mode |
| `tolerance` | 5 | colour change ignored between frames (fewer bytes) |

## Performance

The frame loop costs about half a millisecond and 3 percent of a core at 60 fps on a 200×55
terminal; only changed cells are sent (25 to 35 KB per frame). Lag can therefore only come from
a terminal that cannot drain bytes fast enough. A lag guard detects blocked writes, halves the
frame rate for a few seconds and raises the colour tolerance so fewer cells change, then relaxes
again; under a terminal throttled to 600 KB/s that keeps the battle at 25 fps. `reverie --perf
FILE` writes one line per second with fps, per-stage timings, bytes per frame, the longest
frame gap and CPU share. If your terminal struggles, set `fps = 30`.

## Security and quality

reverie takes over your terminal, so it is built to be small, memory-safe and auditable:

- one dependency (`libc`); `cargo audit`, `cargo clippy -D warnings`, `cargo fmt --check` and
  `--locked` builds are CI gates; every `unsafe` block carries a justification that clippy
  checks; integer overflow checks stay on in release;
- everything a model produces is filtered to printable ASCII before it can reach the terminal
  or a file; models never get a tool, a file or a shell;
- the binary never opens a network connection; the sidecar downloads models through
  `huggingface_hub` (safetensors, no remote code), pinnable to a commit; it passes `bandit`
  and `ruff`;
- the terminal is restored on every exit path, including SIGTERM and panics;
- `reverie remove` deletes exactly what reverie created.

Details and the threat model: [SECURITY.md](SECURITY.md). Run the whole gate locally with
`scripts/qa.sh --full`.

## Development

```sh
scripts/qa.sh --full                                # fmt, clippy, audit, build, bench, tests, pty harness
reverie bench --size 200x55 | python3 eval/check_bench.py /dev/stdin     # frame time and bytes vs eval/thresholds.toml
reverie snapshot --scene ufo --out u.ansi && python3 eval/ansi2png.py u.ansi u.png   # deterministic frame to PNG
```

How it is put together, from the SIGALRM idle wake to the sidecar protocol and the genetic
evolver: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Release history: [CHANGELOG.md](CHANGELOG.md).

## Known limits

- Character cells, not pixels: a maximised terminal gives 200×110 pixels. Sextant and octant
  characters could quadruple that on recent terminals; not done yet.
- The commanders are 0.6B to 1.7B models. They misread the field, forget a saucer or echo the
  enemy's cry now and then; the steering layer keeps the saucers flying sensibly regardless.
- The macOS watcher and the MLX backend compile and are unit-tested but have not been run on a
  Mac yet.
- bash shows the cursor at column 0 after waking until the first keystroke; the line is intact.

MIT licensed.
