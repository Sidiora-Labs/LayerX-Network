# hpx-registry — tiny stdlib-only Go service. Artifacts + registry data are
# bind-mounted at runtime (not baked), so re-publishing is just a file copy.
FROM golang:1.22-alpine AS build
ARG SOURCE_REVISION=unknown
WORKDIR /src
COPY go.mod ./
COPY main.go ./
RUN CGO_ENABLED=0 go build -trimpath -ldflags "-s -w -X main.sourceRevision=${SOURCE_REVISION}" -o /hpx-registry .

FROM alpine:3.20
ARG SOURCE_REVISION=unknown
LABEL org.opencontainers.image.source="https://github.com/Sidiora-Labs/LayerX-Protocol" \
      org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.title="HyperPax HPX Registry"
RUN adduser -D -u 10001 hpx
COPY --from=build /hpx-registry /usr/local/bin/hpx-registry
USER hpx
EXPOSE 8099
ENTRYPOINT ["/usr/local/bin/hpx-registry"]
