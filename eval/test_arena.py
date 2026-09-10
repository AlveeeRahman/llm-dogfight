#!/usr/bin/env python3
"""GPU-free checks for agents/arena.py: prompt building, reply parsing, lessons memory.

  python3 eval/test_arena.py
"""
import importlib.util
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("arena", os.path.join(HERE, "..", "agents", "arena.py"))
arena = importlib.util.module_from_spec(spec)
spec.loader.exec_module(arena)

OBS = {"team": 0, "tick": 3, "score": [2, 1], "cows_taken": [1, 0], "rounds": [1, 0], "round": 2, "fh": 55, "ground": 47, "range": 61,
       "mine": [{"id": 0, "x": 20, "y": 20, "hp": 100, "near": 4, "dist": 30, "cow": 0, "cowd": 9, "order": "hunt"},
                {"id": 1, "x": 10, "y": 30, "hp": 30, "near": 4, "dist": 35, "cow": 0, "cowd": 14, "order": "attack E4"},
                {"id": 2, "x": 50, "y": 10, "hp": 80, "near": -1, "dist": -1, "cow": -1, "cowd": -1, "order": "hunt"}],
       "foes": [{"id": 4, "x": 50, "y": 22, "hp": 60, "target": 1}, {"id": 5, "x": 80, "y": 15, "hp": 100, "target": -1}],
       "cows": [{"id": 0, "x": 15, "state": "free"}, {"id": 1, "x": 70, "state": "being lifted by enemy E5"}],
       "events": ["your S3 was destroyed by E5; a reinforcement warps in shortly"], "foe_say": "Moo to you!", "cry": True}

fails = 0


def check(name, ok, detail=""):
    global fails
    fails += not ok
    print(f"  {'PASS' if ok else 'FAIL'}  {name}  {detail}")


# prompt
p = arena.describe(OBS)
check("prompt lists own ships", "S0 at (20,20) hp 100" in p and "S2 at (50,10) hp 80, no enemy in the air" in p)
check("prompt lists enemies with targeting", "E4 at (50,22) hp 60, targeting S1" in p)
check("prompt has score, game and cow distances",
      "Game 2 (games won: ZORB 1, KRELL 0)" in p and "2 kills + 1 cows" in p and "nearest free cow C0 at distance 9" in p)
GEN = dict(OBS, regens=[14, 20], alive=[3, 4], max_alive=4, gen=2, corrected=1,
           doctrine="flee below 30 hp when an enemy is in laser range; never flee above 80 hp")
pg = arena.describe(GEN)
check("prompt carries reinforcements and doctrine",
      "Reinforcements left this game: ZORB 14, KRELL 20" in pg and "Your doctrine (generation 2" in pg and "1 of your last orders were corrected" in pg)
check("no doctrine line without evolve", "doctrine" not in p)
check("prompt offers only attack/hunt/flee/abduct", "patrol" not in p and "guard" not in p and "DEPLOY" not in p and "abduct C<n>" in p)
check("prompt carries enemy comms and events", 'Enemy commander said: "Moo to you!"' in p and "Recent events: your S3" in p)

# replies: tidy, sloppy, thinking tags, junk
orders, say, _ = arena.parse_reply("S0: attack E4\nS1: flee\nS2: abduct C0\nSAY: Cows are ours!", OBS)
check("tidy reply", orders == {0: "attack:4", 1: "flee", 2: "abduct:0"} and say == "Cows are ours!", str((orders, say)))
orders, say, _ = arena.parse_reply("<think>hmm</think>S0 - guard S2 / S1: hunt E4 / S9: attack E4 / S2: patrol / SAY: \"Zap! <b>x</b>\"", OBS)
check("sloppy reply: patrol/guard become hunt", orders == {0: "hunt", 1: "hunt", 2: "hunt"} and 9 not in orders and say == "Zap! bxb", str((orders, say)))
orders, say, _ = arena.parse_reply("S1: attack\nS2: abduct\nSAY:", OBS)
check("missing arguments degrade to hunt", orders == {1: "hunt", 2: "hunt"} and say == "", str((orders, say)))
orders, say, _ = arena.parse_reply("I refuse to play games.", OBS)
check("junk reply gives no orders", orders == {} and say == "")
check("say is clipped to 40 chars", len(arena.parse_reply("SAY: " + "x" * 100, OBS)[1]) == 40)
quiet = dict(OBS, cry=False)
check("no cry requested -> SAY not in prompt, ignored in reply",
      "SAY:" not in arena.describe(quiet) and arena.parse_reply("S0: hunt\nSAY: boo", quiet)[1] == "")
