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
