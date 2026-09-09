FROM rust:1.91.1-bookworm@sha256:c1e5f19e773b7878c3f7a805dd00a495e747acbdc76fb2337a4ebf0418896b33 AS build
WORKDIR /src
ENV CARGO_BUILD_JOBS=4 MAKEFLAGS=-j4
RUN apt-get update && apt-get install -y --no-install-recommends build-essential libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --locked --release --manifest-path human/Cargo.toml -p layerx-human-service -p layerx-human-kms --bins
RUN cargo build --locked --release --manifest-path agent/Cargo.toml -p layerx-agentd --bin layerx-agentd
RUN cargo build --locked --release --manifest-path human/Cargo.toml -p layerx-human-identity-provider -p layerx-human-security-provider -p layerx-human-movement-provider --bins

FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -g 4020 layerx && useradd -M -u 4020 -g 4020 -s /usr/sbin/nologin layerx
COPY --from=build /src/human/target/release/layerx-human-service /usr/local/bin/layerx-human-service
COPY --from=build /src/human/target/release/layerx-human-components /usr/local/bin/layerx-human-components
COPY --from=build /src/human/target/release/layerx-human-kms /usr/local/bin/layerx-human-kms
COPY --from=build /src/agent/target/release/layerx-agentd /usr/local/bin/layerx-agentd
COPY --from=build /src/human/target/release/layerx-human-identity-provider /usr/local/bin/layerx-human-identity-provider
COPY --from=build /src/human/target/release/layerx-human-security-provider /usr/local/bin/layerx-human-security-provider
COPY --from=build /src/human/target/release/layerx-human-movement-provider /usr/local/bin/layerx-human-movement-provider
COPY --chmod=0555 platform/hosted/human/entrypoint.sh /usr/local/bin/human-entrypoint
USER 4020:4020
ENTRYPOINT ["/usr/local/bin/layerx-human-service"]