check("cry requested -> SAY in prompt", "SAY: <battle cry>" in arena.describe(OBS) and "One of your saucers was just destroyed" in arena.describe(OBS))
check("template echo is not a cry", arena.parse_reply("S0: hunt\nSAY: S2 was just destroyed: a defiant battle cry", OBS)[1] == "")

# post-mortem + lessons
loss = {"team": 1, "kind": "ship", "ship": 5, "killer": 0, "killer_hp": 80, "dist": 20, "range": 61, "order": "abduct C1",
        "enemies_near": 2, "allies_near": 0, "low_for": 6, "alive_for": 40, "x": 70, "y": 40, "ground": 47}
d = arena.describe_loss(loss)
check("post-mortem text", "S5 was destroyed by enemy E0" in d and "below 35 hp for 6 s" in d and "near the ground" in d, d)
d = arena.describe_loss({"team": 1, "kind": "round", "round": 3, "kills": 2, "losses": 5, "foes_left": "E0 (40 hp), E2 (90 hp)", "cows": 1, "regens_left": 0})
check("game post-mortem text", "lost game 3" in d and "E0 (40 hp), E2 (90 hp)" in d and "one extra saucer" in d, d)
with tempfile.TemporaryDirectory() as tmp:
    path = os.path.join(tmp, "KRELL-test.txt")
    L = arena.Lessons(path)
    distinct = ["flee below 35 hp", "guard the damaged ally", "abduct cows only when enemies are far", "attack the weakest enemy first",
                "deploy immediately after a loss", "patrol near the moon", "hunt in pairs", "never chase into the enemy corner",
                "keep two ships home", "focus fire on one target", "avoid the ground", "retreat from three enemies"]
    for item in distinct:
        L.add(item)
    L.add("retreat from three enemies")  # duplicate ignored
    check("lessons capped and persisted", len(L.items) == arena.MAX_LESSONS and open(path).read().count("\n") == arena.MAX_LESSONS, str(L.items))
    L3 = arena.Lessons(None)
    L3.add("Always prioritize healing damaged ships over fleeing.")
    L3.add("Prioritize healing damaged ships over retreating.")
    L3.add("Never abduct cows while an enemy is within laser range.")
    check("near-duplicate lessons are merged", len(L3.items) == 2, str(L3.items))
    L2 = arena.Lessons(path)
    check("lessons reload", L2.items == L.items and "(8) retreat from three enemies" in L2.prompt())
    check("lessons enter the system prompt", "Lessons from your past losses" in arena.system_prompt(1, L2) and "KRELL" in arena.system_prompt(1, L2))
check("no lessons -> plain prompt", "Lessons" not in arena.system_prompt(0, arena.Lessons(None)))

# chat templates without a system role
class _NoSystemTok:
    def apply_chat_template(self, messages, **kw):
        if any(m["role"] == "system" for m in messages):
            raise ValueError("System role not supported")
        return "|".join(f"{m['role']}:{m['content']}" for m in messages) + "|assistant:"


class _PlainTok:
    def apply_chat_template(self, messages, **kw):
        return "|".join(f"{m['role']}:{m['content']}" for m in messages)


msgs = [{"role": "system", "content": "SYS"}, {"role": "user", "content": "USER"}]
check("system prompt folded into user turn when the template rejects it", arena.chat_text(_NoSystemTok(), msgs) == "user:SYS\n\nUSER|assistant:")
check("template with system role used as is", arena.chat_text(_PlainTok(), msgs) == "system:SYS|user:USER")

# backend selection
check("auto backend is cuda off-Mac", arena.pick_backend("auto") in ("cuda", "mlx") and arena.pick_backend("mlx") == "mlx")

print(f"\n{'ALL PASS' if not fails else f'{fails} FAILURES'}")
sys.exit(1 if fails else 0)
