# Security

reverie runs inside your terminal session and, in `ufo-battle` mode, runs two language models on
your machine. This page says what it does, what it does not do, and how it is checked.

## What the binary does and does not do

- **Touches:** your terminal (raw mode, alternate screen; restored on every exit path including
  SIGTERM/SIGHUP and panics), `~/.config/reverie`, `~/.local/state/reverie`,
  `~/.local/share/reverie`, `${XDG_RUNTIME_DIR:-/tmp}/reverie-$UID` (mode 0700), and your shell
  rc file only through `reverie install` (marked block, timestamped backup) / `reverie uninstall`.
- **Never:** opens a network connection, reads files outside those paths, escalates privileges,
  or runs anything but the sidecar it wrote itself (`arena.py`, rewritten from the embedded copy
  on every start, so a modified copy is overwritten).
- **Model output** is untrusted: cries, lessons and orders are filtered to printable ASCII before
  they reach the terminal, the HUD or a file, so a model cannot emit escape sequences; orders are
  parsed against a fixed verb list and range-checked.
- **The sidecar** (Python, `agents/arena.py`) is the only component that downloads anything
  (Hugging Face models through `huggingface_hub`). Pin a model to a revision with
  `--zorb Qwen/Qwen3-0.6B@<commit>` or `lm_model_a = "…@<commit>"` if you need reproducible weights.
  It runs at lower CPU priority and is killed when the battle ends.
- **`reverie remove`** deletes exactly the paths above plus the binary; nothing else.

## What is enforced on every change (CI gates)

| gate | command |
|---|---|
| formatting | `cargo fmt --check` |
| lints, every warning an error | `cargo clippy --release -- -D warnings` (incl. `undocumented_unsafe_blocks`: every `unsafe` carries a `// SAFETY:` justification) |
| dependency advisories | `cargo audit` (RustSec; the only dependency is `libc`) |
| reproducible builds | `cargo build --release --locked` |
| integer overflow | `overflow-checks = true` in release: overflow aborts (after restoring the terminal) instead of wrapping silently |
| Python security lint | `bandit -r agents/` |
| Python lint | `ruff check agents/ eval/test_arena.py` |
| behaviour | bench against `eval/thresholds.toml`, pty harness (`eval/test_terminal.py`), sidecar tests (`eval/test_arena.py`) |

Run all of it locally with `scripts/qa.sh`.

## Threat model, briefly

- A malicious or confused model: contained by ASCII filtering, the fixed protocol, the doctrine
  guardrails and the fact that the model never gets a tool, a file or a shell.
- A malicious model repository: the sidecar loads weights with `transformers` / `mlx-lm`, which
  use safetensors (no pickle execution) for the catalogued models; pin revisions for stronger
  guarantees. reverie never sets `trust_remote_code`.
- Another local user: state and runtime dirs are per user; the runtime dir is 0700; signals are
  only sent to processes verified to be our own watcher or the shell that started it.
- A crashed or wedged terminal: the watcher only fires when the shell is at its prompt; the saver
  swallows the wake key and restores termios; SIGPIPE cannot wedge output.

## Reporting

Open a GitHub issue with the label `security`, or contact the maintainer privately if the issue
could expose users. Please include `reverie --version`, your terminal and OS, and the relevant
lines of `~/.local/state/reverie/arena.log` or `match.log`.
