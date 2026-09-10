# Long-horizon brief for the next build session

DEFINITIONS
  scene      a type implementing the locked Scene trait in src/scenes/mod.rs (only `ufo` now;
             `ufo-battle` = ufo with language-model commanders)
  pass       `reverie bench --size 200x55` within eval/thresholds.toml AND
             `python3 eval/test_terminal.py` 18/18 AND `python3 eval/test_arena.py` all pass AND
             snapshots reviewed at 80x24/120x36/200x55
  8 GB       the CUDA target: `reverie arena check --load` peak memory must stay well under 8 GB
             (currently 4.6 GB torch / 4.9 GB nvidia-smi); the user's 20 GB card is not the yardstick

TASK (v0.3)
  1. Mac verification: run `reverie install`, the watcher, and `reverie run mlx ufo-battle` on an
     Apple-silicon Mac; fix what breaks (ps parsing, tty atime, mlx_lm template kwargs).
  2. Real GNOME Terminal pass (colours, braille widths, gnome-terminal-server CPU while animating).
  3. Better evolve: merge similar lessons semantically; let a team also learn from its kills;
     surface lessons in `reverie arena lessons` with timestamps.
  4. Packaging: .deb + musl release binaries from CI; make CI green.
  Each item ships only when `pass` holds and the 8 GB budget is re-measured.

DOES NOT COUNT
  editing thresholds.toml, the bench, or the pty harness to turn red into green;
  a change that looks right in one snapshot but regresses bytes/frame;
  "works on my 20 GB card" without the printed peak-memory line; status reports without artifacts.

VERIFICATION
  Every claim traces to a command output from the current session (bench JSON, harness output,
  test_arena output, `arena check --load` output, arena.log excerpt, snapshot PNG path).

RETURN CONDITION
  Return when an item passes, or when it is blocked with the exact blocker.

HUMAN-CONTROLLED
  rc-file edits outside `reverie install`, publishing, threshold changes, merges, deleting the
  commanders' lessons, downloading non-default models.
