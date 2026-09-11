#!/usr/bin/env python3
"""Publish LLM Dogfight to its Hugging Face Space: the README as the Space card and as a
rendered index.html, plus the release tarballs (if any are given) under releases/<tag>/.

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
    front = ("---\ntitle: LLM Dogfight\nemoji: 🛸\ncolorFrom: purple\ncolorTo: green\nsdk: static\npinned: true\nlicense: mit\n"
             "short_description: A UFO dogfight in your terminal, flown by two local LLMs\n---\n\n")
    body = markdown.markdown(readme, extensions=["tables", "fenced_code"])
    html = ("<!doctype html><html><head><meta charset='utf-8'><meta name='viewport' content='width=device-width,initial-scale=1'>"
            "<title>LLM Dogfight</title><style>body{max-width:900px;margin:2rem auto;padding:0 1rem;font:16px/1.55 system-ui,sans-serif;"
            "color:#e6e6ef;background:#0e0f1a}a{color:#7cd7ff}pre{background:#161829;padding:.8rem;overflow-x:auto;border-radius:6px}"
            "code{background:#161829;padding:.1rem .3rem;border-radius:4px}table{border-collapse:collapse}td,th{border:1px solid #2a2d45;padding:.3rem .6rem}"
            "img{max-width:100%}h1,h2{border-bottom:1px solid #2a2d45;padding-bottom:.2rem}</style></head><body>"
            f"<p><a href='{GITHUB}'>Source, issues and releases on GitHub</a></p>{body}</body></html>")
    out = pathlib.Path("target/scratch/hf_space")
    out.mkdir(parents=True, exist_ok=True)
    (out / "README.md").write_text(front + readme, encoding="utf-8")
    (out / "index.html").write_text(html, encoding="utf-8")
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
