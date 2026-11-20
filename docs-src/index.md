# LLM Dogfight documentation

LLM Dogfight is a UFO dogfight in your terminal, flown by two small language models that run on your own machine. Team ZORB and team KRELL each get a commander. About once a second, each commander reads a text description of the field and gives every saucer an order: attack, hunt, flee or abduct a cow. The game is a 0.8 MB Rust binary with one dependency, and the models run in a Python sidecar on CUDA, Apple MLX or the CPU.

```{image} https://raw.githubusercontent.com/AlveeeRahman/llm-dogfight/main/eval/snapshots/battle.png
:alt: A live battle between two local models in the terminal
```

## Start here

::::{grid} 1 2 2 3
:gutter: 3

:::{grid-item-card} Get started
:link: get-started
:link-type: doc
Install the binary and a model runtime, then play your first battle.
:::

:::{grid-item-card} Commands
:link: commands
:link-type: doc
Every command and flag of `dogfight`, read from the source.
:::

:::{grid-item-card} Model templates
:link: templates
:link-type: doc
Ready-made pairs and config files for each GPU and Mac size.
:::

:::{grid-item-card} Models
:link: models
:link-type: doc
The catalogue, any Hugging Face id, bigger machines and models without guardrails.
:::

:::{grid-item-card} Battles
:link: battle
:link-type: doc
Fleets, scoring, orders, battle cries and lessons.
:::

:::{grid-item-card} Doctrine and evolution
:link: doctrine
:link-type: doc
The bounded guardrails each team flies under, and the genetic algorithm that tunes them.
:::

::::

## Three lines to a battle

```sh
cargo install --git https://github.com/AlveeeRahman/llm-dogfight
pip install torch transformers    # NVIDIA GPU, or: pip install mlx-lm on Apple silicon
dogfight                          # the first run downloads the two default models (2.2 GB)
```

No GPU? `dogfight ufo` runs the same fight with the built-in pilots and needs nothing else, and `dogfight cpu` runs the real models on the processor.

```{toctree}
:hidden:

get-started
commands
templates
models
battle
doctrine
configuration
how-it-works
performance-and-security
development
```
