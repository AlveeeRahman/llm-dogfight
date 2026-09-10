# reverie — notes for Claude Code

Terminal screensaver in Rust. When the shell sits idle at the prompt, saucer factions dogfight
over a city (`ufo`); any key returns the user to their prompt with the half-typed line intact.
`ufo-battle` is the same scene with **two small language models commanding the teams** through a
Python sidecar (`agents/arena.py`). Full architecture and history: **HANDOFF.md** (read it before
non-trivial changes). Build history: docs/BUILD_LOG.md (append-only).

## The user's requirements (do not trade these away)
- Primary target: the default Ubuntu terminal — GNOME Terminal / Ptyxis (VTE). VTE has **no sixel
  and no kitty graphics**, so rendering is truecolor cells: half-blocks `▀` + braille + glyphs.
- **Must stay light.** Watcher ~1 MB RSS, saver < 4 ms/frame. The only crate is `libc`. Don't add
  crates without a measured reason; no async runtime, no TUI framework. The ML side lives in the
  Python sidecar, never in the binary.
- **`ufo-battle` must fit an 8 GB CUDA card.** The user's 20 GB RTX 4000 Ada is *not* the
  yardstick. Default pair Qwen3-0.6B + SmolLM2-1.7B-Instruct = 4.6 GB torch peak, 4.9 GB by
  nvidia-smi; the sidecar caps itself at `lm_vram_gb` (6). Bigger pairs go through `lm_quant`.
- Default backend is **cuda**; macOS uses **mlx** (`reverie run mlx ufo-battle`). Both must stay
  one-word choices on the CLI.
- `reverie run ufo` is the baseline (built-in pilots) and must keep working with no Python at all.
- Evolve (lessons after each loss) stays a **toggle** (`lm_evolve`, `--evolve on|off`).
- Only the ufo scene exists now (galaxy/garden/meadow/portrait and the importer were removed on
  2026-09-10; they are in git history if ever wanted back).

## Commands
```sh
cargo build --release                         # binary: target/release/reverie (~0.66 MB); needs Rust >= 1.88
cargo install --path .                        # updates ~/.cargo/bin/reverie (what the shell hook runs)
./target/release/reverie run ufo              # baseline; any key exits
./target/release/reverie run cuda ufo-battle  # LM match (or: reverie arena)
./target/release/reverie arena check --load   # python/GPU/models report + timed decisions + VRAM peak
./target/release/reverie bench --size 200x55 > /tmp/b.jsonl && python3 eval/check_bench.py /tmp/b.jsonl
PATH="$PWD/target/release:$PATH" python3 eval/test_terminal.py     # 18 pty checks (bash/zsh/fish)
python3 eval/test_arena.py                    # sidecar prompt/parse/lessons checks, no GPU
./target/release/reverie snapshot --scene ufo --size 120x36 --frames 240 --seed 42 --out /tmp/s.ansi
python3 eval/ansi2png.py /tmp/s.ansi /tmp/s.png --cols 120 --rows 36   # then LOOK at the PNG
cargo check --release --target aarch64-apple-darwin                    # macOS compile check
```
Eval deps: `python3-pil python3-numpy zsh fish`, DejaVu fonts; `check_bench.py` needs Python ≥ 3.11.
LM deps: `torch` (CUDA build) + `transformers` (5.x) on Linux; `mlx-lm` on Apple silicon.
Models are cached in `~/.cache/huggingface/hub`; the sidecar log is `~/.local/state/reverie/arena.log`.

