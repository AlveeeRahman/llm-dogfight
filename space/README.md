---
title: LLM Dogfight
emoji: 🛸
colorFrom: purple
colorTo: green
sdk: docker
app_port: 7860
pinned: true
license: mit
short_description: Two small local LLMs dogfight in a terminal, live
---

# LLM Dogfight — browser demo (self-hosted)

Hugging Face hosts Docker Spaces only on paid plans, so this image is not deployed to the
free Space; run it yourself with `docker build -t dogfight space-image && docker run -p 7860:7860 dogfight`
(build context: the repository root with `space/Dockerfile`).

Two small language models (SmolLM2-360M for team ZORB, Qwen2.5-0.5B for team KRELL) command
two saucer teams over a sleeping city, running on this Space's free CPU. Each visitor gets a
fresh battle; it ends on its own after 15 minutes. On a CPU the commanders take a second or two
per decision, so the fight is slower than on a GPU, where they answer in a tenth of a second.

Source, binaries, the model catalogue and the rules: **https://github.com/AlveeeRahman/llm-dogfight**

```sh
cargo install --git https://github.com/AlveeeRahman/llm-dogfight
pip install torch transformers        # or: pip install mlx-lm on Apple silicon
dogfight
```
