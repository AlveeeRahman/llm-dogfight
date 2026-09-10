# Loops for continuing reverie (adapted from the Loop Library)

## Scene frame-budget loop
Adapted from Aviv Sheriff's "stable-frame-rate loop": fixes one measured bottleneck at a time until a scene fits the locked terminal budget.

Prompt:
> Improve [scene] in reverie until `reverie bench --scene [scene] --size 200x55` passes eval/thresholds.toml on two consecutive runs. Never edit thresholds.toml, the bench, or eval/test_terminal.py. Change one measured bottleneck per round, rerun the full bench and pty checks, snapshot the scene, and keep the change only if no metric regresses and the snapshot still reads well. Log every attempt in docs/BUILD_LOG.md. Stop on pass, two rounds without progress, or eight rounds.

## Scene fidelity loop
Adapted from the "Boeing 747 benchmark" pattern: renders fixed views, scores them, and fixes only the weakest dimension without breaking performance.

Prompt:
> Raise [scene]'s visual quality in reverie. Render fixed views with `reverie snapshot` at 80x24, 120x36 and 200x55 (seed 42; day and night where relevant) through eval/ansi2png.py. Have a fresh reviewer score silhouette readability, palette, motion and artifacts from 1 to 5. Fix only the weakest dimension while keeping bench and pty checks green. Keep the best version. Stop when every dimension is 4+, two rounds stall, or six rounds.
