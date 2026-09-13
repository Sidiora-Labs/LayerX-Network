#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
cd "$root"
target="$root/.lane-target/receipt-fixtures"
mkdir -p "$target"
"${CC:-gcc}" -std=c17 -pedantic -Werror -Wall -Wextra -Wconversion -Wshadow -Wvla -O2 -Iinclude \
  tests/fixtures/asset/generate_supply_receipts.c src/protocol/lxp_receipt.c \
  src/codec/lxp_codec.c src/protocol/lxp_arena.c src/protocol/lxp_protocol.c \
  src/protocol/lxp_result.c src/protocol/lxp_u128.c src/crypto/lxp_hash.c \
  src/crypto/lxp_ct.c src/crypto/lxp_ed25519.c src/crypto/lxp_merkle.c \
  -lcrypto -o "$target/supply-receipts"
"$target/supply-receipts" tests/fixtures/receipt-supply-v2.bin tests/fixtures/asset
