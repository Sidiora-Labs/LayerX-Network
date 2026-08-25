#!/usr/bin/env bash
# shellcheck disable=SC2034

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$TEST_DIR/../.." && pwd)"

bash -n "$REPO_DIR/layerx"

export LAYERX_DASHBOARD_LIBRARY=1
# shellcheck source=/dev/null
source "$REPO_DIR/layerx"

fail() {
  printf 'dashboard test failed: %s\n' "$1" >&2
  exit 1
}

[[ ${#MODULE_IDS[@]} -eq 9 ]] || fail "expected all nine workspace modules"
[[ "$(selected_count)" == "9" ]] || fail "every module should be selected initially"

ACTION_INDEX=4
build_plan
[[ ${#STEP_MODULE[@]} -eq 57 ]] || fail "full plan should contain all 57 workspace steps"

for expected in core contracts agent human platform programs interop paxeer specgen; do
  found=0
  for actual in "${STEP_MODULE[@]}"; do
    [[ "$actual" == "$expected" ]] && found=1
  done
  [[ $found -eq 1 ]] || fail "full plan omitted $expected"
done

paxeer_chain_build=0
paxeer_docs_install=0
truthful_sdk_label=0
for index in "${!STEP_MODULE[@]}"; do
  [[ "${STEP_COMMAND[index]}" == "make paxeer-build" ]] && paxeer_chain_build=1
  [[ "${STEP_COMMAND[index]}" == "make paxeer-docs-install" ]] && paxeer_docs_install=1
  [[ "${STEP_LABEL[index]}" == "Conformance-test Go and JVM SDKs; compile Swift and C sharp SDKs" ]] && truthful_sdk_label=1
done
[[ $paxeer_chain_build -eq 1 ]] || fail "full plan omitted the Paxeer chain build"
[[ $paxeer_docs_install -eq 1 ]] || fail "full plan omitted the locked Paxeer docs install"
[[ $truthful_sdk_label -eq 1 ]] || fail "SDK plan overstates Swift or C sharp verification"

set_all_modules 0
SELECTED[4]=1
ACTION_INDEX=1
build_plan
[[ ${#STEP_MODULE[@]} -eq 6 ]] || fail "platform install should contain six dependency steps"
[[ "${STEP_COMMAND[0]}" == "cargo fetch --manifest-path platform/Cargo.toml --locked" ]] || fail "platform Rust dependency step drifted"
[[ "${STEP_COMMAND[1]}" == "make platform-js-install" ]] || fail "platform JavaScript install step drifted"
[[ "${STEP_COMMAND[5]}" == "swift package resolve" ]] || fail "platform Swift dependency step drifted"

set_all_modules 1
before="${SELECTED[0]}"
handle_click 5 8
[[ "${SELECTED[0]}" != "$before" ]] || fail "mouse click did not toggle the core module"

FOCUS=0
ENV_INDEX=0
EVENT=right
handle_dashboard_event
[[ $ENV_INDEX -eq 1 ]] || fail "keyboard environment selection failed"

FOCUS=11
ACTION_INDEX=0
EVENT=right
handle_dashboard_event
[[ $ACTION_INDEX -eq 1 ]] || fail "keyboard action selection failed"

set_all_modules 0
SELECTED[8]=1
ACTION_INDEX=4
DRY_RUN=1
build_plan
render_execution() { :; }
render_results() { :; }
read_event() { EVENT=""; }
run_plan
[[ -z "$RUN_DIRECTORY" ]] || fail "plan-only mode created a run directory"
for status in "${STEP_STATUS[@]}"; do
  [[ "$status" == "PLANNED" ]] || fail "plan-only mode did not preserve planned status"
done

printf 'dashboard checks passed\n'
