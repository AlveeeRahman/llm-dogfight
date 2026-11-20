# Development

```{include} ../README.md
:start-after: "## Development"
```

The design, the module map, the sidecar protocol, the file formats and the decision history are in [ARCHITECTURE.md](https://github.com/AlveeeRahman/llm-dogfight/blob/main/docs/ARCHITECTURE.md). [How it works](how-it-works.md) summarises the parts a player meets.

## The documentation

These pages live in `docs-src/` and are built with Sphinx. Pages that cover the game's rules, models, doctrine, performance and security include sections of the README, so the README stays the one place to edit them.

```sh
pip install --require-hashes -r docs-src/requirements.txt
sh docs-src/build.sh          # build with warnings as errors, check the links, pack into docs/site/
```

buildforpeople.dev serves the packed site from `docs/site/` on the main branch under `/docs/llm-dogfight/`.
