# Build log (append-only — agent-oracle segment 3 "durable state")

Each entry: what was tried, evidence, kept/reverted, next step. Never rewrite past entries.

## 2026-09-10 — session 1: research + contract

- Existing tools checked via Firecrawl: drift (Go, ambient braille scenes, shell idle hook),
  terminal-idle (launches cmatrix/asciiquarium/cbonsai), bash-screensavers, .asciidle, pond,
  a ratatui screensaver. None have agent-driven characters, persistent growth, or image import.
- Target terminal: GNOME Terminal / Ptyxis (both VTE). VTE stable builds ship WITHOUT sixel and
  without kitty graphics -> no pixel protocol. Rendering must be cell-based truecolor.
- GPU/CUDA verdict for this target: no benefit. The bottleneck is bytes the terminal must parse,
  not simulation. GPU only pays off with a pixel protocol (kitty/Ghostty), out of scope for v0.1.
- Language: Rust (user constraint: tool must be light). Watcher must be ~1-3 MB RSS.
- EXPERIMENT (pty, bash 5.2.21): trap on USR1/USR2 is DEFERRED while bash idles in readline;
  it only fires after the next Enter. WINCH and ALRM fire immediately. INT fires but wipes the
  half-typed line. => use SIGALRM. Verified saver launch + foreground + partial-line survival
  in bash 5.2, zsh 5.9, fish 3.7.
- Locked eval/thresholds.toml written before implementation.

## 2026-09-10 — session 1 (cont.): implementation + verification

Topology (agent-oracle seg 2): single agent. The codebase is small and tightly
coupled through one Scene contract; multi-agent would add ~15x tokens for no
context-isolation benefit. Parallel scene work is a v0.2 option (one worker per
scene, file-system coordination, this log as shared state).

Built: canvas (half-block + braille + glyph layers), diff encoder (tolerance, SGR/cursor
tracking, DEC 2026), term (raw mode, panic/signal-safe restore), 4 scenes, idle watcher,
shell integration, importer, bench/snapshot. Binary 1.2 MB.

Visual review (Boeing-747 pattern: fixed views, score, fix weakest):
- galaxy v1 FAIL: no arms, blown core, speckle. Root cause: orbits too circular
  (b/a ~0.95) and too little twist. Fix: b/a 0.72, twist 6.2, face-on camera, dust lanes
  offset +0.28 rad, 3x3 soften. -> clear two-arm spiral. KEPT.
- garden: thick stems drew as two sticks -> thick_line(). Soil band 20% -> 14%. KEPT.
- ufo: cows unreadable -> 4x2 body sprite. Stars drawn over moon -> luma mask. KEPT.

Perf loop (stable-frame-rate pattern), locked thresholds at 200x55:
- baseline: galaxy bytes_mean 49.2 KB > 45 FAIL; others pass.
- H1 slower twinkle: 49.2 -> 47.6 KB. KEPT (small, no visual cost).
- H2 inner orbital speed 0.06/(a+.1) -> 0.035/(a+.12): 47.6 -> 32.9 KB. KEPT.
- final: ufo 0.37 ms/23 KB, galaxy 2.7 ms/33 KB, garden 0.45 ms/5 KB, meadow 0.53 ms/16 KB. ALL PASS.

Correctness harness (eval/test_terminal.py): 17/18 on first run.
- restore_after_sigterm FAIL. Investigated before touching anything: the TEST backgrounded
  `reverie &`; job control stopped it with SIGTTOU (state T) before SIGTERM could act.
  Probe showed foreground + external SIGTERM restores termios exactly. HARNESS METHOD FIX
  (pass criteria unchanged): test now runs reverie in the foreground and signals it
  externally. Flagged for human review as a change to a locked file.
- bash glitch found in the demo video: after wake the cursor sits at col 0 and the next
  typed key rendered wrongly. Fix: trap sends `kill -WINCH $$` so readline repaints.
  Verified in a pty (typed 'X' appends correctly). Cursor-at-col-0 until first key remains
  (cosmetic, documented).
- final: 18/18 PASS (bash, zsh, fish). Watcher RSS 1.49 MB. Import peak 21 MB.

Self-improvement (seg 5): NOT enabling self-modifying loops. The evaluator is fast and
deterministic for perf, but visual quality is judge-only — the domain where such loops
fail. Use the two bounded loops in docs/LOOPS.md with a human reviewing snapshots.

Next: see docs/BRIEF.md.

## 2026-09-10 — session 2: ufo only, language-model commanders, macOS/MLX, evolve

