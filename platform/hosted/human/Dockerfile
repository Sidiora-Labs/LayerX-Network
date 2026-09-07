FROM rust:1.91.1-bookworm@sha256:c1e5f19e773b7878c3f7a805dd00a495e747acbdc76fb2337a4ebf0418896b33 AS build
WORKDIR /src
ENV CARGO_BUILD_JOBS=4 MAKEFLAGS=-j4
RUN apt-get update && apt-get install -y --no-install-recommends build-essential libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --locked --release --manifest-path human/Cargo.toml -p layerx-human-service --bins

FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -g 4020 layerx && useradd -M -u 4020 -g 4020 -s /usr/sbin/nologin layerx
COPY --from=build /src/human/target/release/layerx-human-service /usr/local/bin/layerx-human-service
COPY --from=build /src/human/target/release/layerx-human-components /usr/local/bin/layerx-human-components
USER 4020:4020
ENTRYPOINT ["/usr/local/bin/layerx-human-service"]
