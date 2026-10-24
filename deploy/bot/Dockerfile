FROM rust:1.99-slim-bookworm AS launcher-build
WORKDIR /launcher
ENV CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
COPY launcher/Cargo.toml launcher/Cargo.lock ./
COPY launcher/src ./src
COPY launcher/tests ./tests
COPY deploy/bot/config.template.toml deploy/bot/TOOLS.md deploy/bot/IDENTITY.md /deploy/bot/
RUN cargo test --locked --lib --test config --test skills_mcp --test startup --test agentmail && cargo build --release --locked

FROM scratch AS support-binaries
COPY --from=launcher-build /launcher/target/release/heimdall-launcher /heimdall-launcher

FROM rust:1.99-slim-bookworm AS runtime-source
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates patch pkg-config g++ libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /runtime
ENV CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 \
    CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_PROFILE_RELEASE_OPT_LEVEL=1
RUN curl --fail --location --retry 3 -H 'User-Agent: HeimdallClawd-build' https://api.github.com/repos/zeroclaw-labs/zeroclaw/tarball/c473221313341a853e04122f93bb9bd26b5e6c47 -o /tmp/runtime.tar.gz \
    && echo 'ae1916dd79364f936a6600e8eb143aa1f405750d501f6c5ef7f1a83f717d574c  /tmp/runtime.tar.gz' | sha256sum -c - \
    && tar -xzf /tmp/runtime.tar.gz --strip-components=1 -C /runtime && rm /tmp/runtime.tar.gz
COPY deploy/bot/patches/private-telegram.patch /tmp/private-telegram.patch
COPY deploy/bot/patches/http-secret-origins.patch /tmp/http-secret-origins.patch
COPY deploy/bot/patches/http-diagnostics.patch /tmp/http-diagnostics.patch
COPY deploy/bot/patches/gateway-admin-auth.patch /tmp/gateway-admin-auth.patch
COPY deploy/bot/patches/sop-workshop-guidance.patch /tmp/sop-workshop-guidance.patch
COPY deploy/bot/patches/responses-tool-history.patch /tmp/responses-tool-history.patch
COPY deploy/bot/patches/browser-output.patch /tmp/browser-output.patch
RUN patch --fuzz=0 -p1 < /tmp/http-secret-origins.patch && patch --fuzz=0 -p1 < /tmp/http-diagnostics.patch && patch --fuzz=0 -p1 < /tmp/private-telegram.patch && patch --fuzz=0 -p1 < /tmp/gateway-admin-auth.patch && patch --fuzz=0 -p1 < /tmp/sop-workshop-guidance.patch && patch --fuzz=0 -p1 < /tmp/responses-tool-history.patch && patch --fuzz=0 -p1 < /tmp/browser-output.patch
# Build dashboard assets only from the checksum-pinned runtime source.
FROM node:26-bookworm-slim AS dashboard-node
FROM runtime-source AS runtime-web
COPY --from=dashboard-node /usr/local/ /usr/local/
RUN cd web && npm ci --ignore-scripts && cd .. && cargo web build

FROM runtime-source AS runtime-compile
COPY --from=runtime-web /runtime/web/dist /runtime/web/dist
RUN cargo build --release --locked --no-default-features --features agent-runtime,tools-external,tool-project-intel,channel-telegram,gateway,schema-export,plugins-wasm-cranelift,embedded-web,webauthn,channel-acp-server,browser-native --bin zeroclaw

FROM scratch AS runtime-binary
COPY --from=runtime-compile /runtime/target/release/zeroclaw /zeroclaw

FROM launcher-build AS launcher-validation
COPY prebuilt/zeroclaw /usr/local/bin/zeroclaw
RUN chmod 755 /usr/local/bin/zeroclaw
ENV HEIMDALL_TEST_RUNTIME=/usr/local/bin/zeroclaw
RUN cargo test --locked

