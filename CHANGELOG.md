# Changelog

## 0.3.4 — 2026-09-10

- The sidecar's GPU memory cap is now `auto` (card memory minus 2 GB) instead of a fixed 6 GB,
  so larger cards can run larger pairs without editing the config; out-of-memory errors say
  what to do; unknown model aliases are refused up front instead of silently falling back to
  the built-in pilots.

## 0.3.3 — 2026-09-10

- Llama removed from the catalogue (Meta's repositories are gated; no Llama entry at all now).

## 0.3.2 — 2026-09-10

- `reverie remove` now also deletes the rc-file backups and the models it downloaded (only
  those reverie knows about; the rest of the Hugging Face cache is untouched). `--keep-models`
  opts out. Nothing is left behind.

## 0.3.1 — 2026-09-10

- Model catalogue is now entirely ungated: the gated `meta-llama` and `google` entries were
  replaced by the ungated `unsloth` mirrors, and LFM2 (1.2B, 700M), OLMo 2 1B, Falcon3 1B,
  h2o-danube3 500M and Qwen2.5 3B were added. `reverie models` says so.
- README rewritten for the public release.

## 0.3.0 — 2026-09-10

The tool became the LLM dogfight itself: `reverie` starts a battle between two small local
language models; the idle screensaver is an optional mode.

- **Battle**: 4 saucers per team on screen, reinforcements 4 s after a loss out of a budget of
  20 per game; a team with nothing in the air and no reinforcement left loses the game; the
  loser of a game starts the next one with an extra saucer; games won, kills and cows persist
  per model pair; `match.log` records every event.
- **Commanders**: orders are attack / hunt / flee / abduct; a battle cry after each loss; lessons
  written from post-mortems (ship and game) and kept in the prompt; `evolve` adds a genetic
  algorithm over a bounded tactical doctrine that is enforced on every order.
- **Graphics**: dithered sky, aurora, moonlit clouds, dynamic lighting, farm with barn, fence,
  road and ten grazing cows, shaded saucers with trails and shield shimmer, title card and game
  banners, scoreboard.
- **CLI**: `reverie [cuda|mlx|evolve|ufo]`, `--zorb`/`--krell` with a model catalogue
  (`reverie models`), `reverie reset model|score|all`, `reverie remove` (clean uninstall),
  `--perf FILE` for live frame statistics.
- **Robustness**: lag guard (frame-rate halving plus adaptive encoder tolerance on slow
  terminals), non-blocking sidecar writes, integer overflow checks in release (which caught a
  noise-sampler overflow), every `unsafe` documented, `cargo audit`, `bandit`, `ruff`, locked
  builds, `scripts/qa.sh`.
- **Platforms**: CUDA via torch + transformers; Apple silicon via mlx-lm; watcher portable to
  macOS (untested on real hardware).

## 0.2.0 — 2026-09-10

- Removed the galaxy, garden, meadow and portrait scenes and the image importer.
- Added the language-model commanders (Python sidecar over a line protocol), the CUDA and MLX
  backends, and post-mortem lessons.

## 0.1.0 — 2026-09-10

- First version: cell-based truecolor renderer (half-blocks, braille, glyphs), diff encoder,
  idle watcher with SIGALRM wake, shell integration for bash/zsh/fish, four scenes, bench and
  pty harness.
