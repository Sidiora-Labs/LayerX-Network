SHELL := /bin/sh

CC ?= cc
AR ?= ar
BUILD_DIR ?= build
OPT_LEVEL ?= -O2
LXP_REVISION ?= unknown
EXTRA_CFLAGS ?=
EXTRA_LDFLAGS ?=
RUN_PREFIX ?=
QUALIFICATION_CORPUS ?= build/qualification/replay/replay-10m.lxq
FUZZ_QUAL_ITERATIONS ?= 100000
AGENT_CARGO ?= cargo
AGENT_MANIFEST := agent/Cargo.toml
AGENT_FUZZ_TOOLCHAIN ?= nightly-2025-11-10
HUMAN_CARGO ?= cargo
HUMAN_MANIFEST := human/Cargo.toml
HUMAN_WEB_DIR := human/apps/web
HUMAN_NPM ?= npm --prefix $(HUMAN_WEB_DIR)
INTEROP_CARGO ?= cargo
INTEROP_MANIFEST := interop/Cargo.toml
PAXEER_DIR := $(CURDIR)/paxeer-network
PAXEER_MAKE := $(MAKE) -C $(PAXEER_DIR)
HPX_ORIGIN ?= https://node.hyperpaxeer.com

CPPFLAGS := -Iinclude \
	-DLXP_BUILD_TARGET_TRIPLE=\"$(shell $(CC) -dumpmachine)\" \
	-DLXP_BUILD_OPTIMISATION=\"$(OPT_LEVEL)\" \
	-DLXP_BUILD_REVISION=\"$(LXP_REVISION)\"
CFLAGS := -std=c17 -pedantic -Werror -Wall -Wextra -Wconversion -Wshadow -Wvla \
	-fno-strict-aliasing -ffp-contract=off $(OPT_LEVEL) $(EXTRA_CFLAGS)

LIB_SOURCES := $(filter-out src/storage/lxp_projection.c,$(shell find src -type f -name '*.c' -print | LC_ALL=C sort))
LIB_OBJECTS := $(patsubst %.c,$(BUILD_DIR)/obj/%.o,$(LIB_SOURCES))
LIBRARY := $(BUILD_DIR)/liblayerx.a
TEST_LIB_OBJECTS := $(patsubst %.c,$(BUILD_DIR)/test-obj/%.o,$(LIB_SOURCES))
TEST_LIBRARY := $(BUILD_DIR)/liblayerx-testing.a

.PHONY: all build clean reproducible layerxd test test-harness list-tests \
	test-result test-protocol test-arena test-sanitizer-smoke \
	test-sanitizer-suite test-codec test-codec-limits test-codec-version \
	test-codec-vectors fuzz-codec-smoke test-crypto-hash test-crypto-ed25519 \
	test-crypto-secp256k1 test-merkle test-merkle-proof test-crypto-ct \
	test-crypto-suite test-crypto-sanitizers \
	test-arith-u128 test-arith-u256 test-arith-rounding test-arith-property \
	test-arith-nofloat \
	test-log test-log-durability test-recovery test-projection test-rebuild \
	test-journal \
	test-activity-codec test-envelope test-verify-pool test-admission \
	test-idempotency \
	test-fee-gate \
	test-identity \
	test-grants \
	test-authority-resolve \
	test-allowance \
	test-revocation \
	test-rotation \
	test-kernel \
	test-module-ctx \
	test-dispatch \
	test-receipts \
	test-state-root \
	test-replay-golden test-replay-golden-local \
	test-ledger-accounts test-ledger-transfer test-ledger-set test-ledger-send \
	test-ledger-receive \
	test-ledger-receipt \
	test-asset-registry \
	test-asset-balance \
	test-asset-transfer \
	test-asset-deposit \
	test-asset-withdraw \
	test-asset-reserve \
	test-escrow-open \
	test-escrow-capture \
	test-escrow-timeout \
	test-escrow-dispute \
	test-escrow-invariants \
	test-budget-create \
	test-budget-period \
	test-budget-spend \
	test-budget-delegate \
	test-budget-revoke \
	test-stream-open \
	test-stream-accrual \
	test-stream-meter \
	test-stream-settle \
	test-stream-lifecycle \
	test-service-offer \
	test-service-commit \
	test-service-attest \
	test-service-deliver \
	test-service-acceptance \
	test-service-dispute \
	test-oracle-adapter \
	test-oracle-intake \
	test-oracle-bounds \
	test-oracle-root \
	test-oracle-failclosed \
	test-perps-market \
	test-perps-book \
	test-perps-margin \
	test-perps-funding \
	test-perps-liquidation \
	test-perps-insurance \
	test-wave-9 \
	test-batch \
	test-sequencer \
	test-batch-time \
	test-batch-seal \
	test-batch-distribute \
	test-sequencer-recovery \
	test-wave-10 \
	test-replica \
	test-replay \
	test-replica-divergence \
	test-snapshot \
	test-history \
	test-replay-crossarch \
	test-wave-11 \
	test-guarantor \
	test-guarantor-cert \
	test-guarantor-bond \
	test-equivocation \
	test-guarantor-disagreement \
	test-da \
	test-da-possession \
	test-da-retrieval \
	test-da-challenge \
	test-da-unavailable \
	test-governance \
	test-governance-activation \
	test-governance-emergency \
	test-fees \
	test-metering \
	test-fee-replay \
	test-paxeer \
	test-paxeer-bond \
	test-bridge-deposit \
	test-bridge-withdraw \
	test-emergency-exit \
	test-reserve \
	test-gateway \
	test-gateway-send \
	test-gateway-receive \
	test-receipt-offline \
	test-layerxd \
	test-tools \
	test-genesis \
	test-genesis-import \
	test-genesis-reconcile \
	test-legacy-readonly \
	test-shadow \
	test-contract-state-surface test-contracts \
	qualify-replay qualify-faults qualify-fuzz qualify-fuzz-run qualify-arith \
	test-wave-12 \
	test-wave-8 \
	scan-consensus public-audit ci \
	agent-build agent-test agent-lint agent-fuzz agent-fuzz-all \
	agent-fuzz-wire agent-fuzz-interface agent-fuzz-long agent-fuzz-minimize \
	agent-check agent-qualify-fuzz \
	agent-test-errors agent-check-boundary agent-test-sanitize \
	agent-test-types-ids mirror-live mirror-verify-live

all: build

mirror-live:
	$(INTEROP_CARGO) build --locked --release --manifest-path $(INTEROP_MANIFEST) \
		--package layerx-mirror --bin layerx-mirror-publisher
	LAYERX_MIRROR_PUBLISHER_BIN=interop/target/release/layerx-mirror-publisher \
		LAYERX_MIRROR_FAULT_CONTROLLER="$${LAYERX_MIRROR_FAULT_CONTROLLER:?set the authenticated devnet fault controller}" \
		./scripts/qualify-mirror-live.sh

mirror-verify-live:
	$(INTEROP_CARGO) build --locked --release --manifest-path $(INTEROP_MANIFEST) \
		--package layerx-mirror --bin layerx-mirror-verify
	LAYERX_MIRROR_VERIFY_BIN=interop/target/release/layerx-mirror-verify \
		./scripts/qualify-mirror-verification-live.sh

build: $(LIBRARY)

$(LIBRARY): $(LIB_OBJECTS)
	@mkdir -p $(@D)
	$(AR) rcsD $@ $(LIB_OBJECTS)

$(BUILD_DIR)/obj/%.o: %.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) -MMD -MP -c $< -o $@

$(TEST_LIBRARY): $(TEST_LIB_OBJECTS)
	@mkdir -p $(@D)
	$(AR) rcsD $@ $(TEST_LIB_OBJECTS)

$(BUILD_DIR)/test-obj/%.o: %.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) -DLXP_TESTING $(CFLAGS) -MMD -MP -c $< -o $@

clean:
	rm -rf -- build

reproducible:
	rm -rf -- build-repro-a build-repro-b
	$(MAKE) --no-print-directory BUILD_DIR=build-repro-a build
	$(MAKE) --no-print-directory BUILD_DIR=build-repro-b build
	cmp build-repro-a/liblayerx.a build-repro-b/liblayerx.a
	rm -rf -- build-repro-a build-repro-b

$(BUILD_DIR)/tests/lxp_test_result: tests/protocol/lxp_test_result.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-result: $(BUILD_DIR)/tests/lxp_test_result
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_result

$(BUILD_DIR)/tests/lxp_test_protocol: tests/protocol/lxp_test_protocol.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-protocol: $(BUILD_DIR)/tests/lxp_test_protocol
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_protocol

$(BUILD_DIR)/tests/lxp_test_arena: tests/protocol/lxp_test_arena.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

$(BUILD_DIR)/tests/lxp_test_arena_asan: tests/protocol/lxp_test_arena.c \
		src/protocol/lxp_arena.c src/protocol/lxp_result.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) -O1 -g -fsanitize=address \
		-fno-omit-frame-pointer $^ -o $@

test-arena: $(BUILD_DIR)/tests/lxp_test_arena \
		$(BUILD_DIR)/tests/lxp_test_arena_asan
	$(BUILD_DIR)/tests/lxp_test_arena
	ASAN_OPTIONS=abort_on_error=1:detect_leaks=1:poison_heap=1 \
		$(BUILD_DIR)/tests/lxp_test_arena_asan

$(BUILD_DIR)/tests/lxp_test_harness: tests/lxp_test_harness.c \
		tests/protocol/lxp_test_smoke.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) -Itests $(CFLAGS) tests/lxp_test_harness.c \
		tests/protocol/lxp_test_smoke.c $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-harness: $(BUILD_DIR)/tests/lxp_test_harness
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_harness

list-tests: $(BUILD_DIR)/tests/lxp_test_harness
	$(BUILD_DIR)/tests/lxp_test_harness --list

test: test-result test-protocol test-arena test-harness test-codec \
	test-codec-limits test-codec-version test-codec-vectors fuzz-codec-smoke \
	test-crypto-suite test-arith-u128 test-arith-u256 test-arith-rounding \
	test-arith-property test-arith-nofloat test-log test-log-durability \
	test-recovery test-projection test-rebuild test-journal \
	test-activity-codec test-envelope test-verify-pool test-admission \
	test-idempotency test-fee-gate test-identity test-grants \
	test-authority-resolve test-allowance test-revocation test-rotation

$(BUILD_DIR)/tests/lxp_test_kernel: tests/protocol/lxp_test_kernel.c \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(PROGRAMS_RUNTIME_LIB) \
		$(LIBRARY) $(EXTRA_LDFLAGS) -lcrypto -pthread -ldl -lm -o $@

test-kernel: $(BUILD_DIR)/tests/lxp_test_kernel
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_kernel

$(BUILD_DIR)/tests/lxp_test_module_ctx: \
		tests/protocol/lxp_test_module_ctx.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-module-ctx: $(BUILD_DIR)/tests/lxp_test_module_ctx
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_module_ctx
	tools/ci/symbol-allowlist.sh "$(BUILD_DIR)"

$(BUILD_DIR)/tests/lxp_test_dispatch: tests/protocol/lxp_test_dispatch.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-dispatch: $(BUILD_DIR)/tests/lxp_test_dispatch
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_dispatch

$(BUILD_DIR)/tests/lxp_test_receipts: tests/protocol/lxp_test_receipts.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-receipts: $(BUILD_DIR)/tests/lxp_test_receipts
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_receipts

$(BUILD_DIR)/tests/lxp_test_state_root: tests/state/lxp_test_state_root.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-state-root: $(BUILD_DIR)/tests/lxp_test_state_root
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_state_root

$(BUILD_DIR)/tests/lxp_test_golden_replay: \
		tests/replay/lxp_test_golden_replay.c $(LIBRARY) \
		tests/replay/golden/history.lxl
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-replay-golden-local: $(BUILD_DIR)/tests/lxp_test_golden_replay
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_golden_replay
	tools/ci/symbol-allowlist.sh "$(BUILD_DIR)"

test-replay-golden:
	tools/ci/replay-matrix.sh

$(BUILD_DIR)/tests/test_account_id: tests/ledger/test_account_id.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -pthread -o $@

test-ledger-accounts: $(BUILD_DIR)/tests/test_account_id
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_account_id

$(BUILD_DIR)/tests/test_apply_transfer: tests/ledger/test_apply_transfer.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-ledger-transfer: $(BUILD_DIR)/tests/test_apply_transfer
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_apply_transfer

