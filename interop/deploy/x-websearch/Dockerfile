FROM rust:1.91.1-bookworm@sha256:c1e5f19e773b7878c3f7a805dd00a495e747acbdc76fb2337a4ebf0418896b33 AS build
RUN apt-get update \
    && apt-get install --yes --no-install-recommends libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
ENV CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/src/.x-websearch-target
COPY . .
RUN cargo build --locked --manifest-path interop/Cargo.toml --release --package x-websearch --bin x-websearch

FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 65532 x-websearch \
    && useradd --system --uid 65532 --gid 65532 --home-dir /nonexistent --shell /usr/sbin/nologin x-websearch \
    && mkdir -p /etc/x-websearch /var/lib/x-websearch \
    && chown 65532:65532 /var/lib/x-websearch \
    && chmod 0700 /var/lib/x-websearch
COPY --from=build /src/.x-websearch-target/release/x-websearch /usr/local/bin/x-websearch
USER 65532:65532
EXPOSE 8480
VOLUME ["/var/lib/x-websearch"]
ENTRYPOINT ["/usr/local/bin/x-websearch"]
CMD ["--config", "/etc/x-websearch/config.json"]
