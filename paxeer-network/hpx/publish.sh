#!/usr/bin/env bash
# =============================================================================
# publish.sh — assemble the HyperPax node install package into the artifact
# root served by hpx-registry (default /srv/hpx/artifacts).
#
# Run this once now, and again any time you rebuild paxd or change the chain
# config. It is the ONLY step that touches the source chain files; everything
# downstream (installer, CLI, nodes) just pulls from the served artifacts.
#
#   sudo bash publish.sh
# =============================================================================
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC_BIN="${SRC_BIN:-/root/project-Quorum/paxeer-v3-matrix-release/build/paxd}"
SRC_CFG="${SRC_CFG:-/root/.paxeer/config}"
WASM_ROOT="${WASM_ROOT:-/root/project-Quorum/paxeer-v3-matrix-release}"
OUT="${HPX_ARTIFACTS_DIR:-/srv/hpx/artifacts}"

CHAIN_ID="${CHAIN_ID:-hyperpax_125-1}"
EVM_CHAIN_ID="${EVM_CHAIN_ID:-125}"
P2P_PORT="${P2P_PORT:-26656}"
RPC_PORT="${RPC_PORT:-26657}"
# The genesis validator is the permanent seed every node dials first.
SEED_PEER="${SEED_PEER:-e9c56cbadc4a96b67f69dcaaa7b4691851e945ca@31.220.74.140:26656}"

say() { printf "\033[0;36m[publish]\033[0m %s\n" "$*"; }
die() { printf "\033[0;31m[publish] ERROR:\033[0m %s\n" "$*" >&2; exit 1; }

[ -f "$SRC_BIN" ] || die "paxd binary not found: $SRC_BIN"
[ -f "$SRC_CFG/genesis.json" ] || die "genesis not found: $SRC_CFG/genesis.json"
[ -f "$SRC_CFG/config.toml" ] || die "config.toml not found: $SRC_CFG/config.toml"
[ -f "$SRC_CFG/app.toml" ] || die "app.toml not found: $SRC_CFG/app.toml"

mkdir -p "$OUT/lib" "$OUT/config/fullnode" "$OUT/config/validator"

# ── binary + checksum ────────────────────────────────────────────────────────
say "copying paxd binary"
install -m 0755 "$SRC_BIN" "$OUT/paxd.new" && mv -f "$OUT/paxd.new" "$OUT/paxd"
SHA=$(sha256sum "$OUT/paxd" | awk '{print $1}')
echo "$SHA" > "$OUT/paxd.sha256"
say "paxd sha256 = $SHA"

VERSION=$(LD_LIBRARY_PATH="$WASM_ROOT/wasm-runtime/internal/api" "$SRC_BIN" version 2>/dev/null || echo "v6.1.6-2218-g4f5889e00")
say "paxd version = $VERSION"

# ── libwasmvm shared objects (CGO runtime dep) ───────────────────────────────
say "copying libwasmvm shared objects"
copied=0
for so in \
  "$WASM_ROOT/wasm-runtime/internal/api/libwasmvm.x86_64.so" \
  "$WASM_ROOT/wasm-runtime/internal/api/libwasmvm.aarch64.so" \
  "$WASM_ROOT/wasm/x/wasm/artifacts/v152/api/libwasmvm152.x86_64.so" \
  "$WASM_ROOT/wasm/x/wasm/artifacts/v152/api/libwasmvm152.aarch64.so" \
  "$WASM_ROOT/wasm/x/wasm/artifacts/v155/api/libwasmvm155.x86_64.so" \
  "$WASM_ROOT/wasm/x/wasm/artifacts/v155/api/libwasmvm155.aarch64.so" ; do
  if [ -f "$so" ]; then install -m 0644 "$so" "$OUT/lib/"; copied=$((copied+1)); fi
done
[ "$copied" -ge 2 ] || die "expected libwasmvm .so files under $WASM_ROOT, found $copied"
say "copied $copied libwasmvm objects"

# ── genesis ──────────────────────────────────────────────────────────────────
say "copying genesis.json"
install -m 0644 "$SRC_CFG/genesis.json" "$OUT/genesis.json"

# ── config variants ──────────────────────────────────────────────────────────
# Produce fullnode + validator config.toml from the live config (CometBFT v1 /
# Pax layout — hyphenated keys). We set the node `mode` and enable pex; moniker,
# external-address and persistent/bootstrap peers are filled per host by the
# hpx CLI, so we just normalise them to a clean default here.
make_config() {
  local mode="$1" dst="$2"
  sed -E \
    -e 's|^moniker = .*|moniker = "hpx-node"|' \
    -e "s|^mode = .*|mode = \"$mode\"|" \
    -e 's|^external-address = .*|external-address = ""|' \
    -e 's|^persistent-peers = .*|persistent-peers = ""|' \
    -e 's|^bootstrap-peers = .*|bootstrap-peers = ""|' \
    -e 's|^pex = .*|pex = true|' \
    "$SRC_CFG/config.toml" > "$dst"
}
say "generating fullnode config (mode=full)"
make_config "full" "$OUT/config/fullnode/config.toml"
install -m 0644 "$SRC_CFG/app.toml" "$OUT/config/fullnode/app.toml"

say "generating validator config (mode=validator)"
make_config "validator" "$OUT/config/validator/config.toml"
install -m 0644 "$SRC_CFG/app.toml" "$OUT/config/validator/app.toml"

# ── CLI + scripts ────────────────────────────────────────────────────────────
say "copying hpx CLI + install scripts"
for f in hpx get-hpx.sh uninstall.sh; do
  [ -f "$REPO/$f" ] && install -m 0755 "$REPO/$f" "$OUT/$f" || say "  (skip missing $f)"
done

# ── chain-info manifest ──────────────────────────────────────────────────────
say "writing chain-info.json"
cat > "$OUT/chain-info.json" <<JSON
{
  "chain_id": "$CHAIN_ID",
  "evm_chain_id": $EVM_CHAIN_ID,
  "paxd_version": "$VERSION",
  "paxd_sha256": "$SHA",
  "p2p_port": $P2P_PORT,
  "rpc_port": $RPC_PORT,
  "seeds": ["$SEED_PEER"],
  "published_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
JSON

say "done. artifact tree:"
( cd "$OUT" && find . -maxdepth 2 -type f | sort | sed 's/^/    /' )