$(BUILD_DIR)/tests/test_transfer_set: tests/ledger/test_transfer_set.c \
		$(TEST_LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) -DLXP_TESTING $(CFLAGS) $< $(TEST_LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-ledger-set: $(BUILD_DIR)/tests/test_transfer_set
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_transfer_set

$(BUILD_DIR)/tests/test_send: tests/ledger/test_send.c fuzz/fuzz_lxp_send.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/ledger/test_send.c fuzz/fuzz_lxp_send.c \
		$(LIBRARY) $(EXTRA_LDFLAGS) -lcrypto -pthread -o $@

test-ledger-send: $(BUILD_DIR)/tests/test_send
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_send

$(BUILD_DIR)/tests/test_receive: tests/ledger/test_receive.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-ledger-receive: $(BUILD_DIR)/tests/test_receive
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_receive

$(BUILD_DIR)/tests/test_receipt: tests/ledger/test_receipt.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-ledger-receipt: $(BUILD_DIR)/tests/test_receipt
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_receipt
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_asset_registry: tests/modules/test_asset_registry.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-asset-registry: $(BUILD_DIR)/tests/test_asset_registry
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_asset_registry

$(BUILD_DIR)/tests/test_asset_state: tests/modules/test_asset_state.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-asset-balance: $(BUILD_DIR)/tests/test_asset_state
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_asset_state

$(BUILD_DIR)/tests/test_asset_transfer: tests/modules/test_asset_transfer.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-asset-transfer: $(BUILD_DIR)/tests/test_asset_transfer
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_asset_transfer
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_asset_deposit: tests/modules/test_asset_deposit.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-asset-deposit: $(BUILD_DIR)/tests/test_asset_deposit
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_asset_deposit

$(BUILD_DIR)/tests/test_asset_withdraw: tests/modules/test_asset_withdraw.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-asset-withdraw: $(BUILD_DIR)/tests/test_asset_withdraw
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_asset_withdraw

$(BUILD_DIR)/tests/test_asset_reserve: tests/modules/test_asset_reserve.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-asset-reserve: $(BUILD_DIR)/tests/test_asset_reserve
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_asset_reserve

$(BUILD_DIR)/tests/test_escrow_open: tests/modules/test_escrow_open.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-escrow-open: $(BUILD_DIR)/tests/test_escrow_open
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_escrow_open
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_escrow_capture: tests/modules/test_escrow_capture.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-escrow-capture: $(BUILD_DIR)/tests/test_escrow_capture
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_escrow_capture
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_escrow_timeout: tests/modules/test_escrow_timeout.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-escrow-timeout: $(BUILD_DIR)/tests/test_escrow_timeout
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_escrow_timeout
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_escrow_dispute: tests/modules/test_escrow_dispute.c \
		$(TEST_LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) -DLXP_TESTING $(CFLAGS) $< $(TEST_LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-escrow-dispute: $(BUILD_DIR)/tests/test_escrow_dispute
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_escrow_dispute
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_escrow_invariants: \
		tests/modules/test_escrow_invariants.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-escrow-invariants: $(BUILD_DIR)/tests/test_escrow_invariants
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_escrow_invariants
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_budget_create: tests/modules/test_budget_create.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-budget-create: $(BUILD_DIR)/tests/test_budget_create
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_budget_create
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_budget_period: tests/modules/test_budget_period.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-budget-period: $(BUILD_DIR)/tests/test_budget_period
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_budget_period

$(BUILD_DIR)/tests/test_budget_spend: tests/modules/test_budget_spend.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-budget-spend: $(BUILD_DIR)/tests/test_budget_spend
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_budget_spend
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_budget_delegate: tests/modules/test_budget_delegate.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-budget-delegate: $(BUILD_DIR)/tests/test_budget_delegate
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_budget_delegate
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_budget_close: tests/modules/test_budget_close.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-budget-revoke: $(BUILD_DIR)/tests/test_budget_close
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_budget_close
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_stream_open: tests/modules/test_stream_open.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-stream-open: $(BUILD_DIR)/tests/test_stream_open
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_stream_open
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_stream_accrue: tests/modules/test_stream_accrue.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-stream-accrual: $(BUILD_DIR)/tests/test_stream_accrue
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_stream_accrue

$(BUILD_DIR)/tests/test_stream_meter: tests/modules/test_stream_meter.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-stream-meter: $(BUILD_DIR)/tests/test_stream_meter
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_stream_meter

$(BUILD_DIR)/tests/test_stream_settle: tests/modules/test_stream_settle.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-stream-settle: $(BUILD_DIR)/tests/test_stream_settle
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_stream_settle
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_stream_lifecycle: \
		tests/modules/test_stream_lifecycle.c $(TEST_LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) -DLXP_TESTING $(CFLAGS) $< $(TEST_LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-stream-lifecycle: $(BUILD_DIR)/tests/test_stream_lifecycle
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_stream_lifecycle
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_service_offer: tests/modules/test_service_offer.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-service-offer: $(BUILD_DIR)/tests/test_service_offer
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_service_offer
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_service_commit: tests/modules/test_service_commit.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-service-commit: $(BUILD_DIR)/tests/test_service_commit
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_service_commit
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_service_attest: tests/modules/test_service_attest.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-service-attest: $(BUILD_DIR)/tests/test_service_attest
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_service_attest
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_service_deliver: tests/modules/test_service_deliver.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-service-deliver: $(BUILD_DIR)/tests/test_service_deliver
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_service_deliver
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_service_acceptance: \
		tests/modules/test_service_acceptance.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-service-acceptance: $(BUILD_DIR)/tests/test_service_acceptance
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_service_acceptance
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_service_dispute: \
		tests/modules/test_service_dispute.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-service-dispute: $(BUILD_DIR)/tests/test_service_dispute
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_service_dispute
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_oracle_adapter: tests/network/test_oracle_adapter.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

$(BUILD_DIR)/tests/test_oracle_replay_absent: \
		tests/network/test_oracle_replay_absent.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-oracle-adapter: $(BUILD_DIR)/tests/test_oracle_adapter \
		$(BUILD_DIR)/tests/test_oracle_replay_absent
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_oracle_adapter
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_oracle_replay_absent
	! nm $(BUILD_DIR)/tests/test_oracle_replay_absent | \
		grep -q lx_oracle_adapter_run
	sh tools/lx_oracle_adapter_isolation.sh

$(BUILD_DIR)/tests/test_oracle_intake: tests/modules/test_oracle_intake.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-oracle-intake: $(BUILD_DIR)/tests/test_oracle_intake
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_oracle_intake
	tools/lxp_check_sole_writer.sh
	sh tools/lx_oracle_adapter_isolation.sh

$(BUILD_DIR)/tests/test_oracle_checks: tests/modules/test_oracle_checks.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-oracle-bounds: $(BUILD_DIR)/tests/test_oracle_checks
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_oracle_checks
	sh tools/lx_oracle_adapter_isolation.sh

$(BUILD_DIR)/tests/test_oracle_root: tests/sequencer/test_oracle_root.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-oracle-root: $(BUILD_DIR)/tests/test_oracle_root
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_oracle_root

$(BUILD_DIR)/tests/test_oracle_halt: tests/modules/test_oracle_halt.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-oracle-failclosed: $(BUILD_DIR)/tests/test_oracle_halt
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_oracle_halt
	sh tools/lx_oracle_adapter_isolation.sh

$(BUILD_DIR)/tests/test_perps_market: tests/modules/test_perps_market.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-perps-market: $(BUILD_DIR)/tests/test_perps_market
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_perps_market
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_perps_book: tests/modules/test_perps_book.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-perps-book: $(BUILD_DIR)/tests/test_perps_book
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_perps_book
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_perps_position: tests/modules/test_perps_position.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-perps-margin: $(BUILD_DIR)/tests/test_perps_position
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_perps_position
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_perps_funding: tests/modules/test_perps_funding.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-perps-funding: $(BUILD_DIR)/tests/test_perps_funding
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_perps_funding
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_perps_liquidate: \
		tests/modules/test_perps_liquidate.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-perps-liquidation: $(BUILD_DIR)/tests/test_perps_liquidate
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_perps_liquidate
	tools/lxp_check_sole_writer.sh

$(BUILD_DIR)/tests/test_perps_insurance: \
		tests/modules/test_perps_insurance.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-perps-insurance: $(BUILD_DIR)/tests/test_perps_insurance
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_perps_insurance
	tools/lxp_check_sole_writer.sh

test-wave-9: test-perps-market test-perps-book test-perps-margin \
		test-perps-funding test-perps-liquidation test-perps-insurance

$(BUILD_DIR)/tests/test_batch_header: tests/test_batch_header.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-batch: $(BUILD_DIR)/tests/test_batch_header
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_batch_header

$(BUILD_DIR)/tests/test_sequencer_sequence: \
		tests/test_sequencer_sequence.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-sequencer: $(BUILD_DIR)/tests/test_sequencer_sequence
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_sequencer_sequence

$(BUILD_DIR)/tests/test_batch_time: tests/test_batch_time.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-batch-time: $(BUILD_DIR)/tests/test_batch_time
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_batch_time
	sh tools/lxp_check_no_clock.sh

$(BUILD_DIR)/tests/test_batch_seal: tests/test_batch_seal.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-batch-seal: $(BUILD_DIR)/tests/test_batch_seal
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_batch_seal

$(BUILD_DIR)/tests/test_batch_publish: tests/test_batch_publish.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-batch-distribute: $(BUILD_DIR)/tests/test_batch_publish
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_batch_publish

$(BUILD_DIR)/tests/test_sequencer_recovery: \
		tests/test_sequencer_recovery.c src/storage/lxp_projection.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/test_sequencer_recovery.c \
		src/storage/lxp_projection.c $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -lsqlite3 -pthread -o $@

test-sequencer-recovery: $(BUILD_DIR)/tests/test_sequencer_recovery
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_sequencer_recovery

test-wave-10: test-batch test-sequencer test-batch-time test-batch-seal \
		test-batch-distribute test-sequencer-recovery

$(BUILD_DIR)/tests/test_replica_ingest: tests/test_replica_ingest.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-replica: $(BUILD_DIR)/tests/test_replica_ingest
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_replica_ingest

$(BUILD_DIR)/tests/test_replica_replay: tests/test_replica_replay.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-replay: $(BUILD_DIR)/tests/test_replica_replay
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_replica_replay
	! nm -u $(BUILD_DIR)/obj/src/replica/lxp_replay.o | grep -q sqlite3_

$(BUILD_DIR)/tests/test_replica_divergence: \
		tests/test_replica_divergence.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-replica-divergence: $(BUILD_DIR)/tests/test_replica_divergence
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_replica_divergence

$(BUILD_DIR)/tests/test_snapshot: tests/test_snapshot.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-snapshot: $(BUILD_DIR)/tests/test_snapshot
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_snapshot

$(BUILD_DIR)/tests/test_history_query: tests/test_history_query.c $(LIBRARY) \
		migrations/0007_history_index.sql
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -lsqlite3 -o $@

test-history: $(BUILD_DIR)/tests/test_history_query
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_history_query

test-replay-crossarch: tests/vectors/replay_corpus.lxb
	sh tools/lxp_replay_matrix.sh

test-wave-11: test-replica test-replay test-replica-divergence \
		test-snapshot test-history test-replay-crossarch

$(BUILD_DIR)/tests/test_guarantor_duties: \
		tests/test_guarantor_duties.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-guarantor: $(BUILD_DIR)/tests/test_guarantor_duties
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_guarantor_duties

$(BUILD_DIR)/tests/test_guarantor_cert: tests/test_guarantor_cert.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-guarantor-cert: $(BUILD_DIR)/tests/test_guarantor_cert
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_guarantor_cert

$(BUILD_DIR)/tests/test_guarantor_bond: tests/test_guarantor_bond.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-guarantor-bond: $(BUILD_DIR)/tests/test_guarantor_bond
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_guarantor_bond

$(BUILD_DIR)/tests/test_equivocation: tests/test_equivocation.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-equivocation: $(BUILD_DIR)/tests/test_equivocation
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_equivocation

$(BUILD_DIR)/tests/test_guarantor_disagreement: \
		tests/test_guarantor_disagreement.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-guarantor-disagreement: $(BUILD_DIR)/tests/test_guarantor_disagreement
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_guarantor_disagreement

$(BUILD_DIR)/tests/test_da_bundle: tests/test_da_bundle.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-da: $(BUILD_DIR)/tests/test_da_bundle
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_da_bundle

$(BUILD_DIR)/tests/test_da_possession: tests/test_da_possession.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-da-possession: $(BUILD_DIR)/tests/test_da_possession
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_da_possession

$(BUILD_DIR)/tests/test_da_retrieval: tests/test_da_retrieval.c \
		cmd/layerx-verify/lxp_verify_fetch.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/test_da_retrieval.c \
		cmd/layerx-verify/lxp_verify_fetch.c $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-da-retrieval: $(BUILD_DIR)/tests/test_da_retrieval
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_da_retrieval

$(BUILD_DIR)/tests/test_da_challenge: tests/test_da_challenge.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-da-challenge: $(BUILD_DIR)/tests/test_da_challenge
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_da_challenge

$(BUILD_DIR)/tests/test_da_unavailable: tests/test_da_unavailable.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-da-unavailable: $(BUILD_DIR)/tests/test_da_unavailable
	tools/lxp_da_withhold.sh all $(BUILD_DIR)/tests/test_da_unavailable

$(BUILD_DIR)/tests/test_governance_params: \
		tests/test_governance_params.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-governance: $(BUILD_DIR)/tests/test_governance_params
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_governance_params

$(BUILD_DIR)/tests/test_governance_activation: \
		tests/test_governance_activation.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-governance-activation: $(BUILD_DIR)/tests/test_governance_activation
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_governance_activation

$(BUILD_DIR)/tests/test_governance_emergency: \
		tests/test_governance_emergency.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-governance-emergency: $(BUILD_DIR)/tests/test_governance_emergency
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_governance_emergency

$(BUILD_DIR)/tests/test_fees: tests/test_fees.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-fees: $(BUILD_DIR)/tests/test_fees
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_fees

$(BUILD_DIR)/tests/test_metering: tests/test_metering.c fuzz/fuzz_meter.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/test_metering.c fuzz/fuzz_meter.c \
		$(LIBRARY) $(EXTRA_LDFLAGS) -lcrypto -o $@

test-metering: $(BUILD_DIR)/tests/test_metering
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_metering

$(BUILD_DIR)/tests/test_fee_replay: tests/test_fee_replay.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-fee-replay: $(BUILD_DIR)/tests/test_fee_replay test-dispatch
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_fee_replay

test-wave-12: test-guarantor test-guarantor-cert test-guarantor-bond \
		test-equivocation test-guarantor-disagreement test-da \
		test-da-possession test-da-retrieval test-da-challenge \
		test-da-unavailable test-governance test-governance-activation \
		test-governance-emergency test-fees test-metering test-fee-replay

$(BUILD_DIR)/tests/test_paxeer_custody: tests/test_paxeer_custody.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

$(BUILD_DIR)/contracts/.paxeer-built: contracts/LayerXCustody.sol \
		contracts/CheckpointRegistry.sol contracts/GuarantorBond.sol
	@mkdir -p $(@D)
	forge build --offline --root . --contracts contracts --out $(BUILD_DIR)/contracts
	@touch $@

test-paxeer: $(BUILD_DIR)/tests/test_paxeer_custody \
		$(BUILD_DIR)/contracts/.paxeer-built
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_paxeer_custody

$(BUILD_DIR)/tests/test_paxeer_bond: tests/test_paxeer_bond.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-paxeer-bond: $(BUILD_DIR)/tests/test_paxeer_bond \
		$(BUILD_DIR)/contracts/.paxeer-built
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_paxeer_bond

$(BUILD_DIR)/tests/test_bridge_deposit: tests/test_bridge_deposit.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-bridge-deposit: $(BUILD_DIR)/tests/test_bridge_deposit test-contracts
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_bridge_deposit

$(BUILD_DIR)/tests/test_bridge_withdraw: tests/test_bridge_withdraw.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-bridge-withdraw: $(BUILD_DIR)/tests/test_bridge_withdraw test-contracts
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_bridge_withdraw

$(BUILD_DIR)/tests/test_emergency_exit: tests/test_emergency_exit.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-emergency-exit: $(BUILD_DIR)/tests/test_emergency_exit test-contracts
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_emergency_exit

$(BUILD_DIR)/tests/test_reserve_reconcile: \
		tests/test_reserve_reconcile.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

$(BUILD_DIR)/tools/lxp-reserve-report: tools/lxp_reserve_report.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< -o $@

test-reserve: $(BUILD_DIR)/tests/test_reserve_reconcile \
		$(BUILD_DIR)/tools/lxp-reserve-report test-contracts
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_reserve_reconcile
	$(BUILD_DIR)/tools/lxp-reserve-report --classes

$(BUILD_DIR)/tests/test_gateway_requirement: \
		tests/test_gateway_requirement.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

$(BUILD_DIR)/obj/fuzz/fuzz_gateway_json.o: fuzz/fuzz_gateway_json.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) -c $< -o $@

test-gateway: $(BUILD_DIR)/tests/test_gateway_requirement \
		$(BUILD_DIR)/obj/fuzz/fuzz_gateway_json.o
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_gateway_requirement

$(BUILD_DIR)/tests/test_gateway_send: tests/test_gateway_send.c $(TEST_LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) -DLXP_TESTING $(CFLAGS) $< $(TEST_LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-gateway-send: $(BUILD_DIR)/tests/test_gateway_send
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_gateway_send

$(BUILD_DIR)/tests/test_gateway_receive: tests/test_gateway_receive.c $(TEST_LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) -DLXP_TESTING $(CFLAGS) $< $(TEST_LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

test-gateway-receive: $(BUILD_DIR)/tests/test_gateway_receive
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_gateway_receive

$(BUILD_DIR)/tests/test_receipt_offline: tests/test_receipt_offline.c \
		cmd/layerx-verify/lxp_verify_receipt.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/test_receipt_offline.c \
		cmd/layerx-verify/lxp_verify_receipt.c $(LIBRARY) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -o $@

test-receipt-offline: $(BUILD_DIR)/tests/test_receipt_offline
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_receipt_offline

$(BUILD_DIR)/tests/explorer_fixture: tests/explorer_fixture.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -o $@

LAYERXD_SOURCES = \
	cmd/layerxd/lxp_daemon_main.c \
	cmd/layerxd/lxp_daemon_config.c \
	cmd/layerxd/lxp_daemon_shutdown.c \
	cmd/layerxd/lxp_daemon_receipt_authority.c \
	cmd/layerxd/lxp_daemon_protocol.c \
	cmd/layerxd/lxp_daemon_listener.c \
	cmd/layerxd/lxp_daemon_lni.c \
	cmd/layerxd/lxp_daemon_batch_wal.c \
	cmd/layerxd/lxp_daemon_process.c \
	cmd/layerxd/lxp_daemon_authority_replica.c \
	cmd/layerxd/lxp_daemon_cli.c

$(BUILD_DIR)/bin/layerxd: cmd/layerxd/main.c $(LAYERXD_SOURCES) $(LIBRARY) \
		$(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) cmd/layerxd/main.c $(LAYERXD_SOURCES) \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -ldl -lm -o $@

layerxd: $(BUILD_DIR)/bin/layerxd

$(BUILD_DIR)/tests/test_layerxd: tests/test_layerxd.c $(LAYERXD_SOURCES) \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/test_layerxd.c $(LAYERXD_SOURCES) \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -ldl -lm -o $@

test-layerxd: $(BUILD_DIR)/tests/test_layerxd
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_layerxd

TOOL_SOURCES = \
	cmd/layerxctl/lxp_ctl_main.c \
	cmd/layerx-verify/lxp_verify_main.c \
	cmd/layerx-verify/lxp_verify_fetch.c \
	cmd/layerx-genesis/lxp_genesis_cli.c

$(BUILD_DIR)/tests/test_tools: tests/test_tools.c $(TOOL_SOURCES) $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/test_tools.c $(TOOL_SOURCES) \
		$(LIBRARY) $(EXTRA_LDFLAGS) -lcrypto -pthread -o $@

test-tools: $(BUILD_DIR)/tests/test_tools
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_tools

$(BUILD_DIR)/tests/test_genesis_manifest: tests/test_genesis_manifest.c \
		cmd/layerx-genesis/lxp_genesis_main.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/test_genesis_manifest.c \
		cmd/layerx-genesis/lxp_genesis_main.c $(LIBRARY) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -o $@

test-genesis: $(BUILD_DIR)/tests/test_genesis_manifest
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_genesis_manifest

$(BUILD_DIR)/tests/test_genesis_import: tests/test_genesis_import.c \
		cmd/layerx-genesis/lxp_import.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/test_genesis_import.c \
		cmd/layerx-genesis/lxp_import.c $(LIBRARY) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -o $@

test-genesis-import: $(BUILD_DIR)/tests/test_genesis_import
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_genesis_import

$(BUILD_DIR)/tests/test_genesis_reconcile: tests/test_genesis_reconcile.c \
		cmd/layerx-genesis/lxp_reconcile.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/test_genesis_reconcile.c \
		cmd/layerx-genesis/lxp_reconcile.c $(LIBRARY) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -o $@

test-genesis-reconcile: $(BUILD_DIR)/tests/test_genesis_reconcile
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_genesis_reconcile

$(BUILD_DIR)/tests/test_legacy_readonly: tests/test_legacy_readonly.c \
		tools/lxp_legacy_reader.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/test_legacy_readonly.c \
		tools/lxp_legacy_reader.c $(LIBRARY) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -o $@

test-legacy-readonly: $(BUILD_DIR)/tests/test_legacy_readonly
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_legacy_readonly

$(BUILD_DIR)/tests/test_shadow_replay: tests/test_shadow_replay.c \
		tools/lxp_shadow_compare.c tools/lxp_legacy_reader.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/test_shadow_replay.c \
		tools/lxp_shadow_compare.c tools/lxp_legacy_reader.c $(LIBRARY) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -o $@

test-shadow: $(BUILD_DIR)/tests/test_shadow_replay
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_shadow_replay

test-contract-state-surface:
	tools/ci/solidity-state-surface.sh

test-contracts: test-contract-state-surface
	forge test --offline --root .

qualify-replay:
	tools/lxp_qual_replay_matrix.sh

$(BUILD_DIR)/tests/test_qual_recovery: tests/test_qual_recovery.c \
		tests/qualification/lxp_qual_faults.c \
		src/storage/lxp_projection.c $(LIBRARY) \
		migrations/0001_projection.sql
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/test_qual_recovery.c \
		tests/qualification/lxp_qual_faults.c \
		src/storage/lxp_projection.c $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -lsqlite3 -pthread -o $@

qualify-faults: $(BUILD_DIR)/tests/test_qual_recovery
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_qual_recovery

$(BUILD_DIR)/tests/test_fuzz_smoke: tests/test_fuzz_smoke.c \
		fuzz/fuzz_activity.c fuzz/fuzz_signature.c \
		fuzz/fuzz_transfer_set.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/test_fuzz_smoke.c \
		fuzz/fuzz_activity.c fuzz/fuzz_signature.c \
		fuzz/fuzz_transfer_set.c $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

qualify-fuzz-run: $(BUILD_DIR)/tests/test_fuzz_smoke
	test -r "$(QUALIFICATION_CORPUS)"
	mkdir -p "$(BUILD_DIR)/qualification/fuzz-corpus"
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_fuzz_smoke \
		"$(QUALIFICATION_CORPUS)" "$(BUILD_DIR)/qualification/fuzz-corpus" \
		"$(FUZZ_QUAL_ITERATIONS)"

qualify-fuzz: qualify-fuzz-run
	ASAN_OPTIONS=abort_on_error=1:detect_leaks=1:strict_string_checks=1 \
	$(MAKE) --no-print-directory BUILD_DIR=build/qualification/fuzz-asan \
		EXTRA_CFLAGS="$(SANITIZER_CFLAGS) -fsanitize=address" \
		EXTRA_LDFLAGS="-fsanitize=address" FUZZ_QUAL_ITERATIONS=25000 \
		qualify-fuzz-run
	UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1 \
	$(MAKE) --no-print-directory BUILD_DIR=build/qualification/fuzz-ubsan \
		EXTRA_CFLAGS="$(SANITIZER_CFLAGS) -fsanitize=undefined" \
		EXTRA_LDFLAGS="-fsanitize=undefined" FUZZ_QUAL_ITERATIONS=25000 \
		qualify-fuzz-run

QUAL_ARITH_SOURCES := tests/test_qual_arith.c \
	tests/qualification/lxp_qual_arith.c \
	src/protocol/lxp_u128.c src/protocol/lxp_u256.c \
	src/protocol/lxp_i128.c src/protocol/lxp_result.c

$(BUILD_DIR)/tests/test_qual_arith: $(QUAL_ARITH_SOURCES)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(QUAL_ARITH_SOURCES) \
		$(EXTRA_LDFLAGS) -o $@

qualify-arith: test-arith-u128 test-arith-u256 test-arith-rounding \
		test-arith-property test-arith-nofloat \
		$(BUILD_DIR)/tests/test_qual_arith
	$(RUN_PREFIX) $(BUILD_DIR)/tests/test_qual_arith
	tools/lxp_arith_proof.sh "$(BUILD_DIR)"

test-wave-8: test-service-offer test-service-commit test-service-attest \
		test-service-deliver test-service-acceptance test-service-dispute \
		test-oracle-adapter test-oracle-intake test-oracle-bounds \
		test-oracle-root test-oracle-failclosed


$(BUILD_DIR)/tests/lxp_test_codec_primitives: \
		tests/codec/lxp_test_codec_primitives.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-codec: $(BUILD_DIR)/tests/lxp_test_codec_primitives
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_codec_primitives

$(BUILD_DIR)/tests/lxp_test_codec_composite: \
		tests/codec/lxp_test_codec_composite.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-codec-limits: $(BUILD_DIR)/tests/lxp_test_codec_composite
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_codec_composite

$(BUILD_DIR)/tests/lxp_test_codec_strict: \
		tests/codec/lxp_test_codec_strict.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-codec-version: $(BUILD_DIR)/tests/lxp_test_codec_strict
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_codec_strict

$(BUILD_DIR)/tests/lxp_test_codec_vectors: \
		tests/codec/lxp_test_codec_vectors.c $(LIBRARY) \
		tests/vectors/codec/valid.lxv tests/vectors/codec/adversarial.lxv
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-codec-vectors: $(BUILD_DIR)/tests/lxp_test_codec_vectors
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_codec_vectors

$(BUILD_DIR)/tests/lxp_fuzz_codec: fuzz/lxp_fuzz_codec.c \
		src/codec/lxp_codec.c src/protocol/lxp_arena.c \
		src/protocol/lxp_protocol.c src/protocol/lxp_result.c \
		src/protocol/lxp_u128.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) -O1 -g -fno-omit-frame-pointer \
		-fsanitize=address,undefined $^ -fsanitize=address,undefined -o $@

fuzz-codec-smoke: $(BUILD_DIR)/tests/lxp_fuzz_codec
	ASAN_OPTIONS=abort_on_error=1:detect_leaks=1 \
	UBSAN_OPTIONS=halt_on_error=1 $(BUILD_DIR)/tests/lxp_fuzz_codec

$(BUILD_DIR)/tests/lxp_test_hash: tests/crypto/lxp_test_hash.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-crypto-hash: $(BUILD_DIR)/tests/lxp_test_hash
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_hash

$(BUILD_DIR)/tests/lxp_test_ed25519: tests/crypto/lxp_test_ed25519.c \
		fuzz/lxp_fuzz_signature.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/crypto/lxp_test_ed25519.c \
		fuzz/lxp_fuzz_signature.c $(LIBRARY) $(EXTRA_LDFLAGS) -lcrypto -o $@

test-crypto-ed25519: $(BUILD_DIR)/tests/lxp_test_ed25519
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_ed25519

$(BUILD_DIR)/tests/lxp_test_secp256k1: tests/crypto/lxp_test_secp256k1.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -lcrypto -o $@

test-crypto-secp256k1: $(BUILD_DIR)/tests/lxp_test_secp256k1
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_secp256k1

$(BUILD_DIR)/tests/lxp_test_merkle: tests/crypto/lxp_test_merkle.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -lcrypto -o $@

test-merkle: $(BUILD_DIR)/tests/lxp_test_merkle
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_merkle

$(BUILD_DIR)/tests/lxp_test_merkle_proof: tests/crypto/lxp_test_merkle_proof.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -lcrypto -o $@

test-merkle-proof: $(BUILD_DIR)/tests/lxp_test_merkle_proof
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_merkle_proof

$(BUILD_DIR)/tests/lxp_test_ct: tests/crypto/lxp_test_ct.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -lcrypto -o $@

test-crypto-suite: test-crypto-hash test-crypto-ed25519 \
		test-crypto-secp256k1 test-merkle test-merkle-proof \
		$(BUILD_DIR)/tests/lxp_test_ct
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_ct

test-crypto-sanitizers:
	ASAN_OPTIONS=abort_on_error=1:detect_leaks=1 \
	$(MAKE) --no-print-directory BUILD_DIR=build/crypto-asan \
		EXTRA_CFLAGS="$(SANITIZER_CFLAGS) -fsanitize=address" \
		EXTRA_LDFLAGS="-fsanitize=address" test-crypto-suite
	UBSAN_OPTIONS=halt_on_error=1 \
	$(MAKE) --no-print-directory BUILD_DIR=build/crypto-ubsan \
		EXTRA_CFLAGS="$(SANITIZER_CFLAGS) -fsanitize=undefined" \
		EXTRA_LDFLAGS="-fsanitize=undefined" test-crypto-suite
	TSAN_OPTIONS=halt_on_error=1 \
	$(MAKE) --no-print-directory BUILD_DIR=build/crypto-tsan \
		EXTRA_CFLAGS="$(SANITIZER_CFLAGS) -fsanitize=thread" \
		EXTRA_LDFLAGS="-fsanitize=thread" \
		RUN_PREFIX="setarch $(shell uname -m) -R" test-crypto-suite

test-crypto-ct: $(BUILD_DIR)/tests/lxp_test_ct
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_ct
	tools/ci/symbol-allowlist.sh "$(BUILD_DIR)"
	$(MAKE) --no-print-directory test-crypto-sanitizers

$(BUILD_DIR)/tests/lxp_test_u128: tests/arith/lxp_test_u128.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

$(BUILD_DIR)/tests/lxp_test_u128_ubsan: tests/arith/lxp_test_u128.c \
		src/protocol/lxp_u128.c src/protocol/lxp_result.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(SANITIZER_CFLAGS) -fsanitize=undefined \
		$^ -fsanitize=undefined -o $@

test-arith-u128: $(BUILD_DIR)/tests/lxp_test_u128 \
		$(BUILD_DIR)/tests/lxp_test_u128_ubsan
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_u128
	UBSAN_OPTIONS=halt_on_error=1 $(BUILD_DIR)/tests/lxp_test_u128_ubsan

$(BUILD_DIR)/tests/lxp_test_u256: tests/arith/lxp_test_u256.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

$(BUILD_DIR)/tests/lxp_test_u256_ubsan: tests/arith/lxp_test_u256.c \
		src/protocol/lxp_u128.c src/protocol/lxp_u256.c src/protocol/lxp_result.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(SANITIZER_CFLAGS) -fsanitize=undefined \
		$^ -fsanitize=undefined -o $@

test-arith-u256: $(BUILD_DIR)/tests/lxp_test_u256 \
		$(BUILD_DIR)/tests/lxp_test_u256_ubsan
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_u256
	UBSAN_OPTIONS=halt_on_error=1 $(BUILD_DIR)/tests/lxp_test_u256_ubsan

$(BUILD_DIR)/tests/lxp_test_rounding: tests/arith/lxp_test_rounding.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-arith-rounding: $(BUILD_DIR)/tests/lxp_test_rounding
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_rounding

ARITH_PROPERTY_SOURCES := tests/arith/lxp_test_arith_property.c \
	tests/arith/lxp_arith_reference.c fuzz/lxp_fuzz_arith.c

$(BUILD_DIR)/tests/lxp_test_arith_property: $(ARITH_PROPERTY_SOURCES) $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) -I. -Itests/arith $(CFLAGS) $(ARITH_PROPERTY_SOURCES) \
		$(LIBRARY) $(EXTRA_LDFLAGS) -o $@

$(BUILD_DIR)/tests/lxp_test_arith_property_san: $(ARITH_PROPERTY_SOURCES) \
		src/protocol/lxp_u128.c src/protocol/lxp_u256.c \
		src/protocol/lxp_i128.c src/protocol/lxp_result.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) -I. -Itests/arith $(CFLAGS) $(SANITIZER_CFLAGS) \
		-fsanitize=address,undefined $^ -fsanitize=address,undefined -o $@

test-arith-property: $(BUILD_DIR)/tests/lxp_test_arith_property \
		$(BUILD_DIR)/tests/lxp_test_arith_property_san
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_arith_property
	ASAN_OPTIONS=abort_on_error=1:detect_leaks=1 \
	UBSAN_OPTIONS=halt_on_error=1 $(BUILD_DIR)/tests/lxp_test_arith_property_san

$(BUILD_DIR)/tests/lxp_test_nofloat: tests/arith/lxp_test_nofloat.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-arith-nofloat: build $(BUILD_DIR)/tests/lxp_test_nofloat
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_nofloat
	@if $(CC) $(CPPFLAGS) $(CFLAGS) -c tests/arith/compile_fail_double.c \
		-o $(BUILD_DIR)/tests/compile_fail_double.o >/dev/null 2>&1; then \
		echo "double-to-amount compile-fail test unexpectedly compiled" >&2; \
		exit 1; \
	fi
	@if $(CC) $(CPPFLAGS) $(CFLAGS) -c tests/arith/compile_fail_plus.c \
		-o $(BUILD_DIR)/tests/compile_fail_plus.o >/dev/null 2>&1; then \
		echo "bare amount addition compile-fail test unexpectedly compiled" >&2; \
		exit 1; \
	fi
	tools/ci/no-float-scan.sh "$(BUILD_DIR)"

$(BUILD_DIR)/tests/lxp_test_log: tests/storage/lxp_test_log.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-log: $(BUILD_DIR)/tests/lxp_test_log
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_log

$(BUILD_DIR)/tests/lxp_test_log_durability: \
		tests/storage/lxp_test_log_durability.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-log-durability: $(BUILD_DIR)/tests/lxp_test_log_durability
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_log_durability

$(BUILD_DIR)/tests/lxp_test_recovery: tests/storage/lxp_test_recovery.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-recovery: $(BUILD_DIR)/tests/lxp_test_recovery
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_recovery

$(BUILD_DIR)/tests/lxp_test_projection: tests/storage/lxp_test_projection.c \
		src/storage/lxp_projection.c $(LIBRARY) \
		migrations/0001_projection.sql
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/storage/lxp_test_projection.c \
		src/storage/lxp_projection.c $(LIBRARY) \
		$(EXTRA_LDFLAGS) -lsqlite3 -pthread -o $@

test-projection: $(BUILD_DIR)/tests/lxp_test_projection build
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_projection
	tools/ci/symbol-allowlist.sh "$(BUILD_DIR)"

$(BUILD_DIR)/tests/lxp_test_rebuild: tests/storage/lxp_test_rebuild.c \
		src/storage/lxp_projection.c $(LIBRARY) migrations/0001_projection.sql
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/storage/lxp_test_rebuild.c \
		src/storage/lxp_projection.c $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lsqlite3 -pthread -o $@

$(BUILD_DIR)/tools/log_inspect: tools/log_inspect.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-rebuild: $(BUILD_DIR)/tests/lxp_test_rebuild $(BUILD_DIR)/tools/log_inspect
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_rebuild

$(BUILD_DIR)/tests/lxp_test_journal: tests/state/lxp_test_journal.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -pthread -o $@

$(BUILD_DIR)/tests/lxp_test_journal_tsan: tests/state/lxp_test_journal.c \
		src/state/lxp_journal.c src/crypto/lxp_hash.c \
		src/state/lxp_idempotency.c \
		src/crypto/lxp_ct.c \
		src/protocol/lxp_protocol.c src/protocol/lxp_result.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(SANITIZER_CFLAGS) -fsanitize=thread \
		$^ -fsanitize=thread -pthread -o $@

test-journal: $(BUILD_DIR)/tests/lxp_test_journal \
		$(BUILD_DIR)/tests/lxp_test_journal_tsan
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_journal
	TSAN_OPTIONS=halt_on_error=1 setarch $(shell uname -m) -R \
		$(BUILD_DIR)/tests/lxp_test_journal_tsan

$(BUILD_DIR)/tests/lxp_test_activity_codec: \
		tests/protocol/lxp_test_activity_codec.c fuzz/lxp_fuzz_activity.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) tests/protocol/lxp_test_activity_codec.c \
		fuzz/lxp_fuzz_activity.c $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-activity-codec: $(BUILD_DIR)/tests/lxp_test_activity_codec
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_activity_codec

