FROM rust:1.91.1-alpine3.22 AS build
WORKDIR /src
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig
COPY . .
RUN cargo build --locked --release --manifest-path platform/Cargo.toml -p layerx-platform-registry --bin layerx-program-registry --bin layerx-cgroup-exec

FROM alpine:3.22
RUN apk add --no-cache bubblewrap ca-certificates libgcc openssl && addgroup -g 4030 layerx && adduser -D -H -u 4030 -G layerx layerx
COPY --from=build /src/platform/target/release/layerx-program-registry /usr/local/bin/layerx-program-registry
COPY --from=build /src/platform/target/release/layerx-cgroup-exec /usr/bin/layerx-cgroup-exec
USER 4030:4030
CMD ["layerx-program-registry"]
