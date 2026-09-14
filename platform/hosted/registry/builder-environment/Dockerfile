FROM rust:1.91.1-bookworm@sha256:c1e5f19e773b7878c3f7a805dd00a495e747acbdc76fb2337a4ebf0418896b33 AS certificates
FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171
COPY --from=certificates /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
ENV DEBIAN_FRONTEND=noninteractive
RUN rm -f /etc/apt/sources.list /etc/apt/sources.list.d/debian.sources \
    && printf '%s\n' \
       'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/20260905T000000Z bookworm main' \
       'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/20260905T000000Z bookworm-updates main' \
       'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian-security/20260905T000000Z bookworm-security main' \
       > /etc/apt/sources.list \
    && apt-get update && apt-get install -y --no-install-recommends \
       ca-certificates curl xz-utils gcc libc6-dev make binutils clang-16 lld-16 \
    && rm -rf /var/lib/apt/lists/*
RUN cp -L /usr/bin/clang-16 /usr/bin/clang && cp -L /usr/bin/wasm-ld-16 /usr/bin/wasm-ld
RUN curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
       https://nodejs.org/dist/v22.20.0/node-v22.20.0-linux-x64.tar.xz -o /tmp/node.tar.xz \
    && echo '00bbd05e306ea68b6e13e17360d0e2f680b493ef95f2fea1c4296ff7437530bc  /tmp/node.tar.xz' | sha256sum --check --strict \
    && mkdir -p /opt/node && tar -C /opt/node --strip-components=1 -xJf /tmp/node.tar.xz \
    && rm /tmp/node.tar.xz && rm -rf /opt/node/include /opt/node/share
ENV PATH=/opt/node/bin:$PATH
COPY package.json package-lock.json /opt/assemblyscript/
RUN npm ci --prefix /opt/assemblyscript --ignore-scripts --no-audit --no-fund && npm cache clean --force \
    && cp -a /opt/assemblyscript/node_modules/assemblyscript /opt/assemblyscript/node_modules/binaryen /opt/assemblyscript/node_modules/long /opt/node/lib/node_modules/ \
    && cp /opt/assemblyscript/package-lock.json /opt/node/lib/assemblyscript-package-lock.json \
    && rm -rf /opt/assemblyscript \
    && for tool in asc:assemblyscript/bin/asc.js npm:npm/bin/npm-cli.js npx:npm/bin/npx-cli.js; do \
       name=${tool%%:*}; script=${tool#*:}; rm -f /opt/node/bin/$name; \
       printf '#!/bin/sh\nexec /opt/node/bin/node /opt/node/lib/node_modules/%s "$@"\n' "$script" > /opt/node/bin/$name; \
       chmod 0755 /opt/node/bin/$name; done
ENV RUSTUP_HOME=/opt/cache/rustup CARGO_HOME=/opt/cache/cargo
COPY rust-downloads.lock install-rust.sh /tmp/
RUN sh /tmp/install-rust.sh \
    && rm /tmp/install-rust.sh /tmp/rust-downloads.lock /opt/cache/cargo/env \
    && rm -rf /opt/cache/rustup/tmp /opt/cache/rustup/downloads /opt/cache/cargo/registry
COPY vendor /opt/cache/vendor
COPY cargo-config.toml /opt/cache/cargo/config.toml
COPY layerx-rustc /opt/cache/cargo/bin/layerx-rustc
COPY layerx-build /bin/layerx-build
RUN chmod 0755 /bin/layerx-build /opt/cache/cargo/bin/layerx-rustc \
    && printf 'LayerX hosted builder environment v1\nrust 1.91.1 (wasm32-unknown-unknown)\nclang 16 + wasm-ld\nnode 22.20.0 + assemblyscript 0.27.31\n' > /etc/layerx-builder-release