$(BUILD_DIR)/tests/lxp_test_envelope: tests/protocol/lxp_test_envelope.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-envelope: $(BUILD_DIR)/tests/lxp_test_envelope
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_envelope

$(BUILD_DIR)/tests/lxp_test_verify_pool: tests/network/lxp_test_verify_pool.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -pthread -o $@

$(BUILD_DIR)/tests/lxp_test_verify_pool_tsan: \
		tests/network/lxp_test_verify_pool.c src/network/lxp_verify_pool.c \
		src/crypto/lxp_ed25519.c src/crypto/lxp_hash.c src/crypto/lxp_ct.c \
		src/protocol/lxp_protocol.c src/protocol/lxp_result.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(SANITIZER_CFLAGS) -fsanitize=thread \
		$^ -fsanitize=thread -lcrypto -pthread -o $@

test-verify-pool: $(BUILD_DIR)/tests/lxp_test_verify_pool \
		$(BUILD_DIR)/tests/lxp_test_verify_pool_tsan
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_verify_pool
	TSAN_OPTIONS=halt_on_error=1 setarch $(shell uname -m) -R \
		$(BUILD_DIR)/tests/lxp_test_verify_pool_tsan

$(BUILD_DIR)/tests/lxp_test_admission: tests/sequencer/lxp_test_admission.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-admission: $(BUILD_DIR)/tests/lxp_test_admission
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_admission

