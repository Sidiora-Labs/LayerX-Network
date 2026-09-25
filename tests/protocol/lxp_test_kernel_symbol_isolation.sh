#!/bin/sh
set -eu

object=${1:?usage: lxp_test_kernel_symbol_isolation.sh KERNEL_OBJECT}
undefined_symbols=$(nm -u "$object")

if printf '%s\n' "$undefined_symbols" | awk '{print $NF}' | \
    grep -Eq '^getenv(@.*)?$'; then
    echo "kernel object imports the process environment" >&2
    exit 1
fi
