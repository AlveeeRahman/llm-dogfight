# reverie

A tiny terminal screensaver for Linux. When your shell sits idle at the prompt,
your terminal daydreams — then any key puts you back exactly where you were,
half-typed command included.

| scene | what happens |
|---|---|
| `meadow` | painterly hillside: towering cumulus, wind rolling through grass, a girl in a straw hat with a moss sprite, day → sunset → night (fireflies) → dawn |
| `ufo` | two saucer factions dogfight over a sleeping city — AI pilots pursue, orbit, flee when damaged, and abduct cows when bored |
| `galaxy` | ~10–26k stars on density-wave orbits (real spiral-arm physics), dust lanes, supernovae, comets |
| `garden` | **persistent**: plants sprout, grow, bloom, seed and wither across sessions; the sky follows your real clock |
| `portrait:NAME` | **your image**, imported with `reverie import`, living in the meadow (breathes, sways, blinks) |

Built for the default Ubuntu terminal (GNOME Terminal / Ptyxis, both VTE):
truecolor half-block + braille rendering, no graphics protocol needed. Works in any
truecolor terminal (256-colour fallback).

## Install

```sh
cargo install --path .          # or grab a release binary and put it on your PATH
reverie                         # preview right now (any key exits)
reverie install                 # auto-start when your prompt idles (bash, zsh, fish)
```

`install` appends a marked block to your rc file (with a timestamped backup);
`reverie uninstall` removes it and stops the watchers.

## Use

```sh
reverie run --scene galaxy                 # one scene
reverie list                               # scenes + imported sprites
reverie pause 60 / reverie resume          # e.g. during a screen share
reverie import me.png --name me --eyes 505,550,695,550   # image -> animated character
reverie run --scene portrait:me
reverie config > ~/.config/reverie/config.toml           # then edit
```

Config highlights: `idle_seconds` (300), `scenes`, `rotate_minutes`, `fps` (30),
`unfocused_fps` (12), `max_instances` (3), `tolerance` (5), `garden_speed`,
`day_cycle_seconds`, `character`.

## How it stays light

* **Watcher: ~1.5 MB RSS per shell**, sleeps until the idle deadline, polls nothing.
* **Saver: ~0.5–2.6 ms per frame, ~5–33 KB of output per frame** at 200×55 (see bench).
  The encoder only redraws changed cells, ignores sub-threshold colour drift, and
  tracks cursor/SGR state — terminal CPU scales with bytes, so bytes are the budget.
* **1.2 MB static-ish binary**, deps: `libc` (+ `image` only for the importer).
* Image import: 21 MB peak for a 1200×1600 photo (hard ceiling 4 GB; decoder capped at 1 GiB).

## How idle detection works (and why SIGALRM)

Measured, not assumed: bash **defers USR1/USR2 traps while idle in readline** (they
only fire after the next Enter), but services **SIGALRM immediately**. So a watcher
checks that the shell owns the terminal (no command running) and the tty has had
no input for `idle_seconds`, writes a marker, and sends SIGALRM; the shell's trap
runs `reverie run --idle-trigger`, which verifies the marker (stray alarms are no-ops).
Verified in bash 5.2, zsh 5.9, fish 3.7. Your half-typed line survives; the wake
key is swallowed.

## Verification

```sh
reverie bench --size 200x55                    # vs locked eval/thresholds.toml
python3 eval/test_terminal.py                  # 18 pty checks across bash/zsh/fish
reverie snapshot --scene meadow --out m.ansi && python3 eval/ansi2png.py m.ansi m.png
```

## Known limits

* GNOME Terminal/Ptyxis have no pixel graphics protocol, so art is at half-block
  resolution (a maximised 200×55 terminal = 200×110 pixels). GPU/CUDA adds nothing
  on this target; see docs/BUILD_LOG.md.
* bash: after waking, the cursor shows at column 0 until your first keystroke
  (the line itself is intact and typing continues correctly).
* tty idle time has 8-second kernel granularity; `idle_seconds` is honoured ±8 s.
* zsh users with `TMOUT` + their own `TRAPALRM` are chained, not replaced.
* Imported images are animated procedurally (breathe/sway/blink/GIF frames), not by
  a generative model.

MIT licensed. The meadow character and all scenes are original.
