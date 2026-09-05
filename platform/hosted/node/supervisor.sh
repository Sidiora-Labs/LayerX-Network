#!/usr/bin/env bash
# LayerX beta node supervisor.
#
# One supervisor runs per daemon container and both share the run directory:
#
#   supervisor.sh --role sequencer --data-dir DIR --run-dir DIR [--layerxd P] \
#       -- <bootstrap.sh arguments except --data-dir and --run-dir>
#   supervisor.sh --role replica --data-dir DIR --run-dir DIR [--layerxd P]
#
# The sequencer supervisor bootstraps the data directory on first start (or
# reuses it when node.env is present), publishes the generation number to the
# run directory, waits for the replica supervisor to report its daemon up for
# that generation, starts `layerxd --serve`, and answers requests on the
# pod-local unix socket RUN_DIR/supervisor.sock:
#
#   reset\n   -> stop both daemons, discard the data directory contents,
#                re-run bootstrap.sh, restart both, answer
#                {"state":"reset","reset_id":"<16 hex>"}
#   status\n  -> {"state":"running","generation":N}
#
# The replica supervisor starts `layerxd --authority-replica` for every
# generation the sequencer supervisor publishes and stops it when the
# sequencer supervisor asks, via files in the run directory:
#
#   generation                 current generation, written by the sequencer side
#   replica-ready.<gen>        replica daemon running for <gen>
#   reset.<id>.stop-replica    sequencer side asks the replica side to stop
#   reset.<id>.replica-stopped replica side has stopped its daemon
#
# A daemon that exits on its own ends the supervisor with status 1 so the pod
# restarts it against the retained data directory.
set -euo pipefail

log() { printf 'supervisor[%s]: %s\n' "${ROLE:-handler}" "$*" >&2; }
fail() { log "$*"; exit 1; }

ROLE=""
DATA_DIR=""
RUN_DIR=""
LAYERXD=""
SOCAT=""
BOOTSTRAP_ARGS=()
HANDLE=0
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)

while [ $# -gt 0 ]; do
    case "$1" in
        --role) ROLE=$2; shift 2 ;;
        --data-dir) DATA_DIR=$2; shift 2 ;;
        --run-dir) RUN_DIR=$2; shift 2 ;;
        --layerxd) LAYERXD=$2; shift 2 ;;
        --socat) SOCAT=$2; shift 2 ;;
        --handle) HANDLE=1; shift ;;
        --) shift; BOOTSTRAP_ARGS=("$@"); break ;;
        *) fail "unknown argument $1" ;;
    esac
done

[ -n "$DATA_DIR" ] || fail "--data-dir is required"
[ -n "$RUN_DIR" ] || fail "--run-dir is required"
mkdir -p "$DATA_DIR" "$RUN_DIR"
DATA_DIR=$(readlink -f "$DATA_DIR")
RUN_DIR=$(readlink -f "$RUN_DIR")
SUPERVISOR_SOCKET="$RUN_DIR/supervisor.sock"
PID_FILE="$RUN_DIR/supervisor.pid"
GENERATION_FILE="$RUN_DIR/generation"

json_reply() { printf '%s\n' "$1"; }

# --- connection handler (spawned by socat per connection) -------------------
if [ "$HANDLE" -eq 1 ]; then
    IFS= read -r -t 10 line || line=""
    line=${line%$'\r'}
    case "$line" in
        reset)
            id=$(od -An -N8 -tx1 /dev/urandom | tr -d ' \n')
            [ -r "$PID_FILE" ] || { json_reply '{"error":{"code":"supervisor_unavailable","retry":"after","retry_after_seconds":5}}'; exit 0; }
            supervisor_pid=$(cat "$PID_FILE")
            : > "$RUN_DIR/reset-request.$id"
            if ! kill -USR1 "$supervisor_pid" 2>/dev/null; then
                rm -f "$RUN_DIR/reset-request.$id"
                json_reply '{"error":{"code":"supervisor_unavailable","retry":"after","retry_after_seconds":5}}'
                exit 0
            fi
            deadline=$(( $(date +%s) + 300 ))
            while [ ! -e "$RUN_DIR/reset-done.$id" ]; do
                if [ -e "$RUN_DIR/reset-failed.$id" ]; then
                    rm -f "$RUN_DIR/reset-failed.$id"
                    json_reply '{"error":{"code":"reset_failed","retry":"after","retry_after_seconds":30}}'
                    exit 0
                fi
                [ "$(date +%s)" -lt "$deadline" ] || { json_reply '{"error":{"code":"reset_timeout","retry":"after","retry_after_seconds":60}}'; exit 0; }
                sleep 0.2
            done
            rm -f "$RUN_DIR/reset-done.$id"
            json_reply "{\"state\":\"reset\",\"reset_id\":\"$id\"}"
            ;;
        status)
            generation=0
            [ -r "$GENERATION_FILE" ] && generation=$(cat "$GENERATION_FILE")
            if [ -r "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
                json_reply "{\"state\":\"running\",\"generation\":$generation}"
            else
                json_reply "{\"state\":\"stopped\",\"generation\":$generation}"
            fi
            ;;
        *)
            json_reply '{"error":{"code":"unknown_request","retry":"never","retry_after_seconds":0}}'
            ;;
    esac
    exit 0
