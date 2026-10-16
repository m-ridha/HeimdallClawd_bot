FROM rust:1.99-slim-bookworm AS executor-build
RUN apt-get update && apt-get install -y --no-install-recommends git python3 jq && rm -rf /var/lib/apt/lists/*
WORKDIR /executor
COPY executor/Cargo.toml executor/Cargo.lock ./
COPY executor/src ./src
COPY executor/tests ./tests
COPY executor/entrypoint.sh ./
RUN cargo test --locked && cargo build --release --locked

FROM rust:1.99-slim-bookworm AS launcher-build
WORKDIR /launcher
ENV CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
COPY launcher/Cargo.toml launcher/Cargo.lock ./
COPY launcher/src ./src
COPY launcher/tests ./tests
COPY deploy/bot/config.template.toml deploy/bot/TOOLS.md deploy/bot/IDENTITY.md /deploy/bot/
RUN cargo test --locked --lib --test config --test skills_mcp --test startup && cargo build --release --locked

FROM scratch AS support-binaries
COPY --from=launcher-build /launcher/target/release/heimdall-launcher /heimdall-launcher
COPY --from=executor-build /executor/target/release/heimdall-executor /heimdall-executor

FROM rust:1.99-slim-bookworm AS runtime-source
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates patch pkg-config g++ libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /runtime
ENV CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 \
    CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_PROFILE_RELEASE_OPT_LEVEL=1
RUN curl --fail --location --retry 3 -H 'User-Agent: HeimdallClawd-build' https://api.github.com/repos/zeroclaw-labs/zeroclaw/tarball/cb2b20a9fe59004b7ef03e123942d31f29236d15 -o /tmp/runtime.tar.gz \
    && echo '81c5a9dd51ab4738d3e1d71dd93b191de15935f4b0d6e83f6a7a28f2a3fdf8a4  /tmp/runtime.tar.gz' | sha256sum -c - \
    && tar -xzf /tmp/runtime.tar.gz --strip-components=1 -C /runtime && rm /tmp/runtime.tar.gz
COPY deploy/bot/patches/private-telegram.patch /tmp/private-telegram.patch
COPY deploy/bot/patches/remote-skills.patch /tmp/remote-skills.patch
COPY deploy/bot/patches/wasm-wit.patch /tmp/wasm-wit.patch
COPY deploy/bot/patches/http-secret-origins.patch /tmp/http-secret-origins.patch
RUN patch -p1 < /tmp/http-secret-origins.patch && patch -p1 < /tmp/private-telegram.patch && patch -p1 < /tmp/remote-skills.patch && patch -p1 < /tmp/wasm-wit.patch
# Build dashboard assets only from the checksum-pinned runtime source.
FROM node:24-bookworm-slim AS dashboard-node
FROM runtime-source AS runtime-web
COPY --from=dashboard-node /usr/local/ /usr/local/
RUN cd web && npm ci --ignore-scripts && cd .. && cargo web build

FROM runtime-source AS runtime-compile
COPY --from=runtime-web /runtime/web/dist /runtime/web/dist
RUN cargo build --release --locked --no-default-features --features agent-runtime,channel-telegram,gateway,schema-export,plugins-wasm-cranelift,embedded-web,webauthn,channel-acp-server --bin zeroclaw

FROM scratch AS runtime-binary
COPY --from=runtime-compile /runtime/target/release/zeroclaw /zeroclaw

FROM launcher-build AS launcher-validation
COPY prebuilt/zeroclaw /usr/local/bin/zeroclaw
RUN chmod 755 /usr/local/bin/zeroclaw
ENV HEIMDALL_TEST_RUNTIME=/usr/local/bin/zeroclaw
RUN cargo test --locked

FROM runtime-source AS native-test-compile
COPY executor/contract/mcp-client.rs crates/zeroclaw-tools/tests/heimdall_mcp.rs
COPY deploy/tests/remote-skills.rs crates/zeroclaw-runtime/tests/heimdall_remote_skills.rs
COPY deploy/tests/logging-privacy.rs crates/zeroclaw-channels/tests/heimdall_logging.rs
COPY deploy/tests/tool-inventory.rs crates/zeroclaw-runtime/tests/heimdall_inventory.rs
COPY deploy/tests/http-secret-origins.rs crates/zeroclaw-tools/tests/heimdall_http_origins.rs
COPY deploy/tests/skills-mcp.rs crates/zeroclaw-tools/tests/heimdall_skills_stdio.rs
COPY deploy/tests/codespace-mcp.rs crates/zeroclaw-tools/tests/heimdall_codespace_stdio.rs
COPY deploy/tests/config-secrets.rs crates/zeroclaw-config/tests/heimdall_secrets.rs
# Compile contracts before copying release binaries/configuration: those inputs are
# read at test runtime, and must not invalidate the expensive test compiler layer.
RUN cargo test --locked -p zeroclaw-channels --no-default-features --features channel-telegram --lib heimdall --no-run \
    && cargo test --locked -p zeroclaw-channels --no-default-features --features channel-telegram --test heimdall_logging --no-run \
    && cargo test --locked -p zeroclaw-tools --no-default-features --test heimdall_mcp --test heimdall_skills_stdio --test heimdall_codespace_stdio --test heimdall_http_origins --no-run \
    && cargo test --locked -p zeroclaw-config --no-default-features --features schema-export --test heimdall_secrets --no-run \
    && cargo test --locked -p zeroclaw-runtime --no-default-features --test heimdall_remote_skills --test heimdall_inventory --no-run

FROM native-test-compile AS runtime-build
COPY executor/entrypoint.sh /tmp/executor-entrypoint.sh
COPY prebuilt/zeroclaw /runtime/target/release/zeroclaw
COPY prebuilt/heimdall-executor /usr/local/bin/heimdall-executor
RUN chmod 755 /runtime/target/release/zeroclaw /usr/local/bin/heimdall-executor
COPY --from=launcher-validation /launcher/target/release/heimdall-launcher /usr/local/bin/heimdall-launcher
COPY deploy/bot/config.template.toml /tmp/heimdall-config.toml
COPY deploy/bot/TOOLS.md /tmp/heimdall-TOOLS.md
COPY deploy/bot/evals /tmp/heimdall-evals
ENV HEIMDALL_TEST_DAEMON_VERBOSE=0 HEIMDALL_TEST_TOOLS=/tmp/heimdall-TOOLS.md HEIMDALL_TEST_ENTRYPOINT=/tmp/executor-entrypoint.sh HEIMDALL_TEST_CONFIG=/tmp/heimdall-config.toml HEIMDALL_TEST_SKILLS_MCP=/usr/local/bin/heimdall-launcher
RUN cargo test --locked -p zeroclaw-channels --no-default-features --features channel-telegram --lib heimdall \
    && cargo test --locked -p zeroclaw-channels --no-default-features --features channel-telegram --test heimdall_logging \
    && cargo test --locked -p zeroclaw-tools --no-default-features --test heimdall_mcp --test heimdall_skills_stdio --test heimdall_codespace_stdio --test heimdall_http_origins \
    && cargo test --locked -p zeroclaw-config --no-default-features --features schema-export --test heimdall_secrets \
    && cargo test --locked -p zeroclaw-runtime --no-default-features --test heimdall_remote_skills --test heimdall_inventory \
    && /runtime/target/release/zeroclaw eval run --suite /tmp/heimdall-evals --mode replay \
    && touch /contract-passed

FROM scratch AS release-binaries
COPY --from=runtime-build /runtime/target/release/zeroclaw /zeroclaw
COPY --from=launcher-validation /launcher/target/release/heimdall-launcher /heimdall-launcher
COPY --from=runtime-build /usr/local/bin/heimdall-executor /heimdall-executor
COPY --from=runtime-build /contract-passed /contract-passed

FROM debian:bookworm-slim AS gh-download
ARG GH_VERSION=2.102.0
ARG GH_SHA256=bb766f710eef8ede859c18578c72c327597cd4c8a85b06001b1f3843c6019386
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates && rm -rf /var/lib/apt/lists/* \
    && curl --fail --location --retry 3 -o /tmp/gh.tar.gz "https://github.com/cli/cli/releases/download/v${GH_VERSION}/gh_${GH_VERSION}_linux_amd64.tar.gz" \
    && echo "${GH_SHA256}  /tmp/gh.tar.gz" | sha256sum -c - \
    && tar -xzf /tmp/gh.tar.gz --strip-components=2 -C /usr/local/bin "gh_${GH_VERSION}_linux_amd64/bin/gh"

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 gosu git lynx openssh-client && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 1000 heimdall && useradd --uid 1000 --gid 1000 --no-create-home heimdall
COPY --from=gh-download /usr/local/bin/gh /usr/local/bin/gh
COPY --from=runtime-build /runtime/target/release/zeroclaw /usr/local/bin/zeroclaw
COPY --from=launcher-validation /launcher/target/release/heimdall-launcher /usr/local/bin/heimdall-launcher
LABEL org.opencontainers.image.title="HeimdallClawd" \
    io.heimdall.runtime.version="0.8.5" io.heimdall.runtime.revision="cb2b20a9fe59004b7ef03e123942d31f29236d15"
COPY deploy/bot/evals /usr/share/heimdall/evals
ENV HOME=/data/zeroclaw ZEROCLAW_CONFIG_DIR=/data/zeroclaw PORT=3888
EXPOSE 3888
ENTRYPOINT ["/usr/local/bin/heimdall-launcher"]
