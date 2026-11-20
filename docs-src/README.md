# docs-src: the LLM Dogfight documentation

Sphinx sources for the documentation at https://buildforpeople.dev/docs/llm-dogfight/.

- `conf.py`: the site settings, and the hook that turns README links and images into GitHub addresses for included sections.
- `*.md`: one file per page. Pages on the rules, models, doctrine, performance and security include sections of the top-level README with `{include}`, so edit those in the README.
- `_static/astryx.css`, `_static/glow.js`, `_static/fonts/`: the Astryx Neutral look shared with the OpenCarMind and TigerOCR docs.
- `package.py`: packs the built site and checks every link under the path it is served from.
- `build.sh`: build, pack and check. Commit `docs/site/` afterwards, since buildforpeople.dev reads it from the main branch.

```sh
pip install --require-hashes -r docs-src/requirements.txt
sh docs-src/build.sh
```
