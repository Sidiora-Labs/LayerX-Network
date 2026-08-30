#!/bin/sh
set -eu

runtime_root=${1:-programs/crates/layerx-programs-runtime/src}

rust_code() {
    awk '
    BEGIN { block = 0 }
    {
        line = $0
        i = 1
        while (i <= length(line)) {
            pair = substr(line, i, 2)
            char = substr(line, i, 1)
            if (block) {
                if (pair == "*/") { block = 0; i += 2 } else { i++ }
                continue
            }
            if (pair == "//") break
            if (pair == "/*") { block = 1; i += 2; continue }
            if (char == "\"") {
                i++
                escaped = 0
                while (i <= length(line)) {
                    quoted = substr(line, i, 1)
                    i++
                    if (escaped) { escaped = 0; continue }
                    if (quoted == "\\") { escaped = 1; continue }
                    if (quoted == "\"") break
                }
                printf " "
                continue
            }
            printf "%s", char
            i++
        }
        printf " "
    }
    END { printf "\n" }
    ' "$1" | sed 's/[[:space:]]*::[[:space:]]*/::/g'
}

check_root() {
    root=$1
    failed=0

    for path in \
        budget.rs \
        abi/mod.rs abi/balance.rs abi/capability.rs abi/codec.rs abi/context.rs \
        abi/event_tests.rs abi/host_state.rs abi/manifest.rs abi/response.rs abi/storage_ops.rs \
        host/mod.rs host/balance.rs host/context.rs host/memory.rs host/storage.rs host/events.rs host/calls.rs \
        host/transfer.rs host/scan.rs host/crypto.rs host/signature.rs
    do
        if [ ! -f "$root/$path" ]; then
            echo "runtime module boundary: missing $path" >&2
            failed=1
        fi
    done

    for legacy in abi.rs host.rs
    do
        if [ -e "$root/$legacy" ]; then
            echo "runtime module boundary: legacy $legacy remains" >&2
            failed=1
        fi
    done

    for production_path in "$root"/ffi*.rs "$root"/lifecycle.rs
    do
        if [ -f "$production_path" ] && grep -E '_for_qualification' "$production_path" >/dev/null
        then
            echo "runtime module boundary: production transition reaches qualification-only API" >&2
            failed=1
        fi
    done

    if [ -d "$root/abi" ]; then
        actual=$(
            for file in "$root"/abi/*.rs
            do
                [ -f "$file" ] && basename "$file"
            done | sort
        )
        expected=$(printf '%s\n' balance.rs capability.rs codec.rs context.rs event_tests.rs host_state.rs manifest.rs mod.rs response.rs storage_ops.rs)
        if [ "$actual" != "$expected" ]; then
            echo "runtime module boundary: unexpected ABI module inventory" >&2
            failed=1
        fi
    fi
    if [ -d "$root/host" ]; then
        actual=$(
            for file in "$root"/host/*.rs
            do
                [ -f "$file" ] && basename "$file"
            done | sort
        )
        expected=$(printf '%s\n' balance.rs calls.rs context.rs crypto.rs events.rs memory.rs mod.rs scan.rs signature.rs storage.rs transfer.rs)
        if [ "$actual" != "$expected" ]; then
            echo "runtime module boundary: unexpected host module inventory" >&2
            failed=1
        fi
    fi

    if [ -d "$root/host" ]; then
        for family in storage events calls transfer scan crypto signature
        do
            file="$root/host/$family.rs"
            [ -f "$file" ] || continue
            flattened=$(rust_code "$file")
            for sibling in storage events calls transfer scan crypto signature
            do
                [ "$sibling" = "$family" ] && continue
                if printf '%s\n' "$flattened" | grep -E "(super::|crate::host::)([[:space:]]*\\{[^}]*|)$sibling(::|[^[:alnum:]_])" >/dev/null
                then
                    echo "runtime module boundary: $family imports a sibling host family" >&2
                    failed=1
                fi
                if printf '%s\n' "$flattened" | grep -E "crate::[[:space:]]*\\{[^}]*host::$sibling(::|[^[:alnum:]_])" >/dev/null
                then
                    echo "runtime module boundary: $family imports a sibling host family" >&2
                    failed=1
                fi
            done
            if printf '%s\n' "$flattened" | grep -E 'crate::host|crate::[[:space:]]*\{[^}]*host|use[[:space:]]+crate[[:space:]]+as[[:space:]]+[[:alnum:]_]+|use[[:space:]]+crate::[[:space:]]*\{[^}]*self[[:space:]]+as[[:space:]]+[[:alnum:]_]+|use[[:space:]]+super[[:space:]]+as[[:space:]]+[[:alnum:]_]+|use[[:space:]]+super::[[:space:]]*\{[^}]*self[[:space:]]+as[[:space:]]+[[:alnum:]_]+|extern[[:space:]]+crate[[:space:]]+self[[:space:]]+as[[:space:]]+[[:alnum:]_]+' >/dev/null
            then
                echo "runtime module boundary: $family names or aliases a forbidden parent" >&2
                failed=1
            fi
            if printf '%s\n' "$flattened" | grep -E '(^|[^[:alnum:]_])(Abi|Composition|Storage|Meter)([^[:alnum:]_]|$)|struct[[:space:]]+RuntimeState|impl[[:space:]]+RuntimeState' >/dev/null
            then
                echo "runtime module boundary: $family reaches state outside RuntimeState" >&2
                failed=1
            fi
        done
    fi

    return "$failed"
}

if [ "${1:-}" = "--self-test" ]; then
    fixture=$(mktemp -d)
    trap 'find "$fixture" -type f -delete; find "$fixture" -depth -type d -exec rmdir {} \; 2>/dev/null || true' EXIT
    mkdir -p "$fixture/abi" "$fixture/host"
    for path in budget.rs abi/mod.rs abi/balance.rs abi/capability.rs abi/codec.rs abi/context.rs abi/event_tests.rs abi/host_state.rs abi/manifest.rs abi/response.rs abi/storage_ops.rs host/mod.rs host/balance.rs host/context.rs host/memory.rs host/storage.rs host/events.rs host/calls.rs host/transfer.rs host/scan.rs host/crypto.rs host/signature.rs
    do
        : > "$fixture/$path"
    done
    if ! check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: valid layout was rejected" >&2
        exit 1
    fi
    rm -f "$fixture/budget.rs"
    if check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: missing budget module was accepted" >&2
        exit 1
    fi
    : > "$fixture/budget.rs"
    printf '%s\n' 'fn invalid() { execute_authorized_budgeted_for_qualification(); }' > "$fixture/ffi.rs"
    if check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: qualification API reached production transition" >&2
        exit 1
    fi
    : > "$fixture/ffi.rs"
    printf '%s\n' '// Meter is only reachable via RuntimeState.' 'const NOTE: &str = "Abi Storage Composition Meter";' > "$fixture/host/storage.rs"
    if ! check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: comments or strings were treated as code" >&2
        exit 1
    fi
    printf '%s\n' 'use super :: events :: register;' > "$fixture/host/storage.rs"
    if check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: sibling import was accepted" >&2
        exit 1
    fi
    printf '%s\n' 'use crate::{host::events}; fn violation(){events::register();}' > "$fixture/host/storage.rs"
    if check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: grouped crate host import was accepted" >&2
        exit 1
    fi
    printf '%s\n' 'use super as host_root; fn violation(){host_root::events::register();}' > "$fixture/host/storage.rs"
    if check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: host parent alias was accepted" >&2
        exit 1
    fi
    printf '%s\n' 'use crate::host; fn violation(){host::events::register();}' > "$fixture/host/storage.rs"
    if check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: bare host parent was accepted" >&2
        exit 1
    fi
    printf '%s\n' 'use crate::host::{self as host_root}; fn violation(){host_root::events::register();}' > "$fixture/host/storage.rs"
    if check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: grouped host self alias was accepted" >&2
        exit 1
    fi
    printf '%s\n' 'use crate as root; fn violation(){root::host::events::register();}' > "$fixture/host/storage.rs"
    if check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: crate root alias was accepted" >&2
        exit 1
    fi
    printf '%s\n' 'use super::{self as host_root}; fn violation(){host_root::events::register();}' > "$fixture/host/storage.rs"
    if check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: grouped super self alias was accepted" >&2
        exit 1
    fi
    printf '%s\n' 'use crate::{self as root}; fn violation(){root::host::events::register();}' > "$fixture/host/storage.rs"
    if check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: grouped crate self alias was accepted" >&2
        exit 1
    fi
    printf '%s\n' 'extern crate self as root; fn violation(){root::host::events::register();}' > "$fixture/host/storage.rs"
    if check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: extern crate root alias was accepted" >&2
        exit 1
    fi
    printf '%s\n' 'use crate::storage::Storage;' > "$fixture/host/storage.rs"
    if check_root "$fixture" >/dev/null 2>&1; then
        echo "runtime module boundary self-test: direct state access was accepted" >&2
        exit 1
    fi
    echo "runtime module boundary self-test: sibling, parent-alias and direct-state violations rejected"
    exit 0
fi

check_root "$runtime_root"
sh "$0" --self-test
