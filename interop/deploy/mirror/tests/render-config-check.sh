#!/usr/bin/env bash
# Offline check of interop/deploy/mirror/render-config.py: a configuration
# rendered from deployment-shaped inputs is accepted by the publisher's own
# configuration loader, the signer handles and socket fall back to the
# co-located layerx-mirror-signer container unless they are overridden, and a
# configuration outside the publisher's bounds is refused by the publisher. No
# network and no cluster are involved.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../../../.." && pwd)
RENDERER="$SCRIPT_DIR/../render-config.py"
STATUS_PORT=${LAYERX_MIRROR_CHECK_PORT:-19491}

fail() { printf 'render-config-check: error: %s\n' "$*" >&2; exit 1; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
chmod 0700 "$WORK"

: "${CARGO_TARGET_DIR:=$REPO_ROOT/interop/target}"
export CARGO_TARGET_DIR
cargo build --locked --manifest-path "$REPO_ROOT/interop/Cargo.toml" \
    --package layerx-mirror --bin layerx-mirror-publisher >&2
PUBLISHER="$CARGO_TARGET_DIR/debug/layerx-mirror-publisher"
[ -x "$PUBLISHER" ] || fail "layerx-mirror-publisher was not built at $PUBLISHER"

openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj '/O=LayerX mirror check/CN=mirror-rpc' \
    -keyout "$WORK/rpc.key" -out "$WORK/rpc.pem" 2>/dev/null
openssl x509 -in "$WORK/rpc.pem" -outform DER -out "$WORK/rpc.der"
for name in ethereum-a ethereum-b solana-a solana-b; do
    (umask 077; openssl rand -hex 32 > "$WORK/$name.token")
done

openssl genpkey -algorithm ed25519 -out "$WORK/solana-publisher.key" 2>/dev/null
SOLANA_PUBLIC=$(openssl pkey -in "$WORK/solana-publisher.key" -pubout -outform DER 2>/dev/null \
    | tail -c 32 | python3 -c '
import sys
alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
raw = sys.stdin.buffer.read()
number = int.from_bytes(raw, "big")
text = ""
while number:
    number, remainder = divmod(number, 58)
    text = alphabet[remainder] + text
print("1" * (len(raw) - len(raw.lstrip(b"\x00"))) + text)
')
[ -n "$SOLANA_PUBLIC" ] || fail "the Ed25519 publisher public key could not be encoded"

# The secp256k1 generator point: a real curve point, so the publisher's signer
# configuration check verifies a genuine SEC1 key rather than a placeholder.
ETHEREUM_PUBLIC=0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798
PROGRAM=$(python3 -c '
import hashlib
alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
raw = hashlib.sha256(b"layerx-mirror-check/program").digest()
number = int.from_bytes(raw, "big")
text = ""
while number:
    number, remainder = divmod(number, 58)
    text = alphabet[remainder] + text
print(text)
')

render() {
    python3 "$RENDERER" \
        --output "$1" \
        --state-directory "$WORK/state" \
        --first-batch-number 1 \
        --status-listen "127.0.0.1:$STATUS_PORT" \
        --lni-socket /run/layerx/node/layerxd.lni.sock \
        --network-id 1 \
        --protocol-version 3 \
        --ethereum-endpoint "https://ethereum-a.invalid:9443/rpc,ethereum-a,$WORK/rpc.der,$WORK/ethereum-a.token" \
        --ethereum-endpoint "https://ethereum-b.invalid:9443/rpc,ethereum-b,$WORK/rpc.der,$WORK/ethereum-b.token" \
        --ethereum-chain-id 125 \
        --ethereum-genesis-hash "$(printf 'genesis' | sha256sum | cut -d ' ' -f 1)" \
        --ethereum-archive-contract 00000000000000000000000000000000000000ff \
        --ethereum-archive-code-hash "$(printf 'code' | sha256sum | cut -d ' ' -f 1)" \
        --ethereum-signer-public-key "$ETHEREUM_PUBLIC" \
        --solana-endpoint "https://solana-a.invalid:443/rpc,solana-a,$WORK/rpc.der,$WORK/solana-a.token" \
        --solana-endpoint "https://solana-b.invalid:443/rpc,solana-b,$WORK/rpc.der,$WORK/solana-b.token" \
        --solana-genesis-hash "$PROGRAM" \
        --solana-archive-program "$PROGRAM" \
        --solana-upgradeable-loader BPFLoaderUpgradeab1e11111111111111111111111 \
        --solana-program-data-account "$PROGRAM" \
        --solana-program-code-hash "$(printf 'elf' | sha256sum | cut -d ' ' -f 1)" \
        --solana-signer-public-key "$SOLANA_PUBLIC" \
        "${@:2}"
}

mkdir -p "$WORK/state"
render "$WORK/config.json"
python3 - "$WORK/config.json" <<'PYDEFAULTS' || fail "the rendered configuration does not reach the co-located signer"
import json
import sys
config = json.load(open(sys.argv[1]))
ethereum = config["ethereum"]["signer"]
solana = config["solana"]["signer"]
socket = {"kind": "uds", "socket": "/run/mirror-signer/signer.sock"}
assert ethereum["transport"] == socket, ethereum["transport"]
assert solana["transport"] == socket, solana["transport"]
assert ethereum["key_handle"] == "mirror/ethereum/beta", ethereum["key_handle"]
assert solana["key_handle"] == "mirror/solana/beta", solana["key_handle"]
PYDEFAULTS

status=0
timeout 10 "$PUBLISHER" "$WORK/config.json" > "$WORK/publisher.log" 2>&1 || status=$?
if [ "$status" -ne 124 ]; then
    cat "$WORK/publisher.log" >&2
    fail "the publisher did not accept the rendered configuration (exit $status)"
fi

python3 - "$WORK/config.json" "$WORK/out-of-bounds.json" <<'PY'
import json
import sys
config = json.load(open(sys.argv[1]))
config["poll_interval_ms"] = 1
json.dump(config, open(sys.argv[2], "w"))
PY
status=0
timeout 10 "$PUBLISHER" "$WORK/out-of-bounds.json" > "$WORK/refused.log" 2>&1 || status=$?
[ "$status" -eq 1 ] || fail "the publisher accepted an out-of-bounds poll interval (exit $status)"
grep -q 'refused startup' "$WORK/refused.log" \
    || fail "the publisher did not report a configuration refusal"

status=0
render_output=$(render "$WORK/never.json" 2>&1) || status=$?
[ "$status" -eq 0 ] || fail "the renderer refused a valid deployment: $render_output"
status=0
render_output=$(python3 "$RENDERER" --output "$WORK/never.json" --state-directory relative \
    --first-batch-number 1 --status-listen 127.0.0.1:1 --lni-socket /run/s --network-id 1 \
    --protocol-version 3 --ethereum-chain-id 125 --ethereum-genesis-hash 00 \
    --ethereum-archive-contract 00 --ethereum-archive-code-hash 00 \
    --ethereum-signer-key-handle owner/ethereum --ethereum-signer-public-key "$ETHEREUM_PUBLIC" \
    --ethereum-signer-socket /s --solana-genesis-hash "$PROGRAM" \
    --solana-archive-program "$PROGRAM" --solana-upgradeable-loader "$PROGRAM" \
    --solana-program-data-account "$PROGRAM" --solana-program-code-hash 00 \
    --solana-signer-key-handle owner/solana --solana-signer-public-key "$SOLANA_PUBLIC" \
    --solana-signer-socket /s 2>&1) || status=$?
[ "$status" -ne 0 ] || fail "the renderer accepted a relative state directory"

status=0
render_output=$(render "$WORK/shared-handle.json" --ethereum-signer-key-handle shared \
    --solana-signer-key-handle shared 2>&1) || status=$?
[ "$status" -ne 0 ] || fail "the renderer accepted one key handle for both publisher keys"

render "$WORK/external.json" --ethereum-signer-key-handle owner/ethereum \
    --ethereum-signer-socket /run/signers/ethereum.sock \
    --solana-signer-key-handle owner/solana --solana-signer-socket /run/signers/solana.sock
python3 - "$WORK/external.json" <<'PYEXTERNAL' || fail "an external signer override is not carried into the configuration"
import json
import sys
config = json.load(open(sys.argv[1]))
ethereum = config["ethereum"]["signer"]
solana = config["solana"]["signer"]
assert ethereum["key_handle"] == "owner/ethereum", ethereum["key_handle"]
assert ethereum["transport"]["socket"] == "/run/signers/ethereum.sock", ethereum["transport"]
assert solana["key_handle"] == "owner/solana", solana["key_handle"]
assert solana["transport"]["socket"] == "/run/signers/solana.sock", solana["transport"]
PYEXTERNAL

printf 'render-config-check: the rendered mirror configuration is accepted by the publisher loader\n' >&2
