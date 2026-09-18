FROM rust:1.91.1-bookworm@sha256:c1e5f19e773b7878c3f7a805dd00a495e747acbdc76fb2337a4ebf0418896b33 AS build
WORKDIR /src
ENV CARGO_BUILD_JOBS=4 MAKEFLAGS=-j4
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential libssl-dev libsqlite3-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY . .
ARG LXP_REVISION=unknown
RUN rustup show active-toolchain \
    && make LXP_REVISION="$LXP_REVISION" layerxd layerx-archive-codec \
    && test -x build/bin/layerxd \
    && test -x build/bin/layerx-archive-codec

FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 libsqlite3-0 python3 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -g 4027 layerx-relay-archive \
    && useradd -M -u 4027 -g 4027 -s /usr/sbin/nologin layerx-relay-archive \
    && install -d -m 0755 -o 4027 -g 4027 /opt/layerx/relay_archive \
    && install -d -m 0750 -o 4027 -g 4027 /var/lib/layerx/relay-archive
COPY --from=build /src/build/bin/layerxd /usr/local/bin/layerxd
COPY --from=build /src/build/bin/layerx-archive-codec /usr/local/bin/layerx-archive-codec
COPY --from=build /src/platform/relay_archive/__init__.py /opt/layerx/relay_archive/__init__.py
COPY --from=build /src/platform/relay_archive/runtime.py /opt/layerx/relay_archive/runtime.py
COPY --from=build /src/platform/relay_archive/store.py /opt/layerx/relay_archive/store.py
COPY --from=build /src/platform/relay_archive/protocol.py /opt/layerx/relay_archive/protocol.py
COPY --from=build /src/platform/relay_archive/forward.py /opt/layerx/relay_archive/forward.py
COPY --from=build /src/platform/relay_archive/peers.py /opt/layerx/relay_archive/peers.py
ENV LAYERX_RELAY_ARCHIVE_RUNTIME=/opt/layerx/relay_archive/runtime.py \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1
VOLUME ["/var/lib/layerx/relay-archive"]
EXPOSE 9443
USER 4027:4027
ENTRYPOINT ["/usr/local/bin/layerxd"]
CMD ["--relay-archive", "/etc/layerx/relay-archive.json"]
