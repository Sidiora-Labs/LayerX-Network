ARG PAXCTL_VERSION=v0.0.5-pax.1

FROM docker.io/golang:1.25.6-bookworm@sha256:2f768d462dbffbb0f0b3a5171009f162945b086f326e0b2a8fd5d29c3219ff14 AS paxctl
ARG PAXCTL_VERSION
RUN GOBIN=/usr/bin go install github.com/Paxeer-Network/paxctl@${PAXCTL_VERSION}

FROM docker.io/golang:1.25.6-bookworm@sha256:2f768d462dbffbb0f0b3a5171009f162945b086f326e0b2a8fd5d29c3219ff14 AS builder
WORKDIR /go/src/pax-chain

COPY wasm/x/wasm/artifacts/v152/api/*.so /tmp/wasmd-libs/
COPY wasm/x/wasm/artifacts/v155/api/*.so /tmp/wasmd-libs/
COPY wasm-runtime/internal/api/*.so /tmp/wasmvm-libs/
ARG TARGETARCH
RUN mkdir -p /go/lib && \
    case "${TARGETARCH}" in \
      amd64) ARCH_SUFFIX="x86_64" ;; \
      arm64) ARCH_SUFFIX="aarch64" ;; \
      *) echo "Unsupported architecture: ${TARGETARCH}" && exit 1 ;; \
    esac && \
    cp /tmp/wasmd-libs/libwasmvm152.${ARCH_SUFFIX}.so /go/lib/ && \
    cp /tmp/wasmd-libs/libwasmvm155.${ARCH_SUFFIX}.so /go/lib/ && \
    cp /tmp/wasmvm-libs/libwasmvm.${ARCH_SUFFIX}.so /go/lib/

COPY go.* ./
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    go mod download

COPY . .
ENV CGO_ENABLED=1
ARG PAX_CHAIN_REF=""
ARG GO_BUILD_TAGS=""
ARG GO_BUILD_ARGS=""
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    BUILD_TAGS="netgo ledger ${GO_BUILD_TAGS}" && \
    VERSION_PKG="github.com/sidiora-labs/paxeer-network/sdk/version" && \
    PAX_CHAIN_VERSION=$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' version.json) && \
    LDFLAGS="\
      -X ${VERSION_PKG}.Name=pax \
      -X ${VERSION_PKG}.AppName=paxd \
      -X ${VERSION_PKG}.Version=${PAX_CHAIN_VERSION} \
      -X ${VERSION_PKG}.Commit=${PAX_CHAIN_REF:-unknown} \
      -X '${VERSION_PKG}.BuildTags=${BUILD_TAGS}'" && \
    go build -tags "${BUILD_TAGS}" -ldflags "${LDFLAGS}" ${GO_BUILD_ARGS} -o /go/bin/paxd ./daemon/paxd

FROM docker.io/ubuntu:24.04@sha256:104ae83764a5119017b8e8d6218fa0832b09df65aae7d5a6de29a85d813da2fb

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /go/bin/paxd /usr/bin/
COPY --from=paxctl /usr/bin/paxctl /usr/bin/
COPY --from=builder /go/lib/*.so /usr/lib/

ENTRYPOINT ["/usr/bin/paxd"]
