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
