# Battles

```{include} ../README.md
:start-after: "## Rules of a battle"
:end-before: "## Guardrails: the doctrine"
```

```{image} https://raw.githubusercontent.com/AlveeeRahman/llm-dogfight/main/eval/snapshots/hud.png
:alt: The HUD with both commanders' battle cries and lessons
```

## What the files hold

| file | what it holds |
|---|---|
| `~/.local/state/dogfight/match.log` | one line per kill, reinforcement, abduction and game result |
| `~/.local/state/dogfight/score-<ZORB model>-vs-<KRELL model>.txt` | games won, kills and cows for that pair |
| `~/.local/state/dogfight/lessons/` | one file of lessons per team and model |
| `~/.local/state/dogfight/arena.log` | everything the Python sidecar writes besides its protocol lines |

`dogfight arena lessons` prints the lessons, `dogfight reset score` clears the scores and `dogfight reset model` clears the lessons and doctrines. The state folder follows `XDG_STATE_HOME` when it is set.
