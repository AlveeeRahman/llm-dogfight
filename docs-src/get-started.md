# Get started

LLM Dogfight has two parts. The game is a Rust binary called `dogfight`. The commanders run in a Python sidecar that loads the two models with PyTorch on an NVIDIA GPU, with MLX on Apple silicon, or on the CPU.

## Install the game

With the Rust toolchain from [rustup.rs](https://rustup.rs):

```sh
cargo install --git https://github.com/AlveeeRahman/llm-dogfight
```

Or download a binary for Linux or macOS from the [Releases page](https://github.com/AlveeeRahman/llm-dogfight/releases) and put it on your `PATH`. Check it with `dogfight --version`.

## Install a model runtime

Pick the one that matches your machine:

| machine | install | notes |
|---|---|---|
| NVIDIA GPU, 8 GB or more | `pip install torch transformers` | needs a CUDA build of torch |
| Apple silicon Mac | `pip install mlx-lm` | the models share unified memory |
| No GPU | `pip install torch transformers` | `dogfight cpu`: slow, tiny models only |

Bigger pairs on a CUDA card can be loaded in 8-bit or 4-bit with `pip install bitsandbytes` and `lm_quant = "8bit"` in the config.

The sidecar runs with `python3` by default. If torch or mlx-lm lives in another interpreter, set `lm_python` in `~/.config/dogfight/config.toml` (see [Configuration](configuration.md)).

## Check the setup

```sh
dogfight arena check          # Python, torch or mlx, the GPU, and whether the two models are downloaded
dogfight arena check --load   # also loads both models and times one decision
dogfight arena pull           # downloads the two models ahead of the first battle
```

## Play

```sh
dogfight                      # CUDA, or MLX on Apple silicon, picked for you
dogfight cuda                 # or mlx, or cpu: the backend chosen by hand
dogfight ufo                  # the built-in pilots: no models, no GPU
dogfight evolve               # the battle, plus a genetic algorithm that tunes each team's doctrine
```

Any key ends the fight and gives the terminal back as it was. The first battle downloads the default pair, Qwen3-0.6B for ZORB and SmolLM2-360M for KRELL, 2.2 GB together.

## Choose the commanders

```sh
dogfight models                                    # the catalogue, what fits your machine, the strongest pair
dogfight --zorb qwen3-0.6b --krell gemma3-1b       # an alias from the catalogue
dogfight --zorb Qwen/Qwen3-1.7B --krell HuggingFaceTB/SmolLM2-1.7B-Instruct   # or any Hugging Face id
```

[Model templates](templates.md) has a ready pair for each GPU and Mac size.

## Remove it

```sh
dogfight remove               # lists everything it would delete and asks first
dogfight remove --keep-models # keeps the downloaded models
```
