#!/usr/bin/env bash
set -euo pipefail
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4} MAKEFLAGS=${MAKEFLAGS:--j4}
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
[[ $(id -u) == 0 ]]
build_dir=${1:-build}
if [[ $build_dir != /* ]]; then build_dir="$root/$build_dir"; fi
native_bin=${LAYERX_TEST_NATIVE_BIN_DIR:-"$build_dir/bin"}
sequencer_binary="$native_bin/layerxd"
if [[ ${2:-} == --maintenance-crash ]]; then
    sequencer_binary="$build_dir/tests/lxp_test_maintenance_crash"
fi
work=$(mktemp -d "${LAYERX_TEST_ADMISSION_LOG_DIR:-/tmp}/lxp-program-admission-XXXXXX")
runtime=$(mktemp -d /tmp/lxp-program-admission-run-XXXXXX)
replica_pid= sequencer_pid= settlement_pid=
cleanup() {
    result=$?
    for pid in "$sequencer_pid" "$replica_pid" "$settlement_pid"; do
        if [[ -n "$pid" ]]; then kill "$pid" 2>/dev/null || true; wait "$pid" || true; fi
    done
    rm -rf "$runtime"
    if [[ "$result" == 0 && -z ${LAYERX_TEST_ADMISSION_LOG_DIR:-} ]]; then rm -rf "$work"; else printf 'native admission evidence: %s\n' "$work" >&2; fi
}
trap cleanup EXIT
chmod 0755 "$work"
python3 - "$work" <<'PY'
import os, pathlib, socket, sys
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
sys.path.insert(0, "tests/support")
from lxgb_metadata import metadata
root = pathlib.Path(sys.argv[1])
for name, value in [('sequencer', 0x22), ('treasury', 0x11)]:
    path = root / name
    path.write_bytes(bytes([value]) * 32)
    path.chmod(0o600)
ports = []
sockets = []
for _ in range(3):
    sock = socket.socket()
    sock.bind(('127.0.0.1', 0))
    sockets.append(sock)
    ports.append(str(sock.getsockname()[1]))
(root / 'ports').write_text(' '.join(ports) + '\n')
issuer = Ed25519PrivateKey.from_private_bytes(bytes([0x11]) * 32).public_key().public_bytes_raw()
(root / 'metadata').write_bytes(metadata(bytes.fromhex('b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898'), issuer, os.urandom(32)))
PY
read -r program_port replica_port rpc_port < "$work/ports"
mkfifo "$work/replica-ready"
exec {replica_ready_fd}<>"$work/replica-ready"
if [[ ${2:-} == --maintenance-crash ]]; then
    mkfifo "$work/apply-gate"
    exec {apply_gate_fd}<>"$work/apply-gate"
    export LXP_TEST_APPLY_GATE_FD="$apply_gate_fd" LXP_TEST_CRASH_BOUNDARY="$3" LXP_TEST_CRASH_OCCURRENCE="$4"
fi
bootstrap_extra=()
bootstrap_environment=(env)
custody_mode=0
case ${2:-} in
    --withdraw|--module-maintenance|--metered-allowance) custody_mode=1 ;;
esac
if [[ $custody_mode == 1 ]]; then
    bootstrap_extra+=(--custody-profile "$LAYERX_TEST_WITHDRAW_PROFILE" --settlement-env "$work/settlement.env")
    for name in LAYERX_NODE_PAXEER_CHAIN_ID LAYERX_NODE_SETTLEMENT_CONTRACT LAYERX_NODE_CHECKPOINT_REGISTRY LAYERX_NODE_PAXEER_RPC_ADDRESS LAYERX_NODE_PAXEER_RPC_PORT; do
        bootstrap_environment+=(-u "$name")
    done
else
    bootstrap_environment+=(LAYERX_NODE_PAXEER_CHAIN_ID=31337
        LAYERX_NODE_SETTLEMENT_CONTRACT=0x1111111111111111111111111111111111111111
        LAYERX_NODE_CHECKPOINT_REGISTRY=0x2222222222222222222222222222222222222222
        LAYERX_NODE_PAXEER_RPC_ADDRESS=127.0.0.1 LAYERX_NODE_PAXEER_RPC_PORT="$rpc_port")
fi
if [[ ${2:-} == --module-maintenance ]]; then
    for module in escrow budget stream service perps; do
        bootstrap_extra+=(--enable-module "$module")
    done
fi
"${bootstrap_environment[@]}" \
bash platform/hosted/node/bootstrap.sh --data-dir "$work/data" --run-dir "$runtime" \
    --network-id 77 --genesis-metadata "$work/metadata" --sequencer-key "$work/sequencer" --treasury-key "$work/treasury" \
    --lni-uid 4021 --lni-gid 4021 --program-port "$program_port" --replica-port "$replica_port" \
    --layerxd "$native_bin/layerxd" --genesis-build "$native_bin/layerx-genesis-build" "${bootstrap_extra[@]}" \
    > "$work/bootstrap.log" 2>&1
if [[ $custody_mode == 1 ]]; then
    "${LAYERX_TEST_PYTHON:-python3}" tests/daemon/withdraw-custody.py --register "$work" "$LAYERX_TEST_WITHDRAW_RPC"
    settlement_lines=$(bash platform/hosted/node/bootstrap.sh --check-settlement "$work/settlement.env")
    while IFS= read -r line; do export "$line"; done <<< "$settlement_lines"
fi
if [[ ${2:-} == --module-maintenance ]]; then
    python3 - "$work/data/identities.txt" <<'PYPROVIDER'
from pathlib import Path
import sys
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
public = Ed25519PrivateKey.from_private_bytes(bytes([0x33]) * 32).public_key().public_bytes_raw()
did = ('did:layerx:' + public.hex()).encode()
with Path(sys.argv[1]).open('a') as identities:
    identities.write(did.hex() + ':' + public.hex() + ':0\n')
for target in Path(sys.argv[1]).parents[1].glob('guarantor-*/identity/identities.txt'):
    target.write_bytes(Path(sys.argv[1]).read_bytes())
PYPROVIDER
fi
if [[ ${2:-} == --availability-batches ]]; then
    mkdir "$work/availability-output"
    chown 4021:4021 "$work/availability-output"
    python3 tests/daemon/availability-settlement.py "$work" "$build_dir/tests/lxp_test_daemon_finality_authority" > "$work/availability-settlement.log" 2>&1 &
    settlement_pid=$!
    for ((attempt=0; attempt<3000; attempt++)); do
        [[ ! -f "$work/availability-chain-ready.json" ]] || break
        kill -0 "$settlement_pid"
        sleep 0.1
    done
    read -r availability_bond availability_registry availability_port < <(python3 - "$work/availability-chain-ready.json" <<'PYCHAIN'
import json, sys
value = json.load(open(sys.argv[1]))
print(value['bond'], value['registry'], value['port'])
PYCHAIN
)
fi
(set -a; source "$work/data/replica.env"; export LAYERX_AUTHORITY_READY_FD="$replica_ready_fd"; exec "$native_bin/layerxd" --authority-replica "$work/data/replica.conf") > "$work/replica.log" 2>&1 &
replica_pid=$!
IFS= read -r -n 1 -t 20 replica_ready <&"$replica_ready_fd"
[[ "$replica_ready" == R ]]
(source platform/hosted/node/sequencer-env.sh; layerx_sequencer_environment "$work/data/sequencer.env"; if [[ ${2:-} == --availability-batches ]]; then export LAYERX_NODE_SETTLEMENT_CONTRACT="$availability_bond" LAYERX_NODE_CHECKPOINT_REGISTRY="$availability_registry" LAYERX_NODE_PAXEER_RPC_PORT="$availability_port"; fi; exec "$sequencer_binary" --serve "$work/data/sequencer.conf") > "$work/sequencer.log" 2>&1 &
sequencer_pid=$!
python3 - "$runtime/layerxd.lni.sock" "$sequencer_pid" <<'PYWAIT'
import os, socket, sys, time
for attempt in range(200):
    os.kill(int(sys.argv[2]), 0)
    try:
        with socket.socket(socket.AF_UNIX) as connection:
            connection.connect(sys.argv[1])
        break
    except OSError:
        time.sleep(0.1)
else:
    raise SystemExit("daemon did not accept LNI connections")
PYWAIT
if [[ ${2:-} == --module-maintenance || ${2:-} == --metered-allowance ]]; then
    client_name=${2#--}
    client_name=${client_name//-/_}
    cp "$build_dir/tests/lxp_test_$client_name" "$work/client"
    mkdir "$work/scenario"
    chown 4021:4021 "$work/scenario"
    scenario_state="$work/scenario"
    if [[ ${2:-} == --metered-allowance ]]; then scenario_state="$work/scenario/state"; fi
elif [[ ${2:-} == --grant-issuance ]]; then
    cp "$build_dir/tests/lxp_test_grant_issuance" "$work/client"
    mkdir "$work/grants"
    chown 4021:4021 "$work/grants"
else
    cp "$build_dir/tests/lxp_test_program_admission" "$work/client"
fi
chmod 0755 "$work/client"
if [[ ${2:-} == --owner-authority ]]; then
    export LAYERX_TEST_OWNER_AUTHORITY_SOCKET="$runtime/layerxd.lni.sock"
    "${LAYERX_TEST_PYTHON:-python3}" tests/daemon/post-lxip.py "$work" --prepare-only
    cp "$3" "$work/owner-authority-test"
    chmod 0755 "$work/owner-authority-test"
    LAYERX_TEST_OWNER_AUTHORITY_PUBLIC=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["public_key"])' "$work/human-evidence-input/owner-admission.json")
    LAYERX_TEST_OWNER_AUTHORITY_DID=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["did"])' "$work/human-evidence-input/owner-admission.json")
    LAYERX_TEST_OWNER_AUTHORITY_ASSET=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["asset"])' "$work/data/treasury.json")
    chown 4021:4021 "$work/human-owner" "$work/human-owner/owner.seed"
    export LAYERX_TEST_OWNER_AUTHORITY_KEY_FILE="$work/human-owner/owner.seed"
    export LAYERX_TEST_OWNER_AUTHORITY_PUBLIC LAYERX_TEST_OWNER_AUTHORITY_DID LAYERX_TEST_OWNER_AUTHORITY_ASSET
    setpriv --reuid=4021 --regid=4021 --clear-groups "$work/owner-authority-test" \
        --exact human_runtime::owner_authority_tests::real_owner_authority_prepares_over_temp_socket --nocapture --test-threads=1
    exit 0
elif [[ ${2:-} == --post-lxip ]]; then
    export LAYERX_TEST_OWNER_AUTHORITY_SOCKET="$runtime/layerxd.lni.sock"
    "${LAYERX_TEST_PYTHON:-python3}" tests/daemon/post-lxip.py "$work"
    exit 0
elif [[ ${2:-} == --grant-issuance ]]; then
    setpriv --reuid=4021 --regid=4021 --clear-groups "$work/client" "$runtime/layerxd.lni.sock" --grant-issuance "$work/grants/state"
elif [[ ${2:-} == --module-maintenance || ${2:-} == --metered-allowance ]]; then
    setpriv --reuid=4021 --regid=4021 --clear-groups "$work/client" "$runtime/layerxd.lni.sock" "$2" "$scenario_state"
elif [[ ${2:-} == --maintenance-crash ]]; then
    setpriv --reuid=4021 --regid=4021 --clear-groups "$work/client" "$runtime/layerxd.lni.sock" --maintenance-queue
    printf G >&"$apply_gate_fd"
    result=0
    wait "$sequencer_pid" || result=$?
    [[ "$result" == $((128 + $3)) ]]
    sequencer_pid=
elif [[ ${2:-} == --availability-batches ]]; then
    setpriv --reuid=4021 --regid=4021 --clear-groups "$work/client" "$runtime/layerxd.lni.sock" --availability-batches > "$work/availability-activity-id"
    cp "$3" "$work/availability-test"
    chmod 0755 "$work/availability-test"
    LAYERX_TEST_AVAILABILITY_WORK="$work" LAYERX_TEST_AVAILABILITY_SOCKET="$runtime/layerxd.lni.sock" LAYERX_TEST_AVAILABILITY_STAGE=retained \
        setpriv --reuid=4021 --regid=4021 --clear-groups "$work/availability-test" \
        --exact real_daemon_availability_refusals --nocapture --test-threads=1
    for ((attempt=0; attempt<3000; attempt++)); do
        [[ ! -f "$work/availability-finality-ready" ]] || break
        kill -0 "$settlement_pid"
        sleep 0.1
    done
    [[ -f "$work/availability-finality-ready" ]]
    LAYERX_TEST_AVAILABILITY_WORK="$work" LAYERX_TEST_AVAILABILITY_SOCKET="$runtime/layerxd.lni.sock" LAYERX_TEST_AVAILABILITY_STAGE=finalized \
        setpriv --reuid=4021 --regid=4021 --clear-groups "$work/availability-test" \
        --exact real_daemon_availability_refusals --nocapture --test-threads=1
    kill "$sequencer_pid"
    wait "$sequencer_pid"
    sequencer_pid=
    python3 - "$work/data/checkpoints/da/00000000000000000001.lxda" <<'PYCORRUPT'
import os, sys
with open(sys.argv[1], 'r+b') as bundle:
    bundle.truncate(1)
    bundle.flush()
    os.fsync(bundle.fileno())
PYCORRUPT
    (source platform/hosted/node/sequencer-env.sh; layerx_sequencer_environment "$work/data/sequencer.env"; if [[ ${2:-} == --availability-batches ]]; then export LAYERX_NODE_SETTLEMENT_CONTRACT="$availability_bond" LAYERX_NODE_CHECKPOINT_REGISTRY="$availability_registry" LAYERX_NODE_PAXEER_RPC_PORT="$availability_port"; fi; exec "$sequencer_binary" --serve "$work/data/sequencer.conf") >> "$work/sequencer.log" 2>&1 &
    sequencer_pid=$!
    for ((attempt=0; attempt<200; attempt++)); do
        [[ ! -S "$runtime/layerxd.lni.sock" ]] || break
        kill -0 "$sequencer_pid"
        sleep 0.1
    done
    LAYERX_TEST_AVAILABILITY_WORK="$work" LAYERX_TEST_AVAILABILITY_SOCKET="$runtime/layerxd.lni.sock" LAYERX_TEST_AVAILABILITY_STAGE=corrupt \
        setpriv --reuid=4021 --regid=4021 --clear-groups "$work/availability-test" \
        --exact real_daemon_availability_refusals --nocapture --test-threads=1
    exit 0
else
    setpriv --reuid=4021 --regid=4021 --clear-groups "$work/client" "$runtime/layerxd.lni.sock" "${@:2}"
    kill -0 "$sequencer_pid"
fi

if [[ ${2:-} == --maintenance || ${2:-} == --maintenance-crash || ${2:-} == --withdraw || ${2:-} == --grant-issuance || ${2:-} == --module-maintenance || ${2:-} == --metered-allowance ]]; then
    if [[ -n "$sequencer_pid" ]]; then
        kill -KILL "$sequencer_pid"
        wait "$sequencer_pid" || true
    fi
    sequencer_pid=
    kill -KILL "$replica_pid"
    wait "$replica_pid" || true
    replica_pid=
    (set -a; source "$work/data/replica.env"; export LAYERX_AUTHORITY_READY_FD="$replica_ready_fd"; exec "$native_bin/layerxd" --authority-replica "$work/data/replica.conf") >> "$work/replica.log" 2>&1 &
    replica_pid=$!
    IFS= read -r -n 1 -t 20 replica_ready <&"$replica_ready_fd"
    [[ "$replica_ready" == R ]]
    if [[ ${5:-} == --reject-* ]]; then
        python3 - "$work/data" "$5" <<'PYMARKER'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
marker, = root.rglob('initialized-genesis.lxg')
case = sys.argv[2]
record = bytearray(marker.read_bytes())
if case == '--reject-missing':
    marker.unlink()
elif case == '--reject-body':
    record[30] ^= 1
    marker.write_bytes(record)
elif case == '--reject-signature':
    record[-1] ^= 1
    marker.write_bytes(record)
elif case == '--reject-truncated':
    marker.write_bytes(record[:-1])
elif case == '--reject-symlink':
    retained = marker.with_suffix('.retained')
    marker.rename(retained)
    marker.symlink_to(retained)
elif case == '--reject-zero-checkpoint':
    (marker.parent / '00000000000000000000.lxs').write_bytes(b'invalid')
else:
    raise SystemExit('unknown marker mutation')
PYMARKER
        result=0
        (source platform/hosted/node/sequencer-env.sh; layerx_sequencer_environment "$work/data/sequencer.env"; if [[ ${2:-} == --availability-batches ]]; then export LAYERX_NODE_SETTLEMENT_CONTRACT="$availability_bond" LAYERX_NODE_CHECKPOINT_REGISTRY="$availability_registry" LAYERX_NODE_PAXEER_RPC_PORT="$availability_port"; fi; exec "$native_bin/layerxd" --serve "$work/data/sequencer.conf") >> "$work/sequencer.log" 2>&1 || result=$?
        [[ "$result" != 0 ]]
        rg -q 'bootstrap .* failed with result' "$work/sequencer.log"
        exit 0
    fi
    (source platform/hosted/node/sequencer-env.sh; layerx_sequencer_environment "$work/data/sequencer.env"; if [[ ${2:-} == --availability-batches ]]; then export LAYERX_NODE_SETTLEMENT_CONTRACT="$availability_bond" LAYERX_NODE_CHECKPOINT_REGISTRY="$availability_registry" LAYERX_NODE_PAXEER_RPC_PORT="$availability_port"; fi; exec "$native_bin/layerxd" --serve "$work/data/sequencer.conf") >> "$work/sequencer.log" 2>&1 &
    sequencer_pid=$!
    python3 - "$runtime/layerxd.lni.sock" "$sequencer_pid" <<'PYWAIT'
import os, socket, sys, time
for attempt in range(200):
    os.kill(int(sys.argv[2]), 0)
    try:
        with socket.socket(socket.AF_UNIX) as connection:
            connection.connect(sys.argv[1])
        break
    except OSError:
        time.sleep(0.1)
else:
    raise SystemExit("restarted daemon did not accept LNI connections")
PYWAIT
    recovered_mode=--maintenance-recovered
    if [[ ${2:-} == --withdraw ]]; then recovered_mode=--withdraw-recovered; fi
    if [[ ${2:-} == --module-maintenance || ${2:-} == --metered-allowance ]]; then
        setpriv --reuid=4021 --regid=4021 --clear-groups "$work/client" "$runtime/layerxd.lni.sock" "$2-recovered" "$scenario_state"
    elif [[ ${2:-} == --grant-issuance ]]; then
        setpriv --reuid=4021 --regid=4021 --clear-groups "$work/client" "$runtime/layerxd.lni.sock" --grant-issuance-recovered "$work/grants/state"
    else
        setpriv --reuid=4021 --regid=4021 --clear-groups "$work/client" "$runtime/layerxd.lni.sock" "$recovered_mode"
    fi
    kill -0 "$sequencer_pid"
    kill -0 "$replica_pid"
    if [[ ${2:-} != --withdraw && ${2:-} != --grant-issuance && ${2:-} != --module-maintenance && ${2:-} != --metered-allowance ]]; then
        (set -a; source "$work/data/replica.env"; python3 tests/daemon/maintenance-evidence.py)
    fi
fi
