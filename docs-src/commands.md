# Commands

Every command and flag below is read from the argument parser in `src/main.rs`. `dogfight --help` (or `-h`) prints the short form, and `dogfight --version` prints the version.

## Battles

| command | what it does |
|---|---|
| `dogfight` | starts a battle between the two models; the backend is CUDA, or MLX on Apple silicon |
| `dogfight cuda`, `dogfight mlx`, `dogfight cpu` | the same battle with the backend chosen by hand; `cpu` needs no GPU and is slow |
| `dogfight evolve` | a battle where a genetic algorithm also evolves each team's doctrine (see [Doctrine and evolution](doctrine.md)) |
| `dogfight ufo` | the built-in pilots: no models, no GPU, no Python |
| `dogfight battle`, `dogfight ufo-battle` | the same as `dogfight` |
| `dogfight arena` | the same as `dogfight`; `dogfight arena cuda`, `mlx`, `cpu` or `evolve` pick the backend or add evolution |
| `dogfight run [cuda\|mlx\|cpu] [ufo\|ufo-battle] [evolve]` | the long form: the words pick the backend, the scene and evolution, in any order; `play`, `demo` and `preview` are other names for `run` |

Backend and evolution words can be combined, for example `dogfight cuda evolve` or `dogfight run mlx ufo-battle evolve`. If the sidecar cannot start, the battle falls back to the built-in pilots and says why in the HUD.

### Battle options

These work with every battle command.

| option | what it does |
|---|---|
| `--zorb MODEL` | the commander of team ZORB: a catalogue alias or a Hugging Face id, with `@<commit>` to pin the weights |
| `--krell MODEL` | the commander of team KRELL, the same way |
| `--backend auto\|cuda\|mlx\|cpu` | the backend, the same as the backend word |
| `--lessons on\|off` | write and use a lesson after each loss (default on; `--evolve on\|off` is the same flag) |
| `--genetic on\|off` | doctrine evolution, the same as the `evolve` word |
| `--fps N` | frame rate for this run |
| `--seed N` | the random seed of the field |
| `--duration SECS` | end the battle after this many seconds |
| `--perf FILE` | write one line per second: fps, timings per stage, bytes per frame, longest frame gap and CPU share |
| `--shot SECS:FILE` | save the live frame at second SECS to FILE as ANSI text (turn it into a PNG with `eval/ansi2png.py`) |
| `--scene NAME`, `--pilots builtin\|lm` | the scene and who flies it, for `dogfight run` |

An alias that is not in the catalogue and has no `/` stops the command with exit code 2 and suggests aliases that start the same way.

## The sidecar

| command | what it does |
|---|---|
| `dogfight arena check` | reports Python, torch or mlx-lm, the GPU and whether both models are downloaded |
| `dogfight arena check --load` | also loads both models, times one decision and reports the memory they took |
| `dogfight arena pull` | downloads the two configured models ahead of the first battle |
| `dogfight arena lessons` | prints what each commander has learned, and both teams' evolved doctrine populations |
| `dogfight arena forget` | the same as `dogfight reset model` |

Before it runs, `check` and `pull` print the pair, the backend, the GPU memory cap, whether lessons are on, and the config file in use.

## Models

`dogfight models` prints the catalogue with a verdict for your machine:

- the machine line: your GPU and its memory (several GPUs get one model each), or the unified memory of a Mac, and the room left for models;
- one row per model: alias, Hugging Face id, bf16 size, a note, and whether it fits next to your other configured model;
- a `*` next to the current pair;
- the strongest balanced pair for the machine, as a ready `dogfight --zorb ... --krell ...` line.

How the room is worked out is on [Model templates](templates.md#how-the-tool-picks-a-pair).

## Memory and scores

| command | what it does |
|---|---|
| `dogfight reset model` | forgets the commanders' lessons and both evolved doctrines |
| `dogfight reset score` | forgets games won and lost, and the lifetime kills and cows, for every pair |
| `dogfight reset all` | both |

## Setup and removal

| command | what it does |
|---|---|
| `dogfight status` | the config file, frame rates, colour mode, pilots, the pair and backend, the GPU cap, the Python in use, the state folder and the sidecar log |
| `dogfight config` | prints the default config file; copy it to `~/.config/dogfight/config.toml` |
| `dogfight list` | the scenes, and the backend the battle would use |
| `dogfight remove` | lists everything the tool put on the machine and asks before deleting it: config, lessons, scores, logs, the models it downloaded, and the binary |
| `dogfight remove --yes` | the same without the question |
| `dogfight remove --keep-models` | keeps the downloaded models; everything else goes |

`remove` deletes only models the tool would have downloaded itself: the catalogue models and the two configured ones. Anything else in your Hugging Face cache stays.

## Development

| command | what it does |
|---|---|
| `dogfight bench [--scene NAME] [--size 200x55] [--frames 600] [--seed 42]` | renders frames without a terminal and prints one JSON line: mean, p95 and max frame time, bytes per frame and peak memory |
| `dogfight snapshot --scene NAME --out FILE [--size 120x36] [--frames 300] [--seed 42]` | renders one deterministic frame to an ANSI file |

Bench and snapshot always use the built-in pilots, so their output is the same on every run. [Development](development.md) shows how CI uses them.

## Exit codes

| code | meaning |
|---|---|
| 0 | done |
| 1 | a file could not be written, the sidecar failed, or `remove` was declined |
| 2 | an unknown command, subcommand, scene or model alias, or a missing `--out` |
