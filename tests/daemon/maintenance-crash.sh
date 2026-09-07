#!/usr/bin/env bash
set -euo pipefail
build_dir=${1:-build}
for boundary in {8..20}; do
    bash tests/daemon/program-admission.sh "$build_dir" --maintenance-crash "$boundary" 1
done
for boundary in 1 2; do
    for occurrence in {1..5}; do
        bash tests/daemon/program-admission.sh "$build_dir" --maintenance-crash "$boundary" "$occurrence"
    done
done
for occurrence in {1..4}; do
    bash tests/daemon/program-admission.sh "$build_dir" --maintenance-crash 3 "$occurrence"
done