## Definition of done for any change
1. `cargo build --release` clean (0 warnings as of 2026-09-10; don't add any).
2. Bench passes `eval/thresholds.toml` for `ufo` at 200x55.
3. `eval/test_terminal.py` 18/18 — run with `target/release` first on PATH. (On a box without
   zsh/fish only the 14 bash checks can run; say so in the log.)
4. `eval/test_arena.py` all pass; if the sidecar or prompt changed, `reverie arena check --load`
   and a real `ufo-battle` run (`--duration 60` in a pty) with `arena.log` read afterwards.
5. If anything visual changed: snapshot at **80x24, 120x36 and 200x55**, open the PNGs, judge them.
6. Append an entry to docs/BUILD_LOG.md: what changed, evidence (numbers), kept/reverted.

## Locked surfaces — read, never edit to make something pass
- `eval/thresholds.toml` — pass/fail limits. Changing a number is the **user's** decision.
- `eval/test_terminal.py`, `eval/check_bench.py`, `reverie bench` — the measuring instruments.
  (The scene names inside the harness were changed to `ufo` when the other scenes were removed;
  pass criteria unchanged, logged.)
- `Scene` trait in `src/scenes/mod.rs` — bench, snapshot and app all depend on it.
- The arena line protocol in `agents/arena.py`'s docstring — Rust (`src/arena.rs`) parses it.
- docs/BUILD_LOG.md is **append-only**.

## Ask the user first
Editing their `~/.bashrc`/`~/.zshrc`/fish config (only `reverie install` may do that), running
`reverie install/uninstall` on their machine, deleting `~/.local/state/reverie/lessons/` (the
commanders' memories — `reverie arena forget` is the user's call), downloading models other
than the defaults, publishing, pushing, or changing thresholds.

## Pitfalls (each of these was learned the hard way)
- **The wake signal must stay SIGALRM.** Measured: bash *defers* USR1/USR2 traps while idle in
  readline (they fire only after the next Enter); ALRM and WINCH fire immediately. Don't "clean up"
  to USR1. Re-run the pty experiments in HANDOFF.md before touching `src/idle.rs`/`src/shell.rs`.
- The bash trap must end with `kill -WINCH $$` — without it, the first key typed after waking
  renders wrongly. Known cosmetic leftover: cursor shows at column 0 until the first keystroke.
- **The harness tests whatever `reverie` is first on PATH** (the shell snippets call `command
  reverie`). Put `target/release` first, or you're testing the old installed binary.
- Running watchers keep the old binary in memory after `cargo install`. Restart them:
  `pkill -f "reverie watch"`, then open a new terminal.
- tty idle time comes from the tty's atime, which the kernel updates in **8-second steps**.
- **Bytes are the performance budget**, not CPU: VTE's cost scales with the escape codes it
  parses. Anything that changes many cells every frame blows the 45 KB/frame limit. HUD text
  (comms, lessons) only changes when a message arrives, so it is nearly free.
- The encoder's `tolerance` compares against what was last **emitted**, so drift stays bounded.
- A braille dot replaces the whole cell's two half-block pixels. Keep dots off detailed areas;
  `Starfield` skips cells with luma > 0.3.
- Every `render()` must start with `cv.clear_overlays()` and repaint every pixel it owns.
- Determinism: `Ufo::new` (builtin) uses only its seeded `Rng`; bench and snapshot always get the
  builtin pilots (`scenes::make(.., live=false)`). `ufo-battle` is not deterministic.
- Never `println!` while the saver is on screen — write through `Term`. The sidecar's stdout is
  the protocol; its stderr goes to `arena.log`. Release builds use `panic = "abort"`; the panic
  hook calls `term::emergency_restore()` first. Keep it that way.
- `Arena`'s `Drop` waits up to 1.5 s for the Python process to exit, then kills it. It runs after
  `Term::leave()`, so the terminal is already restored while the GPU frees.
- The macOS path (`ps`-based `proc_stat`, `--tty` from the snippet) and MLX backend compile and
  are unit-tested but have **not been run on a Mac**. Treat the first Mac report as a test pass.
- Small models drift: they drop the SAY line, echo the enemy's taunt, or answer `DEPLOY: 0` with
  no ships left. `lm_send` re-launches one saucer after 25 s with none alive; keep that guard.
- Don't put format hints in the reply template itself — SmolLM2 copied "(required)" into its cry.

## Recipes
**New order verb:** `Order` in `src/arena.rs` (+ `describe`/`parse`) → the `match order` arm in
`Ufo::execute` → `ORDER_RE` and the "Orders available" line in `agents/arena.py` → `eval/test_arena.py`.

**New config key:** field in `Config` + `Default` + a match arm in `Config::apply()` + a commented
line in `DEFAULT_TOML`. Unknown keys are ignored, so old configs keep working. LM keys also go
through `arena::command()` as sidecar flags.

**Different models:** `lm_model_a`/`lm_model_b` in the config, then `reverie arena pull` and
`reverie arena check --load`; the printed peak memory must stay under the 8 GB target with margin.

**Visual iteration:** snapshot fixed views → view PNG → fix the single weakest thing → re-bench →
re-snapshot. See docs/LOOPS.md.
