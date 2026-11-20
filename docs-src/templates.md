# Model templates

Each template below is a pair of commanders you can copy. The sizes are the bf16 weights from the catalogue in `src/main.rs`. The pair for each machine is the one `dogfight models` itself suggests on that machine, worked out with the rule described at the end of this page.

## By GPU memory

| card | command | weights | what to expect |
|---|---|---|---|
| any CUDA card | `dogfight --zorb qwen3-0.6b --krell smollm2-360m` | 2.2 GB | the defaults: a decision in about a tenth of a second, fast and messy |
| 8 GB | `dogfight --zorb qwen2.5-1.5b --krell gemma3-1b` | 5.1 GB | the strongest pair the tool finds under the 6 GB cap |
| 12 GB | `dogfight --zorb qwen3-1.7b --krell granite3.3-2b` | 9.2 GB | the smartest small pair |
| 16 GB | `dogfight --zorb smollm3-3b --krell qwen2.5-3b` | 12.4 GB | two 3B commanders |
| 24 GB | `dogfight --zorb qwen3-4b --krell phi4-mini` | 15.7 GB | the best mid-size commanders |
| 32 GB | `dogfight --zorb qwen3-4b --krell granite3.3-8b` | 24.3 GB | a 4B against an 8B |
| 48 GB | `dogfight --zorb qwen3-8b --krell granite3.3-8b` | 32.7 GB | two 8B commanders |
| 80 GB | `dogfight --zorb granite3.3-8b --krell qwen3-14b` | 45.8 GB | the largest model in the catalogue |

Two GPUs give each model its own card, so each card only has to hold one model.

## By Mac memory

On Apple silicon the models share unified memory with everything else. The tool allows 70 % of it for models.

| Mac | command | weights |
|---|---|---|
| 16 GB | `dogfight mlx --zorb qwen3-1.7b --krell smollm3-3b` | 10.3 GB |
| 24 GB | `dogfight mlx --zorb qwen3-4b --krell phi4-mini` | 15.7 GB |
| 32 GB | `dogfight mlx --zorb phi4-mini --krell olmo2-7b` | 22.3 GB |
| 64 GB | `dogfight mlx --zorb qwen3-8b --krell granite3.3-8b` | 32.7 GB |

The `mlx-community/*-4bit` builds are smaller and faster, and they work by id. `Qwen3-8B-4bit` is 4.6 GB and `Qwen3-14B-4bit` is 8.3 GB:

```sh
dogfight mlx --zorb mlx-community/Qwen3-8B-4bit --krell mlx-community/gemma-3-4b-it-4bit
```

## No GPU

```sh
dogfight ufo                                            # the built-in pilots, no models at all
dogfight cpu --zorb qwen2.5-0.5b --krell smollm2-360m   # real models on the processor, 1.7 GB
```

On the CPU both models run in float32, a few tokens per second, which is enough for one order per saucer every few seconds.

## Matchups with character

These pairs show how differently small models play. The first four were played through many full games on an RTX 4000 Ada.

| matchup | command | why it is worth watching |
|---|---|---|
| aggression against caution | `dogfight --zorb qwen3-0.6b --krell smollm2-1.7b` | Qwen attacks, SmolLM2 holds back (4.6 GB) |
| the most even small fight | `dogfight --zorb qwen3-0.6b --krell gemma3-1b` | the lead changes hands most often (3.5 GB) |
| the smartest small pair | `dogfight --zorb granite3.3-2b --krell qwen3-1.7b` | a card above 8 GB, or `lm_quant = "8bit"` (9.2 GB) |
| the defaults | `dogfight --zorb qwen3-0.6b --krell smollm2-360m` | fast decisions, and the small model sometimes skips the order format (2.2 GB) |
| the cow farmer | `dogfight --zorb olmo2-1b --krell qwen3-0.6b` | OLMo 2 is fond of abducting cows (4.5 GB) |
| the thinker | `dogfight --zorb deepseek-r1-1.5b --krell qwen3-0.6b` | the reasoning model thinks out loud and its orders arrive late (5.1 GB) |
| no guardrails | `dogfight --zorb mlabonne/gemma-3-1b-it-abliterated --krell Goekdeniz-Guelmez/Josiefied-Qwen3-1.7B-abliterated-v1` | refusals removed, rude battle cries (5.4 GB) |

Add `evolve` to any of them, for example `dogfight evolve --zorb qwen3-0.6b --krell gemma3-1b`, and a genetic algorithm tunes each team's doctrine as they play.

## Config file templates

To keep a pair, put it in `~/.config/dogfight/config.toml`. The config takes full Hugging Face ids, not aliases. `dogfight config` prints the whole default file.

A 12 GB NVIDIA card:

```toml
lm_backend = "cuda"
lm_model_a = "Qwen/Qwen3-1.7B"                       # team ZORB
lm_model_b = "ibm-granite/granite-3.3-2b-instruct"   # team KRELL
```

The same pair on an 8 GB card, in 8-bit (needs `pip install bitsandbytes`):

```toml
lm_backend = "cuda"
lm_model_a = "Qwen/Qwen3-1.7B"
lm_model_b = "ibm-granite/granite-3.3-2b-instruct"
lm_quant = "8bit"
```

A 16 GB Mac with 4-bit builds:

```toml
lm_backend = "mlx"
lm_model_a = "mlx-community/Qwen3-8B-4bit"
lm_model_b = "mlx-community/gemma-3-4b-it-4bit"
```

Pinned weights, for runs you want to repeat:

```toml
lm_model_a = "Qwen/Qwen3-0.6B@<commit>"
lm_model_b = "HuggingFaceTB/SmolLM2-360M-Instruct@<commit>"
```

Replace `<commit>` with a commit hash from the model's page on Hugging Face.

A long evolution session, with doctrines evolving and a slower order rate:

```toml
lm_genetic = true
lm_evolve = true
lm_think_seconds = 2.0
```

## How the tool picks a pair

`dogfight models` first works out how much room there is for weights:

- on a CUDA card: the card's memory minus 2 GB, which is the sidecar's hard cap, less 0.8 GB for activations and the CUDA context (at least 4 GB before that 0.8 GB comes off);
- with `lm_vram_gb` set: that number less 0.8 GB;
- on a Mac: 70 % of the unified memory;
- with no GPU found: the room of an 8 GB card.

It then looks at every pair of catalogue models from two different publishers whose bf16 weights fit in that room. The strongest balanced pair is the one whose smaller model is largest, and among those, the one with the larger total. The tables on this page are that rule applied to each machine size. On your own machine, `dogfight models` prints the result directly.
