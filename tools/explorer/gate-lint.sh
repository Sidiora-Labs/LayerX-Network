#!/bin/sh
#
# gate-lint.sh
#
# The explorer lint gate, the path the workflow contract names as the lint
# gate. It runs the same three language checks the explorer-lint job runs, in
# the same order: tools/explorer/lint-backend.sh, tools/explorer/lint-frontend.sh
# and tools/explorer/lint-services.sh. Run it from anywhere:
#
#   tools/explorer/gate-lint.sh
#   tools/explorer/gate-lint.sh --check
#
# Each leg is bounded by EXPLORER_GATE_BUDGET_SECONDS, 1500 by default, applied
# through timeout(1). The gate stops on the first leg that fails or exhausts
# its budget and reports that leg, the command it ran, its exit code and its
# log path instead of starting the next one, so a landing decision is never
# open-ended.
#
# Exit codes: 0 when every leg passes; the exit code of the first failing leg;
# 124 when a leg exhausts its budget; 1 when the toolchain does not resolve.

set -eu

readonly DEFAULT_BUDGET_SECONDS=1500
# How long timeout(1) waits after its TERM before it sends KILL.
readonly KILL_GRACE_SECONDS=30
# timeout(1) reports 124 when it terminates a command and 128+KILL when the
# command ignored the termination signal and had to be killed.
readonly TIMEOUT_STATUS=124
readonly KILLED_STATUS=137

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
repo_root=$(CDPATH='' cd -- "$script_dir/../.." && pwd -P)
runner=$repo_root/explorer/deploy/tools/mix-in-builder.sh
dockerfile=$repo_root/explorer/deploy/tools/Dockerfile.elixir-builder
frontend_dir=$repo_root/explorer/frontend
log_dir=${EXPLORER_GATE_LOG_DIR:-$repo_root/build/explorer-gates}
budget=${EXPLORER_GATE_BUDGET_SECONDS:-$DEFAULT_BUDGET_SECONDS}

check_only=0

log() {
  printf 'gate-lint: %s\n' "$*" >&2
}

die() {
  log "$*"
  exit 1
}

usage() {
  cat >&2 <<'USAGE'
Usage: tools/explorer/gate-lint.sh [--check]

Runs the explorer lint gate: the backend formatting and static analysis, the
frontend eslint and type check, and the services formatting and clippy, through
the three lint scripts the explorer-lint job calls.

  --check     validate this script, the container runtime, the builder image
              recipe, the node toolchain and the three lint scripts, then exit
              without running a check
  -h, --help  print this usage

Environment

  EXPLORER_GATE_BUDGET_SECONDS  seconds allowed per leg, 1500 by default; a leg
                                that exhausts it stops the gate with exit code
                                124 instead of starting the next leg
  EXPLORER_GATE_LOG_DIR         directory the leg logs are written to; the
                                default is build/explorer-gates under the
                                repository root, and the backend lint script
                                writes its own per-check logs there too
USAGE
}

# Print an argument list the way a shell would accept it back, so the command a
# failing leg ran can be rerun straight from the report.
quote_command() {
  separator=''
  for argument in "$@"; do
    case $argument in
      '' | *[!A-Za-z0-9_@%+=:,./-]*)
        printf "%s'%s'" "$separator" "$(printf '%s' "$argument" | sed "s/'/'\\\\''/g")"
        ;;
      *)
        printf '%s%s' "$separator" "$argument"
        ;;
    esac
    separator=' '
  done
  printf '\n'
}

report_failure() {
  leg=$1
  status=$2
  leg_log=$3
  shift 3

  if [ "$status" -eq "$TIMEOUT_STATUS" ] || [ "$status" -eq "$KILLED_STATUS" ]; then
    log "TIMEOUT: the $leg leg exhausted its budget of $budget seconds"
  else
    log "FAILED: the $leg leg"
  fi
  log "command: $(quote_command "$@")"
  log "exit code: $status"
  log "log: $leg_log"
  log "logs: $log_dir"
}

