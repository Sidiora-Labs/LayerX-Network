FROM rust:1.91.1-alpine3.22 AS build
WORKDIR /src
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig
COPY . .
RUN cargo build --locked --release --manifest-path interop/Cargo.toml -p layerx-interop-service --bin layerx-interop-gateway

FROM alpine:3.22
RUN apk add --no-cache ca-certificates libgcc openssl && addgroup -g 4020 layerx && adduser -D -H -u 4020 -G layerx layerx
COPY --from=build /src/interop/target/release/layerx-interop-gateway /usr/local/bin/layerx-interop-gateway
USER 4020:4020
ENTRYPOINT ["/usr/local/bin/layerx-interop-gateway"]
