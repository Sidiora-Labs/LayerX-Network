#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
export CARGO_BUILD_JOBS=16 MAKEFLAGS=-j16
baseline=97444fe2a9849672715ca1dc559dcd19ccac7a8e
work=qual-logs/nat3/sole-writer
mkdir -p "$work"
git show "$baseline:src/protocol/lxp_module_ctx.c" > "$work/lxp_module_ctx.c"
cat > "$work/baseline.mk" <<'MAKE'
qual-logs/nat3/sole-writer/module_ctx.o: qual-logs/nat3/sole-writer/lxp_module_ctx.c
	$(CC) $(CPPFLAGS) $(CFLAGS) -DLXP_TESTING -c $< -o $@
qual-logs/nat3/sole-writer/test-credit-before: tests/bridge/test_credit.c qual-logs/nat3/sole-writer/module_ctx.o $(TEST_LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	$(CC) $(CPPFLAGS) $(CFLAGS) -DLXP_TESTING $< qual-logs/nat3/sole-writer/module_ctx.o $(TEST_LIBRARY) $(PROGRAMS_RUNTIME_LIB) $(TEST_LIBRARY) $(EXTRA_LDFLAGS) -lcrypto -pthread -ldl -lm -o $@
MAKE
make -f Makefile -f "$work/baseline.mk" "$work/test-credit-before" \
    build/tests/bridge/sign-credit build/tests/bridge/test-credit build/bin/layerx-genesis-build
"${BRIDGE_PYTHON:-python3}" tests/bridge/qualify_credit.py \
    --compare-baseline "$work/test-credit-before"
