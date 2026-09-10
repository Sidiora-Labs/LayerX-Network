#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../../../.."
export CARGO_BUILD_JOBS=16 MAKEFLAGS=-j16
mkdir -p qual-logs/pay2
make -j16
make -j16 programs/target/debug/liblayerx_programs_sandbox.a
cc -std=c17 -Wall -Wextra -Werror -Iinclude \
  agent/crates/layerx-crypto/tests/native_payment_vectors.c \
  build/liblayerx.a programs/target/debug/liblayerx_programs_sandbox.a \
  build/liblayerx.a -lcrypto -pthread -ldl -lm \
  -o qual-logs/pay2/native-payment-vectors
qual-logs/pay2/native-payment-vectors agent/crates/layerx-crypto/tests/fixtures/payments