$(BUILD_DIR)/tests/lxp_test_idempotency: tests/state/lxp_test_idempotency.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -pthread -o $@

test-idempotency: $(BUILD_DIR)/tests/lxp_test_idempotency
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_idempotency

$(BUILD_DIR)/tests/lxp_test_fee_gate: tests/protocol/lxp_test_fee_gate.c \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(PROGRAMS_RUNTIME_LIB) \
		$(LIBRARY) $(EXTRA_LDFLAGS) -lcrypto -pthread -ldl -lm -o $@

test-fee-gate: $(BUILD_DIR)/tests/lxp_test_fee_gate
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_fee_gate

$(BUILD_DIR)/tests/lxp_test_identity: tests/state/lxp_test_identity.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -lcrypto -o $@

test-identity: $(BUILD_DIR)/tests/lxp_test_identity
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_identity

$(BUILD_DIR)/tests/lxp_test_grants: tests/state/lxp_test_grants.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-grants: $(BUILD_DIR)/tests/lxp_test_grants
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_grants

$(BUILD_DIR)/tests/lxp_test_authority_resolve: \
		tests/state/lxp_test_authority_resolve.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-authority-resolve: $(BUILD_DIR)/tests/lxp_test_authority_resolve
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_authority_resolve

$(BUILD_DIR)/tests/lxp_test_allowance: tests/state/lxp_test_allowance.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-allowance: $(BUILD_DIR)/tests/lxp_test_allowance
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_allowance

$(BUILD_DIR)/tests/lxp_test_revocation: tests/state/lxp_test_revocation.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-revocation: $(BUILD_DIR)/tests/lxp_test_revocation
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_revocation

$(BUILD_DIR)/tests/lxp_test_rotation: tests/state/lxp_test_rotation.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) \
		-lcrypto -o $@

test-rotation: $(BUILD_DIR)/tests/lxp_test_rotation
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_rotation

