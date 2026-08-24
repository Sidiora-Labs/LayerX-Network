FROM rust:1.91.1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --manifest-path platform/Cargo.toml --locked --release -p layerx-platform-dashboard --bin layerx-dashboard

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/platform/target/release/layerx-dashboard /usr/local/bin/layerx-dashboard
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/layerx-dashboard"]
