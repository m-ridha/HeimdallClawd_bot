FROM rust:1.99-slim-bookworm AS executor-build
WORKDIR /executor
COPY executor/Cargo.toml executor/Cargo.lock ./
COPY executor/src ./src
COPY executor/tests ./tests
COPY executor/entrypoint.sh ./
RUN cargo test --locked && cargo build --release --locked

FROM rust:1.99-slim-bookworm AS runtime-build
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates patch pkg-config g++ libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /runtime
ENV CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
RUN curl --fail --location --retry 3 -H 'User-Agent: HeimdallClawd-build' https://api.github.com/repos/zeroclaw-labs/zeroclaw/tarball/cb2b20a9fe59004b7ef03e123942d31f29236d15 -o /tmp/runtime.tar.gz \
    && echo '81c5a9dd51ab4738d3e1d71dd93b191de15935f4b0d6e83f6a7a28f2a3fdf8a4  /tmp/runtime.tar.gz' | sha256sum -c - \
    && tar -xzf /tmp/runtime.tar.gz --strip-components=1 -C /runtime && rm /tmp/runtime.tar.gz
COPY deploy/bot/patches/private-telegram.patch /tmp/private-telegram.patch
RUN patch -p1 < /tmp/private-telegram.patch
COPY --from=executor-build /executor/target/release/heimdall-executor /usr/local/bin/heimdall-executor
COPY executor/entrypoint.sh /tmp/executor-entrypoint.sh
COPY executor/contract/mcp-client.rs crates/zeroclaw-tools/tests/heimdall_mcp.rs
COPY launcher/Cargo.toml launcher/Cargo.lock /launcher/
COPY launcher/src /launcher/src
COPY deploy/bot/config.template.toml /deploy/bot/config.template.toml
RUN cargo build --release --locked --manifest-path /launcher/Cargo.toml && cp /launcher/target/release/heimdall-launcher /usr/local/bin/heimdall-launcher
COPY deploy/tests/skills-mcp.rs crates/zeroclaw-tools/tests/heimdall_skills_stdio.rs
COPY deploy/tests/config-secrets.rs crates/zeroclaw-config/tests/heimdall_secrets.rs
COPY deploy/bot/config.template.toml /tmp/heimdall-config.toml
ENV HEIMDALL_TEST_ENTRYPOINT=/tmp/executor-entrypoint.sh HEIMDALL_TEST_CONFIG=/tmp/heimdall-config.toml HEIMDALL_TEST_SKILLS_MCP=/usr/local/bin/heimdall-launcher
RUN cargo test --locked -p zeroclaw-channels -p zeroclaw-tools -p zeroclaw-infra -p zeroclaw-config --no-default-features --features zeroclaw-channels/channel-telegram,zeroclaw-config/schema-export heimdall \
    && cargo build --release --locked --no-default-features --features agent-runtime,channel-telegram,gateway,schema-export --bin zeroclaw


FROM rust:1.99-slim-bookworm AS launcher-build
WORKDIR /launcher
COPY launcher/Cargo.toml launcher/Cargo.lock ./
COPY launcher/src ./src
COPY launcher/tests ./tests
COPY deploy/bot/config.template.toml /deploy/bot/config.template.toml
COPY --from=runtime-build /runtime/target/release/zeroclaw /usr/local/bin/zeroclaw
ENV HEIMDALL_TEST_RUNTIME=/usr/local/bin/zeroclaw
RUN cargo test --locked && cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 gosu git && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 1000 heimdall && useradd --uid 1000 --gid 1000 --no-create-home heimdall
COPY --from=runtime-build /runtime/target/release/zeroclaw /usr/local/bin/zeroclaw
COPY --from=launcher-build /launcher/target/release/heimdall-launcher /usr/local/bin/heimdall-launcher
LABEL org.opencontainers.image.title="HeimdallClawd" \
      io.heimdall.runtime.version="0.8.5" io.heimdall.runtime.revision="cb2b20a9fe59004b7ef03e123942d31f29236d15"
ENV HOME=/data/zeroclaw ZEROCLAW_CONFIG_DIR=/data/zeroclaw PORT=3888
EXPOSE 3888
ENTRYPOINT ["/usr/local/bin/heimdall-launcher"]
