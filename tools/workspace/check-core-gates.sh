#!/bin/sh
set -eu

inventory=${1:-tools/workspace/core-gates.txt}
makefile=${2:-Makefile}
sanitizer_makefile=tools/build/sanitizers.mk
actual=$(mktemp)
declared=$(mktemp)
trap 'rm -f "$actual" "$declared"' EXIT HUP INT TERM

sed -nE 's/^(test-wave-[0-9]+|qualify-[A-Za-z0-9_-]+|test-sanitizer[A-Za-z0-9_-]*|test-crypto-sanitizers|test-replay-golden[A-Za-z0-9_-]*|test-contracts|reproducible|scan-consensus):.*/\1/p' \
    "$makefile" "$sanitizer_makefile" | LC_ALL=C sort -u > "$actual"
awk -F '|' '!/^#/ && NF == 2 { print $1 }' "$inventory" | LC_ALL=C sort -u > "$declared"
cmp "$actual" "$declared"

core_prerequisites=$(
    awk '
        /^core-test-all:/ { collecting = 1 }
        collecting {
            line = $0
            sub(/^[^:]*:/, "", line)
            gsub(/\\/, "", line)
            printf "%s ", line
            if ($0 !~ /\\$/) exit
        }
    ' "$makefile"
)

while IFS='|' read -r target classification; do
    case "$target" in \#*|'') continue ;; esac
    grep -Eq "^${target}:" "$makefile" "$sanitizer_makefile"
    case "$classification" in
        core-test-all)
            case " $core_prerequisites " in
                *" $target "*) ;;
                *) echo "core-test-all omits declared gate: $target" >&2; exit 1 ;;
            esac
            ;;
        covered-by-*|environment-blocked-*) ;;
        *) echo "unknown core gate classification: $classification" >&2; exit 1 ;;
    esac
done < "$inventory"
