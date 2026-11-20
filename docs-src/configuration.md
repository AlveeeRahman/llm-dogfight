# Configuration

The config file is `~/.config/dogfight/config.toml` (or `$XDG_CONFIG_HOME/dogfight/config.toml`). Without one, the defaults below apply. `dogfight config` prints the default file, and `dogfight status` shows which file is in use.

```sh
mkdir -p ~/.config/dogfight
dogfight config > ~/.config/dogfight/config.toml
```

Command-line options win over the file for one run: `--zorb`, `--krell` and `--backend` replace the pair and the backend, and `--fps`, `--lessons` and `--genetic` replace those settings.

## Every key

The limits are the ones `src/config.rs` applies when it reads the file. A value outside the allowed set falls back to the default, and unknown keys are ignored.

| key | default | allowed | meaning |
|---|---|---|---|
| `lm_backend` | `"auto"` | `auto`, `cuda`, `mlx`, `cpu` | `auto` is MLX on Apple silicon and CUDA elsewhere |
| `lm_model_a` | `"Qwen/Qwen3-0.6B"` | a Hugging Face id, `@<commit>` to pin | the commander of team ZORB |
| `lm_model_b` | `"HuggingFaceTB/SmolLM2-360M-Instruct"` | a Hugging Face id, `@<commit>` to pin | the commander of team KRELL |
| `lm_vram_gb` | `"auto"` | `auto`, or 1 to 512 | GPU memory cap for the sidecar; `auto` is the card's memory minus 2 GB, at least 4 GB |
| `lm_quant` | `"none"` | `none`, `8bit`, `4bit` | bitsandbytes quantization on CUDA, for bigger pairs |
| `lm_python` | `"python3"` | a path or command | the interpreter that has torch or mlx-lm |
| `lm_think_seconds` | `1.0` | seconds | the shortest pause between two sets of orders from one team |
| `lm_evolve` | `true` | `true`, `false` | write a lesson after each loss and keep it in the prompt (`lm_lessons` is the same key) |
| `lm_genetic` | `false` | `true`, `false` | evolve each team's doctrine, the same as the `evolve` word |
| `lm_max_alive` | `4` | 1 to 4 | saucers per team on screen; the loser of a game starts the next with one more |
| `lm_regens` | `20` | 0 to 500 | reinforcements per team per game, then it is a fight to the death |
| `fps` | `30` | 1 to 120 | frames per second |
| `unfocused_fps` | `12` | 1 to 120 | frames per second while the terminal window is unfocused |
| `color` | `"auto"` | `auto`, `truecolor`, `256` | `auto` turns on truecolor when `COLORTERM` says so, in VTE terminals, and in kitty and Ghostty |
| `tolerance` | `5` | 0 to 32 | colour change per channel ignored between frames; higher sends fewer bytes, and 3 to 8 is invisible |
| `ufo_pilots` | `"builtin"` | `builtin`, `lm` | who flies the `ufo` scene under `dogfight run`; `pilots` is the same key |

## The default file

This is what `dogfight config` prints:

```toml
# dogfight — ~/.config/dogfight/config.toml

# Frames per second while animating, and while the window is unfocused.
fps = 30
unfocused_fps = 12

# "auto", "truecolor" or "256"
color = "auto"

# Colour change (0-255 per channel) ignored between frames. Higher = fewer bytes
# for the terminal to draw = lower CPU. 3-8 is invisible in practice.
tolerance = 5

# Who flies the "ufo" scene: "builtin" (heuristic pilots, no GPU) or "lm" (same as the
# "ufo-battle" scene: two small language models command the two teams; needs python3 with
# torch+transformers on a CUDA GPU, or mlx-lm on Apple silicon). Try `dogfight run cuda ufo-battle`;
# check the setup with `dogfight arena check --load`.
ufo_pilots = "builtin"

# Language-model commanders. Defaults are the smallest pair that plays well (2.2 GB together).
lm_backend = "auto"                                 # auto = cuda, or mlx on Apple silicon; cpu = no GPU (tiny models); or `dogfight cuda|mlx|cpu`
lm_model_a = "Qwen/Qwen3-0.6B"                      # team ZORB
lm_model_b = "HuggingFaceTB/SmolLM2-360M-Instruct"  # team KRELL
lm_vram_gb = "auto"                                 # CUDA memory cap for the sidecar: auto = your card's memory
                                                    # minus 2 GB (6 GB on an 8 GB card), or a number of GB
lm_quant = "none"                                   # cuda: none | 8bit | 4bit (bitsandbytes) for bigger models
lm_python = "python3"                               # interpreter that has the ML packages
lm_think_seconds = 1.0                              # minimum pause between a team's orders
lm_evolve = true                                    # learn from every destroyed saucer (lessons persist in
                                                    # ~/.local/state/dogfight/lessons; `dogfight reset model` clears)
lm_genetic = false                                  # evolve each team's bounded doctrine with a genetic algorithm
                                                    # (same as the `evolve` word: `dogfight run ufo-battle evolve`)
lm_max_alive = 4                                    # saucers per team on screen (a game's loser restarts with +1)
lm_regens = 20                                      # reinforcements per team per game; then it is a fight to the death
```

## Where files go

| folder | default | follows |
|---|---|---|
| config | `~/.config/dogfight/` | `XDG_CONFIG_HOME` |
| state: lessons, doctrines, scores, logs | `~/.local/state/dogfight/` | `XDG_STATE_HOME` |
| data | `~/.local/share/dogfight/` | `XDG_DATA_HOME` |
| runtime, readable by you only | `/tmp/dogfight-<uid>/` | `XDG_RUNTIME_DIR` |
| downloaded models | `~/.cache/huggingface/hub/` | `HF_HOME` |

## Hugging Face downloads

Every catalogue model is ungated. Hugging Face slows anonymous downloads, so a free account and a read token lift the limit: run `hf auth login`, or set `HF_TOKEN`. The sidecar picks the token up on its own, and a download that hits the limit says so and retries.