fi

[ "$ROLE" = sequencer ] || [ "$ROLE" = replica ] || fail "--role must be sequencer or replica"

resolve_binary() {
    local given=$1 name=$2 candidate
    if [ -n "$given" ]; then
        [ -x "$given" ] || fail "$name is not executable: $given"
        printf '%s' "$given"
        return
    fi
    for candidate in "$PWD/build/bin/$name" "/usr/local/bin/$name"; do
        if [ -x "$candidate" ]; then printf '%s' "$candidate"; return; fi
    done
    command -v "$name" || fail "$name not found"
}
LAYERXD=$(resolve_binary "$LAYERXD" layerxd)

DAEMON_PID=""

start_daemon() {
    # start_daemon ENV_FILE MODE CONFIG
    local env_file=$1 mode=$2 config=$3
    [ -r "$env_file" ] || fail "environment file missing: $env_file"
    [ -r "$config" ] || fail "configuration missing: $config"
    (
        set -a
        # shellcheck disable=SC1090
        . "$env_file"
        set +a
        exec "$LAYERXD" "$mode" "$config"
    ) &
    DAEMON_PID=$!
    log "started layerxd $mode pid $DAEMON_PID"
}

stop_daemon() {
    if [ -n "$DAEMON_PID" ] && kill -0 "$DAEMON_PID" 2>/dev/null; then
        kill -TERM "$DAEMON_PID" 2>/dev/null || true
        local waited=0
        while kill -0 "$DAEMON_PID" 2>/dev/null && [ "$waited" -lt 100 ]; do
            sleep 0.1
            waited=$((waited + 1))
        done
        if kill -0 "$DAEMON_PID" 2>/dev/null; then
            kill -KILL "$DAEMON_PID" 2>/dev/null || true
        fi
        wait "$DAEMON_PID" 2>/dev/null || true
        log "stopped layerxd pid $DAEMON_PID"
    fi
    DAEMON_PID=""
}

daemon_alive() { [ -n "$DAEMON_PID" ] && kill -0 "$DAEMON_PID" 2>/dev/null; }

wait_for_file() {
    # wait_for_file PATH SECONDS
    local deadline=$(( $(date +%s) + $2 ))
    while [ ! -e "$1" ]; do
        [ "$(date +%s)" -lt "$deadline" ] || return 1
        sleep 0.2
    done
}

