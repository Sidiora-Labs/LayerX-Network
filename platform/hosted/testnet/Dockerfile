FROM rust:1.91.1-alpine3.22 AS build
WORKDIR /src
RUN apk add --no-cache musl-dev openssl-dev pkgconfig
COPY . .
RUN cargo build --locked --release --manifest-path platform/Cargo.toml -p layerx-platform-testnet

FROM alpine:3.22
RUN apk add --no-cache ca-certificates curl jq libgcc openssl && addgroup -g 4020 layerx && adduser -D -H -u 4020 -G layerx layerx
COPY --from=build /src/platform/target/release/layerx-testnet-control /usr/local/bin/layerx-testnet-control
COPY platform/hosted/testnet/run-testnet.sh platform/hosted/testnet/reset-testnet.sh platform/hosted/testnet/render-status.sh /opt/layerx/
USER 4020:4020
ENTRYPOINT ["/usr/local/bin/layerx-testnet-control"]
