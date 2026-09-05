FROM rust:1.91.1-bookworm@sha256:c1e5f19e773b7878c3f7a805dd00a495e747acbdc76fb2337a4ebf0418896b33 AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential libssl-dev libsqlite3-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY . .
ARG LXP_REVISION=unknown
RUN make LXP_REVISION="$LXP_REVISION" layerxd layerx-genesis-build layerxctl \
    && test -x build/bin/layerxd && test -x build/bin/layerx-genesis-build && test -x build/bin/layerxctl

FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 libsqlite3-0 openssl socat bash coreutils \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -g 4020 layerx && useradd -M -u 4020 -g 4020 -s /usr/sbin/nologin layerx \
    && mkdir -p /opt/layerx/migrations /var/lib/layerx /run/layerx \
    && chown 4020:4020 /var/lib/layerx /run/layerx
COPY --from=build /src/build/bin/layerxd /usr/local/bin/layerxd
COPY --from=build /src/build/bin/layerx-genesis-build /usr/local/bin/layerx-genesis-build
COPY --from=build /src/build/bin/layerxctl /usr/local/bin/layerxctl
COPY --from=build /src/migrations/0007_history_index.sql /opt/layerx/migrations/0007_history_index.sql
COPY --from=build /src/platform/hosted/node/bootstrap.sh /opt/layerx/bootstrap.sh
COPY --from=build /src/platform/hosted/node/supervisor.sh /opt/layerx/supervisor.sh
RUN chmod 0755 /opt/layerx/bootstrap.sh /opt/layerx/supervisor.sh
USER 4020:4020
ENTRYPOINT ["/opt/layerx/supervisor.sh"]
