#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)

if ! command -v protoc >/dev/null 2>&1; then
    sudo apt-get update
    sudo apt-get install --yes protobuf-compiler
fi

if command -v go >/dev/null 2>&1; then
    go_bin=$(go env GOBIN)
    if [ -z "$go_bin" ]; then
        go_bin=$(go env GOPATH)/bin
    fi
    PATH="$go_bin:$PATH"
    export PATH
fi

if ! command -v protoc-gen-openapiv2 >/dev/null 2>&1; then
    if ! command -v go >/dev/null 2>&1; then
        echo "go is required to install protoc-gen-openapiv2" >&2
        exit 1
    fi

    go install github.com/grpc-ecosystem/grpc-gateway/v2/protoc-gen-openapiv2@v2.18.1
fi

for service in smart-contract-verifier sig-provider; do
    cd "$root/explorer/services/$service"
    cargo fmt --all --check
    cargo clippy --all-targets --locked -- -D warnings
done