# Runs one leg under the budget from its own working directory, streams its
# output and keeps a copy in the log directory. The pipeline hides the leg's
# exit code from the shell, so the group writes it to a file read back here.
# A leg that fails or times out ends the gate with its own exit code.
run_leg() {
  slug=$1
  leg=$2
  workdir=$3
  shift 3

  [ -d "$workdir" ] || die "the $leg leg has no working directory at $workdir"

  leg_log=$log_dir/$slug.log
  status_file=$log_dir/$slug.status

  log "running the $leg leg: $(quote_command "$@")"
  rm -f "$status_file"
  {
    leg_status=0
    cd "$workdir" || exit 1
    timeout --signal=TERM --kill-after="$KILL_GRACE_SECONDS" "$budget" "$@" 2>&1 ||
      leg_status=$?
    printf '%s' "$leg_status" >"$status_file"
  } | tee "$leg_log"

  [ -f "$status_file" ] || die "the $leg leg left no exit code in $status_file"
  status=$(cat "$status_file")
  rm -f "$status_file"

  if [ "$status" -eq 0 ]; then
    log "the $leg leg passed, log $leg_log"
    return 0
  fi

  report_failure "$leg" "$status" "$leg_log" "$@"
  exit "$status"
}

check_toolchain() {
  sh -n "$0" || die "$0 does not parse"
  log 'the gate script parses'

  command -v timeout >/dev/null 2>&1 || die 'timeout is not on PATH'

  command -v docker >/dev/null 2>&1 || die 'docker is not on PATH'
  docker version --format '{{.Server.Version}}' >/dev/null 2>&1 ||
    die 'the container runtime is not answering'
  log 'the container runtime answers'

  [ -x "$runner" ] || die "$runner is not an executable script"
  [ -f "$dockerfile" ] || die "$dockerfile does not exist"
  "$runner" --check >/dev/null || die "$runner --check did not resolve the builder recipe"
  log 'the builder image recipe resolves'

  command -v node >/dev/null 2>&1 || die 'node is not on PATH'
  command -v yarn >/dev/null 2>&1 || die 'yarn is not on PATH'
  [ -f "$frontend_dir/package.json" ] || die "$frontend_dir/package.json does not exist"
  for frontend_script in lint:eslint lint:tsc; do
    grep -q "\"$frontend_script\":" "$frontend_dir/package.json" ||
      die "the frontend declares no $frontend_script script"
  done
  log 'the node toolchain and the frontend scripts resolve'

  for lint_script in lint-backend.sh lint-frontend.sh lint-services.sh; do
    [ -x "$script_dir/$lint_script" ] ||
      die "$script_dir/$lint_script is not an executable script"
    sh -n "$script_dir/$lint_script" || die "$script_dir/$lint_script does not parse"
  done
  log 'the backend, frontend and services lint scripts parse'

  log "no check was run; the budget per leg is $budget seconds"
}

while [ "$#" -gt 0 ]; do
  case $1 in
    --check)
      check_only=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    *)
      usage
      die "this gate takes no argument, got $1"
      ;;
  esac
done

[ "$#" -eq 0 ] || die "this gate takes no argument, got $1"

case $budget in
  '' | *[!0-9]*)
    die "EXPLORER_GATE_BUDGET_SECONDS must be a whole number of seconds, got '$budget'"
    ;;
esac
[ "$budget" -gt 0 ] || die 'EXPLORER_GATE_BUDGET_SECONDS must be greater than zero'

mkdir -p "$log_dir" || die "the log directory $log_dir could not be created"
log "logs: $log_dir"

if [ "$check_only" -eq 1 ]; then
  check_toolchain
  exit 0
fi

# The backend lint script keeps one log per mix check; the gate collects them
# beside its own leg logs so one directory holds everything the run wrote.
EXPLORER_LINT_LOG_DIR=$log_dir
export EXPLORER_LINT_LOG_DIR

run_leg lint-backend 'backend lint' "$repo_root" "$script_dir/lint-backend.sh"
run_leg lint-frontend 'frontend lint' "$repo_root" "$script_dir/lint-frontend.sh"
run_leg lint-services 'services lint' "$repo_root" "$script_dir/lint-services.sh"

log 'the backend, frontend and services lint checks passed'
log "logs: $log_dir"