FROM runtime-source AS native-test-compile
COPY deploy/tests/telegram-transcription.rs crates/zeroclaw-channels/tests/heimdall_transcription.rs
COPY deploy/tests/logging-privacy.rs crates/zeroclaw-channels/tests/heimdall_logging.rs
COPY deploy/tests/tool-inventory.rs crates/zeroclaw-runtime/tests/heimdall_inventory.rs
COPY deploy/tests/sop-workshop.rs crates/zeroclaw-runtime/tests/heimdall_sop_workshop.rs
COPY deploy/tests/http-secret-origins.rs crates/zeroclaw-tools/tests/heimdall_http_origins.rs
COPY deploy/tests/skills-mcp.rs crates/zeroclaw-tools/tests/heimdall_skills_stdio.rs
COPY deploy/tests/codespace-mcp.rs crates/zeroclaw-tools/tests/heimdall_codespace_stdio.rs
COPY deploy/tests/config-secrets.rs crates/zeroclaw-config/tests/heimdall_secrets.rs
# Compile contracts before copying release binaries/configuration: those inputs are
# read at test runtime, and must not invalidate the expensive test compiler layer.
RUN cargo test --locked -p zeroclaw-channels --no-default-features --features channel-telegram --lib heimdall --no-run \
    && cargo test --locked -p zeroclaw-providers --lib heimdall --no-run \
    && cargo test --locked -p zeroclaw-gateway --no-default-features --lib heimdall --no-run \
    && cargo test --locked -p zeroclaw-channels --no-default-features --features channel-telegram --test heimdall_logging --test heimdall_transcription --no-run \
    && cargo test --locked -p zeroclaw-tools --no-default-features --features tools-external --test heimdall_skills_stdio --test heimdall_codespace_stdio --test heimdall_http_origins --no-run \
    && cargo test --locked -p zeroclaw-config --no-default-features --features schema-export --test heimdall_secrets --no-run \
    && cargo test --locked -p zeroclaw-runtime --no-default-features --features tools-external,tool-project-intel,browser-native --test heimdall_inventory --test heimdall_sop_workshop --no-run

FROM native-test-compile AS runtime-build
COPY prebuilt/zeroclaw /runtime/target/release/zeroclaw
RUN chmod 755 /runtime/target/release/zeroclaw && cp /runtime/target/release/zeroclaw /usr/local/bin/zeroclaw
COPY --from=launcher-validation /launcher/target/release/heimdall-launcher /usr/local/bin/heimdall-launcher
COPY deploy/bot/config.template.toml /tmp/heimdall-config.toml
COPY deploy/bot/TOOLS.md /tmp/heimdall-TOOLS.md
COPY deploy/bot/evals /tmp/heimdall-evals
ENV HEIMDALL_TEST_DAEMON_VERBOSE=0 HEIMDALL_TEST_TOOLS=/tmp/heimdall-TOOLS.md HEIMDALL_TEST_CONFIG=/tmp/heimdall-config.toml HEIMDALL_TEST_SKILLS_MCP=/usr/local/bin/heimdall-launcher
RUN cargo test --locked -p zeroclaw-channels --no-default-features --features channel-telegram --lib heimdall \
    && cargo test --locked -p zeroclaw-providers --lib heimdall \
    && cargo test --locked -p zeroclaw-gateway --no-default-features --lib heimdall \
    && cargo test --locked -p zeroclaw-channels --no-default-features --features channel-telegram --test heimdall_logging --test heimdall_transcription \
    && cargo test --locked -p zeroclaw-tools --no-default-features --features tools-external --test heimdall_skills_stdio --test heimdall_codespace_stdio --test heimdall_http_origins \
    && cargo test --locked -p zeroclaw-config --no-default-features --features schema-export --test heimdall_secrets \
    && cargo test --locked -p zeroclaw-runtime --no-default-features --features tools-external,tool-project-intel,browser-native --test heimdall_inventory --test heimdall_sop_workshop \
    && /runtime/target/release/zeroclaw eval run --suite /tmp/heimdall-evals --mode replay \
    && touch /contract-passed

FROM scratch AS release-binaries
COPY --from=runtime-build /runtime/target/release/zeroclaw /zeroclaw
COPY --from=launcher-validation /launcher/target/release/heimdall-launcher /heimdall-launcher
COPY --from=runtime-build /contract-passed /contract-passed