User direction (in order received): keep only `ufo`; make the saucers "intelligent machine
controlled" with a CUDA option where two small LMs play each other, graphics unchanged, ≤ 8 GB
VRAM for both; must run on the *minimum* 8 GB CUDA device (this 20 GB card is not the yardstick);
add an MLX option for Macs; 4 UFOs per team with DEPLOY-on-decision; models learn from each
destroyed UFO (evolve, as a toggle); default cuda, `reverie run mlx|cuda ufo-battle`, `reverie run
ufo` = baseline; battle cry only when one of the model's UFOs dies.

Removed: galaxy, garden, meadow, portrait, importer (`src/import.rs`, `src/sprite.rs`), the
`image` crate, their snapshots. Binary 1.13 MB -> 0.69 MB. Config keeps accepting the old keys.
LOCKED-FILE NOTE: `eval/test_terminal.py` and `eval/demo.py` named the removed scenes
(`--scene galaxy`, `scenes = [...]`); changed to `ufo`, pass criteria unchanged.

Built:
- `src/arena.rs` + `agents/arena.py` (embedded, written to ~/.local/share/reverie/arena.py):
  line protocol over pipes, reader thread, `Order` enum; backends torch/CUDA (bf16|fp16, hard
  VRAM cap via set_per_process_memory_fraction, optional bnb 8/4-bit) and mlx-lm.
- `ufo.rs` refactor: order → steering (`execute`), heuristic `builtin_order` reproduces v0.1,
  LM plumbing (observations in percent units, pending/tick bookkeeping, 60 s stuck guard, 25 s
  no-ships guard), 4-slot fleets with DEPLOY, post-mortems, HUD (models, cries 20 s, lessons 12 s).
- Evolve: loss → reflection → `LESSON`; ≤ 8 per (team, model), word-set dedup (Jaccard ≥ 0.6),
  persisted in ~/.local/state/reverie/lessons; `reverie arena lessons|forget`.
- macOS: watcher uses `ps -o pgid,tpgid,stat` and `--tty "$(tty)"` from the snippets; errno via
  std; `stop_watchers` via pidfiles. `cargo check --target aarch64-apple-darwin` clean. NOT run
  on a Mac.
- CLI: `run [cuda|mlx] [ufo|ufo-battle]`, `arena [cuda|mlx]`, `arena check [--load] | pull |
  lessons | forget`, `--evolve on|off`. Config: ufo_pilots, lm_backend (default cuda),
  lm_model_a/b, lm_vram_gb (6), lm_quant, lm_python, lm_think_seconds, lm_evolve.
- `eval/test_arena.py` (GPU-free sidecar checks) + CI macos job.

Model selection (HF API, ungated, bf16 GB): Qwen3-0.6B 1.5, Qwen3-1.7B 4.1, Qwen3.5-0.8B 1.75,
Qwen3.5-2B 4.6, SmolLM2-1.7B-Instruct 3.4, SmolLM3-3B 6.2; gemma-3-1b/Llama-3.2-1B gated;
gemma-4-E2B 10 GB. Qwen3-1.7B+SmolLM2 (7.5 GB) REJECTED for the 8 GB target. KEPT: Qwen3-0.6B
(ZORB) vs SmolLM2-1.7B-Instruct (KRELL).

Evidence (RTX 4000 Ada, torch 2.6 cu124, transformers 5.7):
- `reverie arena check --load`: load 2.0 s + 0.7 s; torch peak 4.61 GB both models; decisions
  0.33 s / 0.44 s; reflection 0.20 s.
- pty matches at 200x55 (fps 60): 76 s → 4317 frames, sidecar 4940 MiB by nvidia-smi;
  60 s → 3458 frames, 114 decisions, 23 reflections, 15 deploys, 4942 MiB.
- bench ufo 200x55: 0.15 ms mean, 0.17 p95, 22.6 KB mean, 29.2 KB p95 → PASS (v0.1: 0.37 ms).
- harness: 14/14 bash checks PASS (`--shells bash`); zsh/fish are not installed on this machine
  and sudo needs a password, so their 4 checks could not run here (18/18 expected in CI).
- `eval/test_arena.py` 18/18 PASS. Build: 0 warnings (was 5; dead helpers removed, signal cast
  lint fixed). Snapshots 80x24/120x36/200x55 reviewed: builtin scene unchanged.

Prompt fixes found by reading arena.log (kept): "(required)" hint copied into cries → hint moved
into prose; models echoed the SAY placeholder → `SAY: <battle cry>` + TEMPLATE_ECHO filter;
`ORDER_RE` ate the next line's ship id when an order had no argument → argument must stay on
the line; lessons about "healing ships" → reflection prompt states there is no repair;
post-mortems were sent for builtin-piloted deaths → only for commanded teams.

Installed with `cargo install --path .` (~/.cargo/bin/reverie == target/release). Not committed.
Next: Mac verification pass; real GNOME Terminal pass; see HANDOFF.md §9.
