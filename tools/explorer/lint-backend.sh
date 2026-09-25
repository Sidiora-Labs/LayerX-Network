#!/bin/sh
#
# lint-backend.sh
#
# The explorer backend lint gate: formatting and static analysis over the
# Elixir umbrella under explorer/backend. Both checks run through
# explorer/deploy/tools/mix-in-builder.sh, so the continuous-integration job
# and a developer run the same mix invocations against the same pinned
# toolchain.
#
# Run from anywhere:
#
#   tools/explorer/lint-backend.sh
#
# Every check runs even when an earlier one fails, so one invocation reports
# every finding; the exit code is the first non-zero one, and the failing
# command and its log path are printed at the end.
#
# Exit codes: 0 when both checks pass; the first non-zero mix exit code
# otherwise; 1 when the runner or the log directory does not resolve.

set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
repo_root=$(CDPATH='' cd -- "$script_dir/../.." && pwd -P)
runner=$repo_root/explorer/deploy/tools/mix-in-builder.sh
log_dir=${EXPLORER_LINT_LOG_DIR:-$repo_root/.logs/explorer-lint}

first_status=0
first_command=''
first_log=''

log() {
  printf 'lint-backend: %s\n' "$*" >&2
}

die() {
  log "$*"
  exit 1
}

# Runs one mix invocation through the builder, streams its output and keeps a
# copy in the log directory. The pipeline hides the mix exit code from the
# shell, so the command writes it to a file the caller reads back. The
# invocation is guarded by || so a failing check records its status instead of
# tripping set -e inside the pipeline's subshell.
run_check() {
  name=$1
  shift

  check_log=$log_dir/$name.log
  status_file=$log_dir/$name.status

  log "running mix $*"
  rm -f "$status_file"
  {
    check_status=0
    "$runner" "$@" 2>&1 || check_status=$?
    printf '%s' "$check_status" >"$status_file"
  } | tee "$check_log"

  [ -f "$status_file" ] || die "the $name check left no exit code in $status_file"
  status=$(cat "$status_file")
  rm -f "$status_file"

  if [ "$status" -eq 0 ]; then
    log "mix $* passed"
    return 0
  fi

  log "mix $* failed with exit code $status, log $check_log"
  if [ "$first_status" -eq 0 ]; then
    first_status=$status
    first_command="explorer/deploy/tools/mix-in-builder.sh $*"
    first_log=$check_log
  fi
  return 0
}

[ -x "$runner" ] || die "$runner is not an executable script"
mkdir -p "$log_dir" || die "the log directory $log_dir could not be created"

run_check format format --check-formatted
run_check credo credo --strict

if [ "$first_status" -ne 0 ]; then
  log "FAILED: $first_command (exit code $first_status)"
  log "log: $first_log"
  exit "$first_status"
fi

log "the backend formatting and static analysis checks passed"
log "logs: $log_dir"
