#!/usr/bin/env sh

# Input parameters
NODE_ID=${ID:-0}
ARCH=$(uname -m)
MOCK_BALANCES=${MOCK_BALANCES:-false}

# Build paxd
echo "Building paxd from local branch"
git config --global --add safe.directory /pax-protocol/pax-chain
export LEDGER_ENABLED=false
make clean
# build paxd with the mock balance function enabled
if [ "$MOCK_BALANCES" = true ]; then
    echo "Building with mock balances enabled..."
    make build-linux BUILD_TAGS="mock_balances"
else
    echo "Building with standard configuration..."
    make build-linux
fi
mkdir -p build/generated
echo "DONE" > build/generated/build.complete
