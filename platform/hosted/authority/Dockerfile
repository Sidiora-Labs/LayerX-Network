FROM rust:1.91.1-alpine3.22 AS build
WORKDIR /src
ENV CARGO_BUILD_JOBS=4
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig
COPY . .
RUN cargo build --locked --release --manifest-path platform/Cargo.toml -p layerx-platform-authority -p layerx-runtime-clock

FROM alpine:3.22
RUN apk add --no-cache ca-certificates libgcc openssl util-linux setpriv \
    && ln -s /bin/setpriv /usr/bin/setpriv \
    && addgroup -g 4020 layerx && adduser -D -H -u 4020 -G layerx layerx
RUN /usr/bin/setpriv --help | grep -- --pdeathsig
COPY --from=build /src/platform/target/release/layerx-receipt-authority /usr/local/bin/layerx-receipt-authority
COPY --from=build /src/platform/target/release/layerx-runtime-clock /usr/local/bin/layerx-runtime-clock
USER 4020:4020
ENTRYPOINT ["/usr/local/bin/layerx-runtime-clock", "--runtime-dir", "/tmp", "--", "/usr/local/bin/layerx-receipt-authority"]
