# LLM Dogfight live demo for a Hugging Face Space (free CPU tier): builds the binary, installs
# CPU torch, pre-downloads the two smallest models, and serves the battle in a browser terminal.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY agents ./agents
RUN cargo build --release --locked

FROM python:3.12-slim-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates && rm -rf /var/lib/apt/lists/*
# ttyd: a terminal in the browser
RUN curl -fsSL -o /usr/local/bin/ttyd https://github.com/tsl0922/ttyd/releases/download/1.7.7/ttyd.x86_64 && chmod +x /usr/local/bin/ttyd
RUN pip install --no-cache-dir torch --index-url https://download.pytorch.org/whl/cpu && pip install --no-cache-dir transformers huggingface_hub
RUN useradd -m -u 1000 user
USER user
ENV HOME=/home/user PATH=/home/user/.local/bin:$PATH HF_HUB_DISABLE_PROGRESS_BARS=1
WORKDIR /home/user
COPY --from=build --chown=user /src/target/release/dogfight /home/user/.local/bin/dogfight
COPY --chown=user agents/arena.py /home/user/arena.py
# the demo pair: 1.7 GB on disk, both answer in well under a second of CPU per decision
RUN python3 /home/user/arena.py --pull --model-a HuggingFaceTB/SmolLM2-360M-Instruct --model-b Qwen/Qwen2.5-0.5B-Instruct
RUN mkdir -p .config/dogfight && printf 'lm_backend = "cpu"\nlm_model_a = "HuggingFaceTB/SmolLM2-360M-Instruct"\nlm_model_b = "Qwen/Qwen2.5-0.5B-Instruct"\nlm_think_seconds = 2.0\nfps = 24\n' > .config/dogfight/config.toml
EXPOSE 7860
# every visitor gets their own battle; the terminal is read-only for the page (-W off) so a
# stray key from a viewer cannot end someone else's game
CMD ["ttyd", "-p", "7860", "-t", "fontSize=14", "-t", "disableLeaveAlert=true", "-W", "dogfight", "--duration", "900"]
