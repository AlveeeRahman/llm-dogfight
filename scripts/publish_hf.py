#!/usr/bin/env python3
"""Publish LLM Dogfight to its Hugging Face Space (static, free tier): a landing page with the
screenshots, the README as the Space card, and the release tarballs (if any are given) under
releases/<tag>/. (A live browser demo would need a Docker Space, which Hugging Face only hosts
on paid plans; space/Dockerfile is kept for anyone who wants to run that themselves.)

Two stages, so the token-bearing step in CI does nothing but upload:

  python3 scripts/publish_hf.py build  --out site                          # no token needed
  HF_TOKEN=... python3 scripts/publish_hf.py upload --site site [--tag v0.5.0 --assets DIR]
"""
import argparse
import pathlib
import re
import sys

SPACE = "AlveRahman/llm-dogfight"
GITHUB = "https://github.com/AlveeeRahman/llm-dogfight"
IMAGE_EXT = (".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp")
_LINK = re.compile(r"(\]\()([^)\s]+)(\))")  # the target of any link or image, nested badges included
_IMAGE = re.compile(r"!\[[^\]]*\]\(([^)\s]+)\)")


def absolutize(readme: str, github: str = GITHUB) -> str:
    """Rewrite every relative Markdown link and image in the README to an absolute GitHub URL,
    so the Space card renders away from the repository. Images go through raw.githubusercontent
    (GitHub serves them with the right content type there); everything else through blob/main.
    Absolute URLs, anchors and mailto links are left untouched."""

    def sub(m):
        target = m.group(2)
        if re.match(r"^[a-z][a-z0-9+.-]*:", target) or target.startswith(("#", "//")):
            return m.group(0)
        path = target.lstrip("./")
        base = f"{github}/raw/main/" if path.lower().endswith(IMAGE_EXT) else f"{github}/blob/main/"
        return f"{m.group(1)}{base}{path}{m.group(3)}"

    return _LINK.sub(sub, readme)


def readme_images(readme: str):
    """The repository-relative image paths the README references."""
    return sorted({m.group(1).lstrip("./") for m in _IMAGE.finditer(readme)
                   if not re.match(r"^[a-z][a-z0-9+.-]*:", m.group(1))})


def build(root: pathlib.Path, out: pathlib.Path) -> int:
    import markdown  # imported here so the tests can load this module without the runtime deps

    readme = (root / "README.md").read_text(encoding="utf-8")
    images = readme_images(readme)
    missing = [p for p in images if not (root / p).exists()]
    if missing:
        print("README references images that do not exist: " + ", ".join(missing), file=sys.stderr)
        return 1
    card = absolutize(readme)
    front = ("---\ntitle: LLM Dogfight\nemoji: 🛸\ncolorFrom: purple\ncolorTo: green\nsdk: static\npinned: true\nlicense: mit\n"
             "short_description: A terminal UFO dogfight between two local LLMs\n---\n\n")
    body = markdown.markdown(card, extensions=["tables", "fenced_code"])
    tmpl = (root / "space" / "landing.html").read_text(encoding="utf-8")
    page = tmpl.replace("@GITHUB@", GITHUB).replace("@BODY@", body)
    out.mkdir(parents=True, exist_ok=True)
    (out / "README.md").write_text(front + card, encoding="utf-8")
    (out / "index.html").write_text(page, encoding="utf-8")
    # the landing page shows the screenshots by file name, so ship every image the README uses
    for p in images:
        (out / pathlib.Path(p).name).write_bytes((root / p).read_bytes())
    print(f"built {out}: README.md, index.html, {len(images)} images")
    return 0


def upload(site: pathlib.Path, space: str, tag: str, assets: str) -> int:
    import os

    from huggingface_hub import HfApi

    token = os.environ.get("HF_TOKEN")
    if not token:
        print("HF_TOKEN is not set", file=sys.stderr)
        return 1
    for name in ("README.md", "index.html"):
        if not (site / name).exists():
            print(f"{site / name} is missing; run `publish_hf.py build` first", file=sys.stderr)
            return 1
    api = HfApi(token=token)
    api.create_repo(space, repo_type="space", space_sdk="static", exist_ok=True)
    api.upload_folder(repo_id=space, repo_type="space", folder_path=str(site), commit_message=f"site {tag or 'main'}")
    if tag:
        tarballs = sorted(pathlib.Path(assets).glob("dogfight-*.tar.gz")) if assets else []
        if not tarballs:
            print(f"tag {tag} given but no dogfight-*.tar.gz in {assets or '(no --assets)'}", file=sys.stderr)
            return 1
        api.upload_folder(repo_id=space, repo_type="space", folder_path=assets, path_in_repo=f"releases/{tag}",
                          allow_patterns=["*.tar.gz", "*.sha256"], commit_message=f"release {tag}")
    print(f"published to https://huggingface.co/spaces/{space}")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build", help="render the Space folder from README.md and the screenshots")
    b.add_argument("--out", default="target/scratch/hf_space")
    u = sub.add_parser("upload", help="upload a built folder (and optional release assets) to the Space")
    u.add_argument("--site", default="target/scratch/hf_space")
    u.add_argument("--tag", default="")
    u.add_argument("--assets", default="")
    u.add_argument("--space", default=SPACE)
    a = ap.parse_args()
    root = pathlib.Path(__file__).resolve().parent.parent
    if a.cmd == "build":
        return build(root, pathlib.Path(a.out))
    return upload(pathlib.Path(a.site), a.space, a.tag, a.assets)


if __name__ == "__main__":
    sys.exit(main())
