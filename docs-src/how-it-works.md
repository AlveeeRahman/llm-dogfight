# How it works

LLM Dogfight runs as two processes. The `dogfight` binary is written in Rust. It draws the battle, flies the saucers and enforces the rules. The commanders run in `agents/arena.py`, a Python sidecar that loads both models with transformers on CUDA or the CPU, or with mlx-lm on Apple silicon. The binary starts the sidecar, sends it one JSON line per team about once a second, and reads back one line of orders. The models never get a tool, a file or a shell.

## The line protocol

Standard output of the sidecar is reserved for these lines. Everything else it prints goes to `~/.local/state/dogfight/arena.log`.

| direction | line | meaning |
|---|---|---|
| dogfight to arena | `{"t":"obs","team":0,"tick":7,...}` | the situation for one team, as one JSON line |
| dogfight to arena | `{"t":"loss","kind":"ship"\|"round",...}` | a saucer or the whole fleet was lost: write a lesson |
| dogfight to arena | `{"t":"quit"}` | stop |
| arena to dogfight | `STATUS <text>` | progress, such as loading |
| arena to dogfight | `READY <team> <label> <mem_gb>` | that team's commander is online |
| arena to dogfight | `ORDERS <team> <tick> S0:attack:3 S1:flee ... \| <battle cry>` | one order per saucer, and a battle cry only after a loss |
| arena to dogfight | `LESSON <team> <text>` | what the commander learned from a loss |
| arena to dogfight | `ERROR <text>` | fatal: the sidecar exits and the built-in pilots take over |

## What a commander reads

Each commander gets a system prompt and one user message per turn. The system prompt is fixed, with the two team names filled in:

```text
You are the commander of team {me} in an arcade flying-saucer dogfight against team {foe}. Each turn you give one order per ship: attack, hunt, flee or abduct; there is no idle order. Score = enemy saucers destroyed + cows abducted; a team whose saucers are all destroyed loses the round. Destroyed saucers are replaced automatically after a few seconds. Focus fire on damaged enemies, flee with ships below 35 hp when an enemy is in range, and send a ship to abduct a cow whenever the cow is close (within 20) and no enemy is within laser range. Answer only in the required format.
```

With lessons on, the commander's own lessons follow it, numbered, after "Lessons from your past losses, obey them:".

The user message describes the field in plain lines, in this order:

1. the game number, games won by each side, and this game's kills and cows;
2. the field size, the ground line and the laser range;
3. saucers in the air and reinforcements left for both teams;
4. the team's doctrine and its generation, and how many of its last orders were corrected (with `evolve`);
5. its own saucers: position, hp, the nearest enemy and its distance, the nearest free cow, and the current order;
6. the enemy saucers: position, hp and which of its saucers each one is targeting;
7. the cows on the ground;
8. abduction openings, spelled out, because small models rarely work them out from the numbers alone;
9. recent events, and what the enemy commander last said;
10. the orders available, and the reply format: one `S<n>: <order>` line per saucer, plus a `SAY:` line with a battle cry of 40 characters or fewer after a loss.

## How a reply becomes orders

- Text inside `<think>...</think>` is dropped, so reasoning models can think first.
- Each `S<n>: <order>` line for one of the team's saucers is read. `attack E<n>` and `abduct C<n>` keep their target, `flee` stays `flee`, and every other word the model invents, such as `idle`, `wait` or `patrol`, becomes `hunt`.
- A saucer keeps its last order until a new one arrives.
- The doctrine then checks each order. See [Doctrine and evolution](doctrine.md).
- The battle cry keeps only letters, digits, spaces and simple punctuation, and at most 40 characters. A cry that only repeats the instructions is dropped.
- On the Rust side, only printable ASCII reaches the screen.

## How lessons are written

After each lost saucer, and after each lost game, the losing commander gets a short post-mortem: which enemy destroyed which saucer, at what range, under which order, how many enemies and allies were near, and whether it had been flying damaged without retreating. It answers with one rule of at most 25 words after `LESSON:`. A lesson that nearly repeats one it already has, measured by the words the two share, is dropped. The commander keeps its 8 most recent lessons, one file per team and model in `~/.local/state/dogfight/lessons/`.

## More detail

[ARCHITECTURE.md](https://github.com/AlveeeRahman/llm-dogfight/blob/main/docs/ARCHITECTURE.md) has the module map, the rendering model, the file formats and the decision history.
