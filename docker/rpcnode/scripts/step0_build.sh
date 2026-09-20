#!/usr/bin/env sh

# Input parameters
ARCH=$(uname -m)

# Build paxd
echo "Building paxd from local branch"
git config --global --add safe.directory /pax-protocol/pax-chain
LEDGER_ENABLED=false
make install
mkdir -p build/generated
echo "DONE" > build/generated/build.complete
