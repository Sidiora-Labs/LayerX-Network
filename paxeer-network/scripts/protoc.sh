#!/bin/bash

set -euo pipefail

echo "Generating protobuf code..."

rm -rf ./build/proto

# We have to build regen-network protoc-gen-gocosmos from source because
# the module uses replace directive, which makes it impossible to use
# go install like a healthy human being.
#
# As a workaround, we download the source code to a temporary location
# and build the binary. buf.gen.yaml then implicitly uses the path to the
# built binary. This is ugly but it works, and results in the least amount
# of changes across the repo to have _a_ working solution without accidentally
# breaking anything else or introduce too much change as part of automating
# the proto generation.
go get github.com/regen-network/cosmos-proto/protoc-gen-gocosmos@v0.3.1
mkdir -p ./build/proto/gocosmos
build_out="${PWD}/build/proto/gocosmos"
pushd "$(go env GOMODCACHE)/github.com/regen-network/cosmos-proto@v0.3.1" &&
  go build -o "${build_out}/protoc-gen-gocosmos" ./protoc-gen-gocosmos &&
  popd

go run github.com/bufbuild/buf/cmd/buf@v1.58.0 generate
go run github.com/bufbuild/buf/cmd/buf@v1.58.0 generate --template consensus/internal/buf.gen.yaml
go run github.com/bufbuild/buf/cmd/buf@v1.58.0 generate --template consensus/internal/wireguard.buf.gen.yaml

# We can't manipulate the outputs enough to eliminate the extra move-abouts.
# So we just copy the files we want to the right places manually.
# The repo restructure should help this in the future.
cp -rf ./build/proto/gocosmos/github.com/sidiora-labs/paxeer-network/* ./
cp -rf ./build/proto/gocosmos/github.com/sidiora-labs/paxeer-network/sdk/* ./sdk
cp -rf ./build/proto/gocosmos/github.com/sidiora-labs/paxeer-network/wasm/* ./wasm

# Use gogofaster for Tendermint and IAVL because that is their established generator.
# See ./consensus/internal/buf.gen.yaml.
cp -rf ./build/proto/gogofaster/github.com/sidiora-labs/paxeer-network/consensus/* ./consensus

rm -rf ./build/proto

echo "Protobuf code generation complete."
