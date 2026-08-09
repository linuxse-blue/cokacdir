FROM rust:1.97-bookworm AS cokacdir-builder

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        git \
    && rm -rf /var/lib/apt/lists/* /tmp/*

ARG COKACDIR_COMMIT=5ad75524c1ec566b4c4394d0ff545a5466f2f1da
RUN git init /src \
    && cd /src \
    && git remote add origin https://github.com/kstost/cokacdir.git \
    && git fetch --depth 1 origin "${COKACDIR_COMMIT}" \
    && git checkout --detach FETCH_HEAD

COPY patches/herdr-provider.patch /tmp/herdr-provider.patch
COPY patches/herdr.rs /src/src/services/herdr.rs

RUN cd /src \
    && git apply /tmp/herdr-provider.patch \
    && cargo build --release

FROM debian:bookworm-slim AS herdr-bin

ARG HERDR_VERSION=0.8.0
ARG HERDR_SHA256=b872ea7e40fa2cb17e857ac9b62b1bf26db7b403c622f5d2f3f5b35f6e9acd28
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && curl -fsSL \
        "https://github.com/herdrdev/herdr/releases/download/v${HERDR_VERSION}/herdr-linux-x86_64" \
        -o /usr/local/bin/herdr \
    && echo "${HERDR_SHA256}  /usr/local/bin/herdr" | sha256sum -c - \
    && chmod 0755 /usr/local/bin/herdr \
    && rm -rf /var/lib/apt/lists/* /tmp/*

FROM docker:29-cli AS docker-cli

FROM node:24-bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive
ENV NODE_ENV=production

# cokacdir가 사용할 CLI 경로
ENV AGENT_HOME=/home/cokac/.agents
ENV NPM_CONFIG_PREFIX=/home/cokac/.agents/npm
ENV NODE_PATH=/home/cokac/.agents/npm/lib/node_modules
ENV PATH=/home/cokac/.agents/npm/bin:/home/cokac/.agents/bin:${PATH}
ENV COKAC_CODEX_PATH=/home/cokac/.agents/npm/bin/codex
ENV COKAC_CLAUDE_PATH=/usr/local/bin/claude
ENV COKAC_AGY_PATH=/usr/local/bin/agy
ENV COKAC_HERDR_PATH=/usr/local/bin/herdr
ENV COKACDIR_DEBUG=1
ENV HOME=/home/cokac

WORKDIR /workspace

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        bash \
        openssh-client \
        python3 \
        python3-yaml \
        python3-venv \
        procps \
    && rm -rf /var/lib/apt/lists/*

COPY --from=cokacdir-builder /src/target/release/cokacdir /usr/local/bin/cokacdir
COPY --from=herdr-bin /usr/local/bin/herdr /usr/local/bin/herdr
COPY --from=docker-cli /usr/local/bin/docker /usr/local/bin/docker
COPY --chmod=0755 claude-wrapper.sh /usr/local/bin/claude
COPY --chmod=0755 agy-wrapper.sh /usr/local/bin/agy
COPY --chmod=0755 docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

RUN mkdir -p /workspace /home/cokac/.agents/npm /home/cokac/.agents/bin /home/cokac/.cokacdir /home/cokac/.codex /home/cokac/.claude /home/cokac/.local/share \
    && chown -R 1000:1000 /workspace /home/cokac \
    && node --version \
    && npm --version \
    && python3 --version \
    && docker --version \
    && herdr --version \
    && cokacdir --version

VOLUME ["/workspace", "/home/cokac/.agents", "/home/cokac/.cokacdir", "/home/cokac/.codex", "/home/cokac/.claude", "/home/cokac/.local/share"]

ENTRYPOINT ["docker-entrypoint.sh"]
