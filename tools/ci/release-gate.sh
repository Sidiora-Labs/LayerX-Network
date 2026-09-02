#!/usr/bin/env bash
# Runs one release gate command and records it as an executed-evidence gate
# entry in the schema tools/ci/beta-ledger-check.sh enforces.
#
# usage: tools/ci/release-gate.sh --job <id> --ordinal <n> [--task <task>] [--reqs <req.ac,...>] -- make <target>
#
# The command log and the gate record are written below
# spec/layerx-beta/evidence/<revision>/release-gates/<job>/ so the retained
# runner artifact can be merged into spec/layerx-beta/qualification.kvx
# without rewriting a single value. The exit status is the command's.
set -euo pipefail

usage() {
    sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//' >&2
}

job=""
ordinal=""
task="4.1"
reqs="8.5"
while [ "$#" -gt 0 ]; do
    case $1 in
    --job)
        [ "$#" -ge 2 ] || { usage; exit 2; }
        job=$2
        shift 2
        ;;
    --ordinal)
        [ "$#" -ge 2 ] || { usage; exit 2; }
        ordinal=$2
        shift 2
        ;;
    --task)
        [ "$#" -ge 2 ] || { usage; exit 2; }
        task=$2
        shift 2
        ;;
    --reqs)
        [ "$#" -ge 2 ] || { usage; exit 2; }
        reqs=$2
        shift 2
        ;;
    --)
        shift
        break
        ;;
    -h | --help)
        usage
        exit 0
        ;;
    *)
        usage
        exit 2
        ;;
    esac
done
[ -n "$job" ] && [ -n "$ordinal" ] && [ "$#" -ge 1 ] || { usage; exit 2; }
case $job in
*[!A-Za-z0-9_-]*) echo "release-gate: job id must be [A-Za-z0-9_-]+: $job" >&2; exit 2 ;;
esac
case $ordinal in
'' | *[!0-9]*) echo "release-gate: ordinal must be a positive integer: $ordinal" >&2; exit 2 ;;
esac
[ "$1" = make ] || { echo "release-gate: the gate command must be a make target so the ledger can replay it" >&2; exit 2; }

root=$(git rev-parse --show-toplevel)
revision=$(git -C "$root" rev-parse HEAD)
started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
if [ -n "${GITHUB_ACTIONS:-}" ]; then
    environment="github-actions ${ImageOS:-${RUNNER_OS:-unknown}} $(uname -m) run=${GITHUB_RUN_ID:-0}/${GITHUB_RUN_ATTEMPT:-0}"
else
    environment="${LAYERX_GATE_ENVIRONMENT:-$(uname -s | tr '[:upper:]' '[:lower:]') $(uname -m) $(uname -r)}"
fi
command_text="$*"
evidence_dir="spec/layerx-beta/evidence/$revision/release-gates/$job"
mkdir -p "$root/$evidence_dir"
log="$evidence_dir/command.log"
record="$evidence_dir/record.kvx"

echo "release-gate: $job ($command_text) on $revision at $started_at"
set +e
(cd "$root" && "$@") 2>&1 | tee "$root/$log"
status=${PIPESTATUS[0]}
set -e
if [ "$status" -eq 0 ]; then
    outcome=pass
    note=""
else
    outcome=fail
    note="$command_text exited $status"
fi

reqs_list=""
IFS=, read -r -a req_items <<<"$reqs"
for item in "${req_items[@]}"; do
    [ -n "$item" ] || continue
    reqs_list="$reqs_list${reqs_list:+,}\"$item\""
done
quote() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}
{
    printf '[gate.%s.%s]\n' "$task" "$ordinal"
    printf 'task = "%s"\n' "$(quote "$task")"
    printf 'reqs = [%s]\n' "$reqs_list"
    printf 'revision = "%s"\n' "$revision"
    printf 'command = "%s"\n' "$(quote "$command_text")"
    printf 'environment = "%s"\n' "$(quote "$environment")"
    printf 'started_at = "%s"\n' "$started_at"
    printf 'outcome = "%s"\n' "$outcome"
    printf 'evidence = "%s"\n' "$log"
    printf 'note = "%s"\n' "$(quote "$note")"
} >"$root/$record"
echo "release-gate: $outcome; record at $record"
cat "$root/$record"
exit "$status"