# --- replica role -----------------------------------------------------------
if [ "$ROLE" = replica ]; then
    trap 'stop_daemon; exit 0' TERM INT
    current=""
    while :; do
        wait_for_file "$GENERATION_FILE" 3600 || fail "no generation published within an hour"
        generation=$(cat "$GENERATION_FILE")
        if [ -z "$generation" ] || [ "$generation" = "$current" ]; then
            sleep 0.2
            continue
        fi
        wait_for_file "$DATA_DIR/replica.env" 60 || fail "replica.env missing for generation $generation"
        start_daemon "$DATA_DIR/replica.env" --authority-replica "$DATA_DIR/replica.conf"
        current=$generation
        : > "$RUN_DIR/replica-ready.$generation"
        while :; do
            if ! daemon_alive; then
                wait "$DAEMON_PID" && status=0 || status=$?
                fail "layerxd --authority-replica exited with status $status"
            fi
            stop_request=$(ls "$RUN_DIR"/reset.*.stop-replica 2>/dev/null | head -n 1 || true)
            if [ -n "$stop_request" ]; then
                id=${stop_request##*/reset.}
                id=${id%.stop-replica}
                log "reset $id: stopping the replica"
                stop_daemon
                rm -f "$stop_request" "$RUN_DIR/replica-ready.$current"
                : > "$RUN_DIR/reset.$id.replica-stopped"
                break
            fi
            sleep 0.2
        done
    done
fi

# --- sequencer role ---------------------------------------------------------
SOCAT=${SOCAT:-$(command -v socat || true)}
[ -n "$SOCAT" ] && [ -x "$SOCAT" ] || fail "socat is required for the supervisor socket"
[ -x "$SCRIPT_DIR/bootstrap.sh" ] || fail "bootstrap.sh missing next to supervisor.sh"

RESET_PENDING=0
trap 'RESET_PENDING=1' USR1
SOCAT_PID=""

cleanup() {
    trap - TERM INT EXIT
    stop_daemon
    if [ -n "$SOCAT_PID" ]; then kill "$SOCAT_PID" 2>/dev/null || true; fi
    rm -f "$PID_FILE" "$SUPERVISOR_SOCKET"
}
trap 'cleanup; exit 0' TERM INT
trap cleanup EXIT

run_bootstrap() {
    "$SCRIPT_DIR/bootstrap.sh" --data-dir "$DATA_DIR" --run-dir "$RUN_DIR" --layerxd "$LAYERXD" "$@"
}

publish_generation() {
    local generation=$1
    rm -f "$RUN_DIR"/replica-ready.* 2>/dev/null || true
    printf '%s' "$generation" > "$GENERATION_FILE.tmp"
    mv "$GENERATION_FILE.tmp" "$GENERATION_FILE"
    wait_for_file "$RUN_DIR/replica-ready.$generation" 120 || fail "replica did not come up for generation $generation"
}

GENERATION=0
if [ -r "$GENERATION_FILE" ]; then GENERATION=$(cat "$GENERATION_FILE"); fi
if [ ! -r "$DATA_DIR/node.env" ]; then
    log "bootstrapping $DATA_DIR"
    run_bootstrap --force "${BOOTSTRAP_ARGS[@]}"
fi
GENERATION=$((GENERATION + 1))
publish_generation "$GENERATION"
start_daemon "$DATA_DIR/sequencer.env" --serve "$DATA_DIR/sequencer.conf"

printf '%s' "$$" > "$PID_FILE"
rm -f "$SUPERVISOR_SOCKET"
LNI_GID=$(sed -n 's/^LAYERX_NODE_LNI_ALLOWED_GID=//p' "$DATA_DIR/sequencer.env")
[ -n "$LNI_GID" ] || fail "LAYERX_NODE_LNI_ALLOWED_GID missing from sequencer.env"
"$SOCAT" -T 320 "UNIX-LISTEN:$SUPERVISOR_SOCKET,fork,mode=660,group=$LNI_GID" \
    "EXEC:$0 --handle --data-dir $DATA_DIR --run-dir $RUN_DIR" &
SOCAT_PID=$!
log "supervisor socket $SUPERVISOR_SOCKET"

perform_reset() {
    local id=$1
    log "reset $id: stopping the sequencer"
    stop_daemon
    : > "$RUN_DIR/reset.$id.stop-replica"
    if ! wait_for_file "$RUN_DIR/reset.$id.replica-stopped" 120; then
        rm -f "$RUN_DIR/reset.$id.stop-replica"
        : > "$RUN_DIR/reset-failed.$id"
        fail "reset $id: the replica did not stop"
    fi
    rm -f "$RUN_DIR/reset.$id.replica-stopped"
    log "reset $id: discarding $DATA_DIR and re-running bootstrap"
    if ! run_bootstrap --force "${BOOTSTRAP_ARGS[@]}"; then
        : > "$RUN_DIR/reset-failed.$id"
        fail "reset $id: bootstrap failed"
    fi
    GENERATION=$((GENERATION + 1))
    publish_generation "$GENERATION"
    start_daemon "$DATA_DIR/sequencer.env" --serve "$DATA_DIR/sequencer.conf"
    : > "$RUN_DIR/reset-done.$id"
    log "reset $id: complete at generation $GENERATION"
}

while :; do
    if [ "$RESET_PENDING" -eq 1 ]; then
        RESET_PENDING=0
        for request in "$RUN_DIR"/reset-request.*; do
            [ -e "$request" ] || continue
            rm -f "$request"
            perform_reset "${request##*/reset-request.}"
        done
    fi
    if ! daemon_alive; then
        wait "$DAEMON_PID" && status=0 || status=$?
        fail "layerxd --serve exited with status $status"
    fi
    if ! kill -0 "$SOCAT_PID" 2>/dev/null; then
        fail "supervisor socket listener exited"
    fi
    sleep 0.2
done
