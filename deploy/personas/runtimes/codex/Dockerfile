# syntax=docker/dockerfile:1.7@sha256:a57df69d0ea827fb7266491f2813635de6f17269be881f696fbfdf2d83dda33e

FROM rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1 AS builder

ARG TARGETARCH
ARG CODEX_TARGET_ARCH
ARG CODEX_BWRAP_SUPPORT_SETUID=0
ARG CODEX_LINUX_SANDBOX_DISABLE_SECCOMP=0
ARG CARGO_BUILD_JOBS=1

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl git libcap-dev pkg-config python3 xz-utils \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src/codex
COPY . .

RUN --mount=type=cache,id=codex-personas-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=codex-personas-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=codex-personas-cargo-target-${TARGETARCH},target=/var/cache/codex-target \
    target_arch="${TARGETARCH:-${CODEX_TARGET_ARCH}}" \
    && case "${target_arch}" in \
        amd64) codex_target="x86_64-unknown-linux-gnu" ;; \
        arm64) codex_target="aarch64-unknown-linux-gnu" ;; \
        *) echo "unsupported Docker target architecture: ${target_arch}" >&2; exit 1 ;; \
    esac \
    && rustup target add "${codex_target}" \
    && CODEX_REPO_ROOT=/src/codex \
        CODEX_BWRAP_SUPPORT_SETUID="${CODEX_BWRAP_SUPPORT_SETUID}" \
        CODEX_LINUX_SANDBOX_DISABLE_SECCOMP="${CODEX_LINUX_SANDBOX_DISABLE_SECCOMP}" \
        CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS}" \
        CARGO_TARGET_DIR=/var/cache/codex-target \
        python3 scripts/build_codex_package.py \
            --target "${codex_target}" \
            --variant codex \
            --cargo-profile release \
            --package-dir /opt/codex \
            --force

FROM node:22-bookworm-slim@sha256:83f487e0a63425e5b4d146fb5e5be574bcbe1b7b843d3ebafdd95eaf7767a7e5 AS node_tools

WORKDIR /opt/node-tools
COPY deploy/personas/runtimes/codex/node-tools/package.json ./
COPY deploy/personas/runtimes/codex/node-tools/package-lock.json ./
RUN npm ci --omit=dev --ignore-scripts \
    && test "$(node -p "require('./node_modules/@automattic/simplenote-mcp/package.json').version")" = "2.0.1" \
    && npm cache clean --force

FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171

ARG CODEX_UID=1000
ARG CODEX_GID=1000
ARG CODEX_BWRAP_SUPPORT_SETUID=0
ARG SOURCE_COMMIT=unknown

LABEL org.opencontainers.image.revision="${SOURCE_COMMIT}"

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        ca-certificates curl git jq libcap2 openssh-client procps tzdata \
    && groupadd --non-unique --gid "${CODEX_GID}" codex \
    && useradd --non-unique --uid "${CODEX_UID}" --gid "${CODEX_GID}" --create-home --shell /bin/bash codex \
    && install -d -m 0700 -o codex -g codex /home/codex/.codex \
    && install -d -m 0755 -o codex -g codex /workspaces /shared/tibbit/profile /shared/tibbit/corpus \
    && install -d -m 1777 /private/tmp \
    && install -d -m 0755 /workspace \
    && ln -s /workspaces /home/codex/Documents \
    && ln -s /workspaces/secretary /workspace/secretary \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder --chown=codex:codex /opt/codex /opt/codex
COPY --from=builder --chown=codex:codex /src/codex/scripts/codex-statusline.sh /opt/codex/bin/codex-statusline
COPY --from=node_tools /usr/local/bin/node /usr/local/bin/node
COPY --from=node_tools --chown=codex:codex /opt/node-tools /opt/node-tools
COPY --chown=root:root deploy/personas/runtimes/codex/bwrap-setuid-wrapper /opt/codex/bin/bwrap-setuid-wrapper
COPY --chown=root:root deploy/personas/runtimes/codex/entrypoint.sh /opt/personas/codex/entrypoint.sh

RUN chmod 0755 /opt/codex/bin/codex-statusline \
        /opt/codex/bin/bwrap-setuid-wrapper \
        /opt/personas/codex/entrypoint.sh \
    && ln -s /opt/node-tools/node_modules/.bin/simplenote-mcp /usr/local/bin/simplenote-mcp \
    && if [ "${CODEX_BWRAP_SUPPORT_SETUID}" = "1" ]; then \
        find / -xdev -type f -perm /6000 -exec chmod a-s {} +; \
        chown root:root /opt/codex/codex-resources/bwrap; \
        chmod 4755 /opt/codex/codex-resources/bwrap; \
        ln -s /opt/codex/bin/bwrap-setuid-wrapper /usr/local/bin/bwrap; \
    else \
        chmod 0755 /opt/codex/codex-resources/bwrap; \
        ln -s /opt/codex/codex-resources/bwrap /usr/local/bin/bwrap; \
    fi

ENV CODEX_HOME=/home/codex/.codex \
    HOME=/home/codex \
    PATH=/opt/codex/codex-path:/opt/codex/bin:/usr/local/bin:/usr/bin:/bin \
    TERM=xterm-256color \
    TZ=America/Denver \
    XDG_CACHE_HOME=/home/codex/.codex/cache \
    XDG_CONFIG_HOME=/home/codex/.codex/config \
    XDG_DATA_HOME=/home/codex/.codex/share

USER codex
WORKDIR /workspaces

STOPSIGNAL SIGTERM
ENTRYPOINT ["/opt/personas/codex/entrypoint.sh"]
CMD ["app-server", "--listen", "unix://", "--remote-control"]
