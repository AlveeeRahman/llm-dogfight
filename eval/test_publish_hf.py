#!/usr/bin/env python3
"""The Space card is the README with every relative link made absolute. These checks run the
rewrite against the real README (so a renamed screenshot fails CI) and against fixed cases.
No network, no third-party imports.

  python3 eval/test_publish_hf.py
"""
import importlib.util
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("publish_hf", ROOT / "scripts" / "publish_hf.py")
publish_hf = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publish_hf)
absolutize, readme_images, GITHUB = publish_hf.absolutize, publish_hf.readme_images, publish_hf.GITHUB

fails = 0


def check(name, ok, detail=""):
    global fails
    fails += not ok
    print(f"  {'PASS' if ok else 'FAIL'}  {name}  {detail}")


readme = (ROOT / "README.md").read_text(encoding="utf-8")
images = readme_images(readme)
check("README references at least the three screenshots", len(images) >= 3, ", ".join(images))
check("every referenced image exists", all((ROOT / p).exists() for p in images))
card = absolutize(readme)
check("no relative image link survives", "](eval/" not in card and "](docs/" not in card)
for p in images:
    check(f"{p} points at raw.githubusercontent-style URL", f"]({GITHUB}/raw/main/{p})" in card)
check("SECURITY.md link is absolute", f"]({GITHUB}/blob/main/SECURITY.md)" in card)
check("LICENSE link is absolute", f"]({GITHUB}/blob/main/LICENSE)" in card)

cases = {
    "![a](eval/snapshots/battle.png)": f"![a]({GITHUB}/raw/main/eval/snapshots/battle.png)",
    "![a](./eval/snapshots/hud.png)": f"![a]({GITHUB}/raw/main/eval/snapshots/hud.png)",
    "[s](SECURITY.md)": f"[s]({GITHUB}/blob/main/SECURITY.md)",
    "[d](docs/ARCHITECTURE.md)": f"[d]({GITHUB}/blob/main/docs/ARCHITECTURE.md)",
    "[x](https://example.org/a.png)": "[x](https://example.org/a.png)",
    "[x](#rules-of-a-battle)": "[x](#rules-of-a-battle)",
    "[m](mailto:a@b.c)": "[m](mailto:a@b.c)",
    "[![ci](https://img.shields.io/x.svg)](https://github.com/x/y)": "[![ci](https://img.shields.io/x.svg)](https://github.com/x/y)",
}
for src, want in cases.items():
    got = absolutize(src)
    check(f"rewrite {src}", got == want, "" if got == want else f"got {got}")

print(f"\n{'ALL PASS' if not fails else str(fails) + ' FAILED'}")
sys.exit(1 if fails else 0)
