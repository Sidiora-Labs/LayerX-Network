#!/usr/bin/env bash
set -euo pipefail
export CARGO_BUILD_JOBS=4 MAKEFLAGS=-j4
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
[[ $(id -u) == 0 ]]
build_dir=${1:-build}
work=$(mktemp -d /tmp/lxp-program-admission-XXXXXX)
replica_pid= sequencer_pid=
cleanup() {
    result=$?
    for pid in "$sequencer_pid" "$replica_pid"; do
        if [[ -n "$pid" ]]; then kill "$pid" 2>/dev/null || true; wait "$pid" || true; fi
    done
    if [[ "$result" == 0 ]]; then rm -rf "$work"; else printf 'native admission evidence: %s\n' "$work" >&2; fi
}
trap cleanup EXIT
chmod 0755 "$work"
python3 - "$work" <<'PY'
import os, pathlib, socket, sys
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
PY
read -r program_port replica_port rpc_port < "$work/ports"
mkfifo "$work/replica-ready"
exec {replica_ready_fd}<>"$work/replica-ready"
LAYERX_NODE_PAXEER_CHAIN_ID=31337 \
LAYERX_NODE_SETTLEMENT_CONTRACT=0x1111111111111111111111111111111111111111 \
LAYERX_NODE_CHECKPOINT_REGISTRY=0x2222222222222222222222222222222222222222 \
LAYERX_NODE_PAXEER_RPC_ADDRESS=127.0.0.1 LAYERX_NODE_PAXEER_RPC_PORT="$rpc_port" \
bash platform/hosted/node/bootstrap.sh --data-dir "$work/data" --run-dir "$work/run" \
    --network-id 77 --sequencer-key "$work/sequencer" --treasury-key "$work/treasury" \
    --lni-uid 4021 --lni-gid 4021 --program-port "$program_port" --replica-port "$replica_port" \
    --layerxd "$root/$build_dir/bin/layerxd" --genesis-build "$root/$build_dir/bin/layerx-genesis-build" \
    > "$work/bootstrap.log" 2>&1
(set -a; source "$work/data/replica.env"; export LAYERX_AUTHORITY_READY_FD="$replica_ready_fd"; exec "$root/$build_dir/bin/layerxd" --authority-replica "$work/data/replica.conf") > "$work/replica.log" 2>&1 &
replica_pid=$!
IFS= read -r -n 1 -t 20 replica_ready <&"$replica_ready_fd"
[[ "$replica_ready" == R ]]
(set -a; source "$work/data/sequencer.env"; exec "$root/$build_dir/bin/layerxd" --serve "$work/data/sequencer.conf") > "$work/sequencer.log" 2>&1 &
sequencer_pid=$!
for ((attempt=0; attempt<200; attempt++)); do
    [[ ! -S "$work/run/layerxd.lni.sock" ]] || break
    kill -0 "$sequencer_pid"
    sleep 0.1
done
cp "$build_dir/tests/lxp_test_program_admission" "$work/client"
chmod 0755 "$work/client"
setpriv --reuid=4021 --regid=4021 --clear-groups "$work/client" "$work/run/layerxd.lni.sock" "${@:2}"
kill -0 "$sequencer_pid"

if [[ ${2:-} == --maintenance ]]; then
    kill -KILL "$sequencer_pid"
    wait "$sequencer_pid" || true
    sequencer_pid=
    kill -KILL "$replica_pid"
    wait "$replica_pid" || true
    replica_pid=
    (set -a; source "$work/data/replica.env"; export LAYERX_AUTHORITY_READY_FD="$replica_ready_fd"; exec "$root/$build_dir/bin/layerxd" --authority-replica "$work/data/replica.conf") >> "$work/replica.log" 2>&1 &
    replica_pid=$!
    IFS= read -r -n 1 -t 20 replica_ready <&"$replica_ready_fd"
    [[ "$replica_ready" == R ]]
    (set -a; source "$work/data/sequencer.env"; exec "$root/$build_dir/bin/layerxd" --serve "$work/data/sequencer.conf") >> "$work/sequencer.log" 2>&1 &
    sequencer_pid=$!
    python3 - "$work/run/layerxd.lni.sock" "$sequencer_pid" <<'PYWAIT'
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
    setpriv --reuid=4021 --regid=4021 --clear-groups "$work/client" "$work/run/layerxd.lni.sock" --maintenance-recovered
    kill -0 "$sequencer_pid"
    kill -0 "$replica_pid"
fi
