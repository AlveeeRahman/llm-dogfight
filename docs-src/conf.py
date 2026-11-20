# Sphinx configuration for the LLM Dogfight documentation.
#
#   pip install --require-hashes -r docs-src/requirements.txt
#   sh docs-src/build.sh            build, check and pack into docs/site/ (what buildforpeople.dev serves)
#
# Some pages include sections of the README, so the docs and the README never disagree. Links and images inside
# those sections point at files in the repository; include_read below turns them into GitHub addresses.
import os
import re
from datetime import datetime, timezone

project = "LLM Dogfight"
author = "Mr Pirate"
copyright = f"{datetime.now(timezone.utc).year} Mr Pirate, MIT licence"

REPO = "https://github.com/AlveeeRahman/llm-dogfight"
RAW = "https://raw.githubusercontent.com/AlveeeRahman/llm-dogfight/main/"

extensions = ["myst_parser", "sphinx_design", "sphinx_copybutton"]
myst_enable_extensions = ["colon_fence", "attrs_block", "attrs_inline"]
myst_heading_anchors = 3

exclude_patterns = ["_build", "README.md", "Thumbs.db", ".DS_Store"]
templates_path = ["_templates"]
html_sidebars = {"**": ["docs-nav.html"]}

# buildforpeople.dev serves this build under /docs/llm-dogfight/ and rewrites the canonical link of every page there.
html_baseurl = os.environ.get("DOCS_BASE_URL", "https://buildforpeople.dev/docs/llm-dogfight/")

html_theme = "pydata_sphinx_theme"
html_title = "LLM Dogfight documentation"
html_short_title = "LLM Dogfight"
html_logo = "../docs/assets/icon.png"
html_favicon = "../docs/assets/icon.png"
html_static_path = ["_static"]
html_css_files = ["astryx.css"]
html_js_files = [("glow.js", {"defer": "defer"})]
html_copy_source = False
html_show_sourcelink = False
html_last_updated_fmt = None

html_theme_options = {
    "logo": {"text": "LLM Dogfight", "alt_text": "LLM Dogfight documentation, home"},
    "navbar_align": "left",
    "navbar_center": ["navbar-nav"],
    "navbar_end": ["theme-switcher", "navbar-icon-links"],
    "navbar_persistent": ["search-button-field"],
    "header_links_before_dropdown": 5,
    "icon_links": [
        {"name": "GitHub", "url": REPO, "icon": "fa-brands fa-github"},
        {"name": "Releases", "url": REPO + "/releases", "icon": "fa-solid fa-tag"},
    ],
    "use_edit_page_button": True,
    "show_toc_level": 2,
    "show_prev_next": True,
    "secondary_sidebar_items": ["page-toc", "edit-this-page"],
    "footer_start": ["copyright"],
    "footer_end": [],
    "pygments_light_style": "default",
    "pygments_dark_style": "github-dark",
}
html_context = {
    "github_user": "AlveeeRahman",
    "github_repo": "llm-dogfight",
    "github_version": "main",
    "doc_path": "docs-src",
    "default_mode": "auto",
}

# Markdown links and images in included README sections: images load from raw.githubusercontent.com, files link
# to their page on GitHub. Links to headings (#...) and full URLs stay as they are.
_LINK = re.compile(r"(!?)\[([^\]]*)\]\((?!https?:|#|mailto:)([^)\s]+)\)")


def _rewrite(text):
    def sub(m):
        bang, label, target = m.groups()
        return f"{bang}[{label}]({RAW if bang else REPO + '/blob/main/'}{target})"
    return _LINK.sub(sub, text)


def include_read(app, relative_path, parent_docname, content):
    if str(relative_path).endswith("README.md"):
        content[0] = _rewrite(content[0])


def setup(app):
    app.connect("include-read", include_read)
