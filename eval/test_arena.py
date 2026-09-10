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

OBS = {"team": 0, "tick": 3, "score": [2, 1], "fh": 55, "ground": 47, "range": 61, "free_slots": 1,
       "mine": [{"id": 0, "x": 20, "y": 20, "hp": 100, "near": 4, "dist": 30, "order": "patrol"},
                {"id": 1, "x": 10, "y": 30, "hp": 30, "near": 4, "dist": 35, "order": "attack E4"},
                {"id": 2, "x": 50, "y": 10, "hp": 80, "near": -1, "dist": -1, "order": "hunt"}],
       "foes": [{"id": 4, "x": 50, "y": 22, "hp": 60, "target": 1}, {"id": 5, "x": 80, "y": 15, "hp": 100, "target": -1}],
       "cows": [{"id": 0, "x": 15, "state": "free"}, {"id": 1, "x": 70, "state": "being lifted by enemy E5"}],
       "events": ["your S3 was destroyed by E5; its slot is free (DEPLOY)"], "foe_say": "Moo to you!", "cry": True}

fails = 0


def check(name, ok, detail=""):
    global fails
    fails += not ok
    print(f"  {'PASS' if ok else 'FAIL'}  {name}  {detail}")


# prompt
p = arena.describe(OBS)
check("prompt lists own ships", "S0 at (20,20) hp 100" in p and "S2 at (50,10) hp 80, no enemy in the air" in p)
check("prompt lists enemies with targeting", "E4 at (50,22) hp 60, targeting S1" in p)
check("prompt mentions free slot and DEPLOY", "DEPLOY: <0-1>" in p and "1 of your saucers are destroyed" in p)
check("prompt carries enemy comms and events", 'Enemy commander said: "Moo to you!"' in p and "Recent events: your S3" in p)

# replies: tidy, sloppy, thinking tags, junk
orders, say, deploy = arena.parse_reply("S0: attack E4\nS1: flee\nS2: abduct C0\nDEPLOY: 1\nSAY: Cows are ours!", OBS)
check("tidy reply", orders == {0: "attack:4", 1: "flee", 2: "abduct:0"} and say == "Cows are ours!" and deploy == 1, str((orders, say, deploy)))
orders, say, deploy = arena.parse_reply("<think>hmm</think>S0 - guard S2 / S1: hunt E4 / S9: attack E4 / DEPLOY: 7 / SAY: \"Zap! <b>x</b>\"", OBS)
check("sloppy reply", orders == {0: "guard:2", 1: "hunt"} and 9 not in orders and deploy == 1 and say == "Zap! bxb", str((orders, say, deploy)))
orders, say, deploy = arena.parse_reply("S1: attack\nS2: abduct\nSAY:", OBS)
check("missing arguments degrade", orders == {1: "hunt", 2: "patrol"} and say == "", str((orders, say)))
orders, say, deploy = arena.parse_reply("I refuse to play games.", OBS)
check("junk reply gives no orders", orders == {} and say == "" and deploy == 0)
check("say is clipped to 40 chars", len(arena.parse_reply("SAY: " + "x" * 100, OBS)[1]) == 40)
quiet = dict(OBS, cry=False)
check("no cry requested -> SAY not in prompt, ignored in reply", "SAY:" not in arena.describe(quiet) and arena.parse_reply("S0: hunt\nSAY: boo", quiet)[1] == "")
check("cry requested -> SAY in prompt", "SAY: <battle cry>" in arena.describe(OBS) and "One of your saucers was just destroyed" in arena.describe(OBS))
check("template echo is not a cry", arena.parse_reply("S0: hunt\nSAY: S2 was just destroyed: a defiant battle cry", OBS)[1] == "")

# post-mortem + lessons
loss = {"team": 1, "ship": 5, "killer": 0, "killer_hp": 80, "dist": 20, "range": 61, "order": "abduct C1",
        "enemies_near": 2, "allies_near": 0, "low_for": 6, "alive_for": 40, "x": 70, "y": 40, "ground": 47}
d = arena.describe_loss(loss)
check("post-mortem text", "S5 was destroyed by enemy E0" in d and "below 35 hp for 6 s" in d and "near the ground" in d, d)
with tempfile.TemporaryDirectory() as tmp:
    path = os.path.join(tmp, "KRELL-test.txt")
    L = arena.Lessons(path)
    distinct = ["flee below 35 hp", "guard the damaged ally", "abduct cows only when enemies are far", "attack the weakest enemy first",
                "deploy immediately after a loss", "patrol near the moon", "hunt in pairs", "never chase into the enemy corner",
                "keep two ships home", "focus fire on one target", "avoid the ground", "retreat from three enemies"]
    for l in distinct:
        L.add(l)
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

# backend selection
check("auto backend is cuda off-Mac", arena.pick_backend("auto") in ("cuda", "mlx") and arena.pick_backend("mlx") == "mlx")

print(f"\n{'ALL PASS' if not fails else f'{fails} FAILURES'}")
sys.exit(1 if fails else 0)
