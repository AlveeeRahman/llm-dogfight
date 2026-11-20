#!/bin/sh
# Build the documentation with warnings as errors, check that the packed site works under /docs/llm-dogfight/,
# and write docs/site/llm-dogfight-docs.tar.gz and docs/site/docs-version.json, which buildforpeople.dev serves.
# Run from the repository root after: pip install --require-hashes -r docs-src/requirements.txt
set -e
rm -rf docs-src/_build
python3 -m sphinx -W --keep-going -q -b html docs-src docs-src/_build/html
python3 docs-src/package.py docs-src/_build/html docs/site
python3 docs-src/package.py --check docs/site/llm-dogfight-docs.tar.gz /docs/llm-dogfight/
