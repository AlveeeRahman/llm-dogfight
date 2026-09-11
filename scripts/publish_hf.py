#!/usr/bin/env python3
"""Publish LLM Dogfight to its Hugging Face Space (static, free tier): a landing page with the
screenshots, the README as the Space card, and the release tarballs (if any are given) under
releases/<tag>/. (A live browser demo would need a Docker Space, which Hugging Face only hosts
on paid plans; space/Dockerfile is kept for anyone who wants to run that themselves.)

  HF_TOKEN=... python3 scripts/publish_hf.py [--tag v0.4.1] [--assets DIR]
"""
import argparse
import os
import pathlib
import sys

import markdown
from huggingface_hub import HfApi

SPACE = "AlveRahman/llm-dogfight"
GITHUB = "https://github.com/AlveeeRahman/llm-dogfight"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tag", default="")
    ap.add_argument("--assets", default="")
    ap.add_argument("--space", default=SPACE)
    a = ap.parse_args()
    root = pathlib.Path(__file__).resolve().parent.parent
    readme = (root / "README.md").read_text(encoding="utf-8")
    # absolute links so the card works away from GitHub
    readme = readme.replace("](eval/snapshots/ufo.png)", f"]({GITHUB}/raw/main/eval/snapshots/ufo.png)")
    readme = readme.replace("](SECURITY.md)", f"]({GITHUB}/blob/main/SECURITY.md)").replace("](LICENSE)", f"]({GITHUB}/blob/main/LICENSE)")
    shots = root / "eval" / "snapshots"
    front = ("---\ntitle: LLM Dogfight\nemoji: 🛸\ncolorFrom: purple\ncolorTo: green\nsdk: static\npinned: true\nlicense: mit\n"
             "short_description: Two small local LLMs dogfight in your terminal\n---\n\n")
    body = markdown.markdown(readme, extensions=["tables", "fenced_code"])
    tmpl = (root / "space" / "landing.html").read_text(encoding="utf-8")
    page = tmpl.replace("@GITHUB@", GITHUB).replace("@BODY@", body)
    out = pathlib.Path("target/scratch/hf_space")
    out.mkdir(parents=True, exist_ok=True)
    (out / "README.md").write_text(front + readme, encoding="utf-8")
    (out / "index.html").write_text(page, encoding="utf-8")
    for img in ("battle.png", "hud.png", "abliterated.png"):
        if (shots / img).exists():
            (out / img).write_bytes((shots / img).read_bytes())
    api = HfApi(token=os.environ.get("HF_TOKEN"))
    api.create_repo(a.space, repo_type="space", space_sdk="static", exist_ok=True)
    api.upload_folder(repo_id=a.space, repo_type="space", folder_path=str(out), commit_message=f"site {a.tag or 'main'}")
    if a.assets and a.tag:
        api.upload_folder(repo_id=a.space, repo_type="space", folder_path=a.assets, path_in_repo=f"releases/{a.tag}",
                          allow_patterns=["*.tar.gz", "*.sha256"], commit_message=f"release {a.tag}")
    print(f"published to https://huggingface.co/spaces/{a.space}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
