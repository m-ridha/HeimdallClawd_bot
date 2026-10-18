FROM rust:1.99-slim-bookworm AS rust-tools
# Copy real toolchain binaries/sysroot, not rustup proxies needing a writable HOME.
RUN rustup component add rustfmt clippy \
    && mkdir -p /opt/heimdall-rust \
    && cp -a "$(rustc --print sysroot)/." /opt/heimdall-rust/ \
    && chmod -R go-w /opt/heimdall-rust

FROM rust-tools AS builder
RUN apt-get update && apt-get install -y --no-install-recommends git python3 jq gcc libc6-dev libstdc++6 && rm -rf /var/lib/apt/lists/*
RUN for tool in cargo rustc rustdoc rustfmt cargo-fmt clippy-driver cargo-clippy; do ln -s "/opt/heimdall-rust/bin/$tool" "/usr/bin/$tool"; done
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests
COPY entrypoint.sh ./
RUN cargo test --locked -- --show-output && cargo build --release --locked
FROM rust:1.99-slim-bookworm AS contract
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates pkg-config g++ libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /runtime
RUN curl --fail --location --retry 3 -H 'User-Agent: HeimdallClawd-build' https://api.github.com/repos/zeroclaw-labs/zeroclaw/tarball/cb2b20a9fe59004b7ef03e123942d31f29236d15 -o /tmp/runtime.tar.gz \
    && echo '81c5a9dd51ab4738d3e1d71dd93b191de15935f4b0d6e83f6a7a28f2a3fdf8a4  /tmp/runtime.tar.gz' | sha256sum -c - \
    && tar -xzf /tmp/runtime.tar.gz --strip-components=1 -C /runtime && rm /tmp/runtime.tar.gz
COPY --from=builder /build/target/release/heimdall-executor /usr/local/bin/heimdall-executor
COPY entrypoint.sh /tmp/executor-entrypoint.sh
COPY contract/mcp-client.rs crates/zeroclaw-tools/tests/heimdall_mcp.rs
ENV HEIMDALL_TEST_ENTRYPOINT=/tmp/executor-entrypoint.sh
RUN cargo test --locked -p zeroclaw-tools --test heimdall_mcp && touch /contract-passed
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends git python3 jq gcc libc6-dev libstdc++6 && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 64173 --user-group --no-create-home executor
COPY --from=rust-tools /opt/heimdall-rust /opt/heimdall-rust
RUN for tool in cargo rustc rustdoc rustfmt cargo-fmt clippy-driver cargo-clippy; do ln -s "/opt/heimdall-rust/bin/$tool" "/usr/bin/$tool"; done
COPY --from=builder /build/target/release/heimdall-executor /usr/local/bin/heimdall-executor
COPY --from=contract /contract-passed /usr/local/share/heimdall-contract-passed
COPY entrypoint.sh /usr/local/bin/entrypoint
RUN chmod 755 /usr/local/bin/entrypoint
ENV PORT=3889
EXPOSE 3889
ENTRYPOINT ["/usr/local/bin/entrypoint"]
