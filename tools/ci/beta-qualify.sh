#!/usr/bin/env bash
set -euo pipefail

quote() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

beta_qualify_focused() {
    local root revision ledger directory environment tool version command_text
    local ordinal index prefix started_at outcome note log status failed=0 dirty
    local -a command_words
    root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
    cd "$root"
    [ "$#" -gt 0 ] || { echo 'beta-qualify: use make beta-qualify-focused' >&2; return 2; }
    python3 - "$@" <<'PY'
import re
import sys
from pathlib import Path
sections = re.split(r'(?=^\[)', Path('spec/layerx-beta/spec.kvx').read_text(), flags=re.M)
expected = [re.search(r'^verify_cmd = "([^"]+)"', s, re.M)[1]
            for s in sections if re.match(r'\[task\.[23]\.\d+\]', s)]
commands = sys.argv[1:]
if commands[6:-7] != expected:
    sys.exit('beta-qualify: wave 2/3 commands differ from spec; update the focused recipe')
for command in commands:
    if not re.fullmatch(r'make(?: [a-zA-Z0-9_-]+)+', command):
        sys.exit('beta-qualify: expected plain make targets')
PY
    revision=$(git rev-parse HEAD)
    ledger=spec/layerx-beta/qualification.kvx
    directory="spec/layerx-beta/evidence/$revision/focused"
    mkdir -p "$directory"
    export LAYERX_QUALIFICATION_ARTIFACT_DIR="$root/$directory"
    exec 9>"$directory/.runner.lock"
    flock -n 9 || { echo 'beta-qualify: focused runner already active' >&2; return 2; }
    environment="$(hostname) $(uname -m)"
    for tool in bash make cc gcc-13 clang-18 rustc cargo node npm python3 forge; do
        if command -v "$tool" >/dev/null 2>&1; then
            if version=$("$tool" --version 2>&1); then
                version=${version%%$'\n'*}
            else
                version='version capture failed'
            fi
        else
            version=unavailable
        fi
        environment+="; $tool: $version"
    done
    environment+="; CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-unset}; RAYON_NUM_THREADS=${RAYON_NUM_THREADS:-unset}"
    environment+="; LAYERX_QUALIFICATION_ARTIFACT_DIR=$LAYERX_QUALIFICATION_ARTIFACT_DIR"
    index=0
    for log in "$directory"/[0-9]*-*.log; do
        [ -f "$log" ] || continue
        prefix=${log##*/}
        prefix=${prefix%%-*}
        [[ $prefix =~ ^[0-9]+$ ]] || continue
        [ "$((10#$prefix))" -le "$index" ] || index=$((10#$prefix))
    done
    for command_text in "$@"; do
        read -r -a command_words <<<"$command_text"
        while :; do
            index=$((index + 1))
            printf -v log '%s/%02d-%s.log' "$directory" "$index" "${command_words[1]}"
            [ ! -e "$log" ] && break
        done
        dirty=$(git status --porcelain --untracked-files=no)
        started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
        outcome=pass
        note=""
        status=0
        case " $command_text " in
        *' platform-beta-cluster-up '* | *' platform-hosted-smoke '* | *' platform-beta-cluster-down '*)
            outcome=blocked
            note='Cluster operations are outside focused local authorization; owner must authorize beta infrastructure and execute task 3.7 with cluster identity and boundary evidence.'
            printf '%s\n' "$note" >"$log"
            status=125
            ;;
        *)
            printf 'beta-qualify: %s -> %s\n' "$command_text" "$log"
            set +e
            "${command_words[@]}" >"$log" 2>&1
            status=$?
            set -e
            if [ "$status" -ne 0 ]; then
                outcome=fail
                note="$command_text exited $status"
            fi
            ;;
        esac
        if [ -n "$dirty" ]; then
            note+="${note:+; }Tracked working-tree changes present before command execution; this is development evidence, not immutable release qualification."
        fi
        [ "$outcome" = pass ] || failed=1
        exec 8>>"$ledger"
        flock 8
        ordinal=$(sed -n 's/^\[gate\.5\.1\.\([0-9][0-9]*\)\]$/\1/p' "$ledger" | sort -n | tail -n 1)
        ordinal=$((${ordinal:-0} + 1))
        {
            printf '\n[gate.5.1.%s]\n' "$ordinal"
            printf 'task = "5.1"\nreqs = ["12.2","12.4","12.5"]\n'
            printf 'revision = "%s"\n' "$revision"
            printf 'command = "%s"\n' "$(quote "$command_text")"
            printf 'environment = "%s"\n' "$(quote "$environment")"
            printf 'started_at = "%s"\n' "$started_at"
            printf 'outcome = "%s"\n' "$outcome"
            printf 'evidence = "%s"\n' "$log"
            printf 'note = "%s"\n' "$(quote "$note")"
        } >&8
        flock -u 8
        exec 8>&-
        if [ "$outcome" = blocked ]; then
            printf 'beta-qualify: blocked (not executed): %s\n' "$command_text"
        else
            printf 'beta-qualify: %s (exit %s): %s\n' "$outcome" "$status" "$command_text"
        fi
    done
    return "$failed"
}

beta_qualify_focused "$@"