$(BUILD_DIR)/tests/lxp_test_sanitizer_smoke: \
		tests/protocol/lxp_test_sanitizer_smoke.c $(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -o $@

test-sanitizer-smoke: $(BUILD_DIR)/tests/lxp_test_sanitizer_smoke
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_sanitizer_smoke

test-sanitizer-suite: test-result test-protocol test-harness \
		test-sanitizer-smoke $(BUILD_DIR)/tests/lxp_test_arena
	$(RUN_PREFIX) $(BUILD_DIR)/tests/lxp_test_arena

scan-consensus: build
	tools/ci/no-float-scan.sh "$(BUILD_DIR)"
	tools/ci/symbol-allowlist.sh "$(BUILD_DIR)"

public-audit:
	tools/ci/public-repo-audit.sh

agent-build:
	$(AGENT_CARGO) build --manifest-path $(AGENT_MANIFEST) --locked --workspace

.PHONY: human-js-install
human-js-install:
	$(HUMAN_NPM) ci --ignore-scripts --no-audit --no-fund

human-gen-api:
	$(HUMAN_CARGO) run --manifest-path human/tools/api-gen/Cargo.toml --locked -- human/schema/human-api human/apps/web/src/api/generated

human-build:
	$(HUMAN_CARGO) test --manifest-path human/tools/api-gen/Cargo.toml --locked
	$(HUMAN_CARGO) run --manifest-path human/tools/api-gen/Cargo.toml --locked -- --check human/schema/human-api human/apps/web/src/api/generated
	$(HUMAN_CARGO) build --manifest-path $(HUMAN_MANIFEST) --locked --workspace
	$(HUMAN_NPM) run build

human-test: $(BUILD_DIR)/tests/explorer_fixture
	LAYERX_EXPLORER_CORE_FIXTURE=$(abspath $(BUILD_DIR)/tests/explorer_fixture) \
		$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked --workspace
	$(HUMAN_NPM) test

human-test-unit:
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked --workspace --lib

human-test-integration: $(BUILD_DIR)/tests/explorer_fixture
	LAYERX_EXPLORER_CORE_FIXTURE=$(abspath $(BUILD_DIR)/tests/explorer_fixture) \
		$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked --workspace --tests

human-test-intents:
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-intents

human-test-service:
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service

human-test-agents: test-rotation
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test agent_create
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test agent_controls
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test spend
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test reclaim
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test archive
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test agent_recovery

human-test-journeys:
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test resolver
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test journey_faults
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test deposit
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test withdraw
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test exit
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test move_money

human-test-explorer: $(BUILD_DIR)/tests/explorer_fixture
	LAYERX_EXPLORER_CORE_FIXTURE=$(abspath $<) \
		$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked \
		-p layerx-explorer-index

human-test-notify:
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test notify
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test links

human-test-approvals:
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test approvals
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test render
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test decide

human-test-activity:
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test activity
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test detail
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test export

human-test-paxeer: test-bridge-deposit
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-paxeer-client

human-fuzz-intents:
	cargo +nightly-2025-11-10 fuzz run intent --fuzz-dir human/crates/layerx-intents/fuzz -- -max_total_time=120 -timeout=10

human-test-property:
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked --workspace property_

human-test-fault:
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test journey_faults
	$(HUMAN_CARGO) test --manifest-path $(HUMAN_MANIFEST) --locked -p layerx-human-service --test move_money

human-test-component:
	$(HUMAN_NPM) run test:component

human-e2e-foundation:
	$(HUMAN_NPM) run test:foundation
	$(HUMAN_NPM) run build

human-e2e-perf:
	$(HUMAN_NPM) run build
	HUMAN_E2E_REAL_STACK=1 \
	HUMAN_E2E_LOCAL_PRODUCTION=1 \
	HUMAN_E2E_BASE_URL=http://127.0.0.1:3105 \
	LAYERX_RUM_STORAGE_DIRECTORY=$(abspath human/apps/web/.next/rum-data) \
		$(HUMAN_NPM) run test:perf

human-test-journey:
	$(HUMAN_NPM) run test:journey

human-e2e-journeys:
	$(HUMAN_NPM) run build
	HUMAN_E2E_REAL_STACK=1 \
	HUMAN_E2E_LOCAL_PRODUCTION=1 \
	HUMAN_E2E_BASE_URL=http://127.0.0.1:3105 \
		$(HUMAN_NPM) run test:journey

human-e2e-settings:
	$(HUMAN_NPM) run build
	HUMAN_E2E_REAL_STACK=1 \
	HUMAN_E2E_LOCAL_PRODUCTION=1 \
	HUMAN_E2E_BASE_URL=http://127.0.0.1:3105 \
		$(HUMAN_NPM) run test:settings

human-e2e-explorer:
	$(HUMAN_NPM) run build
	HUMAN_E2E_REAL_STACK=1 \
	HUMAN_E2E_LOCAL_PRODUCTION=1 \
	HUMAN_E2E_BASE_URL=http://127.0.0.1:3105 \
		$(HUMAN_NPM) run test:explorer

human-test-e2e:
	$(HUMAN_NPM) run test:e2e

human-test-visual:
	$(HUMAN_NPM) run test:visual

human-test-e2e-long:
	$(HUMAN_NPM) run test:e2e -- --repeat-each=5

human-lint: human-lint-copy
	$(HUMAN_CARGO) clippy --manifest-path $(HUMAN_MANIFEST) --locked --workspace --all-targets -- -D warnings
	$(HUMAN_CARGO) test --manifest-path human/tools/boundary-check/Cargo.toml --locked
	$(HUMAN_CARGO) run --manifest-path human/tools/boundary-check/Cargo.toml --locked -- human/crates
	sh human/tools/dependency-policy.sh
	cargo deny --manifest-path $(HUMAN_MANIFEST) check advisories bans sources
	$(HUMAN_NPM) run lint

human-lint-copy:
	$(HUMAN_CARGO) clippy --manifest-path human/tools/copy-lint/Cargo.toml --locked --all-targets -- -D warnings
	$(HUMAN_CARGO) test --manifest-path human/tools/copy-lint/Cargo.toml --locked
	$(HUMAN_CARGO) run --manifest-path human/tools/copy-lint/Cargo.toml --locked -- human/apps/web
	$(HUMAN_NPM) run typecheck
	$(HUMAN_NPM) test

human-check-ui:
	$(HUMAN_CARGO) clippy --manifest-path human/tools/ui-gate/Cargo.toml --locked --all-targets -- -D warnings
	$(HUMAN_CARGO) test --manifest-path human/tools/ui-gate/Cargo.toml --locked
	$(HUMAN_CARGO) run --manifest-path human/tools/ui-gate/Cargo.toml --locked -- human/apps/web
	$(HUMAN_NPM) run build:ui
	$(HUMAN_NPM) run typecheck
	$(HUMAN_NPM) test

human-check:
	$(HUMAN_CARGO) check --manifest-path $(HUMAN_MANIFEST) --locked --workspace
	$(HUMAN_CARGO) test --manifest-path human/tools/boundary-check/Cargo.toml --locked
	$(HUMAN_CARGO) run --manifest-path human/tools/boundary-check/Cargo.toml --locked -- human/crates
	$(HUMAN_CARGO) test --manifest-path human/tools/schema-check/Cargo.toml --locked
	$(HUMAN_CARGO) run --manifest-path human/tools/schema-check/Cargo.toml --locked -- human/schema/human-api
	$(HUMAN_NPM) run typecheck

human-check-bundle:
	$(HUMAN_NPM) run build
	$(HUMAN_CARGO) run --manifest-path human/tools/boundary-check/Cargo.toml --locked -- --web-bundle human/apps/web/.next

human-e2e: human-test-e2e

agent-test:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked --workspace

agent-lint:
	$(AGENT_CARGO) clippy --manifest-path $(AGENT_MANIFEST) --locked --workspace --all-targets -- -D warnings
	sh agent/tools/dependency-policy.sh

AGENT_WIRE_FUZZ_TARGETS := primitive_decode envelope_decode payload_decode \
	receipt_decode proof_decode roundtrip
AGENT_INTERFACE_FUZZ_TARGETS := lni_frame contract_request policy_loader tenant_key
AGENT_FUZZ_TARGETS := $(AGENT_WIRE_FUZZ_TARGETS) $(AGENT_INTERFACE_FUZZ_TARGETS)
AGENT_FUZZ_RUNS ?= 128
AGENT_FUZZ_LONG_SECONDS ?= 300
AGENT_FUZZ_MAX_LEN ?= 1048576
AGENT_FUZZ_RSS_MB ?= 512
AGENT_FUZZ_TIMEOUT ?= 2
AGENT_FUZZ_VERBOSITY ?= 0
AGENT_FUZZ_MINIMIZED_ROOT ?= $(abspath $(BUILD_DIR)/agent-fuzz-minimized)

agent-fuzz: agent-fuzz-all

agent-fuzz-wire: AGENT_FUZZ_SELECTED := $(AGENT_WIRE_FUZZ_TARGETS)
agent-fuzz-interface: AGENT_FUZZ_SELECTED := $(AGENT_INTERFACE_FUZZ_TARGETS)
agent-fuzz-all: AGENT_FUZZ_SELECTED := $(AGENT_FUZZ_TARGETS)
agent-fuzz-wire agent-fuzz-interface agent-fuzz-all:
	@set -eu; agent_fuzz_tmp=$$(mktemp -d); \
	trap 'rm -rf -- "$$agent_fuzz_tmp"' EXIT HUP INT TERM; \
	for target in $(AGENT_FUZZ_SELECTED); do \
		mkdir -p "$$agent_fuzz_tmp/$$target"; \
		cd agent/fuzz; \
		$(AGENT_CARGO) +$(AGENT_FUZZ_TOOLCHAIN) fuzz run "$$target" \
			"$$agent_fuzz_tmp/$$target" "corpus/$$target" -- \
			-runs=$(AGENT_FUZZ_RUNS) -max_len=$(AGENT_FUZZ_MAX_LEN) \
			-rss_limit_mb=$(AGENT_FUZZ_RSS_MB) -timeout=$(AGENT_FUZZ_TIMEOUT) \
			-verbosity=$(AGENT_FUZZ_VERBOSITY); \
		cd ../..; \
	done

agent-fuzz-long: AGENT_FUZZ_SELECTED := $(AGENT_FUZZ_TARGETS)
agent-fuzz-wire-long: AGENT_FUZZ_SELECTED := $(AGENT_WIRE_FUZZ_TARGETS)
agent-fuzz-long agent-fuzz-wire-long:
	@set -eu; agent_fuzz_tmp=$$(mktemp -d); \
	trap 'rm -rf -- "$$agent_fuzz_tmp"' EXIT HUP INT TERM; \
	for target in $(AGENT_FUZZ_SELECTED); do \
		mkdir -p "$$agent_fuzz_tmp/$$target"; \
		cd agent/fuzz; \
		$(AGENT_CARGO) +$(AGENT_FUZZ_TOOLCHAIN) fuzz run "$$target" \
			"$$agent_fuzz_tmp/$$target" "corpus/$$target" -- \
			-max_total_time=$(AGENT_FUZZ_LONG_SECONDS) -max_len=$(AGENT_FUZZ_MAX_LEN) \
			-rss_limit_mb=$(AGENT_FUZZ_RSS_MB) -timeout=$(AGENT_FUZZ_TIMEOUT) \
			-verbosity=$(AGENT_FUZZ_VERBOSITY); \
		cd ../..; \
	done

agent-fuzz-minimize: AGENT_FUZZ_SELECTED := $(AGENT_FUZZ_TARGETS)
agent-fuzz-wire-minimize: AGENT_FUZZ_SELECTED := $(AGENT_WIRE_FUZZ_TARGETS)
agent-fuzz-minimize agent-fuzz-wire-minimize:
	@set -eu; mkdir -p "$(AGENT_FUZZ_MINIMIZED_ROOT)"; \
	for target in $(AGENT_FUZZ_SELECTED); do \
		rm -rf -- "$(AGENT_FUZZ_MINIMIZED_ROOT)/$$target"; \
		mkdir -p "$(AGENT_FUZZ_MINIMIZED_ROOT)/$$target"; \
		cp agent/fuzz/corpus/$$target/* "$(AGENT_FUZZ_MINIMIZED_ROOT)/$$target/"; \
		cd agent/fuzz; \
		$(AGENT_CARGO) +$(AGENT_FUZZ_TOOLCHAIN) fuzz cmin "$$target" \
			"$(AGENT_FUZZ_MINIMIZED_ROOT)/$$target" -- \
			-max_len=$(AGENT_FUZZ_MAX_LEN) -rss_limit_mb=$(AGENT_FUZZ_RSS_MB) \
			-timeout=$(AGENT_FUZZ_TIMEOUT); \
		cd ../..; \
	done

agent-check: agent-check-boundary agent-check-secrets agent-test-boundary
	$(AGENT_CARGO) check --manifest-path $(AGENT_MANIFEST) --locked --workspace --all-targets

agent-test-errors:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-types --test errors

agent-test-types-ids:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-types --test ids

agent-test-types-activity:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-types --test activity

agent-test-types-receipt:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-types --test receipt

agent-test-types-verification:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-types --test verification

agent-test-vectors:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-types --test vectors

agent-test-wire-primitives:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-wire --test primitives

agent-test-wire-structures:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-wire --test structures

agent-test-wire-rejection:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-wire --test rejection

agent-test-wire-hashing:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-wire --test hashing

agent-test-crypto-verify: test-crypto-ed25519 test-crypto-secp256k1
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-crypto --test verify

agent-test-crypto-signer:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-crypto --test signer

agent-test-crypto-disclosure:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-crypto --test disclosure
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-crypto --test signer

agent-test-crypto-keystore: test-grants
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-crypto --test keystore

agent-test-crypto-remote:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-crypto --test remote -- --test-threads=1

agent-test-proof-merkle:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-proof --test merkle

agent-test-proof-receipt:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-proof --test receipt

agent-test-proof-inclusion:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-proof --test inclusion

agent-test-proof-checkpoint:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-proof --test checkpoint

agent-test-proof-availability:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-proof --test availability

agent-test-proof-levels:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-proof --tests
	$(AGENT_CARGO) check --manifest-path $(AGENT_MANIFEST) --locked -p layerx-proof --example offline_verify

agent-test-lni-schema:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-client --test lni_schema

agent-test-lni-transport:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-client --test lni_transport
	$(AGENT_CARGO) check --manifest-path agent/fuzz/Cargo.toml --locked --bin lni_frame

agent-test-lni-handshake:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-client --test lni_handshake

agent-test-lni-abi:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-client --test lni_abi
	$(AGENT_CARGO) test --manifest-path agent/tools/boundary-check/Cargo.toml --locked
	$(AGENT_CARGO) run --manifest-path agent/tools/boundary-check/Cargo.toml --locked --quiet -- agent

$(BUILD_DIR)/agent/layerxd-lni: agent/tests/boundary/node/layerxd_lni.c \
		$(LAYERXD_SOURCES) $(LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	mkdir -p $(dir $@)
	$(CC) $(CPPFLAGS) $(CFLAGS) agent/tests/boundary/node/layerxd_lni.c \
		$(LAYERXD_SOURCES) $(LIBRARY) $(PROGRAMS_RUNTIME_LIB) \
		$(EXTRA_LDFLAGS) -lcrypto -lsqlite3 -pthread -ldl -lm -o $@

agent-test-boundary: $(BUILD_DIR)/agent/layerxd-lni
	$(AGENT_CARGO) run --manifest-path agent/tests/boundary/Cargo.toml --locked -- \
		$(CURDIR)/$(BUILD_DIR)/agent/layerxd-lni $(CURDIR)

agent-qualify-boundary: $(BUILD_DIR)/agent/layerxd-lni
	$(AGENT_CARGO) build --manifest-path agent/tests/boundary/Cargo.toml --locked
	$(AGENT_CARGO) run --manifest-path agent/tests/qualify/Cargo.toml --locked -- boundary \
		$(CURDIR) $(CURDIR)/$(BUILD_DIR)/agent/layerxd-lni \
		$(CURDIR)/agent/tests/boundary/target/debug/agent-boundary-conformance

agent-qualify-fabrication:
	$(MAKE) agent-test-sdk-ts
	$(MAKE) agent-test-sdk-py
	$(AGENT_CARGO) run --manifest-path agent/tests/qualify/Cargo.toml --locked -- fabrication $(CURDIR)

agent-qualify-faults: $(BUILD_DIR)/agent/layerxd-lni
	$(MAKE) agent-test-sdk-parity
	$(AGENT_CARGO) run --manifest-path agent/tests/qualify/Cargo.toml --locked -- faults $(CURDIR)

agent-qualify-fuzz:
	$(AGENT_CARGO) run --manifest-path agent/tests/qualify/Cargo.toml --locked -- fuzz \
		$(CURDIR) $(AGENT_FUZZ_MINIMIZED_ROOT)

agent-test-capability-report: agent-test-boundary

agent-test-client-connection:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-client --test connection
	$(AGENT_CARGO) test --manifest-path agent/tools/boundary-check/Cargo.toml --locked
	$(AGENT_CARGO) run --manifest-path agent/tools/boundary-check/Cargo.toml --locked --quiet -- agent

agent-test-client-submit:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-client --test submit
	$(AGENT_CARGO) run --manifest-path agent/tools/boundary-check/Cargo.toml --locked --quiet -- agent

agent-test-client-receipt:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-client --test receipt
	$(AGENT_CARGO) run --manifest-path agent/tools/boundary-check/Cargo.toml --locked --quiet -- agent

agent-test-client-reads:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-client --test reads
	$(AGENT_CARGO) run --manifest-path agent/tools/boundary-check/Cargo.toml --locked --quiet -- agent

agent-test-client-stream:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-client --test stream
	$(AGENT_CARGO) run --manifest-path agent/tools/boundary-check/Cargo.toml --locked --quiet -- agent

agent-test-client-availability:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-client --test availability
	$(AGENT_CARGO) run --manifest-path agent/tools/boundary-check/Cargo.toml --locked --quiet -- agent

agent-test-contract-schema:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agent-api --test schema

agent-test-contract-identity:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agent-api --test identity

agent-test-contract-write:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agent-api --test write

agent-test-contract-read:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agent-api --test read

agent-test-contract-stream:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agent-api --test stream

agent-test-contract-errors:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agent-api --test errors

agent-test-agentd-store:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test store

agent-test-agentd-identity:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test identity

agent-test-agentd-session:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test session

agent-test-agentd-authority:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test authority

agent-test-agentd-revocation:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test revocation

agent-test-agentd-capability:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test capability

agent-test-agentd-narrowing:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test narrowing

agent-test-agentd-attenuation:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test attenuation

agent-test-agentd-ceiling:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test ceiling

agent-test-agentd-enforcement-report:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test enforcement_report

agent-test-agentd-budget-create:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test budget_create

agent-test-agentd-budget-reconcile:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test budget_reconcile

agent-test-agentd-budget-reserve:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test budget_reserve

agent-test-agentd-budget-unknown:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test budget_unknown

agent-test-agentd-budget-divergence:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test budget_divergence

agent-test-agentd-policy:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test policy

agent-test-agentd-policy-version:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test policy_version
	$(AGENT_CARGO) check --manifest-path agent/fuzz/Cargo.toml --locked --bin policy_loader

agent-test-agentd-policy-dryrun:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test policy_dryrun

agent-test-agentd-approval:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test approval

agent-test-approvals:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test approval --test approval_ops --test approval_semantics --test approval_events
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-mcp --test approval

agent-test-policy-adversarial:
	$(AGENT_CARGO) test --manifest-path agent/tools/policy-harness/Cargo.toml --locked
	$(AGENT_CARGO) run --manifest-path agent/tools/policy-harness/Cargo.toml --locked --quiet

agent-test-agentd-prepare:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test prepare

agent-test-agentd-disclosure:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test disclosure

agent-test-agentd-signing:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test signing

agent-test-agentd-signature-binding:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test signature_binding

agent-test-agentd-prepare-expiry:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test prepare_expiry

agent-test-agentd-outbox:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test outbox

agent-test-agentd-idempotency:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test idempotency

agent-test-agentd-unknown:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test unknown

agent-test-agentd-receipts:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test receipts

agent-test-agentd-finality:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test finality

agent-test-agentd-recovery:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test recovery

agent-test-agentd-balance:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test balance

agent-test-agentd-history:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test history

agent-test-agentd-checkpoint:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test checkpoint

agent-test-agentd-availability:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test availability

agent-test-agentd-cache:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test cache

agent-test-agentd-export:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test export
	$(AGENT_CARGO) check --manifest-path $(AGENT_MANIFEST) --locked -p layerx-proof --example offline_export

agent-test-agentd-ingest:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test ingest

agent-test-agentd-subscription:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test subscription

agent-test-agentd-delivery:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test delivery

agent-test-agentd-gaps:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test gaps

agent-test-agentd-webhook:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test webhook

agent-test-agentd-ratelimit:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test ratelimit

agent-test-agentd-admission:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test admission

agent-test-agentd-deadlines:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test deadlines

agent-test-agentd-quota:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test quota

agent-test-limits-exactly-once:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test limits-exactly-once

agent-test-agentd-tenant-store:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test tenant_store
	$(AGENT_CARGO) check --manifest-path agent/fuzz/Cargo.toml --locked --bin tenant_key

agent-test-agentd-tenant-resolve:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test tenant_resolve

agent-test-agentd-tenant-isolation:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test tenant_isolation

agent-test-agentd-tenant-leakage:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test tenant_leakage

agent-test-agentd-audit-chain:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test audit_chain
	$(AGENT_CARGO) check --manifest-path agent/tools/audit-verify/Cargo.toml --locked

agent-test-agentd-audit-coverage:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test audit_coverage

agent-test-agentd-redaction:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test redaction
	$(AGENT_CARGO) test --manifest-path agent/tools/secret-check/Cargo.toml --locked
	$(AGENT_CARGO) run --manifest-path agent/tools/secret-check/Cargo.toml --locked --quiet -- agent

agent-test-agentd-observability:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test observability

agent-test-agentd-audit-export:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test audit_export
	$(AGENT_CARGO) check --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --example review_audit_export

agent-test-agentd-config:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test config

agent-test-agentd-handshake-gate:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test handshake_gate

agent-test-agentd-degraded:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test degraded

agent-test-agentd-migration:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test migration
	$(MAKE) agent-test-boundary

agent-test-agentd-operator:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-agentd --test operator
	$(AGENT_CARGO) run --manifest-path agent/tools/secret-check/Cargo.toml --locked --quiet -- agent

agent-test-mcp-scope:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-mcp --test scope

agent-test-mcp-read:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-mcp --test read

agent-test-mcp-write:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-mcp --test write

agent-test-mcp-approval:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-mcp --test approval

agent-test-mcp-injection:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-mcp --test injection
	$(AGENT_CARGO) test --manifest-path agent/tests/isolation/Cargo.toml --locked mcp_untrusted

agent-test-mcp-readonly:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-mcp --test readonly

agent-test-sdk-rust:
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-sdk --test sdk

agent-sdk-generate:
	cargo run --manifest-path agent/tools/sdk-gen/Cargo.toml --locked -- --write

agent-test-sdk-generate:
	cargo test --manifest-path agent/tools/sdk-gen/Cargo.toml --locked
	cargo run --manifest-path agent/tools/sdk-gen/Cargo.toml --locked -- --check

agent-test-sdk-ts:
	$(MAKE) agent-test-sdk-generate
	cd agent/sdk/typescript && npm test

agent-test-sdk-py:
	$(MAKE) agent-test-sdk-generate
	PYTHONPATH=agent/sdk/python python3 -m unittest discover -s agent/sdk/python/tests -p 'test_*.py'
	PYTHONPATH=agent/sdk/python python3 -m compileall -q agent/sdk/python/layerx_sdk agent/sdk/python/examples

agent-test-sdk-parity: $(BUILD_DIR)/agent/layerxd-lni
	$(MAKE) agent-test-sdk-ts
	$(MAKE) agent-test-sdk-py
	$(AGENT_CARGO) run --manifest-path agent/tests/parity/Cargo.toml --locked -- \
		$(CURDIR)/$(BUILD_DIR)/agent/layerxd-lni $(CURDIR)

agent-test-sdk-compat:
	$(MAKE) agent-test-contract-schema
	$(MAKE) agent-test-sdk-generate
	$(AGENT_CARGO) test --manifest-path agent/tools/doc-check/Cargo.toml --locked
	$(AGENT_CARGO) run --manifest-path agent/tools/doc-check/Cargo.toml --locked -- $(CURDIR)

agent-test-mcp-untrusted-input:
	$(AGENT_CARGO) test --manifest-path agent/tests/isolation/Cargo.toml --locked mcp_untrusted

agent-test-isolation: agent-test-policy-adversarial agent-test-mcp-untrusted-input
	$(AGENT_CARGO) test --manifest-path agent/tests/isolation/Cargo.toml --locked agent_isolation
	$(AGENT_CARGO) run --manifest-path agent/tests/isolation/Cargo.toml --locked --quiet

$(BUILD_DIR)/agent-wire-reference: agent/tools/wire-differential/reference.c $(LIBRARY)
	mkdir -p $(dir $@)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(EXTRA_LDFLAGS) -lcrypto -pthread -o $@

agent-test-wire-parity: $(BUILD_DIR)/agent-wire-reference
	LAYERX_REPOSITORY_ROOT=$(CURDIR) LAYERX_C_REFERENCE=$(CURDIR)/$(BUILD_DIR)/agent-wire-reference \
		$(AGENT_CARGO) test --manifest-path agent/tools/wire-differential/Cargo.toml --locked

agent-qualify-wire: $(BUILD_DIR)/agent-wire-reference
	$(AGENT_CARGO) build --manifest-path agent/tools/wire-differential/Cargo.toml --locked
	$(AGENT_CARGO) run --manifest-path agent/tests/qualify/Cargo.toml --locked -- wire \
		$(CURDIR) $(CURDIR)/$(BUILD_DIR)/agent-wire-reference \
		$(CURDIR)/agent/tools/wire-differential/target/debug/agent-wire-differential

agent-test-sanitize:
	sh agent/tools/run-sanitizers.sh

agent-check-boundary:
	$(AGENT_CARGO) test --manifest-path agent/tools/boundary-check/Cargo.toml --locked
	$(AGENT_CARGO) run --manifest-path agent/tools/boundary-check/Cargo.toml --locked --quiet -- agent

agent-check-secrets:
	$(AGENT_CARGO) test --manifest-path agent/tools/secret-check/Cargo.toml --locked
	$(AGENT_CARGO) run --manifest-path agent/tools/secret-check/Cargo.toml --locked --quiet -- agent
	$(AGENT_CARGO) test --manifest-path $(AGENT_MANIFEST) --locked -p layerx-crypto --test secrets

ci: public-audit test reproducible scan-consensus test-sanitizers

.PHONY: paxeer-build paxeer-lint paxeer-test paxeer-ci paxeer-docs-install \
	paxeer-docs-build paxeer-docs-static-test developer-dashboard-install \
	developer-dashboard-build developer-dashboard-static-test specgen-build \
	specgen-test specgen-lint core-test-all workspace-install workspace-build workspace-test \
	workspace-lint workspace-inventory-check workspace-ci hpx-public-check monorepo-ci \
	paxeer-manifest-install paxeer-manifest-build paxeer-manifest-test \
	paxeer-manifest-lint paxeer-npm-install paxeer-npm-build paxeer-npm-static-test \
	paxeer-tools-install paxeer-hardhat-compilers-ready workspace-node-preflight paxeer-node-preflight paxeer-npm-dependencies-ready core-qualification-environment

workspace-node-preflight:
	@node -e 'const major=Number(process.versions.node.split(".")[0]); if (major < 24) { console.error(`Node >=24 required by workspace packages; found $${process.version}`); process.exit(1); }'

PAXEER_LOCKED_RUST_MANIFESTS := \
	paxeer-network/example/cosmwasm/cw1155/Cargo.toml \
	paxeer-network/example/cosmwasm/cw20/Cargo.toml \
	paxeer-network/example/cosmwasm/cw721/Cargo.toml \
	paxeer-network/example/cosmwasm/echo/Cargo.toml \
	paxeer-network/example/cosmwasm/iter/Cargo.toml \
	paxeer-network/loadtest/contracts/jupiter/Cargo.toml \
	paxeer-network/loadtest/contracts/mars/Cargo.toml \
	paxeer-network/loadtest/contracts/saturn/Cargo.toml \
	paxeer-network/loadtest/contracts/venus/Cargo.toml \
	paxeer-network/parallelization/bank/Cargo.toml \
	paxeer-network/parallelization/staking/Cargo.toml \
	paxeer-network/parallelization/wasm/Cargo.toml \
	paxeer-network/wasm-runtime/libwasmvm/Cargo.toml

PAXEER_NESTED_GO_DIRS := paxeer-network/hpx/registry \
	paxeer-network/sdk/cosmovisor paxeer-network/sdk/ics23
PAXEER_TOOLS_DIR := $(CURDIR)/build/paxeer-tools
PAXEER_GOLANGCI_LINT := $(PAXEER_TOOLS_DIR)/golangci-lint
PAXEER_GOLANGCI_LINT_SUM_FILE := tools/workspace/checksums/golangci-lint-v2.8.0.h1

paxeer-build:
	GOPROXY=off $(PAXEER_MAKE) build

paxeer-lint:
	$(PAXEER_MAKE) GOLANGCI_LINT=$(PAXEER_GOLANGCI_LINT) lint

paxeer-test:
	GOPROXY=off $(PAXEER_MAKE) test

paxeer-ci:
	GOPROXY=off $(PAXEER_MAKE) GOLANGCI_LINT=$(PAXEER_GOLANGCI_LINT) ci

workspace-inventory-check:
	sh tools/workspace/check-paxeer-manifests.sh
	sh tools/workspace/check-core-gates.sh
	sh tools/workspace/check-platform-dependencies.sh

paxeer-node-preflight:
	@node -e 'const major=Number(process.versions.node.split(".")[0]); if (major < 20) { console.error(`Node >=20 required; found $${process.version}`); process.exit(1); }'

paxeer-npm-install: paxeer-node-preflight
	npm --prefix paxeer-network/contracts ci --ignore-scripts --no-audit --no-fund
	npm --prefix paxeer-network/integration_test/dapp_tests ci --ignore-scripts --no-audit --no-fund
	npm --prefix paxeer-network/integration_test/rpc_tests ci --ignore-scripts --no-audit --no-fund
	$(MAKE) paxeer-docs-install

paxeer-npm-dependencies-ready:
	@test -d paxeer-network/contracts/node_modules
	@test -d paxeer-network/integration_test/dapp_tests/node_modules
	@test -d paxeer-network/integration_test/rpc_tests/node_modules
	@test -d paxeer-network/paxeer-docs/node_modules

paxeer-hardhat-compilers-ready: paxeer-npm-dependencies-ready
	node tools/workspace/check-hardhat-compilers.mjs

paxeer-npm-build: paxeer-hardhat-compilers-ready
	npm --prefix paxeer-network/contracts exec -- hardhat compile
	npm --prefix paxeer-network/integration_test/dapp_tests exec -- hardhat compile
	npm --prefix paxeer-network/integration_test/rpc_tests run compile
	$(MAKE) paxeer-docs-build

paxeer-npm-static-test: paxeer-npm-dependencies-ready
	npm --prefix paxeer-network/contracts exec -- tsc --noEmit
	find paxeer-network/integration_test/dapp_tests -type f -name '*.js' -exec node --check {} \;
	npm --prefix paxeer-network/integration_test/rpc_tests exec -- tsc --noEmit
	$(MAKE) paxeer-docs-static-test

paxeer-tools-install:
	@set -eu; \
	if test ! -f "$(PAXEER_GOLANGCI_LINT_SUM_FILE)"; then \
		echo "paxeer-tools-install: missing reviewed module checksum $(PAXEER_GOLANGCI_LINT_SUM_FILE)" >&2; \
		echo "record the exact Sum from: go mod download -json github.com/golangci/golangci-lint/v2@v2.8.0" >&2; \
		exit 1; \
	fi; \
	expected=$$(tr -d '\r\n' < "$(PAXEER_GOLANGCI_LINT_SUM_FILE)"); \
	case "$$expected" in h1:*) ;; *) echo "paxeer-tools-install: invalid h1 checksum record" >&2; exit 1 ;; esac; \
	mkdir -p "$(PAXEER_TOOLS_DIR)"; \
	metadata="$(PAXEER_TOOLS_DIR)/golangci-lint-v2.8.0.module.json"; \
	go mod download -json github.com/golangci/golangci-lint/v2@v2.8.0 > "$$metadata"; \
	actual=$$(awk -F '"' '$$2 == "Sum" { print $$4; exit }' "$$metadata"); \
	test -n "$$actual" || { echo "paxeer-tools-install: module download did not report a Sum" >&2; exit 1; }; \
	test "$$actual" = "$$expected" || { echo "paxeer-tools-install: golangci-lint module checksum mismatch" >&2; exit 1; }; \
	GOBIN="$(PAXEER_TOOLS_DIR)" go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.8.0

paxeer-manifest-install: workspace-inventory-check paxeer-npm-install paxeer-tools-install
	@set -eu; for manifest in $(PAXEER_LOCKED_RUST_MANIFESTS); do cargo fetch --manifest-path "$$manifest" --locked; done
	@set -eu; for directory in $(PAXEER_NESTED_GO_DIRS); do (cd "$$directory" && go mod download); done

paxeer-manifest-build: workspace-inventory-check paxeer-npm-build
	forge build --offline --root paxeer-network
	sh paxeer-network/loadtest/contracts/evm/setup.sh
	forge build --offline --root paxeer-network/loadtest/contracts/evm
	@set -eu; for manifest in $(PAXEER_LOCKED_RUST_MANIFESTS); do cargo build --manifest-path "$$manifest" --locked --offline; done
	@set -eu; for directory in $(PAXEER_NESTED_GO_DIRS); do (cd "$$directory" && GOPROXY=off go build ./...); done

paxeer-manifest-test: workspace-inventory-check paxeer-npm-static-test
	forge test --offline --root paxeer-network
	sh paxeer-network/loadtest/contracts/evm/setup.sh
	forge test --offline --root paxeer-network/loadtest/contracts/evm
	@set -eu; for manifest in $(PAXEER_LOCKED_RUST_MANIFESTS); do cargo test --manifest-path "$$manifest" --locked --offline; done
	@set -eu; for directory in $(PAXEER_NESTED_GO_DIRS); do (cd "$$directory" && GOPROXY=off go test ./...); done

paxeer-manifest-lint: workspace-inventory-check paxeer-npm-static-test
	forge fmt --check --root paxeer-network
	sh paxeer-network/loadtest/contracts/evm/setup.sh
	forge fmt --check --root paxeer-network/loadtest/contracts/evm
	@set -eu; for manifest in $(PAXEER_LOCKED_RUST_MANIFESTS); do cargo clippy --manifest-path "$$manifest" --locked --offline --all-targets -- -D warnings; done
	@test -z "$$(cd paxeer-network && gofmt -l .)"
	$(PAXEER_MAKE) GOLANGCI_LINT=$(PAXEER_GOLANGCI_LINT) lint
	@set -eu; for directory in $(PAXEER_NESTED_GO_DIRS); do (cd "$$directory" && test -z "$$(gofmt -l .)" && GOPROXY=off go vet ./... && go mod verify); done

paxeer-docs-install:
	npm --prefix paxeer-network/paxeer-docs ci --ignore-scripts --no-audit --no-fund

paxeer-docs-build:
	npm --prefix paxeer-network/paxeer-docs run build

paxeer-docs-static-test:
	npm --prefix paxeer-network/paxeer-docs run test:static

developer-dashboard-install:
	node tools/ci/developer-dashboard-lock.mjs
	$(MAKE) human-js-install
	@test ! -e platform/hosted/dashboard/web/node_modules || \
		test "$$(readlink platform/hosted/dashboard/web/node_modules)" = "../../../../human/apps/web/node_modules"
	@test -e platform/hosted/dashboard/web/node_modules || \
		ln -s ../../../../human/apps/web/node_modules platform/hosted/dashboard/web/node_modules

developer-dashboard-build:
	npm --prefix human/apps/web run build --workspace @layerx/ui
	npm --prefix platform/hosted/dashboard/web run build

developer-dashboard-static-test:
	npm --prefix platform/hosted/dashboard/web run test:static

specgen-build:
	cd spec/specgen && go build ./...

specgen-test:
	cd spec/specgen && go test ./...

specgen-lint:
	cd spec/specgen && go vet ./...
	cd spec/specgen && go run . -check

core-test-all: test test-kernel test-module-ctx test-dispatch test-receipts \
	test-state-root test-ledger-accounts test-ledger-transfer test-ledger-set \
	test-ledger-send test-ledger-receive test-ledger-receipt test-asset-registry \
	test-asset-balance test-asset-transfer test-asset-deposit test-asset-withdraw \
	test-asset-reserve test-escrow-open test-escrow-capture test-escrow-timeout \
	test-escrow-dispute test-escrow-invariants test-budget-create test-budget-period \
	test-budget-spend test-budget-delegate test-budget-revoke test-stream-open \
	test-stream-accrual test-stream-meter test-stream-settle test-stream-lifecycle \
	test-wave-8 test-wave-9 test-wave-10 test-wave-11 test-wave-12 test-paxeer \
	test-paxeer-bond test-bridge-deposit test-bridge-withdraw test-emergency-exit \
	test-reserve test-gateway test-gateway-send test-gateway-receive \
	test-receipt-offline test-layerxd test-tools test-genesis test-genesis-import \
	test-genesis-reconcile test-legacy-readonly test-shadow \
	test-replay-golden-local test-contracts qualify-faults qualify-arith \
	reproducible scan-consensus test-sanitizers test-crypto-sanitizers

core-qualification-environment: test-replay-golden qualify-replay qualify-fuzz

workspace-install:
	$(MAKE) workspace-node-preflight
	cargo fetch --manifest-path agent/Cargo.toml --locked
	cargo fetch --manifest-path human/Cargo.toml --locked
	cargo fetch --manifest-path platform/Cargo.toml --locked
	cd programs && cargo fetch --locked
	cargo fetch --manifest-path interop/Cargo.toml --locked
	$(MAKE) platform-js-install programs-js-install
	$(MAKE) developer-dashboard-install paxeer-manifest-install platform-dependencies-install
	cd paxeer-network && go mod download
	cd spec/specgen && go mod download

workspace-build: build agent-build human-build platform-build-all programs-build interop-build \
	paxeer-build paxeer-manifest-build developer-dashboard-build specgen-build
	forge build --offline

workspace-test: core-test-all agent-test human-test platform-test platform-verify-sdks \
	platform-test-tooling platform-test-middleware platform-test-reference-apps \
	platform-test-docs programs-test interop-test paxeer-test \
	paxeer-manifest-test developer-dashboard-static-test specgen-test
	forge test --offline

workspace-lint: agent-lint human-lint platform-lint programs-lint interop-lint \
	paxeer-manifest-lint developer-dashboard-static-test specgen-lint

workspace-ci: public-audit workspace-inventory-check workspace-build workspace-test workspace-lint

hpx-public-check:
	@set -eu; \
	tmp=$$(mktemp -d); trap 'rm -rf "$$tmp"' EXIT HUP INT TERM; \
	curl -fsS "$(HPX_ORIGIN)/healthz" | jq -e '.ok == true and .chain_id == "hyperpax_125-1"' >/dev/null; \
	curl -fsS "$(HPX_ORIGIN)/checksums.txt" -o "$$tmp/checksums.txt"; \
	curl -fsS "$(HPX_ORIGIN)/chain-info.json" -o "$$tmp/chain-info.json"; \
	want=$$(awk '$$2 == "chain-info.json" { print $$1; exit }' "$$tmp/checksums.txt"); \
	[ -n "$$want" ]; \
	[ "$$(sha256sum "$$tmp/chain-info.json" | awk '{print $$1}')" = "$$want" ]; \
	jq -e '.chain_id == "hyperpax_125-1" and (.paxd_sha256 | length == 64)' "$$tmp/chain-info.json" >/dev/null; \
	for path in get-hpx.sh hpx paxd genesis.json lib/libwasmvm.x86_64.so lib/libwasmvm.aarch64.so lib/libwasmvm152.x86_64.so lib/libwasmvm152.aarch64.so lib/libwasmvm155.x86_64.so lib/libwasmvm155.aarch64.so config/fullnode/config.toml config/fullnode/app.toml config/validator/config.toml config/validator/app.toml api/peers api/nodes api/myip api/statesync; do \
		curl -fsSI "$(HPX_ORIGIN)/$$path" >/dev/null; \
	done; \
	[ "$$(curl -sS -o /dev/null -w '%{http_code}' "$(HPX_ORIGIN)/")" = 404 ]; \
	[ "$$(curl -sS -o /dev/null -w '%{http_code}' "$(HPX_ORIGIN)/not-a-public-artifact")" = 404 ]

monorepo-ci: workspace-ci

-include $(LIB_OBJECTS:.o=.d) $(TEST_LIB_OBJECTS:.o=.d)

include tools/build/sanitizers.mk

include platform/Makefile.inc

.PHONY: interop-test-ramps interop-test-ramps-sandbox

interop-test-ramps:
	cargo test --locked --manifest-path platform/Cargo.toml -p layerx-ramp-toolkit
	cargo build --locked --manifest-path platform/Cargo.toml -p layerx-reference-ramp

interop-test-ramps-sandbox:
	sh platform/ramps/sandbox-journey.sh

INTEROP_CARGO ?= cargo
INTEROP_MANIFEST := interop/Cargo.toml

.PHONY: interop-build interop-test interop-lint interop-test-x402 interop-test-mandates interop-test-migration interop-test-migration-testnets interop-test-ucp interop-test-portable interop-test-visa-tap

interop-build:
	$(INTEROP_CARGO) build --manifest-path $(INTEROP_MANIFEST) --locked --workspace

interop-test:
	$(INTEROP_CARGO) test --manifest-path $(INTEROP_MANIFEST) --locked --workspace

interop-test-x402:
	$(INTEROP_CARGO) test --manifest-path $(INTEROP_MANIFEST) --locked -p layerx-x402

interop-test-mandates:
	$(INTEROP_CARGO) test --manifest-path $(INTEROP_MANIFEST) --locked -p layerx-ap2

interop-test-migration:
	$(INTEROP_CARGO) test --manifest-path $(INTEROP_MANIFEST) --locked -p layerx-migrate

interop-test-migration-testnets:
	$(INTEROP_CARGO) test --manifest-path $(INTEROP_MANIFEST) --locked -p layerx-migrate --test testnets -- --ignored --nocapture

interop-test-ucp:
	$(INTEROP_CARGO) test --manifest-path $(INTEROP_MANIFEST) --locked -p layerx-ucp

interop-test-portable:
	$(INTEROP_CARGO) test --manifest-path $(INTEROP_MANIFEST) --locked -p layerx-portable
	$(INTEROP_CARGO) test --manifest-path $(INTEROP_MANIFEST) --locked -p layerx-portable --test receipt_vectors
	$(INTEROP_CARGO) test --manifest-path $(INTEROP_MANIFEST) --locked -p layerx-portable --test external_verification
	$(INTEROP_CARGO) test --manifest-path $(INTEROP_MANIFEST) --locked -p layerx-portable --test independent_verifier

interop-test-visa-tap:
	$(INTEROP_CARGO) test --manifest-path $(INTEROP_MANIFEST) --locked -p layerx-visa-tap

interop-lint:
	$(INTEROP_CARGO) clippy --manifest-path $(INTEROP_MANIFEST) --locked --workspace --all-targets -- -D warnings
	sh interop/tools/dependency-policy.sh
	cargo deny --manifest-path $(INTEROP_MANIFEST) check advisories bans sources

PROGRAMS_CARGO ?= cargo
PROGRAMS_RUNTIME_LIB := programs/target/debug/liblayerx_programs_runtime.a

.PHONY: programs-build programs-lint programs-test programs-core-test programs-protocol-regression programs-adversarial programs-module-boundaries programs-abi-drift programs-porting-v2-references \
	programs-fuzz-smoke programs-differential programs-interpreter-conformance programs-bench programs-interpreter-bench programs-quickstart programs-sdk-rust programs-sdk-c programs-sdk-assemblyscript

.PHONY: programs-js-install
programs-js-install:
	npm --prefix programs/sdk/assemblyscript ci --ignore-scripts --no-audit --no-fund
	npm --prefix programs/sdk/assemblyscript/examples/paid-counter ci --ignore-scripts --no-audit --no-fund

$(PROGRAMS_RUNTIME_LIB):
	cd programs && $(PROGRAMS_CARGO) build --locked --workspace --features layerx-programs-runtime/host-ffi

programs-build:
	cd programs && $(PROGRAMS_CARGO) build --locked --workspace --features layerx-programs-runtime/host-ffi

programs-lint: programs-module-boundaries
	cd programs && $(PROGRAMS_CARGO) clippy --locked --workspace --all-targets --features layerx-programs-runtime/host-ffi -- -D warnings
	sh programs/tools/dependency-policy.sh
	cd programs && $(PROGRAMS_CARGO) deny check advisories bans sources

programs-module-boundaries:
	sh programs/tools/runtime-module-boundaries.sh

programs-abi-drift:
	programs/tools/check-abi-drift.sh
	cd programs && $(PROGRAMS_CARGO) test --locked -p layerx-programs-runtime --test abi_linker

$(BUILD_DIR)/tests/programs_registration: tests/programs/test_registration.c \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(PROGRAMS_RUNTIME_LIB) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -ldl -lm -o $@

$(BUILD_DIR)/tests/programs_lifecycle: tests/programs/test_lifecycle.c \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(PROGRAMS_RUNTIME_LIB) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -ldl -lm -o $@

$(BUILD_DIR)/tests/programs_monetary_law: tests/programs/test_monetary_law.c \
		$(TEST_LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) -DLXP_TESTING $(CFLAGS) $< $(TEST_LIBRARY) $(PROGRAMS_RUNTIME_LIB) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -ldl -lm -o $@

$(BUILD_DIR)/tests/programs_call_activity: tests/programs/test_call_activity.c \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(PROGRAMS_RUNTIME_LIB) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -ldl -lm -o $@

$(BUILD_DIR)/tests/programs_occupancy_batch: tests/programs/test_occupancy_batch.c \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(PROGRAMS_RUNTIME_LIB) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -ldl -lm -o $@

$(BUILD_DIR)/tests/programs_metering_schedule: tests/programs/test_metering_schedule.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -ldl -lm -o $@

$(BUILD_DIR)/tests/programs_fee_governance: tests/programs/test_fee_governance.c \
		$(LIBRARY)
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -ldl -lm -o $@

$(BUILD_DIR)/tests/programs_accounts: tests/programs/test_accounts.c \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(PROGRAMS_RUNTIME_LIB) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -ldl -lm -o $@

$(BUILD_DIR)/tests/programs_winddown: tests/programs/test_winddown.c \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LIBRARY) $(PROGRAMS_RUNTIME_LIB) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -ldl -lm -o $@

programs-core-test: $(BUILD_DIR)/tests/programs_registration \
		$(BUILD_DIR)/tests/programs_lifecycle \
		$(BUILD_DIR)/tests/programs_monetary_law \
		$(BUILD_DIR)/tests/programs_call_activity \
		$(BUILD_DIR)/tests/programs_occupancy_batch \
		$(BUILD_DIR)/tests/programs_metering_schedule \
		$(BUILD_DIR)/tests/programs_fee_governance \
		$(BUILD_DIR)/tests/programs_accounts \
		$(BUILD_DIR)/tests/programs_winddown
	$(RUN_PREFIX) $(BUILD_DIR)/tests/programs_registration
	$(RUN_PREFIX) $(BUILD_DIR)/tests/programs_lifecycle
	$(RUN_PREFIX) $(BUILD_DIR)/tests/programs_monetary_law
	$(RUN_PREFIX) $(BUILD_DIR)/tests/programs_call_activity
	$(RUN_PREFIX) $(BUILD_DIR)/tests/programs_occupancy_batch
	$(RUN_PREFIX) $(BUILD_DIR)/tests/programs_metering_schedule
	$(RUN_PREFIX) $(BUILD_DIR)/tests/programs_fee_governance
	$(RUN_PREFIX) $(BUILD_DIR)/tests/programs_accounts
	$(RUN_PREFIX) $(BUILD_DIR)/tests/programs_winddown

programs-protocol-regression: test-kernel test-module-ctx test-dispatch \
		test-receipts test-state-root test-snapshot test-replay-golden-local

programs-fuzz-smoke:
	cd programs && $(PROGRAMS_CARGO) run --locked -p layerx-programs-fuzz --bin programs-fuzz -- validation fuzz/corpus/validation
	cd programs && $(PROGRAMS_CARGO) run --locked -p layerx-programs-fuzz --bin programs-fuzz -- instantiation fuzz/corpus/instantiation
	cd programs && $(PROGRAMS_CARGO) run --locked -p layerx-programs-fuzz --bin programs-fuzz -- execution fuzz/corpus/execution

programs-adversarial:
	cd programs && $(PROGRAMS_CARGO) test --locked -p layerx-programs-runtime --test isolation --test composition


$(BUILD_DIR)/tests/programs_parallel_differential: programs/tests/differential/parallel.c \
		tests/programs/test_call_activity.c \
		cmd/layerxd/lxp_daemon_batch_wal.c \
		$(LIBRARY) $(PROGRAMS_RUNTIME_LIB) | programs-build
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) programs/tests/differential/parallel.c \
		cmd/layerxd/lxp_daemon_batch_wal.c $(LIBRARY) $(PROGRAMS_RUNTIME_LIB) \
		$(EXTRA_LDFLAGS) -lcrypto -pthread -ldl -lm -o $@

programs-differential: $(BUILD_DIR)/tests/programs_parallel_differential
	cd programs && $(PROGRAMS_CARGO) test --locked -p layerx-programs-runtime --test replay --test determinism
	$(RUN_PREFIX) $(BUILD_DIR)/tests/programs_parallel_differential

programs-interpreter-conformance:
	cd programs && $(PROGRAMS_CARGO) build --locked --release --target wasm32-unknown-unknown -p layerx-programs-interpreter
	cd programs && LAYERX_INTERPRETER_WASM=$$(pwd)/target/wasm32-unknown-unknown/release/layerx_programs_interpreter.wasm $(PROGRAMS_CARGO) test --locked -p layerx-programs-runtime --test interpreter_program

programs-bench: programs-interpreter-bench

programs-interpreter-bench:
	cd programs && $(PROGRAMS_CARGO) build --locked --release --target wasm32-unknown-unknown -p layerx-programs-interpreter
	cd programs && $(PROGRAMS_CARGO) build --locked --release --target wasm32-unknown-unknown -p layerx-interpreter-compiled-equivalent
	cd programs && LAYERX_INTERPRETER_WASM=$$(pwd)/target/wasm32-unknown-unknown/release/layerx_programs_interpreter.wasm LAYERX_COMPILED_EQUIVALENT_WASM=$$(pwd)/target/wasm32-unknown-unknown/release/layerx_interpreter_compiled_equivalent.wasm $(PROGRAMS_CARGO) bench --locked -p layerx-programs-runtime --bench interpreter

programs-test: programs-module-boundaries programs-abi-drift programs-core-test programs-protocol-regression programs-adversarial programs-fuzz-smoke programs-differential programs-interpreter-conformance programs-sdk-rust programs-sdk-c programs-sdk-assemblyscript programs-porting-v2-references
	cd programs && LAYERX_INTERPRETER_WASM=$$(pwd)/target/wasm32-unknown-unknown/release/layerx_programs_interpreter.wasm $(PROGRAMS_CARGO) test --locked --workspace

programs-porting-v2-references: $(BUILD_DIR)/tests/programs_call_activity
	$(PROGRAMS_CARGO) build --locked --manifest-path programs/porting/evm/reference-v2/Cargo.toml --target wasm32-unknown-unknown --release
	$(PROGRAMS_CARGO) build --locked --manifest-path programs/porting/solana/reference-v2/Cargo.toml --target wasm32-unknown-unknown --release
	$(PROGRAMS_CARGO) build --locked --manifest-path programs/porting/cosmwasm/reference-v2/Cargo.toml --target wasm32-unknown-unknown --release
	cd programs && $(PROGRAMS_CARGO) run --locked --quiet -p layerx-program-lint --bin layerx-program-lint -- --abi-version 2 porting/evm/reference-v2 porting/evm/reference-v2/target/wasm32-unknown-unknown/release/layerx_evm_context_reference.wasm
	cd programs && $(PROGRAMS_CARGO) run --locked --quiet -p layerx-program-lint --bin layerx-program-lint -- --abi-version 2 porting/solana/reference-v2 porting/solana/reference-v2/target/wasm32-unknown-unknown/release/layerx_anchor_context_reference.wasm
	cd programs && $(PROGRAMS_CARGO) run --locked --quiet -p layerx-program-lint --bin layerx-program-lint -- --abi-version 2 porting/cosmwasm/reference-v2 porting/cosmwasm/reference-v2/target/wasm32-unknown-unknown/release/layerx_cosmwasm_context_reference.wasm
	$(RUN_PREFIX) $(BUILD_DIR)/tests/programs_call_activity \
		programs/porting/evm/reference-v2/target/wasm32-unknown-unknown/release/layerx_evm_context_reference.wasm \
		programs/porting/solana/reference-v2/target/wasm32-unknown-unknown/release/layerx_anchor_context_reference.wasm \
		programs/porting/cosmwasm/reference-v2/target/wasm32-unknown-unknown/release/layerx_cosmwasm_context_reference.wasm

programs-sdk-c:
	STRICT=1 sh programs/sdk/c/examples/paid-counter/build.sh all
	@mkdir -p $(BUILD_DIR)/tests
	$(CC) $(CPPFLAGS) $(CFLAGS) -I programs/sdk/c/include \
		programs/sdk/c/tests/capability_parity.c \
		programs/sdk/c/src/amount.c programs/sdk/c/src/bytes.c \
		programs/sdk/c/src/capability.c \
		-o $(BUILD_DIR)/tests/programs_sdk_c_capability_parity
	$(RUN_PREFIX) $(BUILD_DIR)/tests/programs_sdk_c_capability_parity

programs-sdk-rust:
	sh programs/sdk/rust/quickstart/build.sh all
	sh programs/sdk/rust/response-fixture/build.sh
	sh programs/sdk/rust/examples/escrow/build.sh
	sh programs/sdk/rust/examples/vault/build.sh

programs-sdk-assemblyscript:
	npm --prefix programs/sdk/assemblyscript run test:source
	cd programs/sdk/assemblyscript/examples/paid-counter && npm run build && npm run lint

programs-quickstart:
	sh programs/sdk/rust/quickstart/build.sh all
