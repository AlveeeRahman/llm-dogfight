#!/usr/bin/env python3
"""Pack the built site for buildforpeople.dev to serve, and check that it works from any sub-path.

    python3 docs-src/package.py BUILD_DIR OUT_DIR          write OUT_DIR/llm-dogfight-docs.tar.gz and docs-version.json
    python3 docs-src/package.py --check TARBALL [PREFIX]   serve the tarball under PREFIX (default /docs/llm-dogfight/)
                                                           and fail on any link that does not load

The tarball holds the HTML site with relative links only, so it can be unpacked under any path. docs-version.json
says which commit it was built from and when. buildforpeople.dev reads both from docs/site/ on the main branch.
"""
import functools
import hashlib
import html.parser
import http.server
import json
import os
import re
import sys
import tarfile
import tempfile
import threading
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
SLUG = "llm-dogfight"


def pack(build, out):
    build, out = Path(build), Path(out)
    out.mkdir(parents=True, exist_ok=True)
    tarball = out / f"{SLUG}-docs.tar.gz"
    skip = {".buildinfo", ".doctrees"}
    with tarfile.open(tarball, "w:gz") as tar:
        for p in sorted(build.rglob("*")):
            rel = p.relative_to(build)
            if rel.parts[0] in skip or p.is_dir():
                continue
            info = tar.gettarinfo(str(p), arcname=str(Path(f"{SLUG}-docs") / rel))
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            with open(p, "rb") as f:
                tar.addfile(info, f)
    # the bundle's own hash: buildforpeople.dev reloads the docs when this changes
    digest = hashlib.sha256(tarball.read_bytes()).hexdigest()[:16]
    meta = {"commit": digest, "built": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")}
    (out / "docs-version.json").write_text(json.dumps(meta, indent=2) + "\n")
    print(f"wrote {tarball} ({tarball.stat().st_size} bytes) and {out / 'docs-version.json'}")


class Links(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.found = []

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        for key in ("href", "src"):
            if key in a and a[key] and not (tag == "link" and a.get("rel") == "canonical"):
                self.found.append(a[key])


def check(tarball, prefix=f"/docs/{SLUG}/"):
    prefix = "/" + prefix.strip("/") + "/"
    with tempfile.TemporaryDirectory() as tmp:
        site = Path(tmp) / prefix.strip("/")
        site.mkdir(parents=True)
        with tarfile.open(tarball) as tar:
            tar.extractall(Path(tmp) / "unpack", filter="data")
        (Path(tmp) / "unpack" / f"{SLUG}-docs").rename(site)
        class Quiet(http.server.SimpleHTTPRequestHandler):
            def log_message(self, *args):
                pass
        handler = functools.partial(Quiet, directory=tmp)
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        root = f"http://127.0.0.1:{server.server_port}"
        problems, seen, pages = [], set(), 0
        for page in sorted(site.rglob("*.html")):
            if page.relative_to(site).parts[0] == "_static":
                continue                       # theme template fragments, not pages
            url = root + prefix + str(page.relative_to(site)).replace(os.sep, "/")
            pages += 1
            parser = Links()
            parser.feed(page.read_text(encoding="utf-8", errors="replace"))
            for link in parser.found:
                if re.match(r"^(?:[a-z]+:|#|//)", link):
                    continue
                if link.startswith("/"):
                    problems.append(f"{page.relative_to(site)}: absolute path {link}")
                    continue
                target = urllib.parse.urljoin(url, link).split("#")[0].split("?")[0]
                if target in seen:
                    continue
                seen.add(target)
                if not target.startswith(root + prefix):
                    problems.append(f"{page.relative_to(site)}: leaves the prefix: {link}")
                    continue
                try:
                    quoted = urllib.parse.quote(target, safe=":/%")
                    with urllib.request.urlopen(urllib.request.Request(quoted, method="HEAD")) as r:
                        if r.status != 200:
                            problems.append(f"{page.relative_to(site)}: {link} -> {r.status}")
                except urllib.error.HTTPError as e:
                    problems.append(f"{page.relative_to(site)}: {link} -> {e.code}")
        server.shutdown()
    for p in problems:
        print("problem:", p)
    print(f"served under {prefix}: {pages} pages, {len(seen)} distinct links, {len(problems)} problems")
    return 1 if problems or pages == 0 else 0


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "--check":
        sys.exit(check(sys.argv[2], *(sys.argv[3:4])))
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    pack(sys.argv[1], sys.argv[2])
