# Models

## The catalogue

```{include} ../README.md
:start-after: "## Models"
:end-before: "## Rules of a battle"
```

## Pick and keep a pair

- For one battle: `dogfight --zorb ALIAS --krell ALIAS`, with a catalogue alias or any Hugging Face id.
- To keep it: set `lm_model_a` and `lm_model_b` to the full Hugging Face ids in `~/.config/dogfight/config.toml`.
- To download ahead of time: `dogfight arena pull`. To measure the peak memory of the pair: `dogfight arena check --load`.

[Model templates](templates.md) lists a ready pair for each GPU and Mac size.
