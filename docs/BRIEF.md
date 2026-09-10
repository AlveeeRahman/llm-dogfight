# Long-horizon brief for the next build session (agent-oracle segment 4)

DEFINITIONS
  scene      a type implementing the locked Scene trait in src/scenes/mod.rs
  pass       `reverie bench --size 200x55` within eval/thresholds.toml AND
             `python3 eval/test_terminal.py` 18/18 AND snapshots reviewed at 80x24/120x36/200x55
  VTE        GNOME Terminal / Ptyxis (the primary target; no pixel protocol)

TASK (v0.2)
  1. kitty-graphics pixel path for kitty/Ghostty (auto-detected; VTE keeps cell mode)
  2. importer: optional ONNX background matting, peak RSS < 4 GB, measured
  3. packaging: .deb + musl release binaries from CI
  Each item ships only when `pass` holds.

DOES NOT COUNT
  editing thresholds.toml, the bench, or the pty harness to turn red into green;
  a scene that looks right in one snapshot but regresses bytes/frame;
  "works in kitty" without re-running the VTE path; status reports without artifacts.

VERIFICATION
  Every claim traces to a command output from the current session (bench JSON,
  harness output, snapshot PNG path). A fresh reviewer scores snapshots.

RETURN CONDITION
  Return when an item passes, or when it is blocked with the exact blocker.

EFFORT
  At least two measured attempts per failing metric before declaring a blocker.
  Stop after two rounds without measurable progress.

HUMAN-CONTROLLED
  rc-file edits outside `reverie install`, publishing, threshold changes, merges.
