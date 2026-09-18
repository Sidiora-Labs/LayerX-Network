FROM rust:1.91.1-bookworm AS build
RUN apt-get update \
    && apt-get install --yes --no-install-recommends libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
ENV CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/src/.lane-target
COPY . .
RUN cargo build --locked --manifest-path interop/Cargo.toml --release --package layerx-mirror --bin layerx-mirror-publisher
RUN cargo build --locked --manifest-path interop/Cargo.toml --release --package layerx-mirror --bin layerx-mirror-verify
RUN cargo build --locked --manifest-path interop/Cargo.toml --release --package layerx-mirror-signer --bin layerx-mirror-signer

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 65532 --home-dir /nonexistent --shell /usr/sbin/nologin layerx
COPY --from=build /src/.lane-target/release/layerx-mirror-publisher /usr/local/bin/layerx-mirror-publisher
COPY --from=build /src/.lane-target/release/layerx-mirror-verify /usr/local/bin/layerx-mirror-verify
COPY --from=build /src/.lane-target/release/layerx-mirror-signer /usr/local/bin/layerx-mirror-signer
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/layerx-mirror-publisher"]
CMD ["/etc/layerx-mirror/config.json"]
